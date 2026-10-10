"""Caller-neutral cfg and counterexample structures (harness F17).

The scripts that read TLC configurations and printed counterexamples —
`tracecheck.py` (the tier-3 counterexample validator), `matrix.py` (the
covering-array driver) and `autosym_mutants.py` (the symmetry mutation test)
— each carried its own copy of the same scanner: the comments of a cfg
stripped, the cfg split into `(keyword, body)` sections, a printed state
split into `(variable, value)` pairs, the loop that closes a lasso
recognized, and the body of the root definition a `SPECIFICATION` names.

Shared here is the *structure* of those five things. Each caller keeps the
policy around it as an explicit adapter at its own side:

- the cfg keywords it recognizes: `tracecheck` reads the shared set,
  `autosym_mutants` also reads `POSTCONDITIONS` and `_PERIODIC` (it re-emits
  the sections of a cfg it patches, so it must see every one of them);
- the syntax it reads and what it takes out of it: a `-tool` message,
  standard output, a `-dump` file, state labels, loops, state dicts;
- how it resolves the root spec and which part of the body it uses;
- whether the printed whitespace inside a value is canonicalized: `state`
  does (a comparison between two engines' printouts), `printed_pairs` does
  not (a value that goes back into generated TLA+, where a string literal
  must keep its own whitespace).

So a difference between two callers is an argument or a thin adapter at the
caller, never a second copy of a scanner.

These scanners are not a TLA+ parser: they read the flat forms TLC and tlx
print — a state is `/\\ v = e` lines — and a cfg's top-level keywords. Code
that needs the specification's real structure goes through the front end
(docs/design.md), not through this module.
"""
import re

# The cfg keywords every caller must see. A caller that needs more passes
# its own tuple to `cfg_sections` (autosym_mutants: POSTCONDITIONS,
# _PERIODIC); the sets are otherwise the same, so a section one caller does
# not recognize is a deliberate difference, not a copied list.
CFG_KEYWORDS = ("CONSTANTS", "CONSTANT", "INIT", "NEXT", "SPECIFICATION", "INVARIANTS", "INVARIANT", "PROPERTIES",
                "PROPERTY", "CONSTRAINTS", "CONSTRAINT", "ACTION_CONSTRAINTS", "ACTION_CONSTRAINT", "SYMMETRY", "VIEW",
                "ALIAS", "CHECK_DEADLOCK", "POSTCONDITION", "TYPE_CONSTRAINT", "TYPE")


def strip_comments(text):
    """`text` without its TLA+ comments: `(* ... *)` (up to the first `*)`)
    and `\\*` to the end of its line. Block comments first: a `\\*` inside
    one is part of that comment, not the start of a line comment."""
    text = re.sub(r"\(\*.*?\*\)", " ", text, flags=re.S)
    return re.sub(r"\\\*[^\n]*", "", text)


def cfg_sections(cfg, keywords=CFG_KEYWORDS):
    """A cfg as `[(keyword, body)]` in text order, its comments removed.
    `keywords`: the section keywords to split at, in addition to those of
    `CFG_KEYWORDS` a caller needs. (A keyword inside a body splits it: these
    scanners know the cfg's top-level layout, not which words a spec may use
    as a value.)"""
    text = strip_comments(cfg)
    pattern = r"(?m)(?<![\w])(" + "|".join(sorted(keywords, key=len, reverse=True)) + r")(?![\w])"
    parts = re.split(pattern, text)
    return [(parts[i], parts[i + 1]) for i in range(1, len(parts) - 1, 2)]


def cfg_names(sections, *keywords):
    """The names the bodies of the given section keywords hold (`INIT Init`
    is one name, `CONSTANTS A B` two)."""
    return [n for k, body in sections if k in keywords for n in body.split()]


def printed_pairs(text):
    """A printed state as `[(variable, value)]`: its `/\\ v = e` lines, a
    value that was broken over several lines joined into one (the lines
    after the first continue it, an empty one does not), the value keeping
    the whitespace it was printed with (see `state`)."""
    pairs = []
    for part in re.split(r"^/\\ ", text.strip("\n"), flags=re.M):
        if not part.strip():
            continue
        var, _, value = part.partition(" = ")
        lines = [line.strip() for line in value.splitlines()]
        pairs.append((var.strip(), " ".join(lines[:1] + [line for line in lines[1:] if line])))
    return pairs


def canonical(value):
    """A printed value with its whitespace collapsed: the same value is
    printed with different line breaks and indentation in different runs."""
    return " ".join(value.split())


def state(text):
    """A printed state as `[(variable, value)]` with canonicalized values:
    the form to compare two engines' printouts with."""
    return [(var, canonical(value)) for var, value in printed_pairs(text)]


# The stuttering marker of a counterexample header (the last state is the
# loop: nothing follows it) and the two ways TLC prints a back edge: with
# -tool the index comes first (`3: Back to state: <A>`), without it after
# the phrase (`Back to state 3: <A>`, the form tlx prints too).
STUTTER = re.compile(r"(?:^|: )Stuttering")
BACK_TO_STATE = (re.compile(r"^(\d+): Back to state"), re.compile(r"^Back to state (\d+):"))


def loop_target(head):
    """The loop a counterexample header `head` names: "stutter" for a
    stuttering last state, the 1-based index of the state a lasso closes on,
    or None for the header of a plain state. (A back edge first: its label is
    the closing step's action, which a stuttering state has none of.)"""
    for pattern in BACK_TO_STATE:
        if m := pattern.match(head):
            return int(m.group(1))
    if STUTTER.search(head):
        return "stutter"
    return None


def definition(text, op):
    """The body of the 0-ary definition `op == ...` in a module's `text`,
    its line comments removed and a trailing line of `=` signs dropped, or
    None. The body ends at the next line that starts in column 0: TLA+ joins
    a definition with its indented lines."""
    m = re.search(r"^%s\s*==(.*?)(?=^\S|\Z)" % re.escape(op), text, re.M | re.S)
    if not m:
        return None
    body = re.sub(r"\\\*.*", "", m.group(1))
    return re.sub(r"^\s*=+\s*$", "", body, flags=re.M)
