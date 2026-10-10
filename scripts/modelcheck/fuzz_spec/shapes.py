"""Bounded state-variable shapes and their membership predicates."""
import math
from modelcheck import fuzz_expr as fx
from modelcheck.fuzz_expr import BOOL, INT, MV, STR


def small_scalar(rng):
    return rng.choice([INT(1), MV, BOOL, STR, INT(1), MV])



def state_type(rng):
    x = rng.random()
    if x < .16:
        return INT(rng.choice([2, 3]))
    if x < .21:
        return BOOL
    if x < .26:
        return STR
    if x < .33:
        return MV
    if x < .46:
        return ("set", small_scalar(rng), rng.choice([2, 3]))
    if x < .56:
        return ("seq", small_scalar(rng), rng.choice([2, 3]))
    if x < .72:
        return ("fcn", rng.choice([INT(1), MV, STR, INT(1)]), rng.choice([INT(2), BOOL, MV, ("set", INT(1), 2), STR]), 3)
    if x < .86:
        fs = sorted(rng.sample(fx.FIELDS, rng.randint(1, 3)))
        kinds = [INT(2), ("set", MV, 2), ("set", INT(1), 2), STR, BOOL, MV]
        rec = [(f, rng.choice(kinds)) for f in fs]
        if not any(t[0] == "set" for _, t in rec) and rng.random() < .7:
            rec[-1] = (rec[-1][0], ("set", rng.choice([MV, INT(1)]), 2))
        return ("rec", tuple(rec))
    if x < .93:
        return ("tup", (small_scalar(rng), small_scalar(rng)))
    return ("bag", rng.choice([INT(1), MV]), 2)



def size(t):
    """The number of values of a bounded shape (strings: the base pool)."""
    k = t[0]
    if k == "mixed":
        return 2  # opaque values in dedicated probes: initial and one successor
    if k == "bool":
        return 2
    if k == "int":
        return 2 * t[1] + 1
    if k == "str":
        return len(fx.STRS)
    if k == "mv":
        return len(fx.MVS)
    if k == "set":
        u = size(t[1])
        return sum(math.comb(u, j) for j in range(min(u, t[2]) + 1))
    if k == "seq":
        u = size(t[1])
        return sum(u ** j for j in range(t[2] + 1))
    if k == "fcn":
        du, ru = size(t[1]), size(t[2])
        return sum(math.comb(du, j) * ru ** j for j in range(min(du, t[3]) + 1))
    if k == "rec":
        return math.prod(size(x) for _, x in t[1])
    if k == "tup":
        return math.prod(size(x) for x in t[1])
    if k == "bag":
        u = size(t[1])
        return sum(math.comb(u, j) * t[2] ** j for j in range(min(u, t[2]) + 1))
    raise ValueError(t)



def ok_pred(t, x, depth=0):
    """A predicate: value `x` has bounded shape `t`."""
    k, y = t[0], f"y{depth}"
    if k == "mixed":
        # Dedicated two-state probes have no shape guards. Their bounds come
        # from pc, rather than a predicate that itself compares mixed values.
        return "TRUE"
    if k == "bool":
        return f"{x} \\in BOOLEAN"
    if k == "int":
        return f"{x} \\in -{t[1]}..{t[1]}"
    if k == "str":
        return f"{x} \\in STRING"
    if k == "mv":
        return f"{x} \\in {{{', '.join(fx.MVS)}}}"
    if k == "set":
        return f"(Cardinality({x}) <= {t[2]} /\\ \\A {y} \\in {x} : {ok_pred(t[1], y, depth + 1)})"
    if k == "seq":
        return f"(Len({x}) <= {t[2]} /\\ \\A {y} \\in 1..Len({x}) : {ok_pred(t[1], f'{x}[{y}]', depth + 1)})"
    if k == "fcn":
        return (f"(Cardinality(DOMAIN {x}) <= {t[3]} /\\ \\A {y} \\in DOMAIN {x} : "
                f"{ok_pred(t[1], y, depth + 1)} /\\ {ok_pred(t[2], f'{x}[{y}]', depth + 1)})")
    if k == "rec":
        return "(" + " /\\ ".join(ok_pred(ft, f"{x}.{f}", depth + 1) for f, ft in t[1]) + ")"
    if k == "tup":
        return "(" + " /\\ ".join(ok_pred(et, f"{x}[{i}]", depth + 1) for i, et in enumerate(t[1], 1)) + ")"
    if k == "bag":
        return (f"(Cardinality(DOMAIN {x}) <= {t[2]} /\\ \\A {y} \\in DOMAIN {x} : "
                f"{ok_pred(t[1], y, depth + 1)} /\\ {x}[{y}] \\in 1..{t[2]})")
    raise ValueError(t)
