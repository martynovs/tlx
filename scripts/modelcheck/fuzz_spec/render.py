"""Deterministic TLA+, cfg and helper-module rendering for state specs."""
import json
import os
from modelcheck import fuzz_expr as fx
from .constants import CONSTANTS_LINE
from .model import fair_set_constant
from .shapes import ok_pred


def ok_name(v):
    return "Ok" + v.upper()



def render_updates(s, a, updates):
    out = []
    for v in a.order:
        kind, n = updates[v]
        if kind == "iput":
            out.append(f"I!Put({n.render()})")
        else:
            out.append(f"{v}' {'=' if kind == 'eq' else '\\in'} {n.render()}")
        if s.ok_guards:
            out.append(f"{ok_name(v)}({v}')")
    return out



def render_action(s, a):
    head = f"{a.name}({a.param[0]})" if a.param else a.name
    conj = [f"({g.render()})" for g in a.guards]
    body = render_updates(s, a, a.updates)
    if a.ite:
        alt = render_updates(s, a, a.ite[1])
        body = [f"IF {a.ite[0].render()} THEN ({' /\\ '.join(body) or 'TRUE'}) ELSE ({' /\\ '.join(alt) or 'TRUE'})"]
    if a.inner and body:
        q, _, qs = a.inner
        body = [f"\\E {q} \\in {qs.render()} : ({' /\\ '.join(f'({b})' for b in body)})"]
    conj += [f"({b})" if not b.startswith("(") else b for b in body]
    rest = [v for v, _ in s.vars if v not in a.order]
    if a.inst_same and s.instance and s.instance[0] in rest:
        rest.remove(s.instance[0])
        conj.append("I!Same")
    if rest:
        if a.unchanged_style == "UNCHANGED":
            conj.append(f"UNCHANGED <<{', '.join(rest)}>>")
        else:
            conj += [f"{v}' = {v}" for v in rest]
    if not conj:
        conj = ["TRUE"]
    return f"{head} ==\n" + "\n".join(f"    /\\ {c}" for c in conj) + "\n"



def action_call(a):
    if a.param:
        p, _, ps = a.param
        return f"\\E {p} \\in {ps.render()} : {a.name}({p})"
    return a.name



def render(s):
    """(module text, cfg text) of spec `s`."""
    vs = [v for v, _ in s.vars]
    lines = [f"---- MODULE {s.name} ----", f"EXTENDS {fx.EXTENDS}", "CONSTANTS " + ", ".join(fx.MVS + [c for c, _, _ in s.consts]),
             f"VARIABLES {', '.join(vs)}", f"vars == <<{', '.join(vs)}>>",
             "Pool == <<" + ", ".join(json.dumps(x) for x in s.pool) + ">>", fx.FIELDS_LINE.rstrip("\n"), RECURSIVE_DEFS]
    for o, _, a, b in s.overrides:
        lines += [f"{o} == {a}", f"{o}Alt == {b}"]
    if s.instance:
        lines.append(f"I == INSTANCE {s.name}_lib WITH w <- {s.instance[0]}")
    for v, t in s.vars:
        lines.append(f"{ok_name(v)}(x) == {ok_pred(t, 'x')}")
    lines.append("TypeOK == " + " /\\ ".join(f"{ok_name(v)}({v})" for v in vs))
    lines.append("Init ==\n" + "\n".join(
        f"    /\\ {v} {'=' if s.init[v][0] == 'eq' else '\\in'} {s.init[v][1].render()}" for v in vs))
    for a in s.actions:
        lines.append(render_action(s, a).rstrip("\n"))
    lines.append("Next ==\n" + "\n".join(f"    \\/ {action_call(a)}" for a in s.actions))
    fair = []
    for kind, i in s.fairness:
        if i is None:
            fair.append(f"{kind}_vars(Next)")
            continue
        a = s.actions[i]
        if a.param and fair_set_constant(a):
            p, _, ps = a.param
            fair.append(f"(\\A {p} \\in {ps.render()} : {kind}_vars({a.name}({p})))")
        else:
            fair.append(f"{kind}_vars({action_call(a)})")
    if s.live and fair:
        lines.append("Fair == " + " /\\ ".join(fair))
    lines.append("Spec == Init /\\ [][Next]_vars" + (" /\\ Fair" if s.live and fair else ""))
    for name, n in s.invariants:
        lines.append(f"{name} == {n.render()}")
    for name, kind, n in s.aprops:
        lines.append(f"{name} == [][{n.render()}]_vars" if kind == "action" else f"{name} == [](({n.render()}))")
    if s.live:
        kind, p, q = s.live
        pt, qt = p.render(), q.render()
        lines.append("Live == " + {"[]<>": f"[]<>({pt})", "<>[]": f"<>[]({pt})", "~>": f"(({pt}) ~> ({qt}))",
                                   "<>": f"<>({pt})", "[]=><>": f"[](({pt}) => <>({qt}))"}[kind])
    if s.constraint is not None:
        lines.append(f"Constr == {s.constraint.render()}")
    if s.aconstraint is not None:
        lines.append(f"AConstr == {s.aconstraint.render()}")
    if s.view is not None:
        lines.append(f"View == {s.view.render()}")
    if s.symmetry:
        lines.append(f"Sym == Permutations({{{', '.join(s.symmetry)}}})")
    lines.append("====")
    cfg = [CONSTANTS_LINE + "".join(f" {c} = {v}" for c, _, v in s.consts), "SPECIFICATION Spec"]
    cfg += [f"CONSTANT {o} <- {o}Alt" for o, _, _, _ in s.overrides]
    invs = (["TypeOK"] if s.typeok else []) + [n for n, _ in s.invariants]
    if invs:
        cfg.append("INVARIANT " + " ".join(invs))
    props = [n for n, _, _ in s.aprops] + (["Live"] if s.live else [])
    if props:
        cfg.append("PROPERTY " + " ".join(props))
    if s.constraint is not None:
        cfg.append("CONSTRAINT Constr")
    if s.aconstraint is not None:
        cfg.append("ACTION_CONSTRAINT AConstr")
    if s.view is not None:
        cfg.append("VIEW View")
    if s.symmetry:
        cfg.append("SYMMETRY Sym")
    if not s.deadlock:
        cfg.append("CHECK_DEADLOCK FALSE")
    return "\n".join(lines) + "\n", "\n".join(cfg) + "\n"



# Recursive operators emitted at module level (StateGen also writes LET RECURSIVE).
RECURSIVE_DEFS = """RECURSIVE RCard(_)
RCard(S) == IF S = {} THEN 0 ELSE 1 + RCard(S \\ {CHOOSE e \\in S : TRUE})
RECURSIVE RLen(_)
RLen(s) == IF s = <<>> THEN 0 ELSE 1 + RLen(Tail(s))"""



def render_lib(s):
    """The module instantiated by `s` (None: none)."""
    if not s.instance:
        return None
    return (f"---- MODULE {s.name}_lib ----\nEXTENDS Integers, FiniteSets, Sequences\nVARIABLE w\n"
            "W == w\nPut(e) == w' = e\nSame == UNCHANGED w\n====\n")



def features(s):
    """A short description of the spec's features (reports, triage)."""
    f = [f"vars={'/'.join(t[0] for _, t in s.vars)}", f"actions={len(s.actions)}"]
    if s.features.get("heterogeneous"):
        f.append("heterogeneous=" + s.features["heterogeneous"])
    if any(a.param for a in s.actions):
        f.append("param")
    if any(a.param and a.param[2].free for a in s.actions):
        f.append("runtime-param")
    if any(a.inner for a in s.actions):
        f.append("inner-exists")
    if any(a.ite for a in s.actions):
        f.append("ite")
    if s.invariants or s.typeok:
        f.append("inv")
    if s.aprops:
        f.append("aprop")
    if s.constraint is not None:
        f.append("constraint")
    if s.aconstraint is not None:
        f.append("aconstraint")
    if s.view is not None:
        f.append("view")
    if s.symmetry:
        f.append("symmetry")
    if s.live:
        f.append("live" + ("+fair" if s.fairness else ""))
    if not s.deadlock:
        f.append("nodeadlock")
    if s.consts:
        f.append("consts")
    if s.overrides:
        f.append("override")
    if s.instance:
        f.append("instance")
    return " ".join(f)



def write_spec(d, s):
    tla, cfg = render(s)
    os.makedirs(d, exist_ok=True)
    files = [(s.name + ".tla", tla), (s.name + ".cfg", cfg)]
    if lib := render_lib(s):
        files.append((s.name + "_lib.tla", lib))
    for name, text in files:
        with open(os.path.join(d, name), "w") as f:
            f.write(text)



def spec_files(d, name):
    """The files of spec `name` in directory `d`."""
    return [f for f in (name + ".tla", name + ".cfg", name + "_lib.tla") if os.path.exists(os.path.join(d, f))]
