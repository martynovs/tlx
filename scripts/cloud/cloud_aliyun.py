"""Alibaba Cloud ECS provider of scripts/cloud_verify.py (`--provider aliyun`, the default).

Spot instance (SpotAsPriceGo, pay-as-you-go per second) running Ubuntu 24.04:
Yitian arm64 (ecs.c8y / ecs.g8y) or AMD x86_64 (ecs.c8a / ecs.g8a), the
cheapest type and zone with stock that DescribeAvailableResource and
DescribeSpotPriceHistory report. Default region cn-hangzhou (alternatives
cn-shanghai, cn-beijing; cn-hongkong, ap-northeast-1, ap-southeast-1 with
--region).

Safety rules:
  - Only the aliyun CLI profile and RAM user of the local cloud config
    (cloud.cloud_common.cloud_identity, docs/cloud-verification.md; the identity is
    checked before anything else). Never the `default` profile.
  - The aliyun CLI and ssh run without proxy variables (a stable source
    address; mainland regions are reached directly).
  - Every instance carries the tag Project=tlx from RunInstances on (the RAM
    policy only allows deleting tagged instances), plus a per-run tag
    tlx-run=<token> to find it after an interrupted launch.
  - Dead-man's switch: AutoReleaseTime (now + max(max-minutes, 32 minutes):
    Aliyun requires at least 30 minutes ahead) and `shutdown -h +N` from the
    user data. DeleteInstance with Force=true in the caller's `finally`.
  - The security group `tlx-verify-ssh` allows tcp 22, 443 and 2222 from this
    machine's public address only (the local network blocks outbound port
    22: the user data starts a second sshd on 443 and 2222). "This machine's
    address" is the one at launch time, and it can move under a running run:
    `allow_ssh` then adds the current one to that same group - add only, never
    opened to 0.0.0.0/0, no other owner's group touched (cloud/allowlist.py).
    The group is shared by every owner on this machine, so the reconcile a
    launch runs is per owner as well (cloud/sgpolicy.py): it revokes only the
    rules this owner's own launches wrote, never another owner's and never a
    rule that names no owner.
"""
import base64, datetime, json, os, re, time, uuid

from cloud.cloud_common import AccountError, CloudStop, current_owner, stop_code, warning, InfraError, KEY_FILE, Offer, SkipOffer, auto_release_time, deadman_user_data, cloud_identity, direct_public_ip, latest_prices, log

from cloud.transport import capture_json, cli_env as transport_env
from cloud import sgpolicy

MAINLAND = ["cn-hangzhou", "cn-shanghai", "cn-beijing"]
REGIONS = MAINLAND + ["cn-hongkong", "ap-northeast-1", "ap-southeast-1"]
# Default types per arch (16 vCPU; the cheapest offer of these wins).
TYPES = {"arm64": ["ecs.c8y.4xlarge", "ecs.g8y.4xlarge"], "x86_64": ["ecs.c8a.4xlarge", "ecs.g8a.4xlarge"]}
IMAGE_FAMILY = {"arm64": "acs:ubuntu_24_04_arm64", "x86_64": "acs:ubuntu_24_04_x64"}
KEY_NAME = "tlx-verify"
SG_NAME = "tlx-verify-ssh"
VPC_NAME = "tlx-verify"
VPC_CIDR = "172.16.0.0/16"
SSH_PORTS = [22, 443, 2222]
DISK_GB = 40
TAG_ARGS = ["--Tag.1.Key", "Project", "--Tag.1.Value", "tlx"]
# Launch refusals that mean "this zone (or type) has nothing now".
ZONE_ERRORS = ("NoStock", "NotSupported", "NotOnSale", "ResourceNotAvailable", "InvalidZoneId", "Zone.", "OperationDenied.NoStock")
# Status values that mean the instance cannot be ssh'ed into any more
# (instance_state, cloud/allowlist.py): released, or not running. Pending and
# Starting exist and are booting.
GONE_STATES = ("Deleted",)
STOPPED_STATES = ("Stopped", "Stopping")


class AliyunError(Exception):
    def __init__(self, code, message, args):
        super().__init__(f"{code}: {message}")
        self.code, self.message, self.cli_args = code, message, args


def cli_env():
    """The aliyun CLI's environment: no proxy, and no credential variables
    that could take precedence over --profile."""
    return transport_env(("ALIBABA_CLOUD_", "ALICLOUD_", "ALIYUN_"))


def parse_error(out):
    """(code, message) of a failed aliyun CLI call: a JSON object
    {"error_code", "message", ...} (CLI 3.5) or "ErrorCode: ...\\nMessage: ..." text."""
    if (i := out.find("{")) >= 0:
        try:
            d = json.loads(out[i:])
            if isinstance(d, dict) and (d.get("error_code") or d.get("Code")):
                return d.get("error_code") or d["Code"], d.get("message") or d.get("Message", "")
        except ValueError:
            pass
    code = m.group(1) if (m := re.search(r"ErrorCode:\s*(\S+)", out)) else "CliError"
    msg = m.group(1).strip() if (m := re.search(r"Message:\s*(.*)", out)) else out.strip()[-500:]
    return code, msg


def identity():
    """The configured {"arn", "profile"} (exits when there is none)."""
    return cloud_identity("aliyun")


def aliyun(product, api, region=None, *args):
    """Run one aliyun CLI call with the configured profile; returns the parsed JSON."""
    cmd = ["aliyun", product, api, "--profile", identity()["profile"]]
    if region:
        cmd += ["--region", region, "--RegionId", region]
    cmd += [str(a) for a in args]
    return capture_json(cmd, env=cli_env(), missing="the aliyun CLI is not installed",
                        error=lambda out: AliyunError(*parse_error(out), cmd))


def items(d, *path):
    for k in path:
        d = (d or {}).get(k)
    return d or []


class AliyunProvider:
    name = "aliyun"
    currency = "CNY"
    ssh_user = "root"
    home = "/root"
    ssh_ports = SSH_PORTS
    default_regions = ["cn-hangzhou"]
    all_regions = REGIONS
    label_suffix = "aliyun"
    default_ssh_proxy = "none"
    min_lifetime_minutes = 32   # AutoReleaseTime: at least 30 minutes ahead
    # launch() calls self.checkpoint() immediately before RunInstances (set by cloud_verify).
    calls_checkpoint = True

    def checkpoint(self):
        pass

    def __init__(self):
        self.owner = current_owner(required=False)
        self._sg_cache = {}
        identity()

    def check_account(self):
        want = identity()
        try:
            ident = aliyun("sts", "GetCallerIdentity")
        except AliyunError as e:
            raise AccountError(f"aliyun profile {want['profile']!r}: {e}") from e
        if ident.get("Arn") != want["arn"]:
            raise AccountError(f"cloud_verify: aliyun profile {want['profile']!r} is {ident.get('Arn')}, expected {want['arn']}; aborting")
        return f"{ident['Arn']} ({ident.get('IdentityType')})"

    def mainland(self, region):
        return region.startswith("cn-") and region != "cn-hongkong"

    def default_types(self, arch, tier="quick"):
        return TYPES[arch]

    def vcpus(self, types):
        args = []
        for i, t in enumerate(types, 1):
            args += [f"--InstanceTypes.{i}", t]
        d = aliyun("ecs", "DescribeInstanceTypes", None, *args)
        out = {}
        for t in items(d, "InstanceTypes", "InstanceType"):
            out[t["InstanceTypeId"]] = (int(t["CpuCoreCount"]), {"ARM": "arm64", "X86": "x86_64"}.get(t.get("CpuArchitecture"), "?"))
        return out

    def stock(self, region, itype):
        """{zone: status category} where `itype` can be launched as spot now."""
        d = aliyun("ecs", "DescribeAvailableResource", region, "--DestinationResource", "InstanceType",
                   "--InstanceChargeType", "PostPaid", "--SpotStrategy", "SpotAsPriceGo", "--InstanceType", itype)
        out = {}
        for z in items(d, "AvailableZones", "AvailableZone"):
            for r in items(z, "AvailableResources", "AvailableResource"):
                for s in items(r, "SupportedResources", "SupportedResource"):
                    if s.get("Value") == itype and s.get("Status") == "Available":
                        out[z["ZoneId"]] = s.get("StatusCategory", "")
        return out

    def offers(self, types, arch, regions):
        info = self.vcpus(types)
        out = []
        for itype in types:
            if itype not in info:
                raise CloudStop(f"cloud_verify: unknown instance type {itype}")
            vcpus, a = info[itype]
            if a != arch:
                raise CloudStop(f"cloud_verify: {itype} is {a}, not {arch} (use --arch)")
            for region in regions:
                zones = self.stock(region, itype)
                d = aliyun("ecs", "DescribeSpotPriceHistory", region, "--NetworkType", "vpc", "--InstanceType", itype, "--OSType", "linux")
                recs = [{"zone": p["ZoneId"], "itype": itype, "price": p["SpotPrice"],
                         "time": datetime.datetime.strptime(p["Timestamp"], "%Y-%m-%dT%H:%M:%SZ").replace(tzinfo=datetime.timezone.utc)}
                        for p in items(d, "SpotPrices", "SpotPriceType")]
                for (zone, _), price in latest_prices(recs).items():
                    if zone in zones:
                        # Zones reported "ClosedWithStock" etc. are tried after those "WithStock".
                        out.append(Offer(price, region, zone, itype, vcpus, zones[zone] != "WithStock"))
        return out

    def image(self, region, arch):
        d = aliyun("ecs", "DescribeImages", region, "--ImageFamily", IMAGE_FAMILY[arch], "--ImageOwnerAlias", "system",
                   "--Architecture", arch, "--Status", "Available")
        imgs = sorted(items(d, "Images", "Image"), key=lambda i: i["CreationTime"])
        if not imgs:
            raise InfraError(f"no {IMAGE_FAMILY[arch]} image in {region}")
        return imgs[-1]["ImageId"]

    # --- network (find_* only read; ensure_* create what is missing) ---

    def find_vpc(self, region):
        vpcs = items(aliyun("vpc", "DescribeVpcs", region, "--VpcName", VPC_NAME), "Vpcs", "Vpc")
        return vpcs[0]["VpcId"] if vpcs else None

    def find_vswitch(self, region, vpc, zone):
        vs = items(aliyun("vpc", "DescribeVSwitches", region, "--VpcId", vpc, "--ZoneId", zone, "--PageSize", 50), "VSwitches", "VSwitch")
        return vs[0]["VSwitchId"] if vs else None

    def find_security_group(self, region, vpc):
        sgs = items(aliyun("ecs", "DescribeSecurityGroups", region, "--VpcId", vpc, "--SecurityGroupName", SG_NAME), "SecurityGroups", "SecurityGroup")
        return sgs[0]["SecurityGroupId"] if sgs else None

    def find_key_pair(self, region):
        kps = items(aliyun("ecs", "DescribeKeyPairs", region, "--KeyPairName", KEY_NAME), "KeyPairs", "KeyPair")
        return kps[0] if kps else None

    def wait_available(self, api, region, key, ident, list_path):
        for _ in range(60):
            objs = items(aliyun("vpc", api, region, f"--{key}", ident), *list_path)
            if objs and objs[0].get("Status") == "Available":
                return
            time.sleep(2)
        raise InfraError(f"{ident} did not become available")

    def ensure_vpc(self, region):
        vpc = self.find_vpc(region)
        if not vpc:
            vpc = aliyun("vpc", "CreateVpc", region, "--VpcName", VPC_NAME, "--CidrBlock", VPC_CIDR,
                         "--Description", "tlx cloud_verify", *TAG_ARGS)["VpcId"]
            log(f"created VPC {VPC_NAME} ({vpc}) in {region}")
            self.wait_available("DescribeVpcs", region, "VpcId", vpc, ("Vpcs", "Vpc"))
        return vpc

    def ensure_vswitch(self, region, vpc, zone):
        vs = self.find_vswitch(region, vpc, zone)
        if vs:
            return vs
        used = {v["CidrBlock"] for v in items(aliyun("vpc", "DescribeVSwitches", region, "--VpcId", vpc, "--PageSize", 50), "VSwitches", "VSwitch")}
        cidr = next(c for c in (f"172.16.{i}.0/24" for i in range(256)) if c not in used)
        vs = aliyun("vpc", "CreateVSwitch", region, "--VpcId", vpc, "--ZoneId", zone, "--CidrBlock", cidr,
                    "--VSwitchName", f"tlx-verify-{zone}", *TAG_ARGS)["VSwitchId"]
        log(f"created vSwitch {vs} ({cidr}) in {zone}")
        self.wait_available("DescribeVSwitches", region, "VSwitchId", vs, ("VSwitches", "VSwitch"))
        return vs

    def ingress(self, p):
        """One Aliyun ingress permission as the shared reconcile reads it
        (cloud/sgpolicy.py)."""
        return sgpolicy.Ingress(protocol=p.get("IpProtocol", ""), port=p.get("PortRange", ""),
                                cidr=p.get("SourceCidrIp", ""), description=p.get("Description", ""),
                                action=p.get("Policy", "ACCEPT"), ref=p.get("SecurityGroupRuleId"))

    def ensure_security_group(self, region, vpc, ips):
        """The security group allowing tcp 22/443/2222 from `ips` only; returns its id.

        The group is shared by every owner on this machine, so the reconcile is
        add-only (cloud/sgpolicy.py): any running lane may borrow an existing
        rule, regardless of its owner tag. Removal is explicit, coordinator-only
        and requires an account with no tagged instances."""
        sg = self.find_security_group(region, vpc)
        if not sg:
            sg = aliyun("ecs", "CreateSecurityGroup", region, "--VpcId", vpc, "--SecurityGroupName", SG_NAME,
                        "--Description", "tlx cloud_verify: ssh from the operator", *TAG_ARGS)["SecurityGroupId"]
            log(f"created security group {SG_NAME} ({sg})")
        perms = items(aliyun("ecs", "DescribeSecurityGroupAttribute", region, "--SecurityGroupId", sg, "--Direction", "ingress"),
                      "Permissions", "Permission")
        owner = sgpolicy.owner_now(self.owner)
        change = sgpolicy.plan([self.ingress(p) for p in perms],
                               sgpolicy.ssh_rules(SSH_PORTS, ips, fmt=lambda p: f"{p}/{p}"), owner)
        log(sgpolicy.capacity_note(len(perms), 200, adding=len(change.missing), combined=True))
        for port, cidr in change.missing:
            aliyun("ecs", "AuthorizeSecurityGroup", region, "--SecurityGroupId", sg, "--IpProtocol", "tcp",
                   "--PortRange", port, "--SourceCidrIp", cidr, "--Description", sgpolicy.describe(owner))
        if change.missing:
            log(f"allowed tcp {', '.join(map(str, SSH_PORTS))} from {', '.join(ips)}")
        if note := sgpolicy.kept_note(change):
            log(note)
        if note := sgpolicy.world_note(change):
            warning(note)
        return sg

    def prune_ssh_rules(self, regions):
        """Delete attributed rules only; caller holds the idle-account guard."""
        for region in regions:
            vpc = self.find_vpc(region)
            sg = self.find_security_group(region, vpc) if vpc else None
            if not sg:
                continue
            perms = items(aliyun("ecs", "DescribeSecurityGroupAttribute", region, "--SecurityGroupId", sg,
                                "--Direction", "ingress"), "Permissions", "Permission")
            rules = sgpolicy.prunable([self.ingress(p) for p in perms])
            if rules:
                args = [v for i, r in enumerate(rules, 1) for v in (f"--SecurityGroupRuleId.{i}", r.ref)]
                aliyun("ecs", "RevokeSecurityGroup", region, "--SecurityGroupId", sg, *args)
            log(f"pruned {len(rules)} owner-tagged ssh rule(s) in {region}; ownerless/manual rules retained")

    def allow_ssh(self, m, ips):
        """Add `ips` to the project's own ssh security group of `m` (the
        recovery path when this machine's public address moved after the
        launch; see cloud/allowlist.py). Add only: no ingress rule is removed,
        no group is created, nothing is opened to 0.0.0.0/0 and another
        owner's group is never touched. Returns a line describing what is
        allowed now."""
        vpc = self.find_vpc(m.region)
        sg = self.find_security_group(m.region, vpc) if vpc else None
        if not sg:
            raise InfraError(f"security group {SG_NAME} does not exist in {m.region}; create it before ssh can work")
        perms = items(aliyun("ecs", "DescribeSecurityGroupAttribute", m.region, "--SecurityGroupId", sg,
                             "--Direction", "ingress"), "Permissions", "Permission")
        have = {(p.get("PortRange"), p.get("SourceCidrIp")) for p in perms if p.get("IpProtocol", "").upper() == "TCP"}
        want = sgpolicy.ssh_rules(SSH_PORTS, ips, fmt=lambda p: f"{p}/{p}")
        missing = sorted(want - have)
        for port, cidr in missing:
            aliyun("ecs", "AuthorizeSecurityGroup", m.region, "--SecurityGroupId", sg, "--IpProtocol", "tcp",
                   "--PortRange", port, "--SourceCidrIp", cidr, "--Description",
                   sgpolicy.describe(sgpolicy.owner_now(self.owner)))
        return (f"security group {sg}: ssh allowed from {', '.join(ips)} "
                + (f"({len(missing)} new rule(s))" if missing else "(rules already present)"))

    def allow_hint(self, m, ips):
        """The exact aliyun CLI call that would allow `ips` into this project's
        own ssh security group of `m` (printed when the tool cannot make the
        change itself). The group id is resolved read-only; a failure to
        resolve it does not raise."""
        try:
            vpc = self.find_vpc(m.region)
            sg = (self.find_security_group(m.region, vpc) if vpc else None) or f"<the {SG_NAME} security group of {m.region}>"
        except AliyunError as e:
            sg = f"<the {SG_NAME} security group of {m.region} ({e})>"
        calls = "\n    ".join(
            f"aliyun ecs AuthorizeSecurityGroup --profile {identity()['profile']} --region {m.region} --RegionId {m.region} "
            f"--SecurityGroupId {sg} --IpProtocol tcp --PortRange {port}/{port} --SourceCidrIp {ip}/32"
            for port in SSH_PORTS for ip in ips)
        return calls

    def ensure_key_pair(self, region, fresh):
        kp = self.find_key_pair(region)
        if kp and not fresh:
            return
        if kp:
            aliyun("ecs", "DeleteKeyPairs", region, "--KeyPairNames", json.dumps([KEY_NAME]))  # the local key was regenerated
        with open(KEY_FILE + ".pub") as stream:
            public_key = stream.read().strip()
        aliyun("ecs", "ImportKeyPair", region, "--KeyPairName", KEY_NAME, "--PublicKeyBody", public_key, *TAG_ARGS)
        log(f"imported key pair {KEY_NAME} in {region}")

    def public_ips(self, proxy):
        if proxy:
            raise InfraError("--ssh-proxy is not supported with --provider aliyun (sshd also listens on 443 and 2222)")
        return [direct_public_ip()]

    # --- instance lifecycle ---

    def run_args(self, m, image, sg, vswitch, max_minutes, now=None):
        o = m.offer
        user_data = deadman_user_data(max_minutes, extra_ssh_ports=[p for p in SSH_PORTS if p != 22])
        return ["--ZoneId", o.zone, "--InstanceType", o.itype, "--ImageId", image, "--SecurityGroupId", sg,
                "--VSwitchId", vswitch, "--KeyPairName", KEY_NAME, "--InstanceName", "tlx-verify", "--HostName", "tlx-verify",
                "--InstanceChargeType", "PostPaid", "--SpotStrategy", "SpotAsPriceGo", "--SpotInterruptionBehavior", "Terminate",
                "--InternetChargeType", "PayByTraffic", "--InternetMaxBandwidthOut", 100,
                "--SystemDisk.Category", "cloud_essd", "--SystemDisk.PerformanceLevel", "PL0", "--SystemDisk.Size", DISK_GB,
                "--UserData", base64.b64encode(user_data.encode()).decode(),
                "--AutoReleaseTime", auto_release_time(now or datetime.datetime.now(datetime.timezone.utc), max_minutes),
                "--Amount", 1, "--ClientToken", m.token, *TAG_ARGS, "--Tag.2.Key", "tlx-run", "--Tag.2.Value", m.token,
                "--Tag.3.Key", "Owner", "--Tag.3.Value", self.owner]

    def describe_plan(self, offer, arch):
        """Read-only checks for --dry-run: image, network, and a RunInstances
        DryRun when the network resources already exist."""
        r = offer.region
        lines = [f"image {self.image(r, arch)}"]
        vpc = self.find_vpc(r)
        vs = self.find_vswitch(r, vpc, offer.zone) if vpc else None
        sg = self.find_security_group(r, vpc) if vpc else None
        kp = self.find_key_pair(r)
        lines.append(f"VPC {vpc or '(would create ' + VPC_NAME + ')'}, vSwitch {vs or '(would create)'}, "
                     f"security group {sg or '(would create ' + SG_NAME + ')'}, key pair {KEY_NAME if kp else '(would import)'}")
        lines.append(f"ssh from {direct_public_ip()} on ports {', '.join(map(str, SSH_PORTS))}")
        if vpc and vs and sg and kp:
            class M:
                pass
            m = M()
            m.offer, m.token = offer, uuid.uuid4().hex
            try:
                aliyun("ecs", "RunInstances", r, *self.run_args(m, self.image(r, arch), sg, vs, 60), "--DryRun", "true")
                lines.append("RunInstances DryRun: accepted?")
            except AliyunError as e:
                if e.code != "DryRunOperation":
                    raise InfraError(f"launch refused: RunInstances DryRun: {e}") from e
                lines.append("RunInstances DryRun: would succeed")
        return lines

    def launch(self, m, ips, fresh_key, max_minutes, arch):
        self.owner = self.owner or current_owner()
        o = m.offer
        self.ensure_key_pair(o.region, fresh_key)
        vpc = self.ensure_vpc(o.region)
        vs = self.ensure_vswitch(o.region, vpc, o.zone)
        sg = self.ensure_security_group(o.region, vpc, ips)
        image = self.image(o.region, arch)
        m.token = uuid.uuid4().hex
        self.checkpoint()
        m.t_launch = time.time()
        try:
            r = aliyun("ecs", "RunInstances", o.region, *self.run_args(m, image, sg, vs, max_minutes))
        except AliyunError as e:
            if any(s in e.code for s in ZONE_ERRORS):
                raise SkipOffer("zone", f"no capacity for {o.itype} in {o.zone} ({e})") from e
            if "Quota" in e.code or "Limit" in e.code:
                raise InfraError(f"quota exceeded in {o.region} ({e})") from e
            if "NotEnoughBalance" in e.code:
                raise InfraError(f"{e}: pay-as-you-go instances need an available balance of at least 100 CNY; "
                                 "top up the account") from e
            raise InfraError(f"launch failed in {o.zone}: {e}") from e
        ids = items(r, "InstanceIdSets", "InstanceIdSet")
        if not ids:
            raise InfraError(f"RunInstances returned no instance: {r}")
        m.id = ids[0]

    def find_launched(self, m):
        if m.id or not m.token:
            return m.id
        try:
            d = aliyun("ecs", "DescribeInstances", m.region, "--Tag.1.Key", "tlx-run", "--Tag.1.Value", m.token)
            ids = [i["InstanceId"] for i in items(d, "Instances", "Instance")]
            m.id = ids[0] if ids else None
        except Exception as e:  # noqa: BLE001
            warning(f"cannot look up the launched instance: {e}")
        return m.id

    def describe(self, region, iid):
        d = aliyun("ecs", "DescribeInstances", region, "--InstanceIds", json.dumps([iid]))
        insts = items(d, "Instances", "Instance")
        return insts[0] if insts else None

    def instance_state(self, m):
        """'running', 'stopped' or 'gone' (see cloud/allowlist.py): whether an
        ssh failure is the instance's fault or the allowlist's."""
        inst = self.describe(m.region, m.id)
        if not inst or inst.get("Status") in GONE_STATES:
            return "gone"
        return "stopped" if inst.get("Status") in STOPPED_STATES else "running"

    def wait_ready(self, m):
        for _ in range(100):
            inst = self.describe(m.region, m.id)
            if inst and inst["Status"] == "Running":
                ips = items(inst, "PublicIpAddress", "IpAddress")
                if not ips:
                    raise InfraError("the instance has no public IP")
                m.ip = ips[0]
                return
            time.sleep(3)
        raise InfraError(f"{m.id} did not reach Running")

    def terminate_hint(self, m):
        return (f"aliyun ecs DeleteInstance --profile {identity()['profile']} --region {m.region} --RegionId {m.region} "
                f"--InstanceId {m.id} --Force true")

    def terminate(self, m):
        # A just-created instance may refuse deletion until it has left Pending.
        for attempt in range(30):
            try:
                aliyun("ecs", "DeleteInstance", m.region, "--InstanceId", m.id, "--Force", "true")
                break
            except AliyunError as e:
                if stop_code(str(e)) in (20, 21, 22, 24):
                    raise InfraError(f"DeleteInstance refused: {e}; AutoReleaseTime remains armed", stop_code(str(e))) from e
                if "NotFound" in e.code:
                    break
                warning(f"DeleteInstance failed ({e}), retrying")
                time.sleep(5)
        else:
            return False
        for _ in range(20):
            inst = self.describe(m.region, m.id)
            if not inst:
                log(f"released {m.id}")
                return True
            time.sleep(3)
        log(f"deleted {m.id} (state: {inst['Status']})")
        return True

    def leftovers(self, regions, owner=None):
        out = []
        tags = TAG_ARGS + (["--Tag.2.Key", "Owner", "--Tag.2.Value", owner] if owner is not None else [])
        for region in regions:
            page = 1
            while True:
                d = aliyun("ecs", "DescribeInstances", region, *tags, "--PageSize", 100, "--PageNumber", page)
                insts = items(d, "Instances", "Instance")
                out += [(region, i["InstanceId"], i["Status"]) for i in insts]
                if not insts or page * 100 >= d.get("TotalCount", len(insts)):
                    break
                page += 1
        return out

    def packages_script(self):
        # Ubuntu images use Aliyun's apt mirrors. cloud-init may still be
        # running apt at first login: wait for it.
        return ("set -e\nexport DEBIAN_FRONTEND=noninteractive NEEDRESTART_SUSPEND=1\n"
                "cloud-init status --wait >/dev/null 2>&1 || true\n"
                "apt-get -o DPkg::Lock::Timeout=600 update -qq >/dev/null\n"
                "apt-get -o DPkg::Lock::Timeout=600 install -y -qq git gcc clang make rsync python3 curl xz-utils >/dev/null\n")

    def java_script(self):
        # TLC for the bench mode: the headless JRE 21 from the same mirror.
        return ("apt-get -o DPkg::Lock::Timeout=600 install -y -qq --no-install-recommends openjdk-21-jre-headless >/dev/null\n"
                "java -version 2>&1 | head -1\n")
