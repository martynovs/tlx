#!/usr/bin/env python3
"""The documented-command contract: what the documents say a script accepts is
checked against the parser that will execute it.

Why (harness audit F9/F10, DOC-10): the drivers' options are documented by hand,
so an instruction could name a flag the parser has never had -- the audit found a
`difftest.py --name` recipe in an agent definition and the dispatch skill, while
the parser takes `--ids` and `--filter` -- and the same hand-copied surface is
where a documented default or a whole option list drifts. Prose scanning cannot
see that; the parser can.

`check()` compares two things with the owning script's execution parser, and
never runs an example:

* the command examples of the tracked documents (inline code spans and fenced
  blocks), and
* the `## Script options` tables of `docs/configuration.md`.

Option names, whether an option takes a value and the declared defaults all come
from the parser itself: a child process imports the tool with
`ArgumentParser.parse_args` replaced, so the first parse aborts before any
handler runs, and with every way of creating a process sealed, so a tool that
forwards its argv to another program (`rustc_slots.py RUSTC ...` `execvp`s it)
cannot run anything either. No example action is executed, and the child's
standard input is closed so a hook cannot hang it (HARNESS-02).

Coverage is limited on purpose and reported rather than assumed (HARNESS-21,
DOC-11):

* only commands that invoke `scripts/*.py` are checked -- not `cargo`, `git`,
  `target/release/tlx`, TLC or a `.sh` script -- and a test module is not a
  documented entry point;
* the modelled shell grammar is word splitting with quotes, a trailing `#`
  comment and `--` ending the options, `<...>` placeholders (one value) and
  `[...]` optional groups (dropped). A pipe, `&&`, `;`, a redirect, `$VAR`, a
  backtick or a line continuation is not modelled: the line is skipped, never
  guessed at;
* a tool whose parser cannot be read -- a library mentioned in prose, a module
  that never parses argv, or one that creates a process before it parses (the
  seal above forbids that) -- is reported as unchecked, not as passing;
* in the option table, a scripts cell is read up to its first `;` (the scripts
  before it carry the option; the rest is the row's prose).

A line carrying the marker `cli-contract: ignore` is exempt: a document may show
a command that is deliberately wrong (the verify skill's `spectest.py --compile`
counter-example) or one that is valid only on an archived revision.
"""

import argparse
import contextlib
import json
import os
import re
import shlex
import subprocess
import sys
from pathlib import Path

#: The child's introspection mode: `--dump TOOL` prints one tool's parser.
DUMP = "--dump"
#: The exemption marker, split so that this module's own source never matches it.
MARKER = "cli-contract: " + "ignore"

#: Shell text outside the modelled grammar (a pipe, redirection, substitution,
#: background job, separator or continuation): the example is skipped.
UNMODELLED = re.compile(r"[|&;$`><\\]")
#: `<...>`: one value the document leaves to the reader, replaced by `VALUE` so
#: that the positional rule below does not read it as a choice the document made.
VALUE = "__value__"
PLACEHOLDER = re.compile(r"<[^<>]*>")
#: `[...]`: an optional group the document leaves out.
OPTIONAL = re.compile(r"\[[^\[\]]*\]")
#: A trailing shell comment: prose, not arguments.
COMMENT = re.compile(r"\s#.*$")
INLINE = re.compile(r"`([^`\n]+)`")
FENCE = re.compile(r"^ {0,3}(?:`{3,}|~{3,})")
TOOL = re.compile(r"^(?:\./)?(?:scripts/)?([A-Za-z0-9_]+)\.py$")
#: A subcommand spelling (`run`, `suite-check`) -- never a shorthand list
#: (`run/ab/disk`) and never a path, neither of which selects one subparser.
SUBCOMMAND = re.compile(r"^[A-Za-z][A-Za-z0-9_-]*$")
OPTION = re.compile(r"^--?[A-Za-z][A-Za-z0-9-]*$")
#: `difftest.py` (120): a script of an option-table row and its documented default.
TABLE_SCRIPT = re.compile(r"`([A-Za-z0-9_]+\.py)`(?:\s*\(([^)]*)\))?")
#: A backticked cell of an option-table row: `` `--timeout S` ``, `` `--extra=ARGS` ``.
TABLE_CELL = re.compile(r"`([^`]*)`")
#: The option spelling a backticked cell starts with, if it starts with one.
TABLE_OPTION = re.compile(r"(--?[A-Za-z][A-Za-z0-9-]*)")
SECTION = re.compile(r"^## ")
TIMEOUT = 60


class Blocked(Exception):
    """A tool tried to create a process while its parser was being read."""


def _no_process(*args, **kwargs):
    raise Blocked("the tool creates a process before it parses argv")


#: Everything in `os` that runs another program. `subprocess.Popen` is the
#: single choke point of the `subprocess` helpers, which reach it by name.
PROCESS_CALLS = ("execv", "execve", "execvp", "execvpe", "posix_spawn", "posix_spawnp",
                 "fork", "forkpty", "system", "spawnl", "spawnle", "spawnlp", "spawnlpe",
                 "spawnv", "spawnve", "spawnvp", "spawnvpe")


@contextlib.contextmanager
def sealed():
    """Make creating a process impossible for the duration, and restore after.

    Reading a parser must not run the tool. An entry point that is a compiler or
    cache client wrapper (`rustc_slots.py RUSTC [ARGS...]`) `execvp`s its first
    argument before it looks at argv at all, so without this the child would run
    whatever the document named -- observed as a stray `sccache --dump`. A tool
    that reaches one of these is doing work before it parses, which is reported
    as unreadable rather than trusted.
    """
    saved = {name: getattr(os, name) for name in PROCESS_CALLS if hasattr(os, name)}
    saved["Popen"] = subprocess.Popen
    for name in saved:
        setattr(subprocess if name == "Popen" else os, name, _no_process)
    try:
        yield
    finally:
        for name, original in saved.items():
            setattr(subprocess if name == "Popen" else os, name, original)


def _node(parser):
    """One parser's metadata: option spellings, value kinds, declared defaults,
    and the choices of its positional arguments."""
    options, kinds, defaults, positionals = {}, {}, {}, []
    for action in parser._actions:
        if isinstance(action, argparse._SubParsersAction):
            continue
        for spelling in action.option_strings:
            options[spelling] = action.dest
            kinds[spelling] = "flag" if action.nargs == 0 else "value"
            if action.default is not None and action.default is not False:
                defaults[spelling] = action.default
        if not action.option_strings and action.choices:
            positionals.append(sorted(str(choice) for choice in action.choices))
    return {"options": options, "kinds": kinds, "defaults": defaults,
            "positionals": positionals}


def _nodes(meta):
    """Every parser node of a tool, as (subcommand or empty, node) pairs."""
    return [("", meta.get("top", {}))] + sorted((meta.get("subcommands") or {}).items())


def _capture(tool):
    """`tool`'s own parser, built without running any handler.

    `ArgumentParser.parse_args` is replaced for the duration, so the tool's first
    parse raises `SystemExit` -- after its parser is built and before the handler
    that would do the work. An import failure and a module that never parses argv
    both come back as unreadable, never as agreement.
    """
    import importlib.util
    sys.path.insert(0, os.path.dirname(os.path.abspath(tool)))
    captured = {}

    def replace(self, args=None, namespace=None):
        captured["parser"] = self
        raise SystemExit(0)

    original = argparse.ArgumentParser.parse_args
    argparse.ArgumentParser.parse_args = replace
    try:
        name = os.path.basename(tool)[:-len(".py")]
        spec = importlib.util.spec_from_file_location(name, tool)
        module = importlib.util.module_from_spec(spec)
        sys.modules[name] = module
        # What the tool prints while its parser is built goes to stderr, so
        # stdout carries the metadata alone and a chatty module is still read.
        with contextlib.redirect_stdout(sys.stderr), sealed():
            spec.loader.exec_module(module)
            sys.argv = [tool, DUMP]
            module.main()
    except SystemExit:
        pass
    except Exception as error:  # a tool that cannot be imported is reported, not trusted
        return {"tool": tool, "introspectable": False, "why": f"{type(error).__name__}: {error}"}
    finally:
        argparse.ArgumentParser.parse_args = original
    parser = captured.get("parser")
    if parser is None:
        return {"tool": tool, "introspectable": False,
                "why": "never parses argv (no parse_args call)"}
    subcommands = {}
    for action in parser._actions:
        if isinstance(action, argparse._SubParsersAction):
            subcommands = {name: _node(sub) for name, sub in action.choices.items()}
    return {"tool": tool, "introspectable": True, "prog": parser.prog,
            "top": _node(parser), "subcommands": subcommands}


def metadata(tool, run=subprocess.run):
    """The parser metadata of `tool`, read from `tool`'s own parser in a child.

    The child is this module in `--dump` mode, so a tool that hangs (a hook
    reading standard input) or crashes cannot hang or mislead the check.
    """
    argv = [sys.executable, os.path.abspath(__file__), DUMP, tool]
    try:
        done = run(argv, capture_output=True, text=True, timeout=TIMEOUT,
                   stdin=subprocess.DEVNULL)
        return json.loads(done.stdout)
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        return {"tool": tool, "introspectable": False, "why": f"{type(error).__name__}: {error}"}


def _spans(text):
    """(line number, candidate) for every code span and fenced line.

    The two surfaces a document puts a command in: an inline code span, and a
    line inside a fence. Prose is not read.
    """
    fence = None
    for number, line in enumerate(text.splitlines(), 1):
        opening = FENCE.match(line)
        if opening:
            fence = None if fence else opening.group(0).strip()[-1]
            continue
        if fence:
            if line.strip():
                yield number, line.strip()
            continue
        for span in INLINE.finditer(line):
            yield number, span.group(1).strip()


def command(span, root="."):
    """(tool, argv) of a documented command example, or None outside the grammar.

    None means "not a command this check models": prose, another program, or a
    shell construct whose meaning would have to be guessed.
    """
    text = COMMENT.sub("", OPTIONAL.sub(" ", PLACEHOLDER.sub(VALUE, span)))
    if UNMODELLED.search(text):
        return None
    try:
        words = shlex.split(text)
    except ValueError:
        return None
    if words and os.path.basename(words[0]) in ("python", "python3"):
        words = words[1:]
    if not words or not TOOL.match(words[0]):
        return None
    tool = os.path.basename(words[0])
    if tool.startswith("test_") or not os.path.isfile(os.path.join(root, "scripts", tool)):
        return None
    return tool, words[1:]


def _accepted(meta):
    """Every option spelling anywhere in a tool's parser tree.

    A shorthand that names an option without repeating its subcommand (`bench.py
    --hog N`) refers to some subcommand's option, so the whole tree is the
    surface such a mention is checked against.
    """
    return {spelling for _, node in _nodes(meta) for spelling in node.get("options", {})}


def _example_problems(where, tool, args, meta):
    """What one command example says that the tool's parser does not accept."""
    if not meta.get("introspectable"):
        return []
    subcommands = meta.get("subcommands") or {}
    node, label, rest = meta.get("top", {}), tool, args
    if subcommands and (not rest or rest[0].startswith("-") or not SUBCOMMAND.match(rest[0])):
        # `bench.py --hog N`: a shorthand for some subcommand's option, or for
        # several of them (`run/ab/disk`). Checked against the whole tree.
        node = None
    elif subcommands:
        if rest[0] not in subcommands:
            return [f"{where}: {tool} has no subcommand {rest[0]}"]
        node, label, rest = subcommands[rest[0]], f"{tool} {rest[0]}", rest[1:]
    accepted = _accepted(meta) if node is None else set(node.get("options", {}))
    kinds = {} if node is None else node.get("kinds", {})
    choices = [] if node is None else node.get("positionals", [])
    problems, positionals, index = [], [], 0
    while index < len(rest):
        word = rest[index]
        if word == "--":
            break
        name = word.split("=")[0]
        if OPTION.match(name):
            if name not in accepted:
                problems.append(f"{where}: {label} does not accept {name}")
            elif "=" not in word and kinds.get(name) == "value":
                index += 1  # that word is this option's value, not a positional
        elif word != VALUE:
            positionals.append(word)
        index += 1
    # A positional the parser constrains to a choice is that tool's subcommand
    # spelling: `corpus.py tlcc` is the same defect as an unknown `bench.py`
    # subcommand, and it is the shape a tool without subparsers uses.
    for position, word in enumerate(positionals):
        allowed = choices[position] if position < len(choices) else None
        if allowed and word not in allowed:
            problems.append(f"{where}: {label} takes no {word} (it takes {', '.join(allowed)})")
    return problems


def _documented(path, text, meta_of, root):
    """(problems, examples) of the command examples in one document."""
    skipped = {number for number, line in enumerate(text.splitlines(), 1) if MARKER in line}
    problems, examples = [], 0
    for number, span in _spans(text):
        if number in skipped:
            continue
        parsed = command(span, root)
        if not parsed:
            continue
        tool, args = parsed
        if not any(OPTION.match(word.split("=")[0]) for word in args):
            continue  # mentions a tool, documents no option
        examples += 1
        problems.extend(_example_problems(f"{path}:{number}", tool, args, meta_of(tool)))
    return problems, examples


def _exists(where, tool, spelling, meta_of):
    if not spelling.startswith("-"):
        return []
    meta = meta_of(tool)
    if not meta.get("introspectable"):
        return []
    if spelling not in _accepted(meta):
        return [f"{where}: {tool} does not accept {spelling}"]
    return []


def _default(where, tool, spelling, declared, meta_of):
    """A documented default against the parser's, where the tool declares one.

    `bench.py (600; `tlcbase` 1800)` declares two and is checked for acceptance
    only: names alone do not establish defaults, and a guess between two of them
    would be exactly the prose reading this check replaces (DOC-10).
    """
    if not declared.isdigit():
        return []
    meta = meta_of(tool)
    if not meta.get("introspectable"):
        return []
    found = {str(node.get("defaults", {}).get(spelling))
             for _, node in _nodes(meta) if spelling in node.get("options", {})}
    found.discard("None")
    if len(found) == 1 and found.pop() != declared:
        return [f"{where}: {tool} documents the default of {spelling} as {declared}, "
                f"the parser declares another"]
    return []


def _spellings(cell):
    """The option spellings a table cell names: `` `--timeout S` ``, `` `-j N` ``."""
    out = []
    for span in TABLE_CELL.findall(cell):
        match = TABLE_OPTION.match(span.strip())
        if match:
            out.append(match.group(1))
    return out


def _option_table_row(where, row, meta_of):
    """One `| \\`--option V\\` | meaning | \\`script.py\\` (default) |` row."""
    scripts = row[2] if len(row) > 2 else ""
    problems = []
    for spelling in _spellings(row[0]):
        for tool, declared in TABLE_SCRIPT.findall(scripts.split(";")[0]):
            problems.extend(_exists(where, tool, spelling, meta_of))
            problems.extend(_default(where, tool, spelling,
                                     (declared or "").strip().strip("`"), meta_of))
    return problems


def _script_table_row(where, tool, options, meta_of):
    return [problem for spelling in _spellings(options)
            for problem in _exists(where, tool, spelling, meta_of)]


def table_problems(path, text, meta_of):
    """The `## Script options` tables of `docs/configuration.md`."""
    problems, option_first, in_section = [], False, False
    for number, line in enumerate(text.splitlines(), 1):
        if SECTION.match(line):
            in_section = line.strip() == "## Script options"
            option_first = False
            continue
        if not in_section or not line.startswith("|"):
            continue
        # An escaped pipe (`list\|tlc`) separates two words of one cell, not cells.
        row = [cell.strip() for cell in re.split(r"(?<!\\)\|", line.strip().strip("|"))]
        if len(row) < 2 or set("".join(row)) <= set("-: "):
            continue
        if row[0] == "Option":
            option_first = True
            continue
        if row[0] == "Script" or not row[0].startswith("`"):
            continue
        where = f"{path}:{number}"
        if option_first:
            problems.extend(_option_table_row(where, row, meta_of))
        else:
            problems.extend(_script_table_row(where, row[0].strip("`"), row[1], meta_of))
    return problems


#: Where the agent definitions live: command recipes for a worker, shipped with
#: the harness, which the published tree does not contain.
AGENT_DEFINITIONS = ".agents/agents"


def sources(root, docs, run=subprocess.run):
    """``(documents, absent)``: what to read, and which source set is missing here.

    The caller's set plus the agent definitions, where the unsupported `--name`
    recipe lived (F9). A definition on disk is read even when the index does not
    list it: an untracked one still documents a command.

    `bench/**` is dropped from the caller's set: those are frozen measurement
    records, not instructions, and `doccheck` reads them for links only. A
    command in an archived report describes the revision that produced it.

    ``absent`` names a source set this checkout does not have (the published
    tree ships no harness). The caller reports it, so a reduced surface is
    visible in the counts instead of reading as agreement about documents that
    were never opened.
    """
    listed = run(["git", "ls-files", "-z", "--", AGENT_DEFINITIONS],
                 capture_output=True, text=True, cwd=root, stdin=subprocess.DEVNULL)
    # A source archive has no index (`doccheck` falls back the same way): read
    # every definition there instead of dropping the surface silently.
    on_disk = sorted(str(path.relative_to(root)) for path in
                     Path(root, AGENT_DEFINITIONS).glob("**/*.md"))
    extra = on_disk if listed.returncode else sorted(
        set(name for name in listed.stdout.split("\0") if name.endswith(".md")) | set(on_disk))
    names = [name for name in [*docs, *extra] if not name.startswith("bench/")]
    present = [name for name in dict.fromkeys(names) if os.path.isfile(os.path.join(root, name))]
    read = [name for name in present if name.startswith(AGENT_DEFINITIONS + "/")]
    return present, [] if read else [AGENT_DEFINITIONS]


def check(root, docs=(), run=subprocess.run):
    """(problems, counts) of the documented-command contract under `root`."""
    cache = {}

    def meta_of(tool):
        if tool not in cache:
            cache[tool] = metadata(os.path.join(root, "scripts", tool), run=run)
        return cache[tool]

    problems, examples = [], 0
    names, absent = sources(root, docs, run=run)
    for name in names:
        with open(os.path.join(root, name), encoding="utf-8", errors="replace") as stream:
            found, count = _documented(name, stream.read(), meta_of, root)
        problems.extend(found)
        examples += count
    configuration = os.path.join("docs", "configuration.md")
    if os.path.isfile(os.path.join(root, configuration)):
        with open(os.path.join(root, configuration), encoding="utf-8", errors="replace") as stream:
            problems.extend(table_problems(configuration, stream.read(), meta_of))
    unreadable = sorted(tool for tool, meta in cache.items() if not meta.get("introspectable"))
    # `parsers` counts the tools actually read: a tool whose parser could not be
    # read is named below instead of being counted as agreement (HARNESS-21).
    counts = {"examples": examples, "parsers": len(cache) - len(unreadable),
              "unreadable": unreadable, "sources": len(names), "absent": absent}
    return problems, counts


def summary(counts):
    """The line the caller prints: what was checked, and what could not be."""
    lines = [f"documented commands: {counts['examples']} examples against {counts['parsers']} "
             f"script parsers in {counts.get('sources', 0)} documents; "
             f"shell grammar limited to quotes, `#`, `--`, `<...>` and `[...]`"]
    if counts["unreadable"]:
        lines.append("documented commands: not checked, their parser could not be read: "
                     + ", ".join(counts["unreadable"]))
    if counts.get("absent"):
        lines.append("documented commands: not in this checkout, their examples are unchecked: "
                     + ", ".join(counts["absent"]))
    return "\n".join(lines)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument(DUMP, metavar="TOOL", nargs="?",
                        help="print the parser metadata of TOOL as JSON (what `check` reads); "
                             "without it, run the check over this repository")
    args = parser.parse_args(argv)
    if args.dump:
        print(json.dumps(_capture(args.dump), default=str))
        return 0
    problems, counts = check(".")
    print(summary(counts))
    print("\n".join(problems))
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
