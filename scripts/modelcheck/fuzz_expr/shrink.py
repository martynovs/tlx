"""One-step shrinking of a discrepancy.

``nodes_with_paths``/``descendants`` enumerate positions, ``replace_at``
rebuilds, ``shrink_candidates`` returns the ordered one-step simplifications
and ``shrink_ty`` the type of a minimal literal.
"""
from .exprtypes import INT, base
from .generation import Gen
from .node import Node

# ---------------------------------------------------------------------------
# Shrinking
# ---------------------------------------------------------------------------
def nodes_with_paths(n, path=()):
    yield path, n
    for i, k in enumerate(n.kids):
        yield from nodes_with_paths(k, path + (i,))


def replace_at(n, path, new):
    if not path:
        return new
    kids = list(n.kids)
    kids[path[0]] = replace_at(kids[path[0]], path[1:], new)
    m = Node.__new__(Node)
    m.parts, m.kids, m.ty, m.binds, m.lit, m.svals, m.tag = n.parts, kids, n.ty, n.binds, n.lit, n.svals, n.tag
    free = set()
    for k in kids:
        free |= k.free
    own = n.free - set().union(*[k.free for k in n.kids]) if n.kids else n.free
    m.free = frozenset((free - set(n.binds)) | own)
    return m


def descendants(n):
    for k in n.kids:
        yield k
        yield from descendants(k)


def shrink_candidates(root, rng):
    """One-step simplifications of `root`, smallest first."""
    out = []
    g = Gen(rng)
    for path, n in nodes_with_paths(root):
        if n.size() == 1:
            continue
        for k in descendants(n):
            if base(k.ty) == base(n.ty) and k.free <= n.free:
                out.append(replace_at(root, path, k))
        # A small literal of the same type.
        if n.tag != "confuse":
            g.nvar, g.env = 0, []
            lit = g.literal(shrink_ty(n.ty))
            out.append(replace_at(root, path, lit))
    seen, uniq = set(), []
    for c in sorted(out, key=lambda c: c.size()):
        txt = c.render()
        if txt not in seen and c.size() < root.size():
            seen.add(txt)
            uniq.append(c)
    return uniq


def shrink_ty(t):
    k = t[0]
    if k in ("set", "seq", "bag"):
        return (k, shrink_ty(t[1]), 1)
    if k == "fcn":
        return (k, t[1], shrink_ty(t[2]), 1)
    if k == "int":
        return INT(1)
    return t
