"""State-fuzzer generation and execution bounds."""

MODES = ("interp", "compile", "paranoid", "w4", "recover")
# The exit status of a run ended right after a checkpoint (TLX_CHECKPOINT_ABORT).
ABORT_EXIT = 134
SAFETY = ("safety failure", "deadlock", "property failure", "init property failure")
UNIVERSE_CAP = 20000      # product of the variables' shape sizes
FILTER_TIMEOUT = 8        # seconds of the first (interpreter) run: longer specs are "too big"
TLC_TIMEOUT = 60
CONSTANTS_LINE = "CONSTANTS m1 = m1 m2 = m2 m3 = m3"
