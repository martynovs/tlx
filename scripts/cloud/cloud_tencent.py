"""Tencent Cloud CVM provider of scripts/cloud_verify.py (`--provider tencent`).

Spot instance (InstanceChargeType SPOTPAID, one-time, billed per second)
running Ubuntu: AMD x86_64 (SA5 Bergamo / SA9 Turin; 16 vCPU for --quick, 64
vCPU for --full) or Ampere arm64 (SR1, ap-guangzhou only), the cheapest type
and zone that DescribeZoneInstanceConfigInfos reports on sale. Default regions
ap-guangzhou and ap-shanghai (alternatives ap-beijing, ap-nanjing,
ap-chengdu; ap-hongkong, ap-singapore with --region). Pay-as-you-go
(POSTPAID_BY_HOUR) only with --on-demand, tried after every spot offer.

Safety rules:
  - Only the tccli profile and CAM sub-user of the local cloud config
    (cloud.cloud_common.cloud_identity, docs/cloud-verification.md; the identity is
    checked before anything else). Never the `default` profile;
    TENCENTCLOUD_* credential variables are removed from tccli's environment
    so that they cannot take precedence.
  - tccli and ssh run without proxy variables (a stable source address).
  - Every instance carries the tag tlx-project=tlx from RunInstances on
    (Tencent reserves the key "Project"), plus a per-run tag tlx-run=<token>
    to find it after an interrupted launch.
  - Dead-man's switch. Tencent offers no external timer for spot instances
    (RunInstances refuses an ActionTimer for them with
    InvalidParameterCombination, ImportInstancesActionTimer with
    UnsupportedOperation.ActionTimerForSpecialInstance), and "no charges when
    shut down" applies neither to spot instances nor to a shutdown from
    inside the OS: a stopped spot instance is billed until it is terminated.
    So the instance terminates itself: it is launched with the CAM role
    `selfterm_role` of the cloud config (CamRoleName; only
    TerminateInstances on instances tagged tlx-project=tlx), and the user data installs cloud_tencent_selfterm.py
    with a systemd timer that calls TerminateInstances on the instance after
    --max-minutes, with the role's temporary credentials from the metadata
    service. `shutdown -h` 5 minutes later is the last resort. Without the
    role (not created yet, or the sub-user may not pass it) the launch falls
    back to the shutdown alone, with a warning. Every instance carries
    tlx-deadline=<UTC time>: each run first terminates tagged instances
    that are stopped or past their deadline (also --cleanup).
    Pay-as-you-go instances (--on-demand) also get an ActionTimer
    TerminateInstances in RunInstances (now + max(max-minutes, 7)).
    After ssh is up, check_deadman verifies the switch (timer and scheduled
    shutdown present, role credentials work) and any failure terminates the
    instance. TerminateInstances in the caller's `finally`.
  - The security group `tlx-verify-ssh` allows tcp 22, 443 and 2222 from this
    machine's public address only (the user data starts a second sshd on 443
    and 2222 for networks that block outbound port 22). "This machine's
    address" is the one at launch time, and it can move under a running run:
    `allow_ssh` then adds the current one to that same group - add only, never
    opened to 0.0.0.0/0, no other owner's group touched (cloud/allowlist.py).
    The group is shared by every owner on this machine, so the reconcile a
    launch runs is add-only as well (cloud/sgpolicy.py): ownership records
    provenance, not exclusive use. Only an idle-account coordinator prune may
    remove attributed rules.
  - Admission: every RunInstances is preceded by the same request as a dry
    run; a quota answer (LimitExceeded.*) stops the launch before anything
    is created (cloud_verify counts it as no launch). A spot-quota refusal is
    zone-scoped, so the launch may still try the other zones of the same
    region with the same instance type and market; only when every zone
    refuses is it a region quota stop (2026-10-08). slot_holders() lists
    every instance of the account for cloud_verify's local admission count.
  - Errors that look like risk control or a blocked account (risk, forbidden,
    audit, frozen, arrears), missing CAM permissions and rejected
    credentials abort the run, each with its own message; nothing retries
    around them.
"""
import base64, datetime, hashlib, json, os, re, time, urllib.parse, uuid

from cloud.cloud_common import AccountError, CloudStop, current_owner, stop_code, warning, InfraError, KEY_FILE, Offer, ProviderQuotaFull, SkipOffer, SlotHolder, auto_release_time, deadman_user_data, cloud_identity, direct_public_ip, log

from cloud.transport import capture_json, cli_env as transport_env
from cloud import sgpolicy

MAINLAND = ["ap-guangzhou", "ap-shanghai", "ap-beijing", "ap-nanjing", "ap-chengdu"]
REGIONS = MAINLAND + ["ap-hongkong", "ap-singapore"]
DEFAULT_REGIONS = ["ap-guangzhou", "ap-shanghai"]
# Default types per arch and tier (the cheapest offer of these wins). SR1
# (Ampere Altra) is the only arm64 family, in ap-guangzhou-6.
TYPES = {
    ("x86_64", "quick"): ["SA5.4XLARGE32", "SA9.4XLARGE32"],
    ("x86_64", "full"): ["SA9.16XLARGE128", "SA5.16XLARGE256", "SA4.16XLARGE128"],
    ("arm64", "quick"): ["SR1.4XLARGE32"],
    ("arm64", "full"): ["SR1.16XLARGE128", "SR1.8XLARGE64"],
}
# Public Ubuntu images: "Ubuntu Server 24.04 LTS 64位" (x86_64), "Ubuntu 22.04(arm64)".
IMAGE_RE = re.compile(r"Ubuntu (?:Server )?(\d\d)\.(\d\d)(?: LTS)?(?: 64位|\(arm64\))")
UBUNTU_MAX = (24, 4)   # the newest release used (as on the other providers)
SG_NAME = "tlx-verify-ssh"
VPC_NAME = "tlx-verify"
VPC_CIDR = "172.16.0.0/16"
SSH_PORTS = [22, 443, 2222]
# TLC's -metadir (target/tlc-states/<pid>-<tid>) holds one model's disk queue
# and checkpoint; a large model alone needs over 13 GB there, and a run that
# is killed at its timeout skips TLC's -cleanup, so the leftovers of earlier
# passes stack up. A 40 GB system disk filled up mid-run on the large external
# models; 200 GB leaves room for several concurrent models plus their
# leftovers (2026-10-08: the missing external oracles).
# TLC's default fingerprint set is also disk-backed (MSBDiskFPSet/DiskStateQueue,
# spilling what does not fit in the heap), so a huge model's own state queue is
# unbounded: at 16 workers BPConProof and TLCSailfish2 filled 200 GB
# (2026-10-09, "when writing the disk (StatePoolWriter.run): No space left on
# device"), which the corpus collection had recorded as a model error until the
# machine failure was classified as one (scripts/tlc.py). 512 GB is the next step
# up, not a measured ceiling: a model that outgrows it wants an explicit
# --disk-gb, and it now fails as an infrastructure failure instead of being
# stored as a model result.
DISK_GB = 512
DISK_TYPE = "CLOUD_BSSD"
def spot_quota(error):
    """Whether `error` is Tencent's zone-scoped spot-quota refusal
    (LimitExceeded.UserSpotQuota / LimitExceeded.SpotQuota). The check is on
    the text, so it also reads a `ProviderQuotaFull` raised from a dry run."""
    return any(code in str(error) for code in SPOT_QUOTA_ERRORS)


def quota_stop(offer, zones):
    """The message of a region-wide spot-quota stop: every zone of the region
    refused the same request, so nothing was launched anywhere (exit 20;
    `--wait-quota` then re-probes this same region and type, never another)."""
    return (f"{offer.itype} in {offer.region}: spot quota refused in every zone "
            f"({', '.join(zones)}); nothing launched")


# "Project" (any case) is a reserved tag key on Tencent Cloud (FailedOperation.TagKeyReserved).
TAG_KEY = "tlx-project"
TAGS = [{"Key": TAG_KEY, "Value": "tlx"}]
# Launch refusals that mean "this zone (or type) has nothing now".
ZONE_ERRORS = ("ResourceInsufficient", "ResourcesSoldOut", "ResourceUnavailable", "InvalidZone", "NoAvailableZone",
               "InstanceTypeNotSupport", "UnsupportedOperation.NoInstanceTypeSupportSpot", "InvalidParameterValue.InstanceTypeNotSupport")
REGION_ERRORS = ("LimitExceeded",)
# A spot-quota refusal is zone-scoped: Tencent's spot limit is enforced per
# zone, so the other zones of the same region may still admit the same request
# (2026-10-08: ap-shanghai-5 refused one more with LimitExceeded.UserSpotQuota
# while a dry run for the same type in ap-shanghai-8 said "would succeed" 20 s
# later; ap-guangzhou refused a third instance with LimitExceeded.SpotQuota).
# Every other LimitExceeded code is an account-wide limit: region-scoped.
SPOT_QUOTA_ERRORS = ("LimitExceeded.UserSpotQuota", "LimitExceeded.SpotQuota")
CHARGE_TYPES = {"spot": "SPOTPAID", "on-demand": "POSTPAID_BY_HOUR"}
# Errors that stop the run at once, by kind (checked in this order):
# the account itself is in trouble,
RISK_RE = re.compile(r"risk|forbid|audit|frozen|arrear|blacklist|AccountQualification|InsufficientBalance", re.I)
# the CAM policy of the sub-user lacks a permission,
DENIED_RE = re.compile(r"Unauthorized|CamNoAuth|PermissionDenied", re.I)
# the credentials were rejected (AuthFailure.SignatureFailure, .SecretIdNotFound, ...).
CREDENTIALS_PREFIX = "AuthFailure"
FATAL_HINTS = {
    "risk": "this looks like risk control or a blocked account; stopping, not retrying",
    "denied": "the CAM policy of the configured sub-user does not allow this (docs/cloud-verification.md); not retrying",
    "credentials": "the credentials of the configured tccli profile were rejected (wrong or disabled key, or clock skew); not retrying",
}
# RunInstances' answer when the role does not exist or the sub-user may not pass it.
ROLE_REFUSED = "AuthFailure.CamRoleNameAuthenticateFailed"
SELFTERM_FILE = os.path.join(os.path.dirname(os.path.abspath(__file__)), "cloud_tencent_selfterm.py")
SELFTERM_PATH = "/usr/local/sbin/tlx-selfterm"
SELFTERM_UNIT = "tlx-selfterm"
SHUTDOWN_GRACE = 5    # minutes from the self-termination to the fallback shutdown
DEADLINE_TAG = "tlx-deadline"
DEADLINE_SLACK = 5    # minutes after the fallback shutdown before a run reaps the instance
# InstanceState values that mean the instance cannot be ssh'ed into any more
# (instance_state, cloud/allowlist.py): gone for good, or not running.
GONE_STATES = ("TERMINATED", "TERMINATING", "LAUNCH_FAILED")
STOPPED_STATES = ("STOPPED", "STOPPING", "SHUTDOWN")


class TencentError(Exception):
    def __init__(self, code, message, cmd):
        super().__init__(f"{code}: {message}")
        self.code, self.message, self.cmd = code, message, cmd

    @property
    def kind(self):
        """'risk', 'denied', 'credentials' (errors that stop the run) or None."""
        if RISK_RE.search(self.code) or RISK_RE.search(self.message):
            return "risk"
        if DENIED_RE.search(self.code):
            return "denied"
        if self.code.startswith(CREDENTIALS_PREFIX):
            return "credentials"
        return None

    @property
    def fatal(self):
        return self.kind is not None

    @property
    def hint(self):
        return FATAL_HINTS.get(self.kind, "")


def cli_env():
    """tccli's environment: no proxy, and no credential variables that could
    take precedence over --profile."""
    return transport_env(("TENCENTCLOUD_", "QCLOUD_"))


def parse_error(out):
    """(code, message) of a failed tccli call:
    "[TencentCloudSDKException] code:X message:Y requestId:Z"."""
    if m := re.search(r"code:(\S+)\s+message:(.*?)(?:\s+requestId:\S*)?\s*$", out, re.S):
        return m.group(1), m.group(2).strip()
    return "CliError", out.strip()[-500:]


def cli_args(params):
    """tccli arguments for an API's parameters: strings as they are, the rest as JSON."""
    out = []
    for k, v in params.items():
        out += [f"--{k}", v if isinstance(v, str) else json.dumps(v, separators=(",", ":"))]
    return out


def identity():
    """The configured {"arn", "profile", "selfterm_role"} (exits when there is
    none). selfterm_role is the instance's CAM role of the dead-man's switch
    (see the module docstring and docs/cloud-verification.md for its policies)."""
    return cloud_identity("tencent")


def tccli(product, api, region=None, **params):
    """Run one tccli call with the configured profile; returns the parsed JSON."""
    cmd = ["tccli", product, api, "--profile", identity()["profile"]] + (["--region", region] if region else []) + cli_args(params)
    return capture_json(cmd, env=cli_env(), missing="tccli is not installed",
                        error=lambda out: TencentError(*parse_error(out), [product, api, region]))


def tag_filter(key, value):
    return {"Name": f"tag:{key}", "Values": [value]}


def key_name(pubkey):
    """The key pair name for a public key (letters, digits and _ only, at
    most 25 characters): a new local key gets a new name, so no key pair is
    ever deleted or overwritten."""
    return "tlx_verify_" + hashlib.sha256(pubkey.strip().encode()).hexdigest()[:10]


def read_public_key():
    """The local public key text, or None when no key pair has been made yet."""
    path = KEY_FILE + ".pub"
    if not os.path.exists(path):
        return None
    with open(path) as source:
        return source.read().strip()


def pick_image(images, arch):
    """The ImageId of the newest plain Ubuntu LTS image not newer than
    UBUNTU_MAX for `arch` ('x86_64' or 'arm64') among DescribeImages' ImageSet."""
    want = {"x86_64": "x86_64", "arm64": "arm"}[arch]
    best = None
    for i in images:
        m = IMAGE_RE.fullmatch(i.get("ImageName", ""))
        if not m or i.get("Architecture") != want or i.get("ImageState", "NORMAL") != "NORMAL":
            continue
        ver = (int(m.group(1)), int(m.group(2)))
        if ver <= UBUNTU_MAX and (best is None or ver > best[0]):
            best = (ver, i["ImageId"], i["ImageName"])
    return best and (best[1], best[2])


def user_data(max_minutes, role):
    """The user data: with `role`, the self-termination timer after
    `max_minutes` and a shutdown SHUTDOWN_GRACE minutes later; without, the
    shutdown after `max_minutes` alone. Plus the extra sshd."""
    shutdown = max_minutes + SHUTDOWN_GRACE if role else max_minutes
    s = deadman_user_data(shutdown, extra_ssh_ports=[q for q in SSH_PORTS if q != 22])
    if role:
        with open(SELFTERM_FILE) as f:
            script = f.read()
        s += ("# self-termination with the instance role's credentials (scripts/cloud/cloud_tencent_selfterm.py)\n"
              f"cat > {SELFTERM_PATH} <<'TLX_SELFTERM_EOF'\n{script}TLX_SELFTERM_EOF\n"
              f"chmod 700 {SELFTERM_PATH}\n"
              f"systemd-run --unit={SELFTERM_UNIT} --on-active={max_minutes}min /usr/bin/python3 {SELFTERM_PATH} terminate {role}\n")
    return s


def deadline(now, max_minutes, role):
    """The tlx-deadline tag value: the time after which the instance is
    surely past its dead-man's switch (yyyymmddTHHMMZ, UTC)."""
    minutes = max_minutes + (SHUTDOWN_GRACE if role else 0) + DEADLINE_SLACK + 1
    return (now.astimezone(datetime.timezone.utc) + datetime.timedelta(minutes=minutes)).strftime("%Y%m%dT%H%MZ")


def expired(inst, now):
    """Whether a tagged instance (DescribeInstances' InstanceSet entry) is a
    leftover to terminate: stopped, or past its tlx-deadline."""
    state = inst.get("InstanceState")
    if state in ("TERMINATING", "SHUTDOWN", "LAUNCH_FAILED"):
        return False
    if state in ("STOPPED", "STOPPING"):
        return True
    tags = {t.get("Key"): t.get("Value") for t in inst.get("Tags") or []}
    try:
        d = datetime.datetime.strptime(tags.get(DEADLINE_TAG, ""), "%Y%m%dT%H%MZ").replace(tzinfo=datetime.timezone.utc)
    except ValueError:
        return False   # (no deadline: an instance of an older version; --cleanup handles it)
    return now > d


class TencentProvider:
    name = "tencent"
    currency = "CNY"
    ssh_user = "ubuntu"
    home = "/home/ubuntu"
    ssh_ports = SSH_PORTS
    default_regions = DEFAULT_REGIONS
    all_regions = REGIONS
    label_suffix = "tencent"
    default_ssh_proxy = "none"
    min_lifetime_minutes = 7   # ActionTimer: more than 5 minutes ahead
    supports_on_demand = True
    tag = f"{TAG_KEY}=tlx"
    default_arch = "x86_64"   # SR1 (arm64) is rarely in stock
    # launch() calls self.checkpoint() immediately before every RunInstances
    # (cloud_verify sets it: a cancel there launches nothing, and a reservation
    # whose checkpoint never passed made no provider call).
    calls_checkpoint = True

    def checkpoint(self):
        pass

    def __init__(self):
        self.owner = current_owner(required=False)
        self.on_demand = False
        self._unit_price = {}    # (zone, itype) -> on-demand price per hour (the spot bid cap)
        self._images = {}
        self._role = None        # the selfterm_role or "" (no role), once looked up
        self.role_note = ""
        identity()

    def selfterm_role(self):
        """The configured selfterm_role if that role exists and the CVM service
        may assume it, else "" (the dead-man's switch is then the shutdown alone)."""
        if self._role is None:
            role = identity()["selfterm_role"]
            try:
                info = tccli("cam", "GetRole", RoleName=role).get("RoleInfo") or {}
                if "cvm.qcloud.com" in urllib.parse.unquote(info.get("PolicyDocument") or ""):
                    self._role, self.role_note = role, f"CAM role {role}: self-termination from the instance"
                else:
                    self._role, self.role_note = "", f"CAM role {role} exists but cvm.qcloud.com may not assume it"
            except TencentError as e:
                if e.kind == "risk":
                    raise InfraError(f"cam GetRole: {e} -- {e.hint}", stop_code(str(e))) from e
                self._role = ""
                self.role_note = (f"no CAM role {role}" if "RoleNotExist" in e.code else f"cannot read CAM role {role} ({e.code})")
        return self._role

    def deadman_warning(self):
        return (f"{self.role_note}; the dead-man's switch is only a shutdown from the user data, and a stopped spot "
                "instance is billed until it is terminated (the next run or --cleanup terminates it; "
                "see docs/cloud-verification.md for the role); this is a warning, not a stop condition: the run continues and "
                "terminates its instance at the end")

    def check_account(self):
        want = identity()
        try:
            ident = tccli("sts", "GetCallerIdentity")
        except TencentError as e:
            raise AccountError(f"tccli profile {want['profile']!r}: {e}") from e
        if ident.get("Arn") != want["arn"]:
            raise AccountError(f"cloud_verify: tccli profile {want['profile']!r} is {ident.get('Arn')}, expected {want['arn']}; aborting")
        return f"{ident['Arn']} ({ident.get('Type')})"

    def balance(self):
        """The account balance as a line for the dry run (billing is usually
        not in the sub-user's policy: then says so)."""
        try:
            d = tccli("billing", "DescribeAccountBalance")
        except TencentError as e:
            return f"WARNING (not a stop condition): balance not readable ({e.code})"
        # Amounts in cents.
        return f"balance: {d.get('RealBalance', d.get('Balance', 0)) / 100:.2f} CNY"

    def mainland(self, region):
        return region in MAINLAND

    def default_types(self, arch, tier="quick"):
        return TYPES[(arch, tier)]

    def zone_configs(self, region, types, charge):
        d = tccli("cvm", "DescribeZoneInstanceConfigInfos", region,
                  Filters=[{"Name": "instance-charge-type", "Values": [charge]}, {"Name": "instance-type", "Values": types}])
        return d.get("InstanceTypeQuotaSet") or []

    def offers(self, types, arch, regions):
        out = []
        seen = set()
        charges = ["SPOTPAID"] + (["POSTPAID_BY_HOUR"] if self.on_demand else [])
        for region in regions:
            for charge in charges:
                for q in self.zone_configs(region, types, charge):
                    seen.add(q["InstanceType"])
                    if q.get("Status") != "SELL":
                        continue
                    a = "arm64" if q["InstanceFamily"].startswith(("SR", "SK")) else "x86_64"
                    if a != arch:
                        raise CloudStop(f"cloud_verify: {q['InstanceType']} is {a}, not {arch} (use --arch)")
                    price = q["Price"]
                    self._unit_price[(q["Zone"], q["InstanceType"])] = price["UnitPrice"]
                    spot = charge == "SPOTPAID"
                    out.append(Offer(price["UnitPriceDiscount"] if spot else price["UnitPrice"], region, q["Zone"], q["InstanceType"],
                                     q["Cpu"], q.get("StatusCategory") not in ("EnoughStock", "NormalStock"),
                                     market="spot" if spot else "on-demand"))
        if unknown := [t for t in types if t not in seen]:
            warning(f"no {', '.join(unknown)} in {', '.join(regions)} (not offered there, or unknown type)")
        return out

    def zone_offers(self, offer):
        """The candidates a launch may try without leaving the offer's region,
        instance type and market: the offer's own zone first, then the other
        SELL zones of the region, cheapest first. A spot-quota refusal is
        zone-scoped (2026-10-08), so only a move inside this list is allowed;
        another region, instance type or provider is never tried."""
        out = [self.copy_offer(offer)]
        try:
            qs = self.zone_configs(offer.region, [offer.itype], CHARGE_TYPES[offer.market])
        except TencentError as e:
            if e.fatal:
                raise
            warning(f"cannot list the zones of {offer.region} for {offer.itype} ({e}); no zone fallback for this offer")
            return out
        rest = []
        for q in qs:
            zone, price = q.get("Zone"), q.get("Price") or {}
            if q.get("Status") != "SELL" or q.get("InstanceType") != offer.itype or zone in (None, offer.zone):
                continue
            unit = price.get("UnitPrice", offer.price)
            self._unit_price[(zone, offer.itype)] = unit
            spot = offer.market == "spot"
            rest.append(Offer(unit if not spot else price.get("UnitPriceDiscount", unit), offer.region, zone, offer.itype,
                              q.get("Cpu", offer.vcpus), q.get("StatusCategory") not in ("EnoughStock", "NormalStock"),
                              market=offer.market))
        return out + sorted(rest, key=lambda o: (o.price, o.zone))

    @staticmethod
    def copy_offer(offer):
        """An independent copy: an offer is shared with the machine and the
        ledger, so a zone fallback must not mutate the one being probed."""
        return Offer(offer.price, offer.region, offer.zone, offer.itype, offer.vcpus, offer.deprioritized, offer.market)

    @staticmethod
    def move_offer(offer, cand):
        """Move `offer` in place onto the candidate: the offer object is shared
        with the machine and the ledger, so a zone fallback must show there."""
        offer.zone, offer.price, offer.deprioritized = cand.zone, cand.price, cand.deprioritized

    def zone_scan(self, offer, probe, zones=None):
        """Try `probe(cand)` on the offer's zone, then (only after a
        zone-scoped spot-quota refusal, and only within the same region,
        instance type and market) on the region's other zones. A refusal of
        any other kind propagates; a spot-quota refusal moves on and is
        logged. `offer` is left on the zone that admitted; when every
        candidate refuses, this raises `ProviderQuotaFull` (code 20) naming
        the zones, and nothing was launched.

        The region's other zones are listed lazily, only after the first
        spot-quota refusal: the common launch makes no extra API call, and
        `zones` (a pre-filtered list) suppresses the fallback entirely."""
        refused = []
        cands = list(zones) if zones is not None else [self.copy_offer(offer)]
        expanded = zones is not None

        def expand():
            nonlocal expanded
            if expanded:
                return
            expanded = True
            tried = {c.zone for c in cands}
            cands.extend(c for c in self.zone_offers(offer) if c.zone not in tried)

        idx = 0
        while True:
            if idx >= len(cands):
                expand()
                if idx >= len(cands):
                    break
            cand = cands[idx]
            idx += 1
            self.move_offer(offer, cand)
            try:
                probe(cand)
                return cand
            except (ProviderQuotaFull, TencentError) as e:
                if not spot_quota(e):
                    raise
            refused.append(cand.zone)
            if idx >= len(cands):
                expand()
            if idx < len(cands):
                log(f"zone {cand.zone}: quota refused; trying {cands[idx].zone} (same region/type)")
        raise ProviderQuotaFull(quota_stop(offer, refused))

    def image(self, region, arch, itype):
        key = (region, arch, itype)
        if key not in self._images:
            d = tccli("cvm", "DescribeImages", region, InstanceType=itype, Limit=100,
                      Filters=[{"Name": "image-type", "Values": ["PUBLIC_IMAGE"]}, {"Name": "platform", "Values": ["Ubuntu"]}])
            img = pick_image(d.get("ImageSet") or [], arch)
            if not img:
                raise InfraError(f"no public Ubuntu {arch} image for {itype} in {region}")
            self._images[key] = img
        return self._images[key]

    # --- network (find_* only read; ensure_* create what is missing) ---

    def find_vpc(self, region):
        vpcs = tccli("vpc", "DescribeVpcs", region, Filters=[{"Name": "vpc-name", "Values": [VPC_NAME]}]).get("VpcSet") or []
        return vpcs[0]["VpcId"] if vpcs else None

    def subnets(self, region, vpc):
        return tccli("vpc", "DescribeSubnets", region, Filters=[{"Name": "vpc-id", "Values": [vpc]}], Limit="100").get("SubnetSet") or []

    def find_subnet(self, region, vpc, zone):
        sn = [s for s in self.subnets(region, vpc) if s.get("Zone") == zone]
        return sn[0]["SubnetId"] if sn else None

    def find_security_group(self, region):
        sgs = tccli("vpc", "DescribeSecurityGroups", region,
                    Filters=[{"Name": "security-group-name", "Values": [SG_NAME]}]).get("SecurityGroupSet") or []
        return sgs[0]["SecurityGroupId"] if sgs else None

    def find_key_pair(self, region, name):
        kps = tccli("cvm", "DescribeKeyPairs", region, Filters=[{"Name": "key-name", "Values": [name]}]).get("KeyPairSet") or []
        return kps[0]["KeyId"] if kps else None

    def ensure_vpc(self, region):
        vpc = self.find_vpc(region)
        if not vpc:
            vpc = tccli("vpc", "CreateVpc", region, VpcName=VPC_NAME, CidrBlock=VPC_CIDR, Tags=TAGS)["Vpc"]["VpcId"]
            log(f"created VPC {VPC_NAME} ({vpc}) in {region}")
        return vpc

    def ensure_subnet(self, region, vpc, zone):
        subnets = self.subnets(region, vpc)
        for s in subnets:
            if s.get("Zone") == zone:
                return s["SubnetId"]
        used = {s["CidrBlock"] for s in subnets}
        cidr = next(c for c in (f"172.16.{i}.0/24" for i in range(256)) if c not in used)
        sn = tccli("vpc", "CreateSubnet", region, VpcId=vpc, SubnetName=f"tlx-verify-{zone}", CidrBlock=cidr, Zone=zone,
                   Tags=TAGS)["Subnet"]["SubnetId"]
        log(f"created subnet {sn} ({cidr}) in {zone}")
        return sn

    def ingress(self, p):
        """One Tencent ingress policy as the shared reconcile reads it
        (cloud/sgpolicy.py)."""
        return sgpolicy.Ingress(protocol=p.get("Protocol", ""), port=str(p.get("Port", "")),
                                cidr=p.get("CidrBlock", ""), description=p.get("PolicyDescription", ""),
                                action=p.get("Action", ""), ref=p.get("PolicyIndex"))

    def ensure_security_group(self, region, ips):
        """The security group allowing tcp 22/443/2222 from `ips` only (and
        all outbound traffic); returns its id.

        The group is shared by every owner on this machine, so the reconcile is
        add-only (cloud/sgpolicy.py): any running lane may borrow an existing
        rule, regardless of its owner tag. Removal is explicit, coordinator-only
        and requires an account with no tagged instances."""
        sg = self.find_security_group(region)
        if not sg:
            try:
                sg = tccli("vpc", "CreateSecurityGroup", region, GroupName=SG_NAME,
                           GroupDescription="tlx cloud_verify: ssh from the operator", Tags=TAGS)["SecurityGroup"]["SecurityGroupId"]
            except TencentError as e:
                if "Unauthorized" in e.code:
                    # (CAM authorizes the security group APIs of the VPC service as cvm:* actions.)
                    raise InfraError(f"{e}: the CAM policy of the `tlx` user needs cvm:CreateSecurityGroup, "
                                     "cvm:CreateSecurityGroupPolicy and cvm:DeleteSecurityGroupPolicy") from e
                raise
            log(f"created security group {SG_NAME} ({sg})")
        pol = tccli("vpc", "DescribeSecurityGroupPolicies", region, SecurityGroupId=sg).get("SecurityGroupPolicySet") or {}
        owner = sgpolicy.owner_now(self.owner)
        change = sgpolicy.plan([self.ingress(p) for p in pol.get("Ingress") or []],
                               sgpolicy.ssh_rules(SSH_PORTS, ips), owner)
        log(sgpolicy.capacity_note(len(pol.get("Ingress") or []), 100, adding=len(change.missing)))
        if change.missing:
            tccli("vpc", "CreateSecurityGroupPolicies", region, SecurityGroupId=sg, SecurityGroupPolicySet={"Ingress": [
                {"Protocol": "TCP", "Port": port, "CidrBlock": cidr, "Action": "ACCEPT",
                 "PolicyDescription": sgpolicy.describe(owner)} for port, cidr in change.missing]})
            log(f"allowed tcp {', '.join(map(str, SSH_PORTS))} from {', '.join(ips)}")
        if note := sgpolicy.kept_note(change):
            log(note)
        if note := sgpolicy.world_note(change):
            warning(note)
        if not any(p.get("Action") == "ACCEPT" and p.get("CidrBlock") == "0.0.0.0/0" for p in pol.get("Egress") or []):
            tccli("vpc", "CreateSecurityGroupPolicies", region, SecurityGroupId=sg, SecurityGroupPolicySet={"Egress": [
                {"Protocol": "ALL", "CidrBlock": "0.0.0.0/0", "Action": "ACCEPT", "PolicyDescription": "cloud_verify"}]})
            log("allowed all outbound traffic")
        return sg

    def prune_ssh_rules(self, regions):
        """Delete attributed rules only; caller holds the idle-account guard."""
        for region in regions:
            sg = self.find_security_group(region)
            if not sg:
                continue
            pol = tccli("vpc", "DescribeSecurityGroupPolicies", region, SecurityGroupId=sg).get("SecurityGroupPolicySet") or {}
            rules = sgpolicy.prunable([self.ingress(p) for p in pol.get("Ingress") or []])
            if rules:
                if not pol.get("Version"):
                    raise InfraError("cannot prune ssh rules without the security-group policy version")
                tccli("vpc", "DeleteSecurityGroupPolicies", region, SecurityGroupId=sg,
                      SecurityGroupPolicySet={"Version": pol["Version"], "Ingress": [{"PolicyIndex": r.ref} for r in rules]})
            log(f"pruned {len(rules)} owner-tagged ssh rule(s) in {region}; ownerless/manual rules retained")

    def allow_ssh(self, m, ips):
        """Add `ips` to the project's own ssh security group of `m` (the
        recovery path when this machine's public address moved after the
        launch; see cloud/allowlist.py). Add only: no ingress rule is removed,
        no group is created, nothing is opened to 0.0.0.0/0 and another
        owner's group is never touched. Returns a line describing what is
        allowed now."""
        sg = self.find_security_group(m.region)
        if not sg:
            raise InfraError(f"security group {SG_NAME} does not exist in {m.region}; create it before ssh can work")
        pol = tccli("vpc", "DescribeSecurityGroupPolicies", m.region, SecurityGroupId=sg).get("SecurityGroupPolicySet") or {}
        have = {(p.get("Port"), p.get("CidrBlock")) for p in pol.get("Ingress") or []
                if p.get("Protocol", "").upper() == "TCP" and p.get("Action") == "ACCEPT"}
        want = sgpolicy.ssh_rules(SSH_PORTS, ips)
        if missing := sorted(want - have):
            tccli("vpc", "CreateSecurityGroupPolicies", m.region, SecurityGroupId=sg, SecurityGroupPolicySet={"Ingress": [
                {"Protocol": "TCP", "Port": port, "CidrBlock": cidr, "Action": "ACCEPT",
                 "PolicyDescription": sgpolicy.describe(sgpolicy.owner_now(self.owner))} for port, cidr in missing]})
        return (f"security group {sg}: ssh allowed from {', '.join(ips)} "
                + (f"({len(missing)} new rule(s))" if missing else "(rules already present)"))

    def allow_hint(self, m, ips):
        """The exact tccli call that would allow `ips` into this project's own
        ssh security group of `m` (printed when the tool cannot make the change
        itself). The group id is resolved read-only; a failure to resolve it
        does not raise."""
        try:
            sg = self.find_security_group(m.region) or f"<the {SG_NAME} security group of {m.region}>"
        except TencentError as e:
            sg = f"<the {SG_NAME} security group of {m.region} ({e})>"
        rules = {"Ingress": [{"Protocol": "TCP", "Port": str(port), "CidrBlock": f"{ip}/32", "Action": "ACCEPT",
                              "PolicyDescription": sgpolicy.describe(sgpolicy.owner_now(self.owner))}
                             for port in SSH_PORTS for ip in ips]}
        return (f"tccli vpc CreateSecurityGroupPolicies --profile {identity()['profile']} --region {m.region} "
                f"--SecurityGroupId {sg} --SecurityGroupPolicySet '{json.dumps(rules, separators=(',', ':'))}'")

    def ensure_key_pair(self, region):
        with open(KEY_FILE + ".pub") as source:
            pub = source.read().strip()
        name = key_name(pub)
        kid = self.find_key_pair(region, name)
        if not kid:
            kid = tccli("cvm", "ImportKeyPair", region, KeyName=name, ProjectId=0, PublicKey=pub,
                        TagSpecification=[{"ResourceType": "keypair", "Tags": TAGS}])["KeyId"]
            log(f"imported key pair {name} ({kid}) in {region}")
        return kid

    def public_ips(self, proxy):
        if proxy:
            raise InfraError("--ssh-proxy is not supported with --provider tencent (sshd also listens on 443 and 2222)")
        return [direct_public_ip()]

    # --- instance lifecycle ---

    def base_params(self, offer, image, sg, subnet, vpc, key_id, token):
        """The parameters RunInstances and InquiryPriceRunInstances share."""
        p = {"Placement": {"Zone": offer.zone, "ProjectId": 0}, "InstanceType": offer.itype, "ImageId": image,
             "SystemDisk": {"DiskType": DISK_TYPE, "DiskSize": getattr(self, "disk_gb", None) or DISK_GB},
             "VirtualPrivateCloud": {"VpcId": vpc, "SubnetId": subnet},
             "InternetAccessible": {"InternetChargeType": "TRAFFIC_POSTPAID_BY_HOUR", "InternetMaxBandwidthOut": 100,
                                    "PublicIpAssigned": True},
             "InstanceCount": 1, "InstanceName": "tlx-verify", "HostName": "tlx-verify",
             "LoginSettings": {"KeyIds": [key_id]}, "SecurityGroupIds": [sg],
             # No monitoring/security/automation agents: a faster boot, nothing else running.
             "EnhancedService": {"SecurityService": {"Enabled": False}, "MonitorService": {"Enabled": False},
                                 "AutomationService": {"Enabled": False}},
             "TagSpecification": [{"ResourceType": "instance", "Tags": TAGS + [{"Key": "tlx-run", "Value": token}, {"Key": "tlx-owner", "Value": self.owner}]}]}
        if offer.market == "spot":
            # Bid the on-demand price: spot is billed at the current spot price, the bid only caps it.
            cap = self._unit_price.get((offer.zone, offer.itype)) or offer.price * 20
            p["InstanceChargeType"] = "SPOTPAID"
            p["InstanceMarketOptions"] = {"MarketType": "spot", "SpotOptions": {"MaxPrice": f"{cap:.4f}", "SpotInstanceType": "one-time"}}
        else:
            p["InstanceChargeType"] = "POSTPAID_BY_HOUR"
        return p

    @staticmethod
    def action_timer(max_minutes, now=None):
        return {"TimerAction": "TerminateInstances", "ActionTime": auto_release_time(
            now or datetime.datetime.now(datetime.timezone.utc), max_minutes, minimum=5, margin=2)}

    def run_params(self, m, image, sg, subnet, vpc, key_id, max_minutes, role, now=None):
        """RunInstances' parameters; `role` the instance's CAM role of the
        self-termination, or "" (the shutdown alone)."""
        now = now or datetime.datetime.now(datetime.timezone.utc)
        p = self.base_params(m.offer, image, sg, subnet, vpc, key_id, m.token)
        p["TagSpecification"][0]["Tags"].append({"Key": DEADLINE_TAG, "Value": deadline(now, max_minutes, role)})
        p["UserData"] = base64.b64encode(user_data(max_minutes, role).encode()).decode()
        if role:
            p["CamRoleName"] = role
        if m.offer.market != "spot":
            # (Refused for spot instances, also later by ImportInstancesActionTimer.)
            p["ActionTimer"] = self.action_timer(max_minutes, now)
        p["ClientToken"] = m.token
        return p

    def describe_plan(self, offer, arch):
        """Read-only checks for --dry-run: balance, image, network, the price
        (InquiryPriceRunInstances) and a RunInstances DryRun when the network
        resources already exist."""
        r = offer.region
        image, image_name = self.image(r, arch, offer.itype)
        lines = [self.balance(), f"image {image} ({image_name})"]
        vpc = self.find_vpc(r)
        sn = self.find_subnet(r, vpc, offer.zone) if vpc else None
        sg = self.find_security_group(r)
        pub = read_public_key()
        kname = key_name(pub) if pub else None
        kid = self.find_key_pair(r, kname) if kname else None
        lines.append(f"VPC {vpc or '(would create ' + VPC_NAME + ')'}, subnet {sn or '(would create)'}, "
                     f"security group {sg or '(would create ' + SG_NAME + ')'}, key pair {kid or '(would import)'}")
        lines.append(f"ssh from {direct_public_ip()} on ports {', '.join(map(str, SSH_PORTS))}")
        if not (vpc and sn and sg and kid):
            # Not without them: a request without VirtualPrivateCloud or
            # SecurityGroupIds makes Tencent create the region's Default-VPC
            # and a `default` security group open to 0.0.0.0/0 (2026-10-05).
            lines.append("WARNING (not a stop condition): InquiryPriceRunInstances and RunInstances DryRun: skipped until the first launch has created "
                         "the VPC, subnet, security group and key pair")
            return lines

        try:
            base = self.base_params(offer, image, sg, sn, vpc, kid, uuid.uuid4().hex)
            pr = tccli("cvm", "InquiryPriceRunInstances", r, **base).get("Price") or {}
            ip, bw = pr.get("InstancePrice") or {}, pr.get("BandwidthPrice") or {}
            lines.append(f"InquiryPriceRunInstances: instance {ip.get('UnitPriceDiscount')} CNY/h "
                         f"(list {ip.get('UnitPrice')}, discount {ip.get('Discount')}%), "
                         f"traffic {bw.get('UnitPriceDiscount')} CNY/GB")
        except TencentError as e:
            if e.fatal:
                raise InfraError(f"InquiryPriceRunInstances: {e} -- {e.hint}", stop_code(str(e))) from e
            raise InfraError(f"launch refused: InquiryPriceRunInstances: {e}") from e
        role = self.selfterm_role()

        # A zone whose subnet does not exist yet cannot be probed read-only
        # (the real launch creates it); only a zone-scoped spot-quota refusal
        # moves to another zone of the same region (2026-10-08). Each candidate
        # is probed with its own zone's subnet: `zone_scan` moves `offer` onto
        # the candidate before the probe, so the zone cannot be read from it.
        base_zone = offer.zone
        cands = self.zone_offers(offer)
        subnets = {c.zone: (sn if c.zone == base_zone else self.find_subnet(r, vpc, c.zone)) for c in cands}
        zones = [c for c in cands if subnets[c.zone]]

        def probe(cand):
            """One read-only dry run of the exact request in `cand`."""
            class M:
                pass
            cand_m = M()
            cand_m.offer, cand_m.token = cand, uuid.uuid4().hex
            try:
                tccli("cvm", "RunInstances", r,
                      **self.run_params(cand_m, image, sg, subnets[cand.zone], vpc, kid, 60, role), DryRun=True)
            except TencentError as e:
                if e.code != "DryRunOperation":
                    raise

        try:
            self.zone_scan(offer, probe, zones)
            lines.append("RunInstances DryRun: would succeed")
        except TencentError as e:
            if role and e.code == ROLE_REFUSED:
                warning("CAM role refused in dry run; launch will fall back to shutdown")
            else:
                raise InfraError(f"launch refused: RunInstances DryRun: {e}", stop_code(str(e))) from e
        if role:
            lines.append(f"dead-man's switch: {self.role_note} (TerminateInstances on itself after --max-minutes), "
                         f"shutdown {SHUTDOWN_GRACE} min later" + (", ActionTimer" if offer.market != "spot" else ""))
        else:
            lines.append("WARNING (not a stop condition): " + self.deadman_warning())
        return lines

    def launch(self, m, ips, fresh_key, max_minutes, arch):
        self.owner = self.owner or current_owner()
        o = m.offer
        kid = self.ensure_key_pair(o.region)
        vpc = self.ensure_vpc(o.region)
        sg = self.ensure_security_group(o.region, ips)
        image, _ = self.image(o.region, arch, o.itype)
        role = self.selfterm_role()
        launched = {}
        while True:
            def probe(cand):
                """Admit and launch `cand` (one zone of this offer's region,
                type and market); a zone-scoped spot-quota refusal hands the
                next zone to zone_scan."""
                sn = self.ensure_subnet(o.region, vpc, cand.zone)
                m.token = uuid.uuid4().hex
                params = self.run_params(m, image, sg, sn, vpc, kid, max_minutes, role)
                self.admit_dry_run(cand, params)
                self.checkpoint()
                m.t_launch = time.time()
                try:
                    launched["response"] = tccli("cvm", "RunInstances", o.region, **params)
                except TencentError as e:
                    if role and e.code == ROLE_REFUSED:
                        raise               # the role is dropped below; the whole launch retries
                    if e.fatal:
                        raise InfraError(f"launch refused in {cand.zone}: {e} -- {e.hint}", stop_code(str(e))) from e
                    if any(s in e.code for s in ZONE_ERRORS):
                        raise SkipOffer("zone", f"no capacity for {o.itype} in {cand.zone} ({e})") from e
                    if spot_quota(e):
                        raise ProviderQuotaFull(f"launch refused in {cand.zone}: {e}") from e
                    if any(s in e.code for s in REGION_ERRORS):
                        raise InfraError(f"quota exceeded in {o.region} ({e})") from e
                    raise InfraError(f"launch failed in {cand.zone}: {e}") from e

            try:
                self.zone_scan(o, probe)
                break
            except TencentError as e:
                if not (role and e.code == ROLE_REFUSED):
                    raise
                # The role exists but the sub-user may not pass it (cam:PassRole).
                self._role, role = "", ""
                self.role_note = f"RunInstances refused CAM role {identity()['selfterm_role']} ({e.code}; cam:PassRole missing?)"
                warning(self.deadman_warning())
        r = launched["response"]
        ids = r.get("InstanceIdSet") or []
        if not ids:
            raise InfraError(f"RunInstances returned no instance: {r}")
        m.id = ids[0]
        m.selfterm_role = role
        if role:
            log(f"dead-man's switch: {m.id} terminates itself {max_minutes} min after boot (CAM role {role}), "
                f"shutdown {SHUTDOWN_GRACE} min later")
        else:
            warning(self.deadman_warning())

    def admit_dry_run(self, offer, params):
        """The provider's own admission of this exact request: RunInstances
        with DryRun (creates nothing). A quota refusal raises
        `ProviderQuotaFull` before the real call, so it launches nothing and
        costs nothing. The account's spot limits are not one number
        (2026-10-08, read-only probes with 7 instances listed: ap-shanghai-5
        refused one more with LimitExceeded.UserSpotQuota while ap-guangzhou
        took two more and refused a third with LimitExceeded.SpotQuota), so
        no local count can stand in for this answer. Other dry-run errors are
        left to the real call, which classifies them."""
        try:
            tccli("cvm", "RunInstances", offer.region, **params, DryRun=True)
        except TencentError as e:
            if e.code in ("DryRunOperation", ROLE_REFUSED):
                return   # (would succeed; or the role: the real call falls back without it)
            if any(s in e.code for s in REGION_ERRORS):
                raise ProviderQuotaFull(f"admission (provider dry run): quota exceeded for {offer.itype} in {offer.zone} "
                                        f"({e}); nothing launched") from e
            if e.fatal:
                raise InfraError(f"launch refused in {offer.zone} (dry run): {e} -- {e.hint}", stop_code(str(e))) from e

    def deadman_script(self, role):
        """Remote shell (run over ssh once the instance is up) that exits
        non-zero unless the dead-man's switch is armed: the user data ran,
        a shutdown is scheduled, and with `role` the self-termination timer
        is active and the role's credentials authenticate a signed request."""
        s = ("cloud-init status --wait >/dev/null 2>&1 || true\n"
             "sudo test -e /run/systemd/shutdown/scheduled || { echo 'no shutdown scheduled'; exit 1; }\n")
        if role:
            s += (f"systemctl is-active --quiet {SELFTERM_UNIT}.timer || {{ echo 'timer {SELFTERM_UNIT} not active'; exit 1; }}\n"
                  f"sudo /usr/bin/python3 {SELFTERM_PATH} check {role}\n")
        return s

    def check_deadman(self, m):
        """Verify the dead-man's switch on the running instance (after ssh is
        up); an InfraError makes the caller terminate it at once."""
        role = getattr(m, "selfterm_role", "")
        p = m.ssh(self.deadman_script(role), check=False, capture=True)
        out = (p.stdout + p.stderr).strip()
        if p.returncode != 0:
            raise InfraError(f"the dead-man's switch of {m.id} is not armed: {out[-500:] or 'exit ' + str(p.returncode)}")
        log("dead-man's switch armed: shutdown scheduled" + (f"; {out.splitlines()[-1]}" if role and out else ""))

    def reap(self, regions=None, now=None):
        """Terminate the tagged instances that are stopped or past their
        tlx-deadline (leftovers of runs whose local process died; a stopped
        spot instance is still billed). Runs before every launch."""
        now = now or datetime.datetime.now(datetime.timezone.utc)
        failed = []
        for region in regions or REGIONS:
            for inst in self.tagged(region, owner=self.owner):
                if not expired(inst, now):
                    continue
                m = type("Leftover", (), {})()
                m.region, m.id = region, inst["InstanceId"]
                log(f"leftover {m.id} in {region} ({inst.get('InstanceState')}, past its dead-man's switch): terminating")
                if not self.terminate(m):
                    failed.append(m.id)
        if failed:
            raise InfraError(f"cannot terminate leftover instance(s) {', '.join(failed)}")

    def find_launched(self, m):
        if m.id or not m.token:
            return m.id
        try:
            d = tccli("cvm", "DescribeInstances", m.region, Filters=[tag_filter("tlx-run", m.token)])
            ids = [i["InstanceId"] for i in d.get("InstanceSet") or []]
            m.id = ids[0] if ids else None
        except Exception as e:  # noqa: BLE001
            warning(f"cannot look up the launched instance: {e}")
        return m.id

    def describe(self, region, iid):
        insts = tccli("cvm", "DescribeInstances", region, InstanceIds=[iid]).get("InstanceSet") or []
        return insts[0] if insts else None

    def instance_state(self, m):
        """'running', 'stopped' or 'gone' (see cloud/allowlist.py): whether an
        ssh failure is the instance's fault or the allowlist's."""
        inst = self.describe(m.region, m.id)
        if not inst:
            return "gone"
        state = inst.get("InstanceState")
        if state in GONE_STATES:
            return "gone"
        return "stopped" if state in STOPPED_STATES else "running"

    def wait_ready(self, m):
        for _ in range(100):
            inst = self.describe(m.region, m.id)
            if inst and inst["InstanceState"] == "RUNNING":
                ips = inst.get("PublicIpAddresses") or []
                if not ips:
                    raise InfraError("the instance has no public IP")
                m.ip = ips[0]
                return
            if inst and inst["InstanceState"] in ("LAUNCH_FAILED", "SHUTDOWN", "TERMINATING"):
                raise InfraError(f"{m.id} is {inst['InstanceState']}")
            time.sleep(3)
        raise InfraError(f"{m.id} did not reach RUNNING")

    def terminate_hint(self, m):
        return f"tccli cvm TerminateInstances --profile {identity()['profile']} --region {m.region} --InstanceIds '[\"{m.id}\"]'"

    def terminate(self, m):
        # A just-created instance may refuse termination until it has left PENDING.
        for _ in range(30):
            try:
                tccli("cvm", "TerminateInstances", m.region, InstanceIds=[m.id])
                break
            except TencentError as e:
                if "NotFound" in e.code or "InvalidInstanceId" in e.code:
                    break
                if e.fatal or stop_code(str(e)) == 20:
                    raise InfraError(f"TerminateInstances refused: {e}" + (f" -- {e.hint}" if e.hint else ""), stop_code(str(e))) from e
                warning(f"TerminateInstances failed ({e}), retrying")
                time.sleep(5)
        else:
            return False
        for _ in range(20):
            inst = self.describe(m.region, m.id)
            if not inst or inst["InstanceState"] in ("SHUTDOWN", "TERMINATING"):
                log(f"terminated {m.id}" + (f" ({inst['InstanceState']})" if inst else ""))
                return True
            time.sleep(3)
        log(f"terminating {m.id} (state: {inst['InstanceState']})")
        return True

    def tagged(self, region, owner=None):
        """DescribeInstances' entries of the instances tagged tlx-project=tlx in `region`."""
        filters = [tag_filter(TAG_KEY, "tlx")]
        if owner is not None:
            filters.append(tag_filter("tlx-owner", owner))
        return self.instances(region, filters)

    def instances(self, region, filters=None):
        """Every DescribeInstances entry in `region` matching `filters` (all pages)."""
        out, offset = [], 0
        while True:
            data = tccli("cvm", "DescribeInstances", region, **({"Filters": filters} if filters else {}), Limit=100, Offset=offset)
            page = data.get("InstanceSet") or []
            out.extend(page)
            offset += len(page)
            if not page or offset >= data.get("TotalCount", offset):
                return out

    def slot_holders(self, regions=None):
        """The provider's view of the account's instances for launch admission:
        every instance of the account in `regions` (default all), tagged or
        not, in any state DescribeInstances still reports (PENDING and
        SHUTDOWN/TERMINATING ones still hold quota), with its owner tag
        ("(untagged)" for an instance not tagged tlx-project=tlx, "(no owner)"
        for a tagged one without tlx-owner), market and dead-man deadline."""
        out = []
        for region in regions or REGIONS:
            for inst in self.instances(region):
                tags = {t.get("Key"): t.get("Value") for t in inst.get("Tags") or []}
                owner = (tags.get("tlx-owner") or "(no owner)") if tags.get(TAG_KEY) == "tlx" else "(untagged)"
                try:
                    due = datetime.datetime.strptime(tags.get(DEADLINE_TAG, ""), "%Y%m%dT%H%MZ").replace(
                        tzinfo=datetime.timezone.utc).timestamp()
                except ValueError:
                    due = None
                out.append(SlotHolder(region, inst.get("InstanceId"), inst.get("InstanceState", "?"), owner,
                                      "spot" if inst.get("InstanceChargeType") == "SPOTPAID" else "on-demand", due))
        return out

    def leftovers(self, regions, owner=None):
        return [(region, i["InstanceId"], i["InstanceState"]) for region in regions for i in self.tagged(region, owner=owner)]

    def packages_script(self):
        # Ubuntu images use Tencent's internal apt mirror (mirrors.tencentyun.com).
        # cloud-init may still be running apt at first login: wait for it.
        return ("set -e\nexport DEBIAN_FRONTEND=noninteractive NEEDRESTART_SUSPEND=1\n"
                "cloud-init status --wait >/dev/null 2>&1 || true\n"
                "sudo -E apt-get -o DPkg::Lock::Timeout=600 update -qq >/dev/null\n"
                "sudo -E apt-get -o DPkg::Lock::Timeout=600 install -y -qq git gcc clang make rsync python3 curl xz-utils >/dev/null\n")

    def java_script(self):
        # TLC for the bench mode: the headless JRE 21 from the same mirror.
        return ("sudo -E apt-get -o DPkg::Lock::Timeout=600 install -y -qq --no-install-recommends openjdk-21-jre-headless >/dev/null\n"
                "java -version 2>&1 | head -1\n")
