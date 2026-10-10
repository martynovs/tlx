"""Printed-value and error classification.

``canon_value`` parses a printed value (sorting function entries and record
fields), ``expected_expr`` re-embeds it without adding identifier tokens,
``lazy_fn_key`` shares a key for lazily-built functions, ``classify`` is the
verdict comparator and ``info_key`` groups informational discrepancies.
"""
import json
import re

from contract import compare as compare_contract

# Kinds of discrepancies that are only reported, not failures: different
# wording of the same kind of error (same exit code), functions printed with
# their entries in another order (TLC prints the result of @@, :> and EXCEPT
# on such functions in construction order, not in normalized order; the values
# are equal), and lazily-built functions (see `lazy_fn_key`).
INFO_KINDS = ("message", "print-order", "lazy-print", "tlc-defect-record-injective")


# `[x \in S |-> e]` where S cannot be enumerated is kept as a lazily-built
# function; ToString then prints the domain and, for the body, TLC the source
# expression (`<expression 1>`) and tlx `...`. Neither is the value, and the
# two renderings can never be compared, so the domain is the only common key.
LAZY_FN = re.compile(r'^\[(\w+) \\in (.+?) \|-> (?:<expression[^>]*>|\.\.\.)\]$')


def lazy_fn_key(text):
    """The domain of a lazily-built function print (`None`: not one)."""
    m = LAZY_FN.match(text)
    return m.group(2) if m else None


def classify(tlc, tlx):
    """None if the results agree, else the kind of discrepancy."""
    if tlc is None or tlx is None:
        return None
    if "infrastructure" in (tlc[0], tlx[0]):
        return "infrastructure"
    if tlc[0] == "timeout" or tlx[0] == "timeout":
        return None if tlc[0] == tlx[0] else "timeout"
    # Use the same verdict comparator as fuzz_spec; neither an error's
    # message nor its exit code changes the tier-1 result.
    kind = {"err": "error", "ok": "success"}
    if compare_contract({"result": kind[tlc[0]]}, {"result": kind[tlx[0]]}).differs:
        # CommunityModules Functions.IsInjective tests SetOfRcdsValue
        # instead of RecordValue (compat.md D-26): records are functions.
        if (tlc[0] == "err" and tlx[0] == "ok" and re.match(
                r"The argument of IsInjective should be a function, but instead it is:\s*\[\w+\s*\|->", tlc[1])):
            return "tlc-defect-record-injective"
        return "error-vs-value"
    if tlc[0] == "err":
        return None if tlc[1] == tlx[1] else "message"
    if tlc[1] == tlx[1]:
        return None
    a, b = canon_value(tlc[1]), canon_value(tlx[1])
    if a is not None and a == b:
        return "print-order"
    la, lb = lazy_fn_key(tlc[1]), lazy_fn_key(tlx[1])
    if la is not None and la == lb:
        return "lazy-print"
    return "value"


VAL_TOKEN = re.compile(r'\s*("(?:[^"\\]|\\.)*"|<<|>>|:>|@@|\|->|\.\.|[{}\[\](),]|-?\d+|[A-Za-z_][A-Za-z0-9_]*)')


def canon_value(text):
    """A printed value with function entries and record fields sorted (None if
    it cannot be parsed)."""
    toks, pos = [], 0
    while pos < len(text):
        m = VAL_TOKEN.match(text, pos)
        if not m:
            if text[pos:].strip() == "":
                break
            return None
        toks.append(m.group(1))
        pos = m.end()
    toks.append(None)
    i = 0

    def parse():
        nonlocal i
        t = toks[i]
        i += 1
        if t in ("{", "<<"):
            close = "}" if t == "{" else ">>"
            items = []
            while toks[i] != close:
                items.append(parse())
                if toks[i] == ",":
                    i += 1
                elif toks[i] != close:
                    raise ValueError
            i += 1
            return (t, tuple(items))
        if t == "[":
            items = []
            while toks[i] != "]":
                f = toks[i]
                if toks[i + 1] != "|->":
                    raise ValueError
                i += 2
                items.append((f, parse()))
                if toks[i] == ",":
                    i += 1
            i += 1
            return ("rec", tuple(sorted(items, key=repr)))
        if t == "(":
            items = []
            while True:
                k = parse()
                if toks[i] != ":>":
                    raise ValueError
                i += 1
                items.append((k, parse()))
                if toks[i] == "@@":
                    i += 1
                elif toks[i] == ")":
                    i += 1
                    break
                else:
                    raise ValueError
            return ("fcn", tuple(sorted(items, key=repr)))
        if t is None or t in ("}", ">>", "]", ")", ",", ":>", "@@", "|->", ".."):
            raise ValueError
        if toks[i] == ".." and re.fullmatch(r"-?\d+", t) and re.fullmatch(r"-?\d+", toks[i + 1] or ""):
            # An interval prints as lo..hi; equal to the enumerated set.
            lo, hi = int(t), int(toks[i + 1])
            i += 2
            return ("{", tuple(str(k) for k in range(lo, hi + 1)))
        return t
    try:
        v = parse()
        return v if toks[i] is None else None
    except (ValueError, IndexError):
        return None


def expected_expr(text):
    """Re-embed a printed value without adding record-field identifiers.

    SANY interns identifier tokens before string literals. TLC may print a
    string-domain function as a record, so inserting that output verbatim
    can change SetToSeq/CHOOSE order in the generated state spec itself.
    """
    value = canon_value(text) if "[" in text or "-2147483648" in text else None
    if value is None:
        return text

    def render(v):
        if isinstance(v, str):
            # TLC parses the magnitude before unary minus; 2147483648 is
            # out of range although the computed value -2147483648 is valid.
            return "(-2147483647 - 1)" if v == "-2147483648" else v
        kind, items = v
        if kind in ("rec", "fcn"):
            pairs = [(json.dumps(k) if kind == "rec" else render(k), render(x)) for k, x in items]
            return "(" + " @@ ".join(k + " :> " + x for k, x in pairs) + ")" if pairs else "<<>>"
        close = "}" if kind == "{" else ">>"
        return kind + ", ".join(map(render, items)) + close

    return render(value)


def info_key(kind, rt, rx):
    if kind == "message":
        return (rt[1], rx[1])
    return re.sub(r'"[^"]*"|-?\d+', "_", rt[1])[:80]
