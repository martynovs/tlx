"""Expression types and their bounds.

``base`` reduces a bounded type to its bare shape, ``fits`` is the bounded
subsumption test the generator uses when it picks a subterm, and ``SCALAR``
marks the atomic types.
"""

# ---------------------------------------------------------------------------
# Types. Bounds are upper bounds on cardinalities (sets, sequences, function
# domains, bags) and on |n| for integers (None: unbounded), so that every
# generated expression is cheap to evaluate.
#   ('bool',) ('str',) ('mv',) ('int', B)
#   ('set', T, n) ('seq', T, n) ('fcn', D, R, n) ('rec', ((f, T), ...))
#   ('tup', (T, ...)) ('bag', T, n)
# ---------------------------------------------------------------------------
BOOL, STR, MV = ("bool",), ("str",), ("mv",)


def INT(b=None):
    return ("int", b)


def base(t):
    k = t[0]
    if k == "int":
        return ("int",)
    if k in ("set", "seq", "bag"):
        return (k, base(t[1]))
    if k == "fcn":
        return (k, base(t[1]), base(t[2]))
    if k == "rec":
        return (k, tuple((f, base(x)) for f, x in t[1]))
    if k == "tup":
        return (k, tuple(base(x) for x in t[1]))
    return t


def fits(a, b):
    """Bounded type `a` satisfies request `b`."""
    if a[0] != b[0]:
        return False
    k = a[0]
    if k == "int":
        return b[1] is None or (a[1] is not None and a[1] <= b[1])
    if k in ("set", "seq", "bag"):
        return a[2] <= b[2] and fits(a[1], b[1])
    if k == "fcn":
        return a[3] <= b[3] and base(a[1]) == base(b[1]) and fits(a[1], b[1]) and fits(a[2], b[2])
    if k in ("rec", "tup"):
        if len(a[1]) != len(b[1]):
            return False
        if k == "rec":
            return all(fa == fb and fits(x, y) for (fa, x), (fb, y) in zip(a[1], b[1]))
        return all(fits(x, y) for x, y in zip(a[1], b[1]))
    return True


SCALAR = lambda t: t[0] in ("bool", "int", "str", "mv")  # noqa: E731
