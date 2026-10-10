"""Caller-neutral fields and counterexample layout for checker tool output."""
import re
from tlc import parse
import tracecheck
from tracecheck import trace_blocks as trace


def layout_free(blocks):
    """A counterexample without the layout of its values (TLC breaks long
    values over several lines, tlx does not: the states are compared as
    values)."""
    def norm(text):
        text = re.sub(r"\s+", " ", text)
        return re.sub(r" ?(<<|>>|[\[\]{}()]) ?", r"\1", text)
    return [(h, norm(b)) for h, b in blocks]



def must(out, rc):
    """The compared fields of a -tool output (and the exit code, shown
    only), and its counterexample: `trace` (its states as values, with
    their headers) and `lasso` ((states, loop), for scripts/tracecheck.py)."""
    r = parse(out, 0, rc)
    blocks = trace(out)
    return {"result": r.get("result"), "violated": r.get("violated"), "exit": rc, "distinct": r.get("distinct"),
            "generated": r.get("generated"), "depth": r.get("depth"), "trace": layout_free(blocks),
            "lasso": tracecheck.lasso(blocks), "optimizations": r.get("optimizations")}
