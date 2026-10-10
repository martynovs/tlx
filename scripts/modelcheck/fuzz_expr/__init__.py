"""Expression fuzzing library: typed random TLA+ expressions and the TLC-vs-tlx
differential campaign.

Package marker. The harness discovers modules recursively (HARNESS-13): making the
directory a package lets the mode lint, the cloud upload, staged-test discovery and
the verify overlay find the modules that move in here without changing that glue.

The pieces keep their historical responsibilities, one module each: ``exprtypes``
and ``node`` define the expression language, ``generation`` produces expressions,
``hetero`` holds the different-kind probes and the classifier for the documented
TLC function-comparison defect, ``shrink`` minimizes a discrepancy, ``render`` and
``serializer`` turn expressions and run results into text, ``runner`` executes TLC
and tlx, and ``campaign`` drives the fuzzing loop and the ``scripts/fuzz_expr.py``
entry point, which stays a compatibility shim.
"""

from .constants import EXTENDS, FIELDS, INT_MAX, MVS, STRS, TLC_SEM, TLX
from .exprtypes import BOOL, INT, MV, SCALAR, STR, base, fits
from .node import Node, P, node_from_json
from .hetero import (HETERO_EVERY, HETERO_OPEN, HETERO_OPS, HETERO_TYPES,
                     MIXED, function_comparison_defect, heterogeneous,
                     heterogeneous_function_kinds, known_expression,
                     max_ty, mixed_collection)
from .generation import Gen
from .shrink import (descendants, nodes_with_paths, replace_at, shrink_candidates,
                     shrink_ty)
from .serializer import (INFO_KINDS, LAZY_FN, VAL_TOKEN, canon_value, classify,
                         expected_expr, info_key, lazy_fn_key)
from .render import (FIELDS_LINE, HEADER, PRINT_RE, const_cfg, const_spec, lift,
                     norm_error, pool_def, state_spec, unescape)
from .runner import Runner, describe_state_failure, paranoid_text, state_error
from .campaign import batched, expressible, minimize, run_fuzz

__all__ = [
    "BOOL", "EXTENDS", "FIELDS", "FIELDS_LINE", "Gen", "HETERO_EVERY",
    "HETERO_OPEN", "HETERO_OPS", "HETERO_TYPES", "HEADER", "INFO_KINDS", "INT",
    "INT_MAX", "LAZY_FN", "MIXED", "MV", "MVS", "Node", "P", "PRINT_RE",
    "Runner", "SCALAR", "STR", "STRS", "TLC_SEM", "TLX", "VAL_TOKEN",
    "base", "batched", "canon_value", "classify", "const_cfg", "const_spec",
    "descendants", "describe_state_failure", "expected_expr", "expressible",
    "fits", "function_comparison_defect", "heterogeneous",
    "heterogeneous_function_kinds", "info_key", "known_expression",
    "lazy_fn_key", "lift", "max_ty", "minimize", "mixed_collection",
    "node_from_json", "nodes_with_paths", "norm_error", "paranoid_text",
    "pool_def", "replace_at", "run_fuzz", "shrink_candidates", "shrink_ty",
    "state_error", "state_spec", "unescape",
]
