"""The documented-command contract checks examples against real parsers.

The defect this guards is the one the harness audit found (F9/F10): the
documents recommended `difftest.py --name`, which the parser has never had, and
nothing compared the two. These tests drive the check from both sides -- a
document whose example the parser refuses, and the same example with a spelling
the parser has -- and they drive the real child for the metadata, so a parser
that is never read (or a tool whose handler runs) fails here.
"""

import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

import cli_contract as contract

ROOT = Path(contract.__file__).resolve().parents[1]

#: A tool that records that its handler ran, so the metadata child can be proved
#: to stop before it: `ran` is written after `parse_args`, never during it.
FIXTURE_TOOL = '''#!/usr/bin/env python3
import argparse, pathlib, sys
def main():
    parser = argparse.ArgumentParser(description="fixture")
    parser.add_argument("--depth", type=int, default=7)
    parser.add_argument("cmd", choices=["list", "tlc"])
    args = parser.parse_args()
    pathlib.Path(__file__).with_name("ran").write_text(str(args))
if __name__ == "__main__":
    sys.exit(main())
'''

NO_PARSER_TOOL = '''#!/usr/bin/env python3
import sys
def main():
    print("a library, not a command line")
if __name__ == "__main__":
    sys.exit(main())
'''

#: A wrapper that runs the program it is given, before it ever looks at its own
#: options: the shape `rustc_slots.py RUSTC [ARGS...]` has.
FORWARDING_TOOL = '''#!/usr/bin/env python3
import os, pathlib, sys
def main():
    marker = pathlib.Path(__file__).with_name("ran")
    os.execvp(sys.executable,
              [sys.executable, "-c",
               "import pathlib,sys; pathlib.Path(sys.argv[1]).write_text('ran')",
               str(marker)])
if __name__ == "__main__":
    sys.exit(main())
'''


def options(*spellings, defaults=(), flags=()):
    """A parser metadata node with `spellings` as value options by default."""
    node = {"options": {s: s.lstrip("-").replace("-", "_") for s in spellings},
            "kinds": {s: ("flag" if s in flags else "value") for s in spellings},
            "defaults": dict(defaults), "positionals": []}
    return node


def metadata(top=None, subcommands=None):
    return {"tool": "tool.py", "introspectable": True, "prog": "tool.py",
            "top": top or options(), "subcommands": subcommands or {}}


def with_positional(node, *choices):
    """The same node with one positional constrained to `choices`."""
    return {**node, "positionals": [sorted(choices)]}


class Documents(unittest.TestCase):
    """Command examples in Markdown, against a parser the test states."""

    def setUp(self):
        self.root = tempfile.TemporaryDirectory(prefix="cli-contract-")
        self.addCleanup(self.root.cleanup)
        for tool in ("difftest.py", "bench.py"):
            Path(self.root.name, "scripts", tool).parent.mkdir(parents=True, exist_ok=True)
            Path(self.root.name, "scripts", tool).write_text("")

    def checked(self, text, meta):
        problems, examples = contract._documented("doc.md", text, lambda _: meta, self.root.name)
        self.assertEqual(examples, 1, problems)
        return problems

    def test_an_option_the_parser_never_had_is_reported(self):
        # The F9 defect: `--name` is not `--ids`/`--filter`.
        meta = metadata(options("--ids", "--filter"))
        self.assertEqual(
            self.checked("Run `python3 scripts/difftest.py --name M003` for one model.\n", meta),
            ["doc.md:1: difftest.py does not accept --name"])

    def test_the_same_example_with_a_real_spelling_passes(self):
        meta = metadata(options("--ids", "--filter"))
        self.assertEqual(self.checked("Run `scripts/difftest.py --ids M003`.\n", meta), [])

    def test_an_unsupported_subcommand_is_reported(self):
        meta = metadata(options(), subcommands={"run": options("--tlc"), "ab": options("--bin")})
        self.assertEqual(
            self.checked("```sh\nscripts/bench.py repr-survey --self-test\n```\n", meta),
            ["doc.md:2: bench.py has no subcommand repr-survey"])

    def test_a_subcommand_less_mention_is_checked_against_every_subcommand(self):
        # `bench.py --hog N` names one subcommand's option without naming it.
        meta = metadata(options(), subcommands={"run": options("--tlc"), "ab": options("--hog")})
        self.assertEqual(self.checked("Measure `scripts/bench.py --hog 4` on the cloud.\n", meta), [])

    def test_the_marker_exempts_a_deliberate_counter_example(self):
        # A document may show a command that is wrong on purpose; the marker is
        # how it says so, and the example then counts as neither checked nor wrong.
        meta = metadata(options("--extra"))
        exempt = ("`scripts/difftest.py --compile` is not the compiled mode"
                  "  # cli-contract: " + "ignore\n")
        self.assertEqual(contract._documented("doc.md", exempt, lambda _: meta, self.root.name),
                         ([], 0))
        self.assertEqual(self.checked(exempt.replace("  # cli-contract: " + "ignore", ""), meta),
                         ["doc.md:1: difftest.py does not accept --compile"])

    def test_shell_outside_the_modelled_grammar_is_skipped(self):
        meta = metadata(options("--ids"))
        for text in ("`scripts/difftest.py --ids M | grep x`\n",
                     "`scripts/difftest.py --ids $MODELS`\n",
                     "`scripts/difftest.py --ids M > out.txt`\n"):
            with self.subTest(text=text):
                self.assertEqual(contract._documented("doc.md", text, lambda _: meta,
                                                      self.root.name)[0], [])

    def test_a_comment_and_a_placeholder_are_not_arguments(self):
        # `-recover` in the trailing comment and `<commit>` are prose and a value,
        # not options the parser must accept (docs/testing.md:285, :776).
        meta = metadata(options("--extra", "--recover"))
        self.assertEqual(self.checked(
            "`scripts/difftest.py --extra=-compile --recover 1   # then -recover it`\n", meta), [])
        bench = metadata(options(), subcommands={"history-report": options("--tlc-baseline")})
        self.assertEqual(self.checked(
            "`scripts/bench.py history-report <commit> --tlc-baseline tlc-<rev>`\n", bench), [])

    def test_a_positional_choice_is_the_subcommand_of_a_tool_without_subparsers(self):
        # `corpus.py list|tlc` is a positional, not a subparser: an unknown one
        # is the same defect as an unknown `bench.py` subcommand.
        meta = metadata(with_positional(options("--name"), "list", "tlc"))
        self.assertEqual(self.checked("`scripts/difftest.py tlcc --name x`\n", meta),
                         ["doc.md:1: difftest.py takes no tlcc (it takes list, tlc)"])
        self.assertEqual(self.checked("```sh\nscripts/difftest.py tlc --name x\n```\n", meta), [])

    def test_a_placeholder_is_a_value_the_document_leaves_open(self):
        # `<cmd>` says the reader chooses; reporting it as a wrong choice would
        # read a placeholder as a claim.
        meta = metadata(with_positional(options("--name"), "list", "tlc"))
        self.assertEqual(self.checked("`scripts/difftest.py <cmd> --name x`\n", meta), [])

    def test_a_tool_without_a_readable_parser_is_unchecked_not_wrong(self):
        self.assertEqual(self.checked("`scripts/difftest.py --anything`\n",
                                      {"introspectable": False, "why": "no parse_args call"}), [])

    def test_an_example_without_an_option_is_not_an_option_claim(self):
        meta = metadata(options("--ids"))
        problems, examples = contract._documented(
            "doc.md", "Run `scripts/difftest.py` from the root.\n", lambda _: meta, self.root.name)
        self.assertEqual((problems, examples), ([], 0))


class OptionTable(unittest.TestCase):
    """The `## Script options` tables of docs/configuration.md."""

    HEAD = "## Script options\n\n"

    def checked(self, body, meta):
        return contract.table_problems("docs/configuration.md", self.HEAD + body, lambda _: meta)

    def test_a_documented_default_is_compared_with_the_parser(self):
        meta = metadata(options("--timeout", defaults={"--timeout": 120}))
        body = ("| Option | Meaning | Scripts |\n|---|---|---|\n"
                "| `--timeout S` | timeout | `difftest.py` (90) |\n")
        self.assertEqual(
            self.checked(body, meta),
            ["docs/configuration.md:5: difftest.py documents the default of --timeout as 90, "
             "the parser declares another"])

    def test_the_declared_default_passes(self):
        meta = metadata(options("--timeout", defaults={"--timeout": 120}))
        body = ("| Option | Meaning | Scripts |\n|---|---|---|\n"
                "| `--timeout S` | timeout | `difftest.py` (120) |\n")
        self.assertEqual(self.checked(body, meta), [])

    def test_a_script_row_names_only_options_the_parser_has(self):
        meta = metadata(options("--ids", "--filter"))
        body = ("| Script | Options |\n|---|---|\n"
                "| `difftest.py` | `--ids A,B`, `--nope` |\n")
        self.assertEqual(self.checked(body, meta),
                         ["docs/configuration.md:5: difftest.py does not accept --nope"])

    def test_an_escaped_pipe_does_not_split_the_row(self):
        # `list\|tlc` is one cell: splitting on it would read the row as two.
        meta = metadata(options("--name", "--only-failed"))
        body = ("| Script | Options |\n|---|---|\n"
                "| `corpus.py` | `list\\|tlc`, `--name NAME`, `--only-failed` |\n")
        self.assertEqual(self.checked(body, meta), [])


class Child(unittest.TestCase):
    """The metadata child: complete argv, a real parser, no handler run."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="cli-contract-child-")
        self.addCleanup(self.tmp.cleanup)

    def tool(self, text, name="fixture_tool.py"):
        path = Path(self.tmp.name, name)
        path.write_text(text)
        return str(path)

    def test_the_child_receives_the_complete_argv(self):
        seen = []

        def runner(argv, **kwargs):
            seen.append(list(argv))
            return subprocess.CompletedProcess(argv, 0, json.dumps(metadata()), "")

        contract.metadata("scripts/difftest.py", run=runner)
        # The executable and every argument, in order: a fake that drops argv[0]
        # or reorders the flags would assert a different command than the child gets.
        self.assertEqual(seen, [[sys.executable, str(ROOT / "scripts" / "cli_contract.py"),
                                 "--dump", "scripts/difftest.py"]])

    def test_the_parser_is_read_without_running_the_handler(self):
        meta = contract.metadata(self.tool(FIXTURE_TOOL))
        self.assertTrue(meta["introspectable"], meta)
        self.assertEqual(sorted(meta["top"]["options"]), ["--depth", "--help", "-h"])
        self.assertEqual(meta["top"]["defaults"]["--depth"], 7)
        self.assertEqual(meta["top"]["positionals"], [["list", "tlc"]])
        self.assertFalse(Path(self.tmp.name, "ran").exists(),
                         "the handler ran: the example action must never execute")

    def test_a_tool_that_never_parses_is_reported_as_unreadable(self):
        meta = contract.metadata(self.tool(NO_PARSER_TOOL))
        self.assertFalse(meta["introspectable"])
        self.assertIn("parse_args", meta["why"])

    def test_a_tool_that_forwards_its_argv_runs_nothing(self):
        # `rustc_slots.py RUSTC ...` execs its first argument before it looks at
        # argv: reading its parser must not run the program the document named.
        path = self.tool(FORWARDING_TOOL)
        ran = Path(self.tmp.name, "ran")
        meta = contract.metadata(path)
        self.assertFalse(meta["introspectable"], meta)
        self.assertIn("process", meta["why"])
        self.assertFalse(ran.exists(), "the forwarded program ran: nothing may execute")

    def test_a_real_driver_is_read_by_its_own_parser(self):
        # No fake: the contract's metadata is the parser of the script it names.
        meta = contract.metadata(str(ROOT / "scripts" / "difftest.py"))
        self.assertTrue(meta["introspectable"], meta)
        self.assertIn("--ids", meta["top"]["options"])
        self.assertNotIn("--name", meta["top"]["options"])
        self.assertEqual(meta["top"]["defaults"]["--timeout"], 120)


class WholeCheck(unittest.TestCase):
    """`check()` end to end, with the child and `git ls-files` driven by a fake."""

    def setUp(self):
        self.root = tempfile.TemporaryDirectory(prefix="cli-contract-root-")
        self.addCleanup(self.root.cleanup)
        Path(self.root.name, "scripts").mkdir()
        Path(self.root.name, "scripts", "difftest.py").write_text("")
        Path(self.root.name, "docs").mkdir()
        self.table("| `difftest.py` | `--ids A,B` |")
        Path(self.root.name, "guide.md").write_text("Run `scripts/difftest.py --name X`.\n")

    def table(self, row):
        Path(self.root.name, "docs", "configuration.md").write_text(
            "## Script options\n\n| Script | Options |\n|---|---|\n" + row + "\n")

    def runner(self, metadata_json=None, dump=None):
        def run(argv, **kwargs):
            if argv[0] == "git":
                return subprocess.CompletedProcess(argv, 0, "agent.md\0", "")
            text = dump if dump is not None else json.dumps(
                metadata_json or metadata(options("--ids", "--filter")))
            return subprocess.CompletedProcess(argv, 0, text, "")
        return run

    def test_check_reports_the_document_and_the_table(self):
        self.table("| `difftest.py` | `--name X` |")
        problems, counts = contract.check(self.root.name, ["guide.md"], run=self.runner())
        self.assertEqual(problems, ["guide.md:1: difftest.py does not accept --name",
                                    "docs/configuration.md:5: difftest.py does not accept --name"])
        self.assertEqual(counts["examples"], 1)
        self.assertEqual(counts["parsers"], 1)

    def test_check_names_a_tool_whose_parser_could_not_be_read(self):
        problems, counts = contract.check(self.root.name, ["guide.md"],
                                          run=self.runner(dump="not json"))
        self.assertEqual(problems, [])
        self.assertEqual(counts["unreadable"], ["difftest.py"])
        self.assertIn("not checked, their parser could not be read: difftest.py",
                      contract.summary(counts))

    def test_bench_measurement_records_are_not_instructions(self):
        Path(self.root.name, "bench").mkdir()
        Path(self.root.name, "bench", "results.md").write_text("`scripts/difftest.py --name X`\n")
        listed = []

        def run(argv, **kwargs):
            listed.extend(str(item) for item in argv)
            if argv[0] == "git":
                return subprocess.CompletedProcess(argv, 0, "", "")
            return subprocess.CompletedProcess(argv, 0, json.dumps(metadata(options("--ids"))), "")

        problems, _ = contract.check(self.root.name, ["bench/results.md"], run=run)
        self.assertEqual(problems, [])
        self.assertIn(".agents/agents", listed)  # the tracked agent definitions are read

    def test_a_checkout_without_the_agent_definitions_says_so(self):
        # The published tree ships no harness, so there is nothing to read from
        # `.agents/agents`: the summary names it instead of letting a smaller
        # example count read as agreement about documents that were never opened.
        problems, counts = contract.check(self.root.name, ["guide.md"], run=self.runner())
        self.assertEqual(counts["absent"], [".agents/agents"])
        self.assertIn("not in this checkout, their examples are unchecked: .agents/agents",
                      contract.summary(counts))

    def test_a_definition_on_disk_is_read_even_when_untracked(self):
        # A definition the index does not list still documents a command; only
        # its absence justifies reporting the reduced surface.
        definitions = Path(self.root.name, ".agents", "agents")
        definitions.mkdir(parents=True)
        definitions.joinpath("worker.md").write_text("`scripts/difftest.py --name X`\n")
        problems, counts = contract.check(self.root.name, [], run=self.runner())
        self.assertEqual(counts["absent"], [])
        self.assertEqual(problems, [".agents/agents/worker.md:1: difftest.py does not accept --name"])


if __name__ == "__main__":
    unittest.main()
