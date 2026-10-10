"""The expression node.

A node is a list of string parts and kid indices (``P`` builds one) plus its
type; ``node_from_json`` reads a minimized finding back.
"""


class Node:
    """An expression: `parts` are strings and kid indices."""
    __slots__ = ("parts", "kids", "ty", "free", "lit", "svals", "binds", "tag")

    def __init__(self, parts, kids, ty, binds=(), lit=False, svals=None, ref=None, tag=""):
        self.parts, self.kids, self.ty, self.binds, self.lit, self.svals, self.tag = parts, kids, ty, binds, lit, svals, tag
        free = set()
        for k in kids:
            free |= k.free
        free -= set(binds)
        if ref:
            free.add(ref)
        self.free = frozenset(free)

    def render(self, sub=None):
        if sub and id(self) in sub:
            return sub[id(self)]
        return "".join(p if isinstance(p, str) else self.kids[p].render(sub) for p in self.parts)

    def size(self):
        return 1 + sum(k.size() for k in self.kids)

    def to_json(self):
        return {"p": self.parts, "k": [k.to_json() for k in self.kids], "t": self.ty, "b": list(self.binds),
                "l": self.lit, "r": sorted(self.free - set().union(*[k.free for k in self.kids])) if self.kids else sorted(self.free)}


def node_from_json(j):
    def tup(x):
        return tuple(tup(y) for y in x) if isinstance(x, list) else x
    kids = [node_from_json(k) for k in j["k"]]
    ref = j["r"][0] if j["r"] else None
    return Node(j["p"], kids, tup(j["t"]), tuple(j["b"]), j["l"], ref=ref)


def P(*parts):
    """Parts of a node: strings and kid indices."""
    return list(parts)
