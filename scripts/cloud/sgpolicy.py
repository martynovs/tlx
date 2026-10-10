"""Add-only ingress planning for the shared ssh group.

Owner tags record provenance, not exclusive use: another launch may borrow any
existing TCP/ACCEPT rule. No launch or transport repair may remove a rule,
even one carrying its own owner tag. Only the explicit coordinator prune may
remove tagged rules, under the launch ledger lock and with no tagged instances
in the account. Provider-specific deletion uses stable references or a version
precondition, never an unguarded whole-set replacement.
"""
import dataclasses
import hashlib

from cloud.cloud_common import InfraError, current_owner

# The marker every rule this project writes carries.
PREFIX = "cloud_verify"
# The tightest description limit of the three providers (Tencent Cloud's
# PolicyDescription): an owner tag that does not fit is shortened and qualified
# with a digest of the full name, so it stays unique and re-derivable.
MAX_DESCRIPTION = 64
_DIGEST_CHARS = 12
# What "everyone" looks like as a source address.
WILDCARDS = ("0.0.0.0/0", "::/0")


def describe(owner):
    """The description a rule written by `owner` carries."""
    if not owner:
        return PREFIX
    tag = f"{PREFIX}:{owner}"
    if len(tag) <= MAX_DESCRIPTION:
        return tag
    digest = hashlib.sha256(owner.encode()).hexdigest()[:_DIGEST_CHARS]
    keep = MAX_DESCRIPTION - len(PREFIX) - 1 - len(digest) - 1
    return f"{PREFIX}:{owner[:keep]}-{digest}"


def names_owner(description):
    """Whether a rule's description claims a specific owner."""
    return (isinstance(description, str) and description.startswith(PREFIX + ":")
            and len(description) > len(PREFIX) + 1)


def is_ours(description, owner):
    """Whether `description` is the one *this* owner writes. An owner-less run
    (a detached checkout) owns nothing: it must not revoke a rule it cannot
    claim."""
    return bool(owner) and description == describe(owner)


def revocable(rule, owner):
    """This owner's own rule, and one the provider can name back when it
    revokes it. A rule without a handle (an address family or rule kind whose
    id the provider did not return) stays: it is not worth guessing at."""
    return rule.ref is not None and is_ours(rule.description, owner)


def owner_now(owner):
    """The owner a run tags its rules with: the owner it was launched under,
    or the current branch's tag. A lookup that fails gives no owner at all -
    the rules then carry the bare marker, which nobody revokes - which is the
    safe direction for a caller that is already reporting a failure."""
    if owner:
        return owner
    try:
        return current_owner(required=False)
    except Exception:  # noqa: BLE001 - only the tag is lost, and it costs a rule nobody cleans up
        return None


def opens_world(cidr):
    """Whether a source address is every address (never written by us)."""
    return cidr in WILDCARDS


def ssh_rules(ports, ips, fmt=str):
    """The ingress rules ssh from `ips` needs: one per (port, address). `fmt`
    renders the port the way a provider spells it (Aliyun: `22/22`)."""
    return {(fmt(p), f"{ip}/32") for p in ports for ip in ips}


@dataclasses.dataclass(frozen=True)
class Ingress:
    """One existing ingress rule as its provider read it back: the fields the
    decision is made from, plus `ref` - the provider's own handle for revoking
    it (Tencent's PolicyIndex, Aliyun's SecurityGroupRuleId, AWS's
    protocol/ports/address tuple), or None when the provider gave none."""
    protocol: str
    port: str
    cidr: str
    description: str = ""
    action: str = "ACCEPT"
    ref: object = None

    def key(self):
        return (self.port, self.cidr)


@dataclasses.dataclass
class Plan:
    """What one launch changes in the shared group."""
    missing: list             # ... that are not there yet, to add
    stale: list               # always empty for a launch (legacy plan surface)
    kept: list                # everything else, left exactly as it is

    @property
    def world(self):
        """Kept rules that let every address in (reported, never revoked)."""
        return [r for r in self.kept if opens_world(r.cidr)]


def plan(rules, want, owner):
    """The change a launch of `owner` makes to a group whose ingress rules are
    `rules`. Raises `InfraError` rather than write a rule from `0.0.0.0/0`."""
    if bad := sorted({cidr for _, cidr in want if opens_world(cidr)}):
        raise InfraError(f"refusing to add an ingress rule from {', '.join(bad)} to the ssh security group: it "
                         "allows the operator's addresses, never everyone")
    # A rule is "there" when the access it grants is there, whoever wrote it:
    # the owners on this machine share one egress address, so the same rule
    # asked for twice is one rule, and asking again is an error at the provider.
    present = {r.key() for r in rules if r.protocol.upper() == "TCP" and r.action.upper() == "ACCEPT"}
    # Access is deduplicated across owners: B may be using a rule tagged A.
    # Ownership is provenance, not an exclusive lease. No launch may revoke.
    return Plan(missing=sorted(set(want) - present), stale=[], kept=list(rules))


def kept_note(plan):
    """The line a launch logs about the rules it left alone, or None."""
    if not plan.kept:
        return None
    untagged = sum(1 for r in plan.kept if not names_owner(r.description))
    return (f"kept {len(plan.kept)} existing ingress rule(s) "
            f"({untagged} of them with no owner tag): another owner's rule may be in use right now")


def world_note(plan):
    """The warning for a kept rule that opens the group to every address, or
    None."""
    if not plan.world:
        return None
    cidrs = ", ".join(sorted({r.cidr for r in plan.world}))
    return (f"the ssh security group allows ingress from {cidrs} and no owner claims that rule: leaving it "
            "alone is this tool's rule (only an idle-account coordinator prune may revoke tagged rules), but the group is "
            "open to the world - someone should remove it")


def prunable(rules):
    """Only attributed project rules with a usable provider reference."""
    return [r for r in rules if names_owner(r.description) and r.ref is not None]


def capacity_note(count, limit, *, adding=0, combined=False):
    """Report observed ingress usage; defaults may be raised at the provider."""
    log = f"ssh security group: {count} existing ingress rule(s), {adding} planned addition(s); default {'total' if combined else 'ingress'} limit {limit}"
    if count + adding >= limit * 0.8:
        log += "; nearing the limit: coordinator may --cleanup --prune-sg only after all tagged instances are gone"
    return log
