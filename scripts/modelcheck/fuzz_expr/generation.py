"""The typed random expression generator (``Gen``).

Operator selection, literal and variable pools, the liftable literals of the
constant tier and the deterministic heterogeneous quota all use the campaign
RNG in the same order as before (CODE-14).
"""
from .constants import FIELDS, INT_MAX, MVS, STRS
from .exprtypes import BOOL, INT, MV, SCALAR, STR, base, fits
from .hetero import HETERO_EVERY, HETERO_TYPES, heterogeneous, max_ty
from .node import Node, P


class Gen:
    def __init__(self, rng, p_unsafe=0.06, p_confuse=0.012):
        self.rng = rng
        self.p_unsafe = p_unsafe
        self.p_confuse = p_confuse
        self.pool = set(STRS)
        self.top_index = 0

    # -- entry point ------------------------------------------------------
    def top(self):
        self.nvar = 0
        self.env = []
        self.confused = False
        i = self.top_index
        self.top_index += 1
        if i % HETERO_EVERY == 0:
            return heterogeneous(self, i // HETERO_EVERY)[0]
        t = self.rand_type(2)
        return self.expr(t, self.rng.randint(2, 5))

    def fresh(self, prefix="x"):
        self.nvar += 1
        return f"{prefix}{self.nvar}"

    def unsafe(self):
        return self.rng.random() < self.p_unsafe

    def rand_scalar(self, small=False):
        r = self.rng.random()
        if r < 0.5:
            return INT(3 if small or self.rng.random() < 0.5 else None)
        if r < 0.75:
            return STR
        if r < 0.9:
            return MV
        return BOOL

    def rand_type(self, d):
        r = self.rng
        if d <= 0:
            return self.rand_scalar()
        x = r.random()
        if x < 0.15:
            return BOOL
        if x < 0.32:
            return INT(None if r.random() < 0.7 else 6)
        if x < 0.37:
            return STR
        if x < 0.40:
            return MV
        if x < 0.62:
            return ("set", self.rand_type(d - 1) if r.random() < 0.3 else self.rand_scalar(), r.choice([4, 6, 8]))
        if x < 0.75:
            return ("seq", self.rand_type(d - 1) if r.random() < 0.3 else self.rand_scalar(), r.choice([3, 4, 5]))
        if x < 0.86:
            return ("fcn", self.rand_scalar(small=True), self.rand_type(d - 1) if r.random() < 0.3 else self.rand_scalar(), r.choice([3, 4]))
        if x < 0.93:
            fs = sorted(r.sample(FIELDS, r.randint(1, 3)))
            return ("rec", tuple((f, self.rand_type(d - 1) if r.random() < 0.2 else self.rand_scalar()) for f in fs))
        if x < 0.97:
            return ("tup", tuple(self.rand_type(d - 1) if r.random() < 0.2 else self.rand_scalar() for _ in range(r.randint(1, 3))))
        return ("bag", self.rand_scalar(small=True), 4)

    # -- environment --------------------------------------------------------
    def with_var(self, name, ty, f):
        self.env.append((name, ty))
        try:
            return f()
        finally:
            self.env.pop()

    def vars_fitting(self, t):
        out, seen_at = [], False
        for name, ty in reversed(self.env):
            if name == "@":
                if seen_at:
                    continue
                seen_at = True
            if fits(ty, t):
                out.append((name, ty))
        return out

    # -- expressions ----------------------------------------------------------
    def expr(self, t, d):
        r = self.rng
        if not self.confused and d > 0 and r.random() < self.p_confuse:
            # A type error probe: a value of another type where `t` is expected.
            self.confused = True
            u = self.rand_type(1)
            if base(u) != base(t):
                n = self.expr(u, d - 1)
                return Node(P(0), [n], t, tag="confuse")
        vs = self.vars_fitting(t)
        if vs and r.random() < (0.6 if d <= 0 else 0.25):
            name, ty = r.choice(vs)
            return Node(P(name), [], ty, ref=name)
        if d <= 0 or r.random() < 0.12:
            return self.literal(t)
        k = t[0]
        prods = getattr(self, "p_" + k)
        for _ in range(4):
            if r.random() < 0.18:
                n = self.generic(t, d)
            else:
                n = prods(t, d)
            if n is not None:
                return n
        return self.literal(t)

    def generic(self, t, d):
        r = self.rng
        c = r.randrange(9)
        if c == 0:
            return Node(P("(IF ", 0, " THEN ", 1, " ELSE ", 2, ")"), [self.expr(BOOL, d - 1), self.expr(t, d - 1), self.expr(t, d - 1)], t,
                        svals=None)
        if c == 1:
            kids = [self.expr(BOOL, d - 1), self.expr(t, d - 1), self.expr(BOOL, d - 1), self.expr(t, d - 1)]
            parts = P("(CASE ", 0, " -> ", 1, " [] ", 2, " -> ", 3)
            if r.random() < 0.8 or not self.unsafe():
                kids.append(self.expr(t, d - 1))
                parts += [" [] OTHER -> ", 4]
            return Node(parts + [")"], kids, t)
        if c == 2:
            u = self.rand_type(1)
            e = self.expr(u, d - 1)
            y = self.fresh()
            body = self.with_var(y, e.ty, lambda: self.expr(t, d - 1))
            return Node(P(f"(LET {y} == ", 0, " IN ", 1, ")"), [e, body], t, binds=(y,))
        if c == 3:
            s = self.expr(("set", t, 6), d - 1)
            x = self.fresh()
            et = s.ty[1] if s.ty[0] == "set" else t
            p = self.with_var(x, et, lambda: self.expr(BOOL, d - 1))
            if not self.unsafe():
                # Guarded: CHOOSE only if a witness exists.
                sn = self.fresh("S")
                dflt = self.expr(et, d - 1)
                return Node(P(f"(LET {sn} == ", 0, f" IN IF \\E {x} \\in {sn} : ", 1, f" THEN CHOOSE {x} \\in {sn} : ", 1, " ELSE ", 2, ")"),
                            [s, p, dflt], et, binds=(x,))
            return Node(P(f"(CHOOSE {x} \\in ", 0, " : ", 1, ")"), [s, p], et, binds=(x,))
        if c == 4 and SCALAR(t):
            # f[k]
            dt = self.rand_scalar(small=True)
            f = self.expr(("fcn", dt, t, 4), d - 1)
            k = self.expr(dt, d - 1)
            if self.unsafe():
                return Node(P("(", 0, ")[", 1, "]"), [f, k], t)
            dflt = self.expr(t, d - 1)
            F, K = self.fresh("F"), self.fresh("K")
            return Node(P(f"(LET {F} == ", 0, f" IN LET {K} == ", 1, f" IN IF {K} \\in DOMAIN {F} THEN {F}[{K}] ELSE ", 2, ")"), [f, k, dflt], t)
        if c == 5:
            # Head / s[i] / Last
            s = self.expr(("seq", t, 4), d - 1)
            dflt = self.expr(t, d - 1)
            op = r.choice(["Head", "Last", "idx"])
            if self.unsafe():
                if op == "idx":
                    return Node(P("(", 0, ")[", 1, "]"), [s, self.expr(INT(3), d - 1)], t)
                return Node(P(f"{op}(", 0, ")"), [s], t)
            S, I = self.fresh("S"), self.fresh("I")
            if op == "idx":
                return Node(P(f"(LET {S} == ", 0, f" IN LET {I} == ", 1, f" IN IF {I} \\in 1..Len({S}) THEN {S}[{I}] ELSE ", 2, ")"),
                            [s, self.expr(INT(3), d - 1), dflt], t)
            return Node(P(f"(LET {S} == ", 0, f" IN IF {S} = <<>> THEN ", 1, f" ELSE {op}({S}))"), [s, dflt], t)
        if c == 6:
            # record field
            fs = sorted(r.sample(FIELDS, r.randint(1, 3)))
            f = r.choice(fs)
            rt = ("rec", tuple((g, t if g == f else self.rand_scalar()) for g in fs))
            return Node(P("(", 0, f").{f}"), [self.expr(rt, d - 1)], t)
        if c == 7:
            # tuple projection
            n = r.randint(1, 3)
            i = r.randint(1, n)
            tt = ("tup", tuple(t if j == i else self.rand_scalar() for j in range(1, n + 1)))
            return Node(P("(", 0, f")[{i}]"), [self.expr(tt, d - 1)], t)
        if c == 8:
            # LET with an operator definition
            u = self.rand_scalar()
            y = self.fresh()
            g = f"G{y}"
            body_def = self.with_var(y, u, lambda: self.expr(t, d - 1))
            arg = self.expr(u, d - 1)
            n = Node(P(f"(LET {g}({y}) == ", 0, f" IN {g}(", 1, "))"), [body_def, arg], t, binds=(y,))
            # (y is bound only in kid 0; free-variable bookkeeping is approximate)
            return n
        return None

    # -- literals ------------------------------------------------------------
    def int_lit(self, b):
        r = self.rng
        if b is None and r.random() < 0.12:
            v = r.choice([INT_MAX, -INT_MAX, INT_MAX - 1, 65536, 46341, 1000000, -1000000])
        else:
            m = 5 if b is None else min(b, 5)
            v = r.randint(-min(3, m), m)
        return Node(P(f"({v})" if v < 0 else str(v)), [], INT(abs(v)), lit=True)

    def literal(self, t):
        r = self.rng
        k = t[0]
        if k == "mixed":
            return self.literal(self.rng.choice(HETERO_TYPES))
        if k == "bool":
            return Node(P(r.choice(["TRUE", "FALSE"])), [], BOOL, lit=True)
        if k == "int":
            return self.int_lit(t[1])
        if k == "str":
            s = r.choice(STRS)
            return Node(P(f'"{s}"'), [], STR, lit=True, svals={s})
        if k == "mv":
            return Node(P(r.choice(MVS)), [], MV, lit=True)
        if k == "set":
            n = r.randint(0, min(t[2], 3))
            kids = [self.literal(t[1]) for _ in range(n)]
            return self.enum("{", "}", kids, ("set", self.join_ty(t[1], kids), n), lit=True)
        if k == "seq":
            n = r.randint(0, min(t[2], 3))
            kids = [self.literal(t[1]) for _ in range(n)]
            return self.enum("<<", ">>", kids, ("seq", self.join_ty(t[1], kids), n), lit=True)
        if k == "fcn":
            if t[3] < 1:
                return Node(P("[e \\in {} |-> ", 0, "]"), [self.literal(t[2])], ("fcn", t[1], t[2], 0), lit=True)
            n = r.randint(1, min(t[3], 3))
            kids = []
            for _ in range(n):
                kids += [self.literal(t[1]), self.literal(t[2])]
            parts = ["("]
            for i in range(n):
                parts += ([" @@ "] if i else []) + [2 * i, " :> ", 2 * i + 1]
            return Node(parts + [")"], kids, ("fcn", self.join_ty(t[1], kids[0::2]), self.join_ty(t[2], kids[1::2]), n), lit=True)
        if k == "rec":
            kids = [self.literal(x) for _, x in t[1]]
            parts = ["["]
            for i, (f, _) in enumerate(t[1]):
                parts += ([", "] if i else []) + [f"{f} |-> ", i]
            return Node(parts + ["]"], kids, ("rec", tuple((f, kk.ty) for (f, _), kk in zip(t[1], kids))), lit=True)
        if k == "tup":
            kids = [self.literal(x) for x in t[1]]
            return self.enum("<<", ">>", kids, ("tup", tuple(kk.ty for kk in kids)), lit=True)
        if k == "bag":
            n = r.randint(0, min(t[2], 3))
            if n == 0:
                return Node(P("EmptyBag"), [], ("bag", t[1], 0), lit=True)
            kids = []
            for _ in range(n):
                kids += [self.literal(t[1]), Node(P(str(r.randint(1, 3))), [], INT(3), lit=True)]
            parts = ["("]
            for i in range(n):
                parts += ([" @@ "] if i else []) + [2 * i, " :> ", 2 * i + 1]
            return Node(parts + [")"], kids, ("bag", self.join_ty(t[1], kids[0::2]), n), lit=True)
        raise ValueError(t)

    def join_ty(self, t, kids):
        """The element type of a collection of `kids` requested as `t`."""
        return t if not kids else max_ty([k.ty for k in kids], t)

    def enum(self, lb, rb, kids, ty, lit=False):
        parts = [lb]
        for i in range(len(kids)):
            parts += ([", "] if i else []) + [i]
        return Node(parts + [rb], kids, ty, lit=lit and all(k.lit for k in kids))

    # -- productions per type ----------------------------------------------------
    def p_bool(self, t, d):
        r = self.rng
        c = r.randrange(16)
        b = lambda: self.expr(BOOL, d - 1)  # noqa: E731
        if c == 0:
            return Node(P("~(", 0, ")"), [b()], BOOL)
        if c == 1:
            op = r.choice(["/\\", "\\/", "=>", "<=>"])
            return Node(P("(", 0, f" {op} ", 1, ")"), [b(), b()], BOOL)
        if c == 2:
            u = self.rand_type(1)
            op = r.choice(["=", "#"])
            if self.unsafe():
                v = self.rand_type(1)
                return Node(P("(", 0, f" {op} ", 1, ")"), [self.expr(u, d - 1), self.expr(v, d - 1)], BOOL)
            return Node(P("(", 0, f" {op} ", 1, ")"), [self.expr(u, d - 1), self.expr(u, d - 1)], BOOL)
        if c == 3:
            op = r.choice(["<", "<=", ">", ">=", "=<", "\\leq", "\\geq"])
            return Node(P("(", 0, f" {op} ", 1, ")"), [self.expr(INT(), d - 1), self.expr(INT(), d - 1)], BOOL)
        if c == 4:
            u = self.rand_type(1)
            s = self.expr(("set", u, 8), d - 1)
            op = r.choice(["\\in", "\\notin"])
            return Node(P("(", 0, f" {op} ", 1, ")"), [self.expr(u, d - 1), s], BOOL)
        if c == 5:
            u = self.rand_type(1)
            return Node(P("(", 0, " \\subseteq ", 1, ")"), [self.expr(("set", u, 6), d - 1), self.expr(("set", u, 6), d - 1)], BOOL)
        if c in (6, 7):
            u = self.rand_type(1)
            s = self.expr(("set", u, 6), d - 1)
            x = self.fresh()
            p = self.with_var(x, s.ty[1] if s.ty[0] == "set" else u, lambda: self.expr(BOOL, d - 1))
            q = r.choice(["\\A", "\\E"])
            return Node(P(f"({q} {x} \\in ", 0, " : ", 1, ")"), [s, p], BOOL, binds=(x,))
        if c == 8:
            # Two bound variables.
            u = self.rand_scalar()
            s = self.expr(("set", u, 4), d - 1)
            x, y = self.fresh(), self.fresh()
            et = s.ty[1] if s.ty[0] == "set" else u
            p = self.with_var(x, et, lambda: self.with_var(y, et, lambda: self.expr(BOOL, d - 1)))
            q = r.choice(["\\A", "\\E"])
            return Node(P(f"({q} {x}, {y} \\in ", 0, " : ", 1, ")"), [s, p], BOOL, binds=(x, y))
        if c == 9:
            u = self.rand_scalar()
            return Node(P("IsFiniteSet(", 0, ")"), [self.expr(("set", u, 6), d - 1)], BOOL)
        if c == 10:
            u = self.rand_scalar()
            op = r.choice(["IsPrefix", "IsSuffix", "IsStrictPrefix"])
            return Node(P(f"{op}(", 0, ", ", 1, ")"), [self.expr(("seq", u, 3), d - 1), self.expr(("seq", u, 4), d - 1)], BOOL)
        if c == 11:
            u = self.rand_scalar()
            return Node(P("Contains(", 0, ", ", 1, ")"), [self.expr(("seq", u, 4), d - 1), self.expr(u, d - 1)], BOOL)
        if c == 12:
            u = self.rand_scalar(small=True)
            return Node(P("IsInjective(", 0, ")"), [self.expr(("fcn", u, self.rand_scalar(), 4), d - 1)], BOOL)
        if c == 13:
            u = self.rand_scalar(small=True)
            if r.random() < 0.5:
                return Node(P("BagIn(", 0, ", ", 1, ")"), [self.expr(u, d - 1), self.expr(("bag", u, 4), d - 1)], BOOL)
            return Node(P("(", 0, " \\sqsubseteq ", 1, ")"), [self.expr(("bag", u, 4), d - 1), self.expr(("bag", u, 4), d - 1)], BOOL)
        if c == 14:
            u = self.rand_type(1)
            return Node(P("(ToString(", 0, ") = ToString(", 1, "))"), [self.expr(u, d - 1), self.expr(u, d - 1)], BOOL)
        if c == 15:
            return Node(P("IsSorted(", 0, ", LAMBDA p, q : p < q)"), [self.expr(("seq", INT(), 4), d - 1)], BOOL)
        return None

    def p_int(self, t, d):
        r = self.rng
        b = t[1]
        c = r.randrange(16)
        if c in (0, 1):
            op = r.choice(["+", "-", "*"])
            if b is None:
                kb = None
            elif op == "*":
                kb = max(1, int(b ** 0.5))
            else:
                kb = b // 2
            if b is not None and kb < 1:
                return None
            return Node(P("(", 0, f" {op} ", 1, ")"), [self.expr(INT(kb), d - 1), self.expr(INT(kb), d - 1)], INT(b))
        if c == 2:
            return Node(P("(-", 0, ")"), [self.expr(INT(b), d - 1)], INT(b))
        if c == 3:
            op = r.choice(["\\div", "%"])
            x = self.expr(INT(b), d - 1)
            if self.unsafe():
                y = self.expr(INT(), d - 1)
            else:
                v = r.choice([1, 2, 3, 5] + ([-1, -2, -3] if op == "\\div" else []))
                y = Node(P(f"({v})" if v < 0 else str(v)), [], INT(abs(v)), lit=True)
            return Node(P("(", 0, f" {op} ", 1, ")"), [x, y], INT(b))
        if c == 4 and b is None:
            e = r.randint(0, 4) if not self.unsafe() else r.choice([0, 31, 32, -1])
            en = Node(P(f"({e})" if e < 0 else str(e)), [], INT(abs(e)), lit=True)
            return Node(P("(", 0, " ^ ", 1, ")"), [self.expr(INT(6 if not self.unsafe() else None), d - 1), en], INT(b))
        if c == 5:
            n = 8 if b is None else min(b, 8)
            return Node(P("Cardinality(", 0, ")"), [self.expr(("set", self.rand_type(1), n), d - 1)], INT(n))
        if c == 6:
            n = 5 if b is None else min(b, 5)
            return Node(P("Len(", 0, ")"), [self.expr(("seq", self.rand_type(1), n), d - 1)], INT(n))
        if c == 7:
            op = r.choice(["Max", "Min"])
            s = self.expr(("set", INT(b), 6), d - 1)
            return Node(P(f"{op}(", 0, ")"), [s], INT(b))
        if b is not None:
            return None
        if c == 8:
            op = r.choice(["SumSet", "ProductSet"])
            return Node(P(f"{op}(", 0, ")"), [self.expr(("set", INT(), 5), d - 1)], INT())
        if c == 9:
            u = self.rand_scalar()
            s = self.expr(("set", u, 6), d - 1)
            x = self.fresh()
            p = self.with_var(x, s.ty[1] if s.ty[0] == "set" else u, lambda: self.expr(BOOL, d - 1))
            return Node(P(f"Quantify(", 0, f", LAMBDA {x} : ", 1, ")"), [s, p], INT(), binds=(x,))
        if c == 10:
            u = self.rand_scalar(small=True)
            if r.random() < 0.5:
                return Node(P("BagCardinality(", 0, ")"), [self.expr(("bag", u, 4), d - 1)], INT())
            return Node(P("CopiesIn(", 0, ", ", 1, ")"), [self.expr(u, d - 1), self.expr(("bag", u, 4), d - 1)], INT())
        if c == 11:
            # (Commutative only: TLC folds in the set's internal order, which
            # for e.g. DOMAIN (5 :> 1 @@ 3 :> 1) is not the normalized one.)
            op = r.choice(["+", "*"])
            s = self.expr(("set", INT(), 5), d - 1)
            return Node(P(f"FoldSet(LAMBDA p, q : p {op} q, ", 0, ", ", 1, ")"), [self.expr(INT(), d - 1), s], INT())
        if c == 12:
            fold = r.choice(["FoldLeft", "FoldRight", "FoldSeq"])
            op = r.choice(["+", "-", "*"])
            s = self.expr(("seq", INT(), 4), d - 1)
            z = self.expr(INT(), d - 1)
            if fold == "FoldRight":
                return Node(P(f"FoldRight(LAMBDA p, q : p {op} q, ", 0, ", ", 1, ")"), [s, z], INT())
            return Node(P(f"{fold}(LAMBDA p, q : p {op} q, ", 0, ", ", 1, ")"), [z, s], INT())
        if c == 13:
            u = self.rand_scalar(small=True)
            return Node(P("SumFunction(", 0, ")"), [self.expr(("fcn", u, INT(), 4), d - 1)], INT())
        if c == 14:
            return Node(P("Len(ToString(", 0, "))"), [self.expr(self.rand_type(1), d - 1)], INT())
        if c == 15:
            u = self.rand_scalar()
            s = self.expr(("set", u, 5), d - 1)
            x = self.fresh()
            body = self.with_var(x, s.ty[1] if s.ty[0] == "set" else u, lambda: self.expr(INT(6), d - 1))
            S, e = self.fresh("S"), self.fresh("e")
            return Node(P(f"MapThenFoldSet(LAMBDA p, q : p + q, 0, LAMBDA {x} : ", 1, f", LAMBDA {S} : CHOOSE {e} \\in {S} : TRUE, ", 0, ")"),
                        [s, body], INT(), binds=(x,))
        return None

    def p_str(self, t, d):
        # Concatenation of strings whose possible values are known (all
        # results are interned up front by the Pool).
        def known(n):
            return n if n.svals is not None else self.literal(STR)
        a, b = known(self.expr(STR, d - 1)), known(self.expr(STR, d - 1))
        vals = {x + y for x in a.svals for y in b.svals}
        if len(vals) > 12:
            return None
        self.pool |= vals
        return Node(P("(", 0, " \\o ", 1, ")"), [a, b], STR, svals=vals)

    def p_mv(self, t, d):
        return None

    def p_set(self, t, d):
        r = self.rng
        et, n = t[1], t[2]
        c = r.randrange(18)
        if c == 0:
            k = r.randint(0, min(n, 3))
            kids = [self.expr(et, d - 1) for _ in range(k)]
            return self.enum("{", "}", kids, ("set", self.join_ty(et, kids), k))
        if c == 1 and et[0] == "int":
            eb = (n - 1) // 2
            if et[1] is not None:
                eb = min(eb, et[1])
            if eb < 1:
                return None
            lo, hi = self.expr(INT(eb), d - 1), self.expr(INT(eb), d - 1)
            return Node(P("(", 0, "..", 1, ")"), [lo, hi], ("set", INT(eb), 2 * eb + 1))
        if c == 2 and n >= 2:
            a, b = self.expr(("set", et, n // 2), d - 1), self.expr(("set", et, n // 2), d - 1)
            op = r.choice(["\\cup", "\\union"])
            return Node(P("(", 0, f" {op} ", 1, ")"), [a, b], ("set", max_ty([a.ty[1], b.ty[1]], et), a.ty[2] + b.ty[2]) if a.ty[0] == b.ty[0] == "set" else t)
        if c == 3:
            a, b = self.expr(("set", et, n), d - 1), self.expr(("set", et, n), d - 1)
            op = r.choice(["\\cap", "\\", "\\intersect"])
            return Node(P("(", 0, f" {op} ", 1, ")"), [a, b], a.ty)
        if c == 4 and n >= 2:
            a, b = self.expr(("set", et, n // 2), d - 1), self.expr(("set", et, n // 2), d - 1)
            return Node(P("SymDiff(", 0, ", ", 1, ")"), [a, b], t)
        if c in (5, 6):
            u = self.rand_type(1)
            s = self.expr(("set", u, n), d - 1)
            x = self.fresh()
            e = self.with_var(x, s.ty[1] if s.ty[0] == "set" else u, lambda: self.expr(et, d - 1))
            return Node(P("{", 1, f" : {x} \\in ", 0, "}"), [s, e], ("set", e.ty, n), binds=(x,))
        if c == 7:
            s = self.expr(("set", et, n), d - 1)
            x = self.fresh()
            p = self.with_var(x, s.ty[1] if s.ty[0] == "set" else et, lambda: self.expr(BOOL, d - 1))
            return Node(P(f"{{{x} \\in ", 0, " : ", 1, "}"), [s, p], s.ty, binds=(x,))
        if c == 8:
            m = r.choice([1, 2, 3])
            k = max(1, n // m)
            ss = self.expr(("set", ("set", et, k), m), d - 1)
            op = r.choice(["UNION ", "FlattenSet("])
            return Node(P(op, 0, ")" if op.endswith("(") else ""), [ss], t)
        if c == 9 and SCALAR(et):
            f = self.expr(("fcn", et, self.rand_scalar(), n), d - 1)
            return Node(P("DOMAIN ", 0), [f], t)
        if c == 10:
            ds = self.rand_scalar(small=True)
            f = self.expr(("fcn", ds, et, n), d - 1)
            return Node(P("Range(", 0, ")"), [f], t)
        if c == 11:
            s = self.expr(("seq", et, n), d - 1)
            return Node(P("ToSet(", 0, ")"), [s], t)
        if c == 12 and et[0] == "set":
            m = min(3, n.bit_length() - 1, et[2])
            if m < 1:
                return None
            s = self.expr(("set", et[1], m), d - 1)
            return Node(P("SUBSET ", 0), [s], ("set", ("set", s.ty[1], m) if s.ty[0] == "set" else et, 2 ** m))
        if c == 13 and et[0] == "fcn" and base(et[1]) == base(et[2]) and n >= 6 and et[3] >= 3:
            s = self.expr(("set", et[1], 3), d - 1)
            return Node(P("Permutations(", 0, ")"), [s], t)
        if c == 14 and et[0] == "fcn":
            # [S -> T]
            k = r.randint(1, min(et[3], 2))
            m = max([j for j in (1, 2, 3) if j ** k <= n] or [0])
            if m < 1:
                return None
            s, tt = self.expr(("set", et[1], k), d - 1), self.expr(("set", et[2], m), d - 1)
            return Node(P("[", 0, " -> ", 1, "]"), [s, tt], t)
        if c == 15 and et[0] == "tup" and len(et[1]) == 2:
            a = self.expr(("set", et[1][0], 3), d - 1)
            b = self.expr(("set", et[1][1], max(1, n // 3)), d - 1)
            return Node(P("(", 0, " \\X ", 1, ")"), [a, b], t)
        if c == 16 and et[0] == "rec":
            m = 2 if 2 ** len(et[1]) <= n else 1
            kids = [self.expr(("set", x, m), d - 1) for _, x in et[1]]
            parts = ["["]
            for i, (f, _) in enumerate(et[1]):
                parts += ([", "] if i else []) + [f"{f} : ", i]
            return Node(parts + ["]"], kids, t)
        if c == 17 and SCALAR(et):
            b = self.expr(("bag", et, n), d - 1)
            return Node(P("BagToSet(", 0, ")"), [b], t)
        return None

    def p_seq(self, t, d):
        r = self.rng
        et, n = t[1], t[2]
        c = r.randrange(17)
        s = lambda m: self.expr(("seq", et, m), d - 1)  # noqa: E731
        if c == 0:
            k = r.randint(0, min(n, 3))
            kids = [self.expr(et, d - 1) for _ in range(k)]
            return self.enum("<<", ">>", kids, ("seq", self.join_ty(et, kids), k))
        if c == 1 and n >= 1:
            return Node(P("Append(", 0, ", ", 1, ")"), [s(n - 1), self.expr(et, d - 1)], t)
        if c == 2:
            return Node(P("(", 0, " \\o ", 1, ")"), [s(n // 2), s(n - n // 2)], t)
        if c == 3:
            if self.unsafe():
                return Node(P("Tail(", 0, ")"), [s(n)], t)
            S = self.fresh("S")
            return Node(P(f"(LET {S} == ", 0, f" IN IF {S} = <<>> THEN {S} ELSE Tail({S}))"), [s(n)], t)
        if c == 4:
            lo, hi = self.expr(INT(3), d - 1), self.expr(INT(4), d - 1)
            if self.unsafe():
                return Node(P("SubSeq(", 0, ", ", 1, ", ", 2, ")"), [s(n), lo, hi], t)
            S, M, N = self.fresh("S"), self.fresh("M"), self.fresh("N")
            return Node(P(f"(LET {S} == ", 0, f" IN LET {M} == ", 1, f" IN LET {N} == ", 2,
                          f" IN IF 1 <= {M} /\\ {N} <= Len({S}) THEN SubSeq({S}, {M}, {N}) ELSE {S})"), [s(n), lo, hi], t)
        if c == 5:
            x = self.fresh()
            q = s(n)
            p = self.with_var(x, q.ty[1] if q.ty[0] == "seq" else et, lambda: self.expr(BOOL, d - 1))
            return Node(P("SelectSeq(", 0, f", LAMBDA {x} : ", 1, ")"), [q, p], t, binds=(x,))
        if c == 6:
            k = r.randint(0, min(n, 4))
            x = self.fresh()
            e = self.with_var(x, INT(k), lambda: self.expr(et, d - 1))
            return Node(P(f"[{x} \\in 1..{k} |-> ", 0, "]"), [e], ("seq", e.ty, k), binds=(x,))
        if c == 7:
            return Node(P("Reverse(", 0, ")"), [s(n)], t)
        if c == 8:
            if self.unsafe():
                return Node(P("Front(", 0, ")"), [s(n)], t)
            S = self.fresh("S")
            return Node(P(f"(LET {S} == ", 0, f" IN IF {S} = <<>> THEN {S} ELSE Front({S}))"), [s(n)], t)
        if c == 9 and n >= 1:
            op = r.choice(["InsertAt", "ReplaceAt", "RemoveAt"])
            q = s(n - 1)
            i = self.expr(INT(3), d - 1)
            S, I, E = self.fresh("S"), self.fresh("I"), self.fresh("E")
            hi = f"Len({S}) + 1" if op == "InsertAt" else f"Len({S})"
            args = f", {I}, {E}" if op != "RemoveAt" else f", {I}"
            kids = [q, i] + ([self.expr(et, d - 1)] if op != "RemoveAt" else [])
            if self.unsafe():
                return Node(P(f"{op}(", 0, ", ", 1) + ([", ", 2] if op != "RemoveAt" else []) + [")"], kids, t)
            return Node(P(f"(LET {S} == ", 0, f" IN LET {I} == ", 1, (f" IN LET {E} == " if op != "RemoveAt" else ""))
                        + ([2] if op != "RemoveAt" else []) + [f" IN IF {I} \\in 1..{hi} THEN {op}({S}{args}) ELSE {S})"], kids, t)
        if c == 10 and n >= 1:
            return Node(P("Cons(", 0, ", ", 1, ")"), [self.expr(et, d - 1), s(n - 1)], t)
        if c == 11 and et[0] == "int":
            op = r.choice(["<", ">", "<="])
            return Node(P(f"SortSeq(", 0, f", LAMBDA p, q : p {op} q)"), [s(n)], t)
        if c == 12:
            return Node(P("SetToSeq(", 0, ")"), [self.expr(("set", et, n), d - 1)], t)
        if c == 13 and n >= 2:
            m = 2
            return Node(P("FlattenSeq(", 0, ")"), [self.expr(("seq", ("seq", et, n // m), m), d - 1)], t)
        if c == 14:
            return Node(P("Remove(", 0, ", ", 1, ")"), [s(n), self.expr(et, d - 1)], t)
        if c == 15:
            return Node(P("ReplaceAll(", 0, ", ", 1, ", ", 2, ")"), [s(n), self.expr(et, d - 1), self.expr(et, d - 1)], t)
        if c == 16 and et[0] == "int":
            op = r.choice(["<", ">"])
            return Node(P("SetToSortSeq(", 0, f", LAMBDA p, q : p {op} q)"), [self.expr(("set", et, n), d - 1)], t)
        return None

    def except_rhs(self, f_ty, path_kids, val_t, d):
        """The new value of an EXCEPT clause (may use @)."""
        if self.rng.random() < 0.4:
            return self.with_var("@", val_t, lambda: self.expr(val_t, d - 1))
        return self.expr(val_t, d - 1)

    def p_fcn(self, t, d):
        r = self.rng
        dt, rt, n = t[1], t[2], t[3]
        c = r.randrange(9)
        if c == 0:
            s = self.expr(("set", dt, n), d - 1)
            x = self.fresh()
            e = self.with_var(x, s.ty[1] if s.ty[0] == "set" else dt, lambda: self.expr(rt, d - 1))
            return Node(P(f"[{x} \\in ", 0, " |-> ", 1, "]"), [s, e], ("fcn", dt, e.ty, n), binds=(x,))
        if c in (1, 2, 3):
            # EXCEPT, possibly with nested paths and several clauses.
            self.env.append(("@", ("none",)))
            f = self.expr(t, d - 1)
            kids = [f]
            parts = ["[", 0, " EXCEPT "]
            for ci in range(r.choice([1, 1, 2])):
                parts += [", "] if ci else []
                k = self.expr(dt, d - 1)
                kids.append(k)
                parts += ["![", len(kids) - 1, "]"]
                vt = rt
                self.env.pop()
                if rt[0] in ("fcn", "rec") and r.random() < 0.5:
                    if rt[0] == "fcn":
                        self.env.append(("@", ("none",)))
                        k2 = self.expr(rt[1], d - 1)
                        self.env.pop()
                        kids.append(k2)
                        parts += ["[", len(kids) - 1, "]"]
                        vt = rt[2]
                    else:
                        fname, vt = r.choice(rt[1])
                        parts += [f".{fname}"]
                kids.append(self.except_rhs(t, None, vt, d))
                parts += [" = ", len(kids) - 1]
                self.env.append(("@", ("none",)))
            self.env.pop()
            n_ = Node(parts + ["]"], kids, f.ty, binds=("@",))
            return n_
        if c == 4 and n >= 2:
            a, b = self.expr(("fcn", dt, rt, n // 2), d - 1), self.expr(("fcn", dt, rt, n // 2), d - 1)
            return Node(P("(", 0, " @@ ", 1, ")"), [a, b], t)
        if c == 5:
            return Node(P("(", 0, " :> ", 1, ")"), [self.expr(dt, d - 1), self.expr(rt, d - 1)], ("fcn", dt, rt, 1))
        if c == 6:
            f = self.expr(t, d - 1)
            return Node(P("Restrict(", 0, ", ", 1, ")"), [f, self.expr(("set", dt, n), d - 1)], t)
        if c == 7 and dt[0] == "int":
            # A sequence is a function.
            return self.expr(("seq", rt, n), d - 1)
        if c == 8 and dt[0] == "int" and rt[0] == "int" and n >= 3:
            # A bag of integers is a function to integers.
            return self.expr(("bag", dt, n), d - 1)
        return None

    def p_rec(self, t, d):
        r = self.rng
        c = r.randrange(3)
        if c == 0:
            kids = [self.expr(x, d - 1) for _, x in t[1]]
            parts = ["["]
            for i, (f, _) in enumerate(t[1]):
                parts += ([", "] if i else []) + [f"{f} |-> ", i]
            return Node(parts + ["]"], kids, ("rec", tuple((f, k.ty) for (f, _), k in zip(t[1], kids))))
        if c == 1:
            f, vt = r.choice(t[1])
            rr = self.with_var("@", ("none",), lambda: self.expr(t, d - 1))
            v = self.except_rhs(t, None, vt, d)
            return Node(P("[", 0, f" EXCEPT !.{f} = ", 1, "]"), [rr, v], rr.ty, binds=("@",))
        if c == 2:
            # A record built as a function over its field names.
            if len(set(base(x) for _, x in t[1])) != 1:
                return None
            vt = t[1][0][1]
            x = self.fresh()
            e = self.with_var(x, STR, lambda: self.expr(vt, d - 1))
            names = ", ".join(f'"{f}"' for f, _ in t[1])
            return Node(P(f"[{x} \\in {{{names}}} |-> ", 0, "]"), [e], t, binds=(x,))
        return None

    def p_tup(self, t, d):
        r = self.rng
        if r.random() < 0.7:
            kids = [self.expr(x, d - 1) for x in t[1]]
            return self.enum("<<", ">>", kids, ("tup", tuple(k.ty for k in kids)))
        i = r.randint(1, len(t[1]))
        tt = self.with_var("@", ("none",), lambda: self.expr(t, d - 1))
        v = self.except_rhs(t, None, t[1][i - 1], d)
        return Node(P("[", 0, f" EXCEPT ![{i}] = ", 1, "]"), [tt, v], tt.ty, binds=("@",))

    def p_bag(self, t, d):
        r = self.rng
        et, n = t[1], t[2]
        c = r.randrange(5)
        if c == 0:
            return Node(P("SetToBag(", 0, ")"), [self.expr(("set", et, n), d - 1)], t)
        if c in (1, 2):
            op = r.choice(["(+)", "(-)", "\\oplus", "\\ominus"])
            return Node(P("(", 0, f" {op} ", 1, ")"), [self.expr(("bag", et, n // 2), d - 1), self.expr(("bag", et, n // 2), d - 1)], t)
        if c == 3:
            return Node(P("ToBag(", 0, ")"), [self.expr(("seq", et, n), d - 1)], t)
        if c == 4:
            x = self.fresh()
            b = self.expr(("bag", et, n), d - 1)
            e = self.with_var(x, et, lambda: self.expr(et, d - 1))
            return Node(P(f"BagOfAll(LAMBDA {x} : ", 1, ", ", 0, ")"), [b, e], t, binds=(x,))
        return None
