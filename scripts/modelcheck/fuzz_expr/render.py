"""TLA+ spec text and error-message normalization.

``HEADER``/``FIELDS_LINE``/``pool_def``/``const_spec``/``const_cfg`` build the
constant-level spec; ``lift``/``state_spec`` build the state-level one.
``norm_error``/``unescape`` strip TLC wrappers from a run's output.
"""
import json
import random
import re

from .constants import EXTENDS, FIELDS
from .serializer import expected_expr

HEADER = "---- MODULE {name} ----\nEXTENDS " + EXTENDS + "\nCONSTANTS m1, m2, m3{extra}\n"


# TLC orders the fields of a record (and so compares records, e.g. which
# element CHOOSE takes from a set of records) by the order in which their
# names were first used as field names in the spec. A record literal with
# all field names right after the pool fixes that order, so that a constant
# spec and a state spec (with other expressions before) agree.
FIELDS_LINE = "Fields == [" + ", ".join(f"{f} |-> 0" for f in FIELDS) + "]\n"


def pool_def(pool, rng):
    strs = sorted(pool)
    rng.shuffle(strs)
    return "Pool == <<" + ", ".join(json.dumps(s) for s in strs) + ">>\n"


def const_spec(name, exprs, pool_line):
    lines = [HEADER.format(name=name, extra=", Start"), pool_line, FIELDS_LINE]
    first = len(lines[0].splitlines()) + 3
    for i, e in enumerate(exprs):
        lines.append(f"ASSUME Start <= {i} => PrintT(<<{i}, ToString({e})>>)\n")
    lines.append("VARIABLE fuzz_x\nInit == fuzz_x = 0\nNext == UNCHANGED fuzz_x\n====\n")
    return "".join(lines), first


def const_cfg(start):
    return f"CONSTANTS m1 = m1 m2 = m2 m3 = m3 Start = {start}\nINIT Init\nNEXT Next\n"


PRINT_RE = re.compile(r'^<<\s*(\d+),\s*"((?:[^"\\]|\\.)*)"\s*>>$', re.M)


def unescape(s):
    return re.sub(r"\\(.)", lambda m: {"n": "\n", "t": "\t"}.get(m.group(1), m.group(1)), s)


def norm_error(out):
    """The core of an error message (without TLC's wrappers and locations)."""
    i = out.find("Error:")
    if i < 0:
        return None
    e = out[i + len("Error:"):]
    for stop in ("\nFinished in", "\nThe error occurred when TLC was evaluating", "\n\nThe behavior up to",
                 "\nError: The behavior up to"):
        j = e.find(stop)
        if j >= 0:
            e = e[:j]
    e = re.sub(r"^\s*Evaluating assumption line \d+, col \d+ to line \d+, col \d+ of module \w+ failed\.\s*", "", e)
    e = re.sub(r"Attempted to apply the operator overridden by the Java method\s*\n.*?,\s*\nbut it produced the following error:\s*\n",
               "", e, flags=re.S)
    e = re.sub(r"\n?(line|Line) \d+, col(umn)? \d+ (to line \d+, col \d+ )?of module \w+\.?", "", e)
    e = re.sub(r"\s+", " ", e).strip()
    return e


def lift(node, rng, prefix, p):
    """Choose closed literal subterms to read from variables: (sub, vars)."""
    sub, vs = {}, []

    def walk(n, liftable=True):
        if liftable and n.lit and not n.free and rng.random() < p and len(vs) < 4:
            name = f"{prefix}_{len(vs)}"
            sub[id(n)] = name
            vs.append((name, n.render()))
            return
        # The body of a LAMBDA stays as written: TLC passes an operator
        # whose body is a numeral or string literal as that value (a Java
        # override rejects it), a variable body as an operator.
        body = {p2 for p1, p2 in zip(n.parts, n.parts[1:]) if isinstance(p1, str) and isinstance(p2, int) and re.search(r"LAMBDA [^:]*: $", p1)}
        for i, k in enumerate(n.kids):
            walk(k, i not in body)
    walk(node)
    return sub, vs


def state_spec(name, items, pool_line):
    """A spec evaluating each item's expression in a state context: in Init,
    in an action of its own (so that one expression the compiler cannot
    handle does not keep the others from native code) or in an invariant.
    Returns (source, {line: item index})."""
    hdr = HEADER.format(name=name, extra="") + pool_line + FIELDS_LINE
    init, invs, lines, body, actions = ["pc = 0"], [], {}, [], []
    allvars = ["pc"]
    for i, (node, ctx, exp, _msg, seed) in enumerate(items):
        rng = random.Random(seed)
        sub, vs = lift(node, rng, f"v_{i}", 0.5)
        text = node.render(sub)
        for v, lit in vs:
            allvars.append(v)
            init.append(f"{v} = {lit}")
        r = f"r_{i}"
        allvars.append(r)
        body.append((f"E_{i} == {text}\n", i))
        body.append((f"X_{i} == {expected_expr(exp) if exp is not None else '0'}\n", None))
        if ctx == "inv":
            init.append(f"{r} = 0")
            invs.append(f"Inv_{i} == E_{i} = X_{i}\n" if exp is not None else f"Inv_{i} == E_{i} = E_{i}\n")
        elif ctx == "init":
            init.append(f"{r} = E_{i}")
            invs.append(f"Inv_{i} == {r} = X_{i}\n")
        else:
            init.append(f"{r} = X_{i}")
            actions.append((f"A_{i}", r, f"{r}' = E_{i}"))
            invs.append(f"Inv_{i} == {r} = X_{i}\n")
    actions.append(("Step", None, None))
    src = hdr + "VARIABLES " + ", ".join(allvars) + "\n"
    ln = src.count("\n") + 1
    for text, i in body:
        if i is not None:
            lines[ln] = i
        src += text
        ln += 1
    src += "Init ==\n" + "".join(f"  /\\ {c}\n" for c in init)
    for an, r, asg in actions:
        others = [v for v in allvars if v not in ("pc", r)]
        src += f"{an} == pc = 0 /\\ pc' = 1" + (f" /\\ {asg}" if asg else "") + \
            (f" /\\ UNCHANGED <<{', '.join(others)}>>" if others else "") + "\n"
    src += "Next == " + " \\/ ".join(an for an, _, _ in actions) + "\n"
    src += "".join(invs) + "====\n"
    return src, lines
