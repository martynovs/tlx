"""State-fuzz library with explicit state contracts.

``model`` owns specs/actions; ``shapes`` and ``expressions`` adapt expression
contracts to bounded state spaces; ``generation`` preserves the per-spec random
stream; ``render`` owns artifact bytes; ``judgment`` owns state-level comparison
and known-defect recognition; ``runner`` owns bounded execution and recovery.
Expression-level value/error judgments stay in ``modelcheck.fuzz_expr``.
``minimization`` owns ordered shrinking; ``artifacts`` owns persistence/report
bytes; ``replay`` owns saved oracle execution; ``campaign`` owns bounded admission
and aggregation. The command entrypoint owns parsing and provisioning only.
"""

from .constants import ABORT_EXIT, CONSTANTS_LINE, FILTER_TIMEOUT, MODES, SAFETY, TLC_TIMEOUT, UNIVERSE_CAP
from .shapes import small_scalar, state_type, size, ok_pred
from .expressions import uses_tostring, StateGen, leaf, text_node, subst
from .model import Action, Spec, fair_set_constant
from .generation import SpecGen
from .render import ok_name, render_updates, render_action, action_call, render, RECURSIVE_DEFS, render_lib, features, write_spec, spec_files
from .judgment import paranoid_text, judge, SHOW, KNOWN, known_issue, heterogeneous_issue, brief
from .runner import mode_variant, run_tlc, run_tlx, tlx_once, symmetry_ok, Runner

from .artifacts import SPECS, VALIDATED, save_regression, normalize, regression_names, write_report
from .minimization import (signature, literal_of, slots, set_slot, subst_all, drop_instance,
                           drop_var, structural_candidates, expression_candidates, spec_size,
                           Shrinker, shrink_findings, shrink_one, MODE_ORDER)
from .replay import record, replay
from .campaign import run, generate, expr_case, known_expr, KNOWN_EXPR

__all__ = [
    "SPECS",
    "VALIDATED",
    "save_regression",
    "normalize",
    "regression_names",
    "write_report",
    "signature",
    "literal_of",
    "slots",
    "set_slot",
    "subst_all",
    "drop_instance",
    "drop_var",
    "structural_candidates",
    "expression_candidates",
    "spec_size",
    "Shrinker",
    "shrink_findings",
    "shrink_one",
    "MODE_ORDER",
    "record",
    "replay",
    "run",
    "generate",
    "expr_case",
    "known_expr",
    "KNOWN_EXPR",
    "ABORT_EXIT",
    "CONSTANTS_LINE",
    "FILTER_TIMEOUT",
    "MODES",
    "SAFETY",
    "TLC_TIMEOUT",
    "UNIVERSE_CAP",
    "small_scalar",
    "state_type",
    "size",
    "ok_pred",
    "uses_tostring",
    "StateGen",
    "leaf",
    "text_node",
    "subst",
    "Action",
    "Spec",
    "fair_set_constant",
    "SpecGen",
    "ok_name",
    "render_updates",
    "render_action",
    "action_call",
    "render",
    "RECURSIVE_DEFS",
    "render_lib",
    "features",
    "write_spec",
    "spec_files",
    "paranoid_text",
    "judge",
    "SHOW",
    "KNOWN",
    "known_issue",
    "heterogeneous_issue",
    "brief",
    "mode_variant",
    "run_tlc",
    "run_tlx",
    "tlx_once",
    "symmetry_ok",
    "Runner",
]
