"""State spec and action contracts, independent of generation and execution."""
import copy
from dataclasses import dataclass, field
from modelcheck.fuzz_expr import Node


@dataclass
class Action:
    name: str
    param: tuple = None         # (name, type, set Node)
    inner: tuple = None         # (name, type, set Node): \E name \in set around the updates
    guards: list = field(default_factory=list)
    updates: dict = field(default_factory=dict)    # var -> ("eq" | "in", Node)
    ite: tuple = None           # (condition Node, {var: ("eq" | "in", Node)}): IF c THEN updates ELSE these
    order: list = field(default_factory=list)      # the updated variables, in conjunct order
    unchanged_style: str = "UNCHANGED"
    inst_same: bool = False     # the instance's variable unchanged as I!Same



@dataclass
class Spec:
    name: str
    vars: list                  # [(name, type)]
    init: dict                  # var -> ("eq" | "in", Node)
    actions: list
    invariants: list = field(default_factory=list)      # [(name, Node)]
    typeok: bool = False
    aprops: list = field(default_factory=list)          # [(name, kind "box" | "action", Node)]
    constraint: Node = None
    aconstraint: Node = None
    view: Node = None
    symmetry: list = None                               # model values
    live: tuple = None                                  # (kind, P, Q)
    fairness: list = field(default_factory=list)        # [(WF | SF, action index | None (Next))]
    deadlock: bool = True
    ok_guards: bool = True
    pool: list = field(default_factory=list)            # the strings, in intern order
    features: dict = field(default_factory=dict)
    consts: list = field(default_factory=list)          # [(name, type, cfg value)]: CONSTANT name, cfg name = value
    overrides: list = field(default_factory=list)       # [(name, type, value, alternative)]: cfg name <- nameAlt
    instance: tuple = None                              # (variable, type): I == INSTANCE <name>_lib WITH w <- variable
    variant: str = None                                 # the key of the mode variants (the generated name)

    def clone(self):
        s = copy.copy(self)
        s.vars = list(self.vars)
        s.init = dict(self.init)
        s.actions = [copy.copy(a) for a in self.actions]
        for a in s.actions:
            a.guards, a.updates, a.order = list(a.guards), dict(a.updates), list(a.order)
            a.ite = (a.ite[0], dict(a.ite[1])) if a.ite else None
        s.invariants, s.aprops, s.fairness = list(self.invariants), list(self.aprops), list(self.fairness)
        s.consts, s.overrides = list(self.consts), list(self.overrides)
        s.symmetry = list(self.symmetry) if self.symmetry else None
        s.features = dict(self.features)
        return s

    def var_type(self, v):
        return dict(self.vars)[v]



def fair_set_constant(a):
    return a.param is None or not any(n.startswith("v") for n in a.param[2].free)
