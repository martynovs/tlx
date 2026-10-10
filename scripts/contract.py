"""TLC compatibility tiers (docs/compat.md section 0), shared by test drivers.

Trace legality remains the responsibility of tracecheck; checked/skipped describe
only evidence actually available to this comparison, never inferred coverage.
The acceptance vocabulary at the end of this module is what the auxiliary
drivers (matrix, sanydiff, autosym_mutants, verify_compare) count their cases
with, so that a run which observed nothing can never read as a success.
"""
from dataclasses import dataclass, field

NAMED_VIOLATIONS = ("safety failure", "property failure", "init property failure", "postcondition failure")
SAFETY = (*NAMED_VIOLATIONS[:3], "deadlock")

# The fields each optimization of tlx's default mode keeps equal to TLC's
# (the `preserved` guarantees of `-stats json`, docs/reductions.md §1.1);
# counts of the reduced search always differ.
PRESERVED = {"symmetry": ("depth", "trace length")}


class OptimizedRun(ValueError):
    """A run with optimizations compared with a TLC oracle as if it were TLC-compatible."""


class NoObservation(ValueError):
    """Neither side of a comparison has a result: nothing was observed."""


# Refuse to compare two results when neither side produced one: a missing
# engine output is an infrastructure failure, never an agreement. `tlc.parse`
# also returns "unknown" for a machine failure (a full disk, a killed JVM),
# whose `infra` field names the reason.
MISSING_RESULT = (None, "unknown")


def preserved(optimizations):
    """The fields every optimization in `optimizations` (tlc.parse) preserves."""
    keep = None
    for o in optimizations:
        fields = next((set(v) for k, v in PRESERVED.items() if o.startswith(k)), set())
        keep = fields if keep is None else keep & fields
    return keep or set()


@dataclass
class Comparison:
    differs: list = field(default_factory=list)
    checked: list = field(default_factory=list)
    skipped: dict = field(default_factory=dict)

    def details(self):
        return {"checked": self.checked, "skipped": self.skipped}


def compare(want, got, *, workers=1, oracle_workers=1, simulation=False,
            deterministic=False, asymmetric_symmetry=False, schedule_dependent=False,
            allow_unnamed_liveness=False, random_choices=False, optimized=False):
    """Compare tlx's result `got` with TLC's `want` by contract tier. A run
    that used optimizations (default mode, `got["optimizations"]`) is refused
    unless `optimized`: then only the verdict and the fields the
    optimizations preserve are compared."""
    if (want or {}).get("result") in MISSING_RESULT and (got or {}).get("result") in MISSING_RESULT:
        raise NoObservation("neither TLC nor tlx produced a result: nothing was observed")
    c = Comparison()
    optimizations = got.get("optimizations") or []
    if optimizations and not optimized:
        raise OptimizedRun(f"the tlx run used optimizations ({'; '.join(optimizations)}): its counts are not TLC's; "
                           f"run tlx with -tlc to compare it with TLC")
    keep = preserved(optimizations) if optimizations else None

    def check(key, equal):
        c.checked.append(key)
        if not equal:
            c.differs.append(key)

    kind = want.get("result")
    if workers != 1 and schedule_dependent:
        check("completion", got.get("result") not in (None, "unknown", "timeout"))
        c.skipped["result"] = "schedule-dependent TLC registers (D-3)"
        return c
    racing_error = (workers != 1 and kind == "error"
                    and got.get("result") in (*SAFETY, "liveness failure", "postcondition failure"))
    check("result", kind == got.get("result") or racing_error)
    if racing_error:
        c.skipped["violated"] = "evaluation error/violation race (D-3)"
    elif kind == "liveness failure":
        if allow_unnamed_liveness and not want.get("violated") and not got.get("violated"):
            c.skipped["violated"] = "unnamed TLC lasso; requires trace validation (D-10)"
        else:
            check("violated", bool(got.get("violated")))
    elif kind in NAMED_VIOLATIONS or want.get("violated") or got.get("violated"):
        check("violated", got.get("violated") == want.get("violated") if want.get("violated") else
              bool(got.get("violated")) if kind in NAMED_VIOLATIONS else not got.get("violated"))

    reason = None
    if simulation:
        reason = "random simulation (D-2)"
    elif random_choices:
        reason = "state-level random choices (D-4)"
    elif kind == "liveness failure":
        reason = "periodic liveness check timing (D-1)"
    elif asymmetric_symmetry and (workers != 1 or oracle_workers != 1):
        reason = "non-equivariant SYMMETRY scheduling (D-22)"
    elif kind != "success" and (workers != 1 or oracle_workers != 1):
        reason = "multi-worker early stop (D-3)"
    if keep is not None and kind != "success":
        # (A search that stops early stops in the same level, but after other
        # states: the deepest level it saw may differ.)
        keep = keep - {"depth"}
    for key in ("distinct", "generated", "depth"):
        if keep is not None and key not in keep:
            c.skipped[key] = "reduced search (" + "; ".join(optimizations) + ")"
        elif reason and not (simulation and deterministic and key == "generated" and workers == 1):
            c.skipped[key] = reason
        elif want.get(key) is None:
            c.skipped[key] = "not recorded by oracle"
        else:
            check(key, got.get(key) == want[key])
    if keep is not None and "trace length" not in keep:
        c.skipped["trace length"] = "reduced search (" + "; ".join(optimizations) + ")"
    elif kind in SAFETY and not reason:
        want_length = len(want["trace"]) if "trace" in want else want.get("trace_length")
        got_length = len(got["trace"]) if "trace" in got else got.get("trace_length")
        if want_length is not None:
            check("trace length", want_length == got_length)
        else:
            c.skipped["trace length"] = "trace not recorded by oracle"
    else:
        c.skipped["trace length"] = reason or "not a safety failure"
    c.skipped["trace validity"] = "not checked by scalar comparator"
    return c


# ------------------------------------------------------- acceptance vocabulary
#
# Every auxiliary driver counts its cases with the same words, so that a run
# which observed nothing - an unmatched selection, an all-invalid mutant set,
# a pair of result directories with no logs in common - or whose engine
# produced no output at all can never read as a successful comparison
# (HARNESS-05, HARNESS-21). A case is one unit of what the driver judges: a
# generated run (matrix), a file (sanydiff), a mutant (autosym_mutants), a
# (suite, id) pair (verify_compare).
#
#   selected      cases the invocation asked for
#   eligible      selected cases the domain judged applicable
#   executed      cases the driver ran and reached a judgment for
#                 (observed + inconclusive + infra_failed)
#   observed      cases whose output the domain could judge
#   skipped       selected cases deliberately not run (filter or policy)
#   inconclusive  runs with no verdict (cut short by the timeout) or cases the
#                 domain cannot judge (an invalid mutant, a timeout-only
#                 difference): no verdict either way, never a failure claim
#   infra_failed  runs with no usable output (crash, missing engine output):
#                 never a semantic rejection and never an agreement
#
# `refusal()` is the gate every one of them exits on.
FIELDS = ("selected", "eligible", "executed", "observed", "skipped", "inconclusive", "infra_failed")
OBSERVED, INCONCLUSIVE, INFRA_FAILED = "observed", "inconclusive", "infra_failed"


def classify(result):
    """The vocabulary class of one parsed engine result (`tlc.parse`, which
    always sets a result: unmatched or crashed output, and a failure of the
    machine the run was on, are "unknown")."""
    kind = (result or {}).get("result")
    if kind in MISSING_RESULT:
        return INFRA_FAILED
    return INCONCLUSIVE if kind == "timeout" else OBSERVED


@dataclass
class Observations:
    """The case counters of one auxiliary driver run. `selected`/`eligible`/
    `skipped` are set by the driver; `count` maintains the rest, so that
    `executed` always equals observed + inconclusive + infra_failed."""

    selected: int = 0
    eligible: int = 0
    executed: int = 0
    observed: int = 0
    skipped: int = 0
    inconclusive: int = 0
    infra_failed: int = 0

    def count(self, class_=OBSERVED, n=1):
        """Count `n` executed cases in `class_`; returns `class_`."""
        if class_ not in (OBSERVED, INCONCLUSIVE, INFRA_FAILED):
            raise ValueError(f"not an observation class: {class_!r}")
        self.executed += n
        setattr(self, class_, getattr(self, class_) + n)
        return class_

    def count_result(self, result, n=1):
        """Count `n` executed cases classified from a parsed engine result."""
        return self.count(classify(result), n)

    def counts(self):
        return {k: getattr(self, k) for k in FIELDS}

    def summary(self):
        """The one count line every auxiliary driver prints (zeros omitted)."""
        return ", ".join(f"{k}={v}" for k, v in self.counts().items() if v)

    def refusal(self):
        """Why this run may not report success, or None (HARNESS-05): a verdict
        needs at least one observation, and a run whose engine produced no
        usable output is an infrastructure failure, never a pass."""
        if self.infra_failed:
            return f"{self.infra_failed} case(s) produced no usable engine output"
        if not self.observed:
            return "no case could be judged: the run observed nothing"
        return None
