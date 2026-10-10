"""AWS EC2 provider of scripts/cloud_verify.py (`--provider aws`).

Paused since 2026-10-04 (the account was restricted after several spot
launches within a short time through a proxy whose exit address varies).
Kept working for when the account is usable again.

Spot instance (Amazon Linux 2023; default c7g.16xlarge for arm64,
c7a.16xlarge for x86_64) in the cheapest AZ of ap-northeast-1 /
ap-southeast-1. Safety: only the profile and account of the local cloud
config (cloud.cloud_common.cloud_identity, docs/cloud-verification.md); every
resource tagged Project=tlx (the IAM policy only lets this user terminate
instances with that tag); shutdown behavior `terminate` plus
`shutdown -h +<max-minutes>` from the user data. SSH is open to this
machine's public IP only (security group `tlx-verify-ssh`); with an HTTP
proxy (https_proxy) ssh tunnels through it and the group allows the proxy's
exit addresses instead (--ssh-proxy). Those are the addresses at launch time,
and they can move under a running run: `allow_ssh` then adds the current ones
to that same group - add only, never opened to 0.0.0.0/0, no other owner's
group touched (cloud/allowlist.py). The group is shared by every owner on this
machine, so the reconcile a launch runs is add-only as well
(cloud/sgpolicy.py). Only an idle-account coordinator prune may remove
attributed rules.
"""
import datetime, re, sys, time, urllib.request, uuid

from cloud.cloud_common import AccountError, CloudStop, current_owner, stop_code, warning, InfraError, KEY_FILE, Offer, SkipOffer, cloud_identity, deadman_user_data, log
from cloud import sgpolicy, deadline

REGIONS = ["ap-northeast-1", "ap-southeast-1"]
SPOT_VCPU_QUOTA = "L-34B43A08"  # All Standard (A, C, D, H, I, M, R, T, Z) Spot Instance Requests
# arch: (AMI SSM parameter, default instance type)
ARCHES = {
    "arm64": ("/aws/service/ami-amazon-linux-latest/al2023-ami-kernel-default-arm64", "c7g.16xlarge"),
    "x86_64": ("/aws/service/ami-amazon-linux-latest/al2023-ami-kernel-default-x86_64", "c7a.16xlarge"),
}
KEY_NAME = "tlx-verify"
SG_NAME = "tlx-verify-ssh"
TAG = {"Key": "Project", "Value": "tlx"}
DISK_GB = 40
# Instance state names that mean the instance cannot be ssh'ed into any more
# (instance_state, cloud/allowlist.py). Pending and running are up or coming up.
GONE_STATES = ("terminated", "shutting-down")
STOPPED_STATES = ("stopped", "stopping")


def revoke_permissions(rules):
    """The `IpPermissions` that revoke `rules`: AWS names an ingress rule by
    its protocol, its port range and its address, so the stale rules of one
    port range become one permission. The description is not part of a rule's
    identity and is left out."""
    grouped = {}
    for r in rules:
        protocol, from_port, to_port, cidr = r.ref
        grouped.setdefault((protocol, from_port, to_port), []).append(cidr)
    return [{"IpProtocol": protocol, **({} if from_port is None else {"FromPort": from_port, "ToPort": to_port}),
             "IpRanges": [{"CidrIp": c} for c in sorted(cidrs)]}
            for (protocol, from_port, to_port), cidrs in grouped.items()]


class AwsProvider:
    name = "aws"
    currency = "USD"
    ssh_user = "ec2-user"
    home = "/home/ec2-user"
    ssh_ports = [22]
    default_regions = REGIONS
    all_regions = REGIONS
    label_suffix = "aws"
    default_ssh_proxy = "auto"
    min_lifetime_minutes = 0

    def __init__(self):
        self.owner = current_owner(required=False)
        self._sess = None
        ident = cloud_identity("aws")
        self.profile, self.account = ident["profile"], ident["account"]

    def sess(self):
        if self._sess is None:
            try:
                import boto3
            except ImportError as e:
                raise AccountError("boto3 is not installed") from e
            self._sess = boto3.Session(profile_name=self.profile)
        return self._sess

    def ec2(self, region):
        if deadline.active():
            from botocore.config import Config
            # One attempt: connect + read share the remaining budget. A later
            # API call recomputes it, rather than reusing a stale SDK timeout.
            return deadline.Client(lambda budget: self.sess().client("ec2", region_name=region,
                    config=Config(connect_timeout=budget / 2, read_timeout=budget / 2,
                                  retries={"total_max_attempts": 1})))
        return self.sess().client("ec2", region_name=region)

    def check_account(self):
        try:
            ident = self.sess().client("sts", region_name=REGIONS[0]).get_caller_identity()
        except AccountError:
            raise
        except Exception as e:  # noqa: BLE001 - missing profile, invalid or disabled key
            raise AccountError(f"profile {self.profile!r}: {type(e).__name__}: {e}") from e
        if ident["Account"] != self.account:
            raise AccountError(f"cloud_verify: profile {self.profile!r} is account {ident['Account']}, expected {self.account}; aborting")
        return f"account {ident['Account']} ({ident['Arn']})"

    def mainland(self, region):
        return False

    def default_types(self, arch, tier="quick"):
        return [ARCHES[arch][1]]

    def spot_quota(self, region):
        try:
            return self.sess().client("service-quotas", region_name=region).get_service_quota(
                ServiceCode="ec2", QuotaCode=SPOT_VCPU_QUOTA)["Quota"]["Value"]
        except Exception as e:  # noqa: BLE001 - the quota is advisory here
            warning(f"{region}: cannot read the spot vCPU quota ({e})")
            return None

    def offers(self, types, arch, regions):
        out = []
        for itype in types:
            info = self.ec2(regions[0]).describe_instance_types(InstanceTypes=[itype])["InstanceTypes"][0]
            vcpus = info["VCpuInfo"]["DefaultVCpus"]
            if arch not in info["ProcessorInfo"]["SupportedArchitectures"]:
                raise CloudStop(f"cloud_verify: {itype} is not {arch} (use --arch)")
            # The quota the API reports is advisory (it has read 0 while
            # launches within 8 vCPUs succeeded): regions that seem too small
            # are tried last, and RunInstances has the final word (a refused
            # launch costs nothing).
            low = set()
            for region in regions:
                q = self.spot_quota(region)
                if q is not None and q < vcpus:
                    low.add(region)
                (warning if region in low else log)(f"{region}: spot vCPU quota {q if q is not None else '?'}" + (f" < {vcpus} vCPUs of {itype}: tried last" if region in low else ""))
            if low == set(regions):
                warning(f"{itype} needs {vcpus} vCPUs, more than the reported spot quota ({SPOT_VCPU_QUOTA}); "
                    "the launch will probably be refused")
            now = datetime.datetime.now(datetime.timezone.utc)
            for region in regions:
                h = self.ec2(region).describe_spot_price_history(
                    InstanceTypes=[itype], ProductDescriptions=["Linux/UNIX"], StartTime=now)["SpotPriceHistory"]
                latest = {}
                for p in h:
                    if p["AvailabilityZone"] not in latest or p["Timestamp"] > latest[p["AvailabilityZone"]]["Timestamp"]:
                        latest[p["AvailabilityZone"]] = p
                out += [Offer(float(p["SpotPrice"]), region, az, itype, vcpus, region in low) for az, p in latest.items()]
        return out

    def ami(self, region, arch):
        image = self.sess().client("ssm", region_name=region).get_parameter(Name=ARCHES[arch][0])["Parameter"]["Value"]
        root = self.ec2(region).describe_images(ImageIds=[image])["Images"][0]["RootDeviceName"]
        return image, root

    def describe_plan(self, offer, arch):
        return [f"AMI {self.ami(offer.region, arch)[0]}"]

    # --- network ---

    def public_ips(self, proxy):
        """The IPv4 addresses ssh connections may come from. Without a proxy,
        this machine's address. With one (ssh then tunnels through it), its
        exit addresses as several echo services see them: a rule-based proxy
        may route different destinations through different exits, so all of
        them are allowed (each a /32 of this operator)."""
        direct = urllib.request.build_opener(urllib.request.ProxyHandler({}))
        proxied = urllib.request.build_opener(urllib.request.ProxyHandler({"http": f"http://{proxy}", "https": f"http://{proxy}"})) if proxy else None
        urls = ["https://checkip.amazonaws.com", "https://api.ipify.org", "https://ipv4.icanhazip.com"]
        ips = set()
        for opener in ([proxied] if proxied else [direct]):
            for url in urls:
                try:
                    with opener.open(url, timeout=deadline.remaining(10)) as r:
                        ip = r.read().decode().strip()
                    if re.fullmatch(r"\d+\.\d+\.\d+\.\d+", ip):
                        ips.add(ip)
                except Exception:  # noqa: BLE001 - some services are unreachable from some networks
                    pass
        if not ips:
            raise InfraError("cannot determine this machine's public IP (checkip.amazonaws.com and others failed)")
        return sorted(ips)

    def ensure_key_pair(self, ec2, fresh):
        from botocore.exceptions import ClientError
        try:
            ec2.describe_key_pairs(KeyNames=[KEY_NAME])
            if not fresh:
                return
            ec2.delete_key_pair(KeyName=KEY_NAME)  # the local key was regenerated
        except ClientError as e:
            if e.response["Error"]["Code"] != "InvalidKeyPair.NotFound":
                raise
        with open(KEY_FILE + ".pub", "rb") as stream:
            public_key = stream.read()
        ec2.import_key_pair(KeyName=KEY_NAME, PublicKeyMaterial=public_key,
                            TagSpecifications=[{"ResourceType": "key-pair", "Tags": [TAG]}])
        log(f"imported key pair {KEY_NAME}")

    def default_vpc(self, ec2):
        vpcs = ec2.describe_vpcs(Filters=[{"Name": "isDefault", "Values": ["true"]}])["Vpcs"]
        if not vpcs:
            raise InfraError("no default VPC")
        return vpcs[0]["VpcId"]

    def ingress(self, perm, cidr, description=""):
        """One AWS ingress range as the shared reconcile reads it
        (cloud/sgpolicy.py). `ref` is what revoking it needs: the protocol,
        the port range and the address, which is how AWS names a rule."""
        from_port, to_port = perm.get("FromPort"), perm.get("ToPort")
        return sgpolicy.Ingress(protocol=perm.get("IpProtocol", ""), port="" if from_port is None else str(from_port),
                                cidr=cidr, description=description,
                                ref=(perm.get("IpProtocol"), from_port, to_port, cidr))

    def ensure_security_group(self, ec2, vpc, ips):
        """The security group allowing tcp/22 from `ips` only; returns its id.

        The group is shared by every owner on this machine, so the reconcile is
        add-only (cloud/sgpolicy.py): any running lane may borrow an existing
        rule, regardless of its owner tag. Removal is explicit, coordinator-only
        and requires an account with no tagged instances.
        IPv6 ranges and group-based rules are never written by this provider
        and are not part of the reconcile at all."""
        groups = ec2.describe_security_groups(Filters=[{"Name": "group-name", "Values": [SG_NAME]},
                                                       {"Name": "vpc-id", "Values": [vpc]}])["SecurityGroups"]
        if groups:
            sg = groups[0]
        else:
            gid = ec2.create_security_group(GroupName=SG_NAME, Description="tlx cloud_verify: ssh from the operator",
                                            VpcId=vpc, TagSpecifications=[{"ResourceType": "security-group", "Tags": [TAG]}])["GroupId"]
            log(f"created security group {SG_NAME} ({gid})")
            sg = {"GroupId": gid, "IpPermissions": []}
        owner = sgpolicy.owner_now(self.owner)
        rules = [self.ingress(perm, r["CidrIp"], r.get("Description", ""))
                 for perm in sg["IpPermissions"] for r in perm.get("IpRanges", [])]
        change = sgpolicy.plan(rules, sgpolicy.ssh_rules(self.ssh_ports, ips), owner)
        count = sum(len(p.get(k, [])) for p in sg["IpPermissions"]
                    for k in ("IpRanges", "Ipv6Ranges", "UserIdGroupPairs", "PrefixListIds"))
        log(sgpolicy.capacity_note(count, 60, adding=len(change.missing)))
        if change.missing:
            ec2.authorize_security_group_ingress(GroupId=sg["GroupId"], IpPermissions=[
                {"IpProtocol": "tcp", "FromPort": 22, "ToPort": 22,
                 "IpRanges": [{"CidrIp": cidr, "Description": sgpolicy.describe(owner)} for _, cidr in change.missing]}])
            log(f"allowed ssh from {', '.join(cidr for _, cidr in change.missing)}")
        if note := sgpolicy.kept_note(change):
            log(note)
        if note := sgpolicy.world_note(change):
            warning(note)
        return sg["GroupId"]

    def prune_ssh_rules(self, regions):
        """Delete attributed rules by stable ID; caller holds idle-account guard."""
        for region in regions:
            ec2 = self.ec2(region)
            groups = ec2.describe_security_groups(Filters=[{"Name": "group-name", "Values": [SG_NAME]},
                    {"Name": "vpc-id", "Values": [self.default_vpc(ec2)]}])["SecurityGroups"]
            for group in groups:
                rules, token = [], None
                while True:
                    page = ec2.describe_security_group_rules(Filters=[{"Name": "group-id", "Values": [group["GroupId"]]}],
                                                            **({"NextToken": token} if token else {}))
                    rules += [r["SecurityGroupRuleId"] for r in page.get("SecurityGroupRules", [])
                              if not r.get("IsEgress") and sgpolicy.names_owner(r.get("Description"))]
                    token = page.get("NextToken")
                    if not token:
                        break
                if rules:
                    ec2.revoke_security_group_ingress(GroupId=group["GroupId"], SecurityGroupRuleIds=rules)
                log(f"pruned {len(rules)} owner-tagged ssh rule(s) in {region}; ownerless/manual rules retained")

    def allow_ssh(self, m, ips):
        """Add `ips` to the project's own ssh security group of `m` (the
        recovery path when this machine's public address moved after the
        launch; see cloud/allowlist.py). Add only: no ingress rule is removed,
        no group is created, nothing is opened to 0.0.0.0/0 and another
        owner's group is never touched. Returns a line describing what is
        allowed now."""
        ec2 = self.ec2(m.region)
        groups = ec2.describe_security_groups(Filters=[{"Name": "group-name", "Values": [SG_NAME]},
                                                       {"Name": "vpc-id", "Values": [self.default_vpc(ec2)]}])["SecurityGroups"]
        if not groups:
            raise InfraError(f"security group {SG_NAME} does not exist in {m.region}; create it before ssh can work")
        sg, mine = groups[0], {f"{ip}/32" for ip in ips}
        have = {r["CidrIp"] for perm in sg["IpPermissions"]
                if perm.get("IpProtocol") == "tcp" and perm.get("FromPort") == 22 and perm.get("ToPort") == 22
                for r in perm.get("IpRanges", [])}
        if new := sorted(mine - have):
            ec2.authorize_security_group_ingress(GroupId=sg["GroupId"], IpPermissions=[
                {"IpProtocol": "tcp", "FromPort": 22, "ToPort": 22,
                 "IpRanges": [{"CidrIp": c, "Description": sgpolicy.describe(sgpolicy.owner_now(self.owner))} for c in new]}])
        return (f"security group {sg['GroupId']}: ssh allowed from {', '.join(ips)} "
                + (f"({len(new)} new rule(s))" if new else "(rules already present)"))

    def allow_hint(self, m, ips):
        """The exact aws CLI call that would allow `ips` into this project's own
        ssh security group of `m` (printed when the tool cannot make the change
        itself); never raises."""
        return ("aws ec2 authorize-security-group-ingress --profile "
                + f"{self.profile} --region {m.region} --group-name {SG_NAME} --protocol tcp --port 22 "
                + " ".join(f"--cidr {ip}/32" for ip in ips))

    def subnet_for(self, ec2, vpc, az):
        subnets = ec2.describe_subnets(Filters=[{"Name": "vpc-id", "Values": [vpc]}, {"Name": "availability-zone", "Values": [az]},
                                                {"Name": "default-for-az", "Values": ["true"]}])["Subnets"]
        if not subnets:
            raise InfraError(f"no default subnet in {az}")
        return subnets[0]["SubnetId"]

    # --- instance lifecycle ---

    def launch(self, m, ips, fresh_key, max_minutes, arch):
        self.owner = self.owner or current_owner()
        from botocore.exceptions import ClientError
        o = m.offer
        ec2 = self.ec2(o.region)
        self.ensure_key_pair(ec2, fresh_key)
        vpc = self.default_vpc(ec2)
        sg = self.ensure_security_group(ec2, vpc, ips)
        image, root_dev = self.ami(o.region, arch)
        subnet = self.subnet_for(ec2, vpc, o.zone)
        m.token = uuid.uuid4().hex
        m.t_launch = time.time()
        tags = [TAG, {"Key": "Name", "Value": "tlx-verify"}, {"Key": "tlx-owner", "Value": self.owner}]
        try:
            r = ec2.run_instances(
                ImageId=image, InstanceType=o.itype, MinCount=1, MaxCount=1, KeyName=KEY_NAME,
                ClientToken=m.token,
                NetworkInterfaces=[{"DeviceIndex": 0, "SubnetId": subnet, "Groups": [sg], "AssociatePublicIpAddress": True,
                                    "DeleteOnTermination": True}],
                InstanceMarketOptions={"MarketType": "spot", "SpotOptions": {
                    "SpotInstanceType": "one-time", "InstanceInterruptionBehavior": "terminate"}},
                InstanceInitiatedShutdownBehavior="terminate",
                BlockDeviceMappings=[{"DeviceName": root_dev, "Ebs": {"VolumeSize": DISK_GB, "VolumeType": "gp3", "DeleteOnTermination": True}}],
                UserData=deadman_user_data(max_minutes).replace("# dead-man's switch", "# dead-man's switch: the shutdown behavior is terminate"),
                TagSpecifications=[{"ResourceType": t, "Tags": tags} for t in ("instance", "volume", "spot-instances-request", "network-interface")])
        except ClientError as e:
            code = e.response["Error"]["Code"]
            err = f"{code}: {e.response['Error'].get('Message', '')}"
            if code in ("InsufficientInstanceCapacity", "SpotMaxPriceTooLow", "InsufficientCapacity", "Unsupported"):
                raise SkipOffer("zone", f"no capacity in {o.zone} ({err})") from e
            if "Limit" in code or "Quota" in code or "MaxSpotInstanceCount" in code:
                # (Instances still shutting down count against the quota.)
                raise InfraError(f"spot quota ({SPOT_VCPU_QUOTA}) exceeded in {o.region} ({err})") from e
            raise InfraError(f"launch failed in {o.zone}: {err}") from e
        m.id = r["Instances"][0]["InstanceId"]

    def find_launched(self, m):
        """The instance id of an interrupted launch call, if the instance exists."""
        if m.id or not m.token:
            return m.id
        try:
            rs = self.ec2(m.region).describe_instances(Filters=[{"Name": "client-token", "Values": [m.token]}])["Reservations"]
            ids = [i["InstanceId"] for r in rs for i in r["Instances"]]
            m.id = ids[0] if ids else None
        except Exception as e:  # noqa: BLE001
            warning(f"cannot look up the launched instance: {e}")
        return m.id

    def instance_state(self, m):
        """'running', 'stopped' or 'gone' (see cloud/allowlist.py): whether an
        ssh failure is the instance's fault or the allowlist's."""
        from botocore.exceptions import ClientError
        ec2 = self.ec2(m.region)
        try:
            rs = ec2.describe_instances(InstanceIds=[m.id])["Reservations"]
        except ClientError as e:
            if e.response["Error"]["Code"] != "InvalidInstanceID.NotFound":
                raise
            return "gone"
        insts = [i for r in rs for i in r["Instances"]]
        if not insts:
            return "gone"
        state = insts[0]["State"]["Name"]
        if state in GONE_STATES:
            return "gone"
        return "stopped" if state in STOPPED_STATES else "running"

    def wait_ready(self, m):
        ec2 = self.ec2(m.region)
        ec2.get_waiter("instance_running").wait(InstanceIds=[m.id], WaiterConfig={"Delay": 3, "MaxAttempts": 100})
        inst = ec2.describe_instances(InstanceIds=[m.id])["Reservations"][0]["Instances"][0]
        m.ip = inst.get("PublicIpAddress")
        if not m.ip:
            raise InfraError("the instance has no public IP")

    def terminate_hint(self, m):
        return f"aws ec2 terminate-instances --profile {self.profile} --region {m.region} --instance-ids {m.id}"

    def terminate(self, m):
        ec2 = self.ec2(m.region)
        for attempt in range(5):
            try:
                ec2.terminate_instances(InstanceIds=[m.id])
                break
            except Exception as e:  # noqa: BLE001 - retry, then tell the operator
                if stop_code(str(e)) in (20, 21, 22, 24):
                    raise InfraError(f"termination refused: {e}", stop_code(str(e))) from e
                warning(f"terminate failed ({e}), retrying")
                time.sleep(3)
        else:
            return False
        try:
            rs = ec2.describe_spot_instance_requests(Filters=[{"Name": "instance-id", "Values": [m.id]}])["SpotInstanceRequests"]
            if rs:
                ec2.cancel_spot_instance_requests(SpotInstanceRequestIds=[r["SpotInstanceRequestId"] for r in rs])
        except Exception as e:  # noqa: BLE001 - a one-time request closes with its instance anyway
            warning(f"cannot cancel the spot request ({e})")
        state = ec2.describe_instances(InstanceIds=[m.id])["Reservations"][0]["Instances"][0]["State"]["Name"]
        log(f"terminated {m.id} (state: {state})")
        return True

    def leftovers(self, regions, owner=None):
        out = []
        for region in regions:
            rs = self.ec2(region).describe_instances(Filters=([{"Name": "tag:tlx-owner", "Values": [owner]}] if owner is not None else []) + [{"Name": "tag:Project", "Values": ["tlx"]},
                                                              {"Name": "instance-state-name", "Values": ["pending", "running", "stopping", "stopped"]}])["Reservations"]
            out += [(region, i["InstanceId"], i["State"]["Name"]) for r in rs for i in r["Instances"]]
        return out

    def packages_script(self):
        return "set -e\nsudo dnf install -y -q git gcc clang make rsync python3 >/dev/null\n"

    def java_script(self):
        # TLC for the bench mode.
        return "sudo dnf install -y -q java-21-amazon-corretto-headless >/dev/null\njava -version 2>&1 | head -1\n"
