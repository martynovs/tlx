"""Seeded generation of finite whole-state specifications."""
import math
import random
from modelcheck import fuzz_expr as fx
from modelcheck.fuzz_expr import Node, P, INT, BOOL, STR, MV, base, fits
from .constants import UNIVERSE_CAP
from .expressions import StateGen, leaf, text_node
from .model import Action, Spec, fair_set_constant
from .shapes import state_type, size


class SpecGen:
    def __init__(self, seed, index):
        self.seed, self.index = seed, index
        self.rng = random.Random(f"fuzz-spec-{seed}-{index}")

    def spec(self):
        r = self.rng
        symmetric = r.random() < .3
        self.g = g = StateGen(r, symmetric)
        name = f"F{self.seed}_{self.index}"
        if self.index % fx.HETERO_EVERY == 0:
            return self.heterogeneous_spec(name)
        nv = r.choice([1, 2, 2, 3, 3, 4])
        for _ in range(30):
            types = [state_type(r) for _ in range(nv)]
            if math.prod(size(t) for t in types) <= UNIVERSE_CAP:
                break
            nv = max(1, nv - (r.random() < .3))
        else:
            types = [INT(2)] * nv
        vs = [(f"v{i + 1}", t) for i, t in enumerate(types)]
        s = Spec(name, vs, {}, [], variant=name)
        s.features["symmetric"] = symmetric
        self.state_env = [(v, t) for v, t in vs]
        self.const_env = []
        if r.random() < .3:
            # (No negative numbers: TLC's cfg parser rejects them in a set; tlx accepts them.)
            choices = [("K", INT(2), str(r.randint(0, 2))), ("SetC", ("set", MV, 2), "{m1, m2}"),
                       ("SetN", ("set", INT(1), 2), r.choice(["{0, 1}", "{1}", "{0}"]))]
            s.consts = r.sample(choices, r.randint(1, 2))
        if r.random() < .2:
            a, b = r.sample(range(-2, 3), 2)
            s.overrides = [("Lim", INT(2), str(a), str(b))]
        self.const_env = [(c, t) for c, t, _ in s.consts] + [(o, t) for o, t, _, _ in s.overrides]
        if r.random() < .25:
            s.instance = r.choice(vs)
            self.state_env.append(("I!W", s.instance[1]))
        for v, t in vs:
            s.init[v] = self.init_clause(v, t)
        for ai in range(r.choice([1, 2, 2, 3, 3, 4])):
            s.actions.append(self.action(s, f"A{ai + 1}"))
        # Invariants (TypeOK holds unless an update escapes its shape).
        s.typeok = r.random() < .4
        for i in range(r.choice([0, 0, 1, 1, 2])):
            s.invariants.append((f"Inv{i + 1}", self.invariant(s)))
        if r.random() < .15:
            s.aprops.append(("AProp1", "action", self.over_steps(BOOL, 2)))
        if r.random() < .08:
            s.aprops.append(("BoxProp1", "box", self.over_state(BOOL, 2)))
        if r.random() < .2:
            s.constraint = self.over_state(BOOL, 2) if r.random() < .4 else self.bound_pred(s)
        if r.random() < .15:
            s.aconstraint = self.over_steps(BOOL, 2)
        if r.random() < .15:
            s.view = self.view(s)
        if symmetric and r.random() < .8:
            s.symmetry = list(fx.MVS) if r.random() < .75 else fx.MVS[:2]
        if r.random() < .25:
            s.live = self.liveness()
            s.fairness = self.fairness(s)
        s.deadlock = r.random() < .6
        s.pool = sorted(g.pool)
        r.shuffle(s.pool)
        return s

    def heterogeneous_spec(self, name):
        probe, inputs = fx.heterogeneous(self.g, self.index // fx.HETERO_EVERY, state=True)
        # Each input is stored as a state variable; the probe is evaluated in
        # Next, after the initial fingerprint. pc bounds the graph to two states.
        vs = [(v, fx.MIXED) for v in inputs] + [("pc", INT(1)), ("r", fx.MIXED)]
        init = {v: ("eq", n) for v, n in inputs.items()}
        init.update(pc=("eq", text_node("0", INT(1))), r=("eq", text_node("0", INT(1))))
        action = Action("A1", guards=[text_node("pc = 0", BOOL)],
                        updates={"pc": ("eq", text_node("1", INT(1))), "r": ("eq", probe)}, order=["pc", "r"])
        return Spec(name, vs, init, [action], deadlock=False, ok_guards=False,
                    pool=sorted(self.g.pool), variant=name, features={"heterogeneous": probe.tag.split(":")[1],
                              "heterogeneous_kinds": sorted({n.ty[0] for e in inputs.values()
                                  for n in [e, *fx.descendants(e)] if n.tag == "heterogeneous-element"})})

    # -- pieces ------------------------------------------------------------
    def reads_state(self, f, tries=4):
        """f() until it reads a variable (a few tries): constant-level
        predicates are rare in real specs."""
        names = {v for v, _ in self.state_env} | {v + "'" for v, _ in self.state_env}
        n = f()
        for _ in range(tries):
            if n.free & names:
                return n
            n = f()
        if n.free & names or n.ty != BOOL or self.rng.random() < .1:
            return n
        v, t = self.rng.choice(self.state_env)
        return Node(P("(", 0, " /\\ ", 1, " = ", 1, ")"), [n, leaf(v, t)], BOOL)

    def env0(self):
        """What expressions over the state may read: constants, cfg
        overridden definitions, the variables (and the instance's view of
        one)."""
        return self.const_env + self.state_env

    def over_state(self, t, d, extra=()):
        return self.reads_state(lambda: self.g.scoped(self.env0() + list(extra), lambda: self.g.expr(t, d)))

    def over_steps(self, t, d):
        primed = [(v + "'", ty) for v, ty in self.state_env]
        return self.reads_state(lambda: self.g.scoped(self.env0() + primed, lambda: self.g.expr(t, d)))

    def init_clause(self, v, t):
        r, g = self.rng, self.g
        if t[0] == "fcn" and r.random() < .5:
            return ("eq", g.scoped(self.const_env, lambda: self.fcn_over_domain(t, None, 2)))
        if r.random() < .3:
            return ("in", g.scoped(self.const_env, lambda: self.choice_set(t)))
        return ("eq", g.scoped(self.const_env, lambda: g.expr(t, 2)))

    def choice_set(self, t):
        """A set of values of shape `t` for `x \\in S`: mostly nonempty by
        construction (S \\cup {e})."""
        g = self.g
        if self.rng.random() < .25:
            return g.expr(("set", t, 3), 2)
        # ((S \\ {e}) \\cup {e}: disjoint, so that TLC's enumeration has no duplicates, see
        # tests/fuzz/specs/FzCupDupGenerated.)
        S, e = g.expr(("set", t, 2), 2), g.expr(t, 1)
        return Node(P("((", 0, " \\ {", 1, "}) \\cup {", 1, "})"), [S, e], ("set", t, 3))

    def domain(self, dt, n, var=None):
        """A domain for a function: enumerated, an interval, the model
        values, or (with `var` a function variable) its run-time DOMAIN."""
        r, g = self.rng, self.g
        c = r.randrange(4)
        if var is not None and c == 0:
            return Node(P("DOMAIN ", 0), [leaf(*var)], ("set", dt, n))
        if dt[0] == "int" and c == 1:
            lo = r.choice([-1, 0])
            hi = min(dt[1], lo + n - 1)
            return text_node(f"{lo}..{hi}" if lo >= 0 else f"({lo})..{hi}", ("set", INT(max(abs(lo), hi)), hi - lo + 1))
        if dt[0] == "mv" and c == 1 and n >= 3:
            return text_node("{" + ", ".join(fx.MVS) + "}", ("set", MV, 3))
        return g.expr(("set", dt, n), 1)

    def fcn_over_domain(self, t, var, d):
        g = self.g
        dom = self.domain(t[1], t[3], var)
        x = g.fresh()
        et = dom.ty[1] if dom.ty[0] == "set" else t[1]
        body = g.with_var(x, et, lambda: g.expr(t[2], d))
        return Node(P(f"[{x} \\in ", 0, " |-> ", 1, "]"), [dom, body], t, binds=(x,))

    def param(self, s, prefix):
        r, g = self.rng, self.g
        name = f"{prefix}{g.fresh('')}"
        sets = [v for v, t in s.vars if t[0] == "set" and fx.SCALAR(t[1])]
        fcns = [v for v, t in s.vars if t[0] == "fcn"]
        c = r.randrange(6)
        if c == 0 and sets:
            v = r.choice(sets)
            t = s.var_type(v)
            return name, t[1], leaf(v, t)
        if c == 1 and fcns:
            v = r.choice(fcns)
            t = s.var_type(v)
            return name, t[1], Node(P("DOMAIN ", 0), [leaf(v, t)], ("set", t[1], t[3]))
        if c in (2, 3) or g.symmetric:
            return name, MV, text_node("{" + ", ".join(fx.MVS) + "}", ("set", MV, 3))
        if c == 4:
            return name, INT(1), text_node(r.choice(["0..1", "(-1)..1", "{0, 1}"]), ("set", INT(1), 3))
        return name, STR, text_node('{"a", "b"}', ("set", STR, 2))

    def action(self, s, name):
        r, g = self.rng, self.g
        a = Action(name)
        env = self.env0()
        if r.random() < .45:
            a.param = self.param(s, "p")
            env.append(a.param[:2])
        for _ in range(r.choice([0, 0, 1, 1, 2])):
            a.guards.append(self.reads_state(lambda: g.scoped(env, lambda: g.expr(BOOL, 2))) if r.random() < .5
                            else self.bound_pred(s))
        names = [v for v, _ in s.vars]
        k = r.choice([1, 1, 1, 2, 2, len(names)]) if r.random() > .05 else 0
        a.order = r.sample(names, min(k, len(names)))
        if a.order and r.random() < .2:
            a.inner = self.param(s, "q")
            env.append(a.inner[:2])
        a.updates = self.updates(s, a.order, env)
        if a.order and r.random() < .15:
            cond = g.scoped(env, lambda: g.expr(BOOL, 2))
            a.ite = (cond, self.updates(s, a.order, env))
        a.unchanged_style = "UNCHANGED" if r.random() < .7 else "eq"
        a.inst_same = s.instance is not None and r.random() < .5
        return a

    def updates(self, s, order, env):
        r, g = self.rng, self.g
        out = {}
        env = list(env)
        for v in order:
            t = s.var_type(v)
            if s.instance and s.instance[0] == v and r.random() < .4:
                # The update through the INSTANCE: I!Put(e) is w' = e with w <- v.
                out[v] = ("iput", g.scoped(env, lambda: g.expr(t, 2)))
            elif r.random() < .25:
                out[v] = ("in", g.scoped(env, lambda: self.choice_set(t)))
            elif r.random() < .6:
                out[v] = ("eq", g.scoped(env, lambda: self.template(v, t)))
            else:
                out[v] = ("eq", g.scoped(env, lambda: g.expr(t, 2)))
            if r.random() < .3:
                # Later conjuncts may read this variable's new value.
                env.append((v + "'", t))
        return out

    def template(self, v, t):
        """An update of `v` in the idioms of real specs (the guard OkV
        keeps it inside its shape)."""
        r, g = self.rng, self.g
        x = leaf(v, t)
        k = t[0]
        e = lambda ty, d=1: g.expr(ty, d)  # noqa: E731
        if k == "int":
            b = t[1]
            c = r.randrange(3)
            if c == 0:
                return Node(P("((", 0, f" + 1) % {b + 1})"), [x], t)
            if c == 1:
                return Node(P("(IF ", 0, f" < {b} THEN ", 0, f" + 1 ELSE -{b})"), [x], t)
            return Node(P("(", 0, " - ", 1, ")"), [x, e(INT(1))], t)
        if k == "bool":
            return Node(P("~", 0), [x], t)
        if k == "set":
            c = r.randrange(4)
            if c == 0:
                return Node(P("(", 0, " \\cup {", 1, "})"), [x, e(t[1])], t)
            if c == 1:
                return Node(P("(", 0, " \\ {", 1, "})"), [x, e(t[1])], t)
            if c == 2:
                y = g.fresh()
                p = g.with_var(y, t[1], lambda: g.expr(BOOL, 1))
                return Node(P(f"{{{y} \\in ", 0, " : ", 1, "}"), [x, p], t, binds=(y,))
            return Node(P("(IF Cardinality(", 0, f") < {t[2]} THEN ", 0, " \\cup {", 1, "} ELSE {})"), [x, e(t[1])], t)
        if k == "seq":
            c = r.randrange(5)
            if c == 0:
                return Node(P("(IF Len(", 0, f") < {t[2]} THEN Append(", 0, ", ", 1, ") ELSE ", 0, ")"), [x, e(t[1])], t)
            if c == 1:
                return Node(P("(IF ", 0, " = <<>> THEN ", 0, " ELSE Tail(", 0, "))"), [x], t)
            if c == 2:
                return Node(P("(IF Len(", 0, f") < {t[2]} THEN <<", 1, ">> \\o ", 0, " ELSE <<>>)"), [x, e(t[1])], t)
            if c == 3:
                return Node(P("[", 0, " EXCEPT ![1] = ", 1, "]"), [x, e(t[1])], t)
            return Node(P("SubSeq(", 0, ", 2, Len(", 0, "))"), [x], t)
        if k == "fcn":
            c = r.randrange(6)
            if c == 0:
                key, val = e(t[1]), g.with_var("@", t[2], lambda: g.expr(t[2], 1))
                return Node(P("[", 0, " EXCEPT ![", 1, "] = ", 2, "]"), [x, key, val], t, binds=("@",))
            if c == 1:
                return Node(P("((", 1, " :> ", 2, ") @@ ", 0, ")"), [x, e(t[1]), e(t[2])], t)
            if c == 2:
                return Node(P("(", 0, " @@ (", 1, " :> ", 2, "))"), [x, e(t[1]), e(t[2])], t)
            if c == 3:
                y = g.fresh()
                key = e(t[1])
                val = g.with_var(y, t[1], lambda: g.expr(t[2], 1))
                return Node(P(f"[{y} \\in DOMAIN ", 0, " \\cup {", 1, f"}} |-> IF {y} \\in DOMAIN ", 0, " THEN ", 0, f"[{y}] ELSE ", 2, "]"),
                            [x, key, val], t, binds=(y,))
            if c == 4:
                return self.fcn_over_domain(t, (v, t), 1)
            y = g.fresh()
            key = e(t[1])
            return Node(P(f"[{y} \\in DOMAIN ", 0, " \\ {", 1, f"}} |-> ", 0, f"[{y}]]"), [x, key], t, binds=(y,))
        if k == "rec":
            f, ft = r.choice(t[1])
            if ft[0] == "set" and r.random() < .5:
                return Node(P("[", 0, f" EXCEPT !.{f} = @ \\cup {{", 1, "}]"), [x, e(ft[1])], t)
            return Node(P("[", 0, f" EXCEPT !.{f} = ", 1, "]"), [x, e(ft)], t)
        if k == "tup":
            i = r.randint(1, len(t[1]))
            return Node(P("[", 0, f" EXCEPT ![{i}] = ", 1, "]"), [x, e(t[1][i - 1])], t)
        if k == "bag":
            op = r.choice(["(+)", "(-)"])
            return Node(P("(", 0, f" {op} SetToBag({{", 1, "}))"), [x, e(t[1])], t)
        return g.expr(t, 2)

    def invariant(self, s):
        if self.rng.random() < .3:
            return self.over_state(BOOL, 2)
        return self.bound_pred(s)

    def bound_pred(self, s):
        """A predicate on one variable that typically holds for a while and
        fails at some depth."""
        r, g = self.rng, self.g
        v, t = r.choice(s.vars)
        x = leaf(v, t)
        k = t[0]
        lit = lambda ty: g.scoped([], lambda: g.literal(ty))  # noqa: E731
        if k == "int":
            return Node(P("(", 0, f" # {r.randint(-t[1], t[1])})"), [x], BOOL)
        if k in ("set", "bag", "fcn") and r.random() < .6:
            dom = "DOMAIN " if k != "set" else ""
            return Node(P(f"(Cardinality({dom}", 0, f") {r.choice(['<', '#'])} {r.randint(2, 3)})"), [x], BOOL)
        if k == "set":
            return Node(P("(", 1, " \\notin ", 0, ")"), [x, lit(t[1])], BOOL)
        if k == "fcn":
            y = g.fresh()
            return Node(P(f"(\\A {y} \\in DOMAIN ", 0, f" : ", 0, f"[{y}] # ", 1, ")"), [x, lit(t[2])], BOOL)
        if k == "seq":
            if r.random() < .5:
                return Node(P("(Len(", 0, f") < {t[2]})"), [x], BOOL)
            return Node(P("(\\A ", f"y \\in 1..Len(", 0, ") : ", 0, "[y] # ", 1, ")"), [x, lit(t[1])], BOOL)
        if k == "rec":
            f, ft = r.choice(t[1])
            if ft[0] == "set":
                return Node(P(f"(Cardinality(", 0, f".{f}) < {r.randint(1, 2)})"), [x], BOOL)
            return Node(P("(", 0, f".{f} # ", 1, ")"), [x, lit(ft)], BOOL)
        if k == "tup":
            i = r.randint(1, len(t[1]))
            return Node(P("(", 0, f"[{i}] # ", 1, ")"), [x, lit(t[1][i - 1])], BOOL)
        return Node(P("(", 0, " # ", 1, ")"), [x, lit(t)], BOOL)

    def view(self, s):
        r = self.rng
        if r.random() < .6 and len(s.vars) > 1:
            keep = sorted(r.sample(range(len(s.vars)), r.randint(1, len(s.vars) - 1)))
            kids = [leaf(*s.vars[i]) for i in keep]
            parts = ["<<"]
            for i in range(len(kids)):
                parts += ([", "] if i else []) + [i]
            return Node(parts + [">>"], kids, ("tup", tuple(k.ty for k in kids)))
        t = self.g.scoped(self.env0(), lambda: self.g.rand_type(1))
        n = self.over_state(t, 2)
        return n if n.free else leaf(*r.choice(s.vars))

    def liveness(self):
        r = self.rng
        p = self.over_state(BOOL, 2)
        q = self.over_state(BOOL, 2)
        return (r.choice(["[]<>", "<>[]", "~>", "<>", "[]=><>"]), p, q)

    def fairness(self, s):
        r = self.rng
        out = []
        if r.random() < .25:
            out.append((r.choice(["WF", "SF"]), None))
        for i in range(len(s.actions)):
            if r.random() < .6:
                out.append((r.choice(["WF", "WF", "SF"]), i))
        return out
