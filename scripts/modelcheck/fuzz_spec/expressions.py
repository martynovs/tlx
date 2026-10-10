"""Expression generation in state scope, with state-level restrictions."""
from modelcheck import fuzz_expr as fx
from modelcheck.fuzz_expr import Gen, Node, P, INT, MV


def uses_tostring(n):
    return any(isinstance(p, str) and "ToString" in p for p in n.parts)



class StateGen(Gen):
    """fuzz_expr's generator without what is tier 3 at state level (the
    printed form of values: ToString), and, for symmetric specs, model
    values taken from variables in scope instead of literals."""

    def __init__(self, rng, symmetric=False):
        super().__init__(rng, p_unsafe=0.02, p_confuse=0.0)
        self.symmetric = symmetric
        self.nvar, self.env, self.confused = 0, [], False

    def p_bool(self, t, d):
        n = super().p_bool(t, d)
        return None if n is not None and uses_tostring(n) else n

    def p_int(self, t, d):
        if self.rng.random() < .1 and (n := self.recursive(t, d)) is not None:
            return n
        n = super().p_int(t, d)
        return None if n is not None and uses_tostring(n) else n

    def recursive(self, t, d):
        """Applications of recursive operators: the module's RCard / RLen
        (RECURSIVE_DEFS) or a LET RECURSIVE countdown."""
        b = t[1]
        k = 3 if b is None else min(b, 3)
        if k < 1:
            return None
        c = self.rng.randrange(3)
        if c == 0:
            return Node(P("RCard(", 0, ")"), [self.expr(("set", self.rand_type(1), k), d - 1)], INT(k))
        if c == 1:
            return Node(P("RLen(", 0, ")"), [self.expr(("seq", self.rand_type(1), k), d - 1)], INT(k))
        f, x = self.fresh("R"), self.fresh("k")
        return Node(P(f"(LET RECURSIVE {f}(_) {f}({x}) == IF {x} <= 0 THEN 0 ELSE 1 + {f}({x} - 1) IN {f}(", 0, "))"),
                    [self.expr(INT(k), d - 1)], INT(k))

    # Bias towards the variables in scope: subexpressions often have the
    # type of a variable (so that comparisons, memberships, quantifiers
    # read the state), and a variable is often used where it fits.
    def scope_types(self, scalar=False):
        return [ty for n, ty in self.env if n != "@" and ty[0] != "none" and (not scalar or fx.SCALAR(ty))]

    def rand_type(self, d):
        vs = self.scope_types()
        if vs and self.rng.random() < .55:
            return self.rng.choice(vs)
        return super().rand_type(d)

    def rand_scalar(self, small=False):
        vs = self.scope_types(scalar=True)
        if vs and self.rng.random() < .5:
            return self.rng.choice(vs)
        return super().rand_scalar(small)

    def expr(self, t, d):
        vs = self.vars_fitting(t)
        if vs and d > 0 and self.rng.random() < .3:
            name, ty = self.rng.choice(vs)
            return Node(P(name), [], ty, ref=name)
        return super().expr(t, d)

    def literal(self, t):
        if self.symmetric and t[0] == "mv":
            vs = self.vars_fitting(MV)
            if vs:
                name, ty = self.rng.choice(vs)
                return Node(P(name), [], ty, ref=name)
        return super().literal(t)

    def scoped(self, env, f):
        saved = self.env
        self.env = list(env)
        try:
            return f()
        finally:
            self.env = saved



def leaf(name, ty):
    """A reference to a variable (state, primed, bound) as an expression."""
    return Node(P(name), [], ty, ref=name)



def text_node(text, ty):
    return Node(P(text), [], ty, lit=True)



def subst(n, name, make):
    """`n` with every reference to `name` replaced by make(type)."""
    if name not in n.free:
        return n
    if not n.kids:
        return make(n.ty)
    kids = [subst(k, name, make) for k in n.kids]
    m = Node.__new__(Node)
    m.parts, m.kids, m.ty, m.binds, m.lit, m.svals, m.tag = n.parts, kids, n.ty, n.binds, n.lit, n.svals, n.tag
    free = set()
    for k in kids:
        free |= k.free
    m.free = frozenset(free - set(n.binds))
    return m
