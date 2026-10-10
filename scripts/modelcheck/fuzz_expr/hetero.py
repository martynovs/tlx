"""Heterogeneous-kind probes and the D-31 classifier.

The probes compare different kinds deterministically (a fixed quota, never
filtered by the expected outcome); the classifier recognizes the documented
TLC function-comparison defect.
"""
import re

from .exprtypes import BOOL, INT, MV, STR, base
from .node import Node, P

# A deterministic quota, independent of the random typed productions. Keep the
# individual nodes structured so the existing lifter and shrinker can operate.
HETERO_EVERY = 4
MIXED = ("mixed",)
HETERO_TYPES = (("rec", (("a", INT(2)), ("b", INT(2)))), ("tup", (INT(2),)),
                ("set", INT(2), 2), ("fcn", INT(2), INT(2), 2), STR, INT(2), MV)
HETERO_OPS = ("cup", "cap", "difference", "subseteq", "in", "SUBSET", "UNION",
              "Cardinality", "CHOOSE", "application", "DOMAIN", "builder", "map", "EXCEPT")


def heterogeneous(g, index, state=False):
    """(probe, inputs) for different-kind comparisons and successful storage.

    Cycle through every operator and every pair of kinds; shuffle operands and
    vary literals using the campaign RNG. Some pairs are comparable (notably
    model values), others must error. Never filter by the expected outcome.
    """
    pairs = [(a, b) for a in range(len(HETERO_TYPES)) for b in range(a + 1, len(HETERO_TYPES))]
    a, b = pairs[(index // len(HETERO_OPS) + index % len(HETERO_OPS)) % len(pairs)]
    kinds = [HETERO_TYPES[a], HETERO_TYPES[b]]
    g.rng.shuffle(kinds)
    elems = [g.literal(t) for t in kinds]
    for e in elems:
        e.tag = "heterogeneous-element"
    # Most state probes store atoms separately, so fingerprinting a mixed
    # set/domain cannot mask the selected operation. Every fourth probe also
    # stores the collections themselves to exercise normalization in storage.
    inputs = {}
    if state and index % 4 != 0:
        inputs = {"vLeft": elems[0], "vRight": elems[1]}
        elems = [Node(P(v), [], n.ty, ref=v) for v, n in inputs.items()]
    st = ("set", MIXED, 2)
    s = g.enum("{", "}", elems, st, lit=True)
    t = g.enum("{", "}", [elems[0]], ("set", MIXED, 1), lit=True)
    f = Node(P("(", 0, " :> ", 1, " @@ ", 2, " :> ", 3, ")"),
             [elems[0], elems[1], elems[1], elems[0]], ("fcn", MIXED, MIXED, 2), lit=not inputs)
    values = Node(P("(1 :> ", 0, " @@ 2 :> ", 1, ")"), elems, ("fcn", INT(2), MIXED, 2), lit=not inputs)
    if not inputs:
        inputs = {"vS": s, "vT": t, "vF": f, "vValues": values}
        if state:
            s, t, f, values = [Node(P(v), [], n.ty, ref=v) for v, n in inputs.items()]
    op = HETERO_OPS[index % len(HETERO_OPS)]
    if op in ("cup", "cap", "difference", "subseteq"):
        token = {"cup": r"\cup", "cap": r"\cap", "difference": "\\", "subseteq": r"\subseteq"}[op]
        n = Node(P("(", 0, f" {token} ", 1, ")"), [s, t], BOOL if op == "subseteq" else st)
    elif op == "in":
        n = Node(P("(", 0, " \\in ", 1, ")"), [elems[g.rng.randrange(2)], s], BOOL)
    elif op == "SUBSET":
        n = Node(P("(SUBSET ", 0, ")"), [s], ("set", st, 4))
    elif op == "UNION":
        n = Node(P("(UNION {", 0, ", ", 1, "})"), [s, t], st)
    elif op == "Cardinality":
        n = Node(P("Cardinality(", 0, ")"), [s], INT(2))
    elif op == "CHOOSE":
        n = Node(P("(CHOOSE h \\in ", 0, " : TRUE)"), [s], MIXED, binds=("h",))
    elif op == "application":
        n = Node(P("(", 0, ")[", 1, "]"), [f, elems[g.rng.randrange(2)]], MIXED)
    elif op == "DOMAIN":
        n = Node(P("(DOMAIN ", 0, ")"), [f], st)
    elif op == "builder":
        n = Node(P("{h \\in ", 0, " : h \\in ", 1, "}"), [s, t], st, binds=("h",))
    elif op == "map":
        n = Node(P("{", 0, "[h] : h \\in DOMAIN ", 0, "}"), [values], st, binds=("h",))
    else:
        n = Node(P("[", 0, " EXCEPT ![", 1, "] = ", 2, "]"), [f, elems[0], elems[1]], f.ty)
    n.tag = "heterogeneous:" + op
    return n, inputs


# Documented TLC defects remain visible to the semantic comparator.
HETERO_OPEN = {
    "heterogeneous-function-comparison-defect": "D-31 (extends D-18): TLC rejects comparisons between function representations or unequal function domains",
}


def mixed_collection(n):
    t = n.get("t", [])
    return (len(t) > 1 and t[0] == "set" and tuple(t[1]) == ("mixed",)
            or any(mixed_collection(k) for k in n.get("k", [])))


def heterogeneous_function_kinds(n):
    """Kinds at a mixed set/domain boundary, not nested function values."""
    kinds = []
    def kind(node):
        t = node.get("t", [None])
        # D-31 probes compare records (string domains) with integer-domain
        # tuples/functions. A same-domain function with incompatible mapped
        # values must not be mistaken for this defect.
        if t[0] == "fcn" and (len(t) < 2 or t[1][0] != "int"):
            return "fcn-other-domain"
        return t[0]
    def walk(node):
        t = node.get("t", [])
        ks = node.get("k", [])
        parts = node.get("p", [])
        if (len(t) > 1 and t[0] == "set" and tuple(t[1]) == ("mixed",)
                and parts and parts[0] == "{"
                and all(isinstance(p, int) or p in ("{", "}", ", ") for p in parts)):
            if ks and all(k.get("t", [None])[0] != "set" for k in ks):
                kinds.extend(kind(k) for k in ks)
        if len(t) > 2 and t[0] == "fcn":
            # The generator's explicit mixed-domain constructor. DOMAIN,
            # application and EXCEPT all consume these keys without a set
            # literal node. Do not collect the function's mapped values.
            if (tuple(t[1]) == ("mixed",) and len(ks) == 4
                    and parts == ["(", 0, " :> ", 1, " @@ ", 2, " :> ", 3, ")"]):
                kinds.extend(kind(k) for k in ks[::2])
            # The generated set-map obtains its elements from this range.
            if (tuple(t[2]) == ("mixed",) and len(ks) == 2
                    and parts == ["(1 :> ", 0, " @@ 2 :> ", 1, ")"]):
                kinds.extend(kind(k) for k in ks)
        for k in node.get("k", []):
            walk(k)
    walk(n)
    return kinds


def function_comparison_defect(kinds, diagnostic):
    """Require function-only operands and TLC's comparison diagnostic."""
    return ("rec" in kinds and len(set(kinds)) >= 2 and set(kinds) <= {"rec", "tup", "seq", "fcn"}
            and bool(re.search(r"Attempted to (?:compare|check equality of)", diagnostic)))


def known_expression(f):
    # Compiled/interpreter disagreements, timeouts and crashes are never D-31.
    if (f.get("layer") == "interp-vs-tlc" and f.get("kind") == "error-vs-value"
            and f.get("tlc", [None])[0] == "err" and f.get("tlx", [None])[0] == "ok"
            and function_comparison_defect(heterogeneous_function_kinds(f.get("node", {})), f["tlc"][1])):
        return "heterogeneous-function-comparison-defect"
    return None


def max_ty(tys, req):
    """A bounded type covering all `tys` (within request `req`)."""
    t0 = tys[0]
    if any(base(t) != base(t0) for t in tys):
        return req
    k = t0[0]
    if k == "int":
        bs = [t[1] for t in tys]
        return INT(None if None in bs else max(bs))
    if k in ("set", "seq", "bag"):
        return (k, max_ty([t[1] for t in tys], req[1] if req[0] == k else t0[1]), max(t[2] for t in tys))
    if k == "fcn":
        return (k, max_ty([t[1] for t in tys], t0[1]), max_ty([t[2] for t in tys], t0[2]), max(t[3] for t in tys))
    return t0
