"""doccheck exit status and fixture handling, run against throwaway repository roots."""
from pathlib import Path
import json
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parent.parent / "doccheck.py"


def standards():
    ids = [f"{area}-{n:02}" for area, count in
           [("CODE", 24), ("DOC", 12), ("HARNESS", 22), ("ARCH", 10)]
           for n in range(1, count + 1)]
    index = "\n".join(f"| [{i}](#{i.lower()}) | Criterion | {i.split('-')[0]} |" for i in ids)
    rules = "\n".join(f"### {i}\n" + "\n".join(
        f"- **{field}:** Evidence." for field in ["Rule", "Rationale", "Checked by", "Example"])
        for i in ids)
    return f"# Standards\n## Rule index\n{index}\n## Rules\n{rules}\n"


def run_doccheck(files, *args, tracked=None, drop=()):
    with tempfile.TemporaryDirectory(prefix="tlx-doccheck-test-") as tmp:
        root = Path(tmp)
        files = {"crates/tlx-base/src/knobs.rs": 'flag("' + 'TLX_' + 'KNOWN")\n',
                 "README.md": "# tlx\n", "AGENTS.md": "# agents\n",
                 "docs/development-standards.md": standards(), **files}  # doccheck: ignore
        files = {name: text for name, text in files.items() if name not in drop}
        for name, text in files.items():
            path = root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text)
            if name == "target/test-release/tlx":
                path.chmod(0o755)
        if tracked is not None:
            subprocess.run(["git", "init", "-q", str(root)], check=True, capture_output=True)
            subprocess.run(["git", "-C", str(root), "add", "--", *tracked], check=True, capture_output=True)
        result = subprocess.run([sys.executable, str(SCRIPT), str(root), *args],
                                text=True, capture_output=True)
        calls = root / 'target/test-release/calls.jsonl'
        result.parse_calls = [json.loads(line) for line in calls.read_text().splitlines()] if calls.exists() else []
        return result


class DoccheckTest(unittest.TestCase):
    def test_clean_tree_exits_zero(self):
        result = run_doccheck({})
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("-- 0 problems", result.stdout)

    def test_problem_exits_one_and_keeps_output_format(self):
        result = run_doccheck({"README.md": "see [x](missing.md)\n"})
        self.assertEqual(result.returncode, 1)
        self.assertIn("README.md:1: broken link missing.md", result.stdout)
        self.assertIn("-- 1 problems", result.stdout)

    def test_fixture_path_in_code_is_reported_unless_marked(self):
        line = 'write("docs/' + 'fixture.md")'
        self.assertEqual(run_doccheck({"scripts/t.py": line + "\n"}).returncode, 1)
        marked = line + "  # doccheck: " + "ignore\n"
        result = run_doccheck({"scripts/t.py": marked})
        self.assertEqual(result.returncode, 0, result.stdout)

    def test_public_entrypoints_reject_broken_links_and_accept_repair(self):
        for path in ["CONTRIBUTING.md", "SECURITY.md", ".agents/skills/check/SKILL.md", "bench/guide.md"]:
            with self.subTest(path=path):
                bad = run_doccheck({path: "[broken](missing.md)\n"})
                self.assertEqual(bad.returncode, 1, bad.stdout + bad.stderr)
                self.assertIn(f"{path}:1: broken link", bad.stdout)
                good = run_doccheck({path: "[fixed](#here)\n## Here\n"})
                self.assertEqual(good.returncode, 0, good.stdout + good.stderr)

    def test_a_partial_checkout_names_what_it_cannot_check(self):
        # A partial checkout may lack AGENTS.md. Report the omission instead of
        # crashing or silently claiming that the document was checked.
        result = run_doccheck({}, drop=("AGENTS.md",))
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("documents: absent from this checkout, not checked: AGENTS.md", result.stdout)
        self.assertIn("-- 0 problems", result.stdout)

    def test_a_link_to_a_file_the_checkout_lacks_is_still_broken(self):
        # Unshipped data is named as text, never linked: the release tree must
        # fail on the reference, not excuse it because the file is elsewhere.
        for link, target in [("missing.md", "missing.md"),
                             ("../AGENTS.md", "../AGENTS.md"),
                             ("../bench/records/one.json", "../bench/records/one.json")]:
            with self.subTest(link=link):
                result = run_doccheck({"docs/performance.md": f"[x]({link})\n"}, drop=("AGENTS.md",))
                self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
                self.assertIn(f"docs/performance.md:1: broken link {target}", result.stdout)

    def test_only_tracked_skill_and_bench_markdown_is_selected(self):
        files = {".agents/skills/tracked/SKILL.md": "# Checked\n",
                 ".agents/skills/local/SKILL.md": "[local](missing.md)\n",
                 "bench/local.md": "[local](missing.md)\n"}
        result = run_doccheck(files, tracked=[".agents/skills/tracked/SKILL.md"])
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        files[".agents/skills/tracked/SKILL.md"] = "[bad](missing.md)\n"
        self.assertEqual(run_doccheck(files, tracked=[".agents/skills/tracked/SKILL.md"]).returncode, 1)

    def test_duplicate_heading_slugs_and_explicit_anchors(self):
        text = ('# Same\n# Same\n# Same\n<a id="custom"></a>\n'
                '[a](#same) [b](#same-1) [c](#same-2) [d](#custom)\n')
        self.assertEqual(run_doccheck({"README.md": text}).returncode, 0)
        bad = run_doccheck({"README.md": text + "[bad](#same-3)\n"})
        self.assertEqual(bad.returncode, 1)
        self.assertIn("missing anchor #same-3", bad.stdout)

    def test_fences_ignore_pseudo_links_sections_and_headings(self):
        for fence in ["```", "~~~~", "   ````"]:
            with self.subTest(fence=fence):
                text = (f"{fence}markdown\n# Ghost\n[bad](missing.md)\n"
                        "docs/absent.md §99\n```\n" + fence + "\n# Real\n[ok](#real)\n")  # doccheck: ignore
                # A shorter delimiter inside a longer fence must not close it.
                if fence == "```":
                    text = text.replace("```\n```\n", "```\n")
                good = run_doccheck({"README.md": text})
                self.assertEqual(good.returncode, 0, good.stdout + good.stderr)
                bad = run_doccheck({"README.md": text + "[bad](#ghost)\n"})
                self.assertEqual(bad.returncode, 1)
                self.assertIn("missing anchor #ghost", bad.stdout)

    def test_standard_schema_rejects_damage_and_accepts_control(self):
        source = standards()
        mutations = {
            "duplicate ID": source.replace("### CODE-02", "### CODE-01"),
            "missing ID": source.replace("### CODE-01", "### Other"),
            "missing field": source.replace("- **Example:** Evidence.\n", "", 1),
            "empty field": source.replace("- **Rule:** Evidence.", "- **Rule:**", 1),
            "duplicate field": source.replace("- **Rule:** Evidence.", "- **Rule:** Evidence.\n- **Rule:** Again.", 1),
            "missing index": source.replace("| [CODE-01](#code-01) | Criterion | CODE |\n", ""),
            "duplicate index": source.replace("[CODE-02](#code-02)", "[CODE-01](#code-01)"),
            "wrong area": source.replace("| Criterion | CODE |", "| Criterion | DOC |", 1),
        }
        for damage, text in mutations.items():
            with self.subTest(damage=damage):
                result = run_doccheck({"docs/development-standards.md": text})  # doccheck: ignore
                self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
                self.assertIn("standards schema:", result.stdout)
        good = run_doccheck({})
        self.assertEqual(good.returncode, 0, good.stdout + good.stderr)
        self.assertIn("68 rules checked", good.stdout)

    def test_required_examples_refuse_missing_and_stale_binary(self):
        files = {"README.md": "```tla\n---- MODULE Good ----\n====\n```\n"}
        optional = run_doccheck(files)
        self.assertEqual(optional.returncode, 0, optional.stdout + optional.stderr)
        self.assertIn("parsing skipped (1 blocks)", optional.stdout)
        required = run_doccheck(files, "--require-examples")
        self.assertEqual(required.returncode, 1, required.stdout + required.stderr)
        self.assertIn("required example parsing unavailable", required.stdout)
        files["target/test-release/tlx"] = f"#!{sys.executable}\nraise SystemExit(2)\n"
        stale = run_doccheck(files, "--require-examples")
        self.assertEqual(stale.returncode, 1, stale.stdout + stale.stderr)
        self.assertIn("cannot parse a trivial module", stale.stdout)

    def test_required_examples_refuse_missing_prerequisite_even_without_modules(self):
        result = run_doccheck({}, "--require-examples")
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn("required example parsing unavailable", result.stdout)
        stale = run_doccheck({"target/test-release/tlx": f"#!{sys.executable}\nraise SystemExit(2)\n"},
                             "--require-examples")
        self.assertEqual(stale.returncode, 1, stale.stdout + stale.stderr)
        self.assertIn("required example parsing unavailable", stale.stdout)

    def test_required_examples_run_parse_only_and_propagate_errors(self):
        files = {"README.md": "```tla\n---- MODULE Good ----\n====\n```\n",
                 "target/test-release/tlx": f"#!{sys.executable}\n" +
                 "import sys, json\nfrom pathlib import Path\n" +
                 "with Path(__file__).with_name('calls.jsonl').open('a') as f:\n" +
                 "    f.write(json.dumps(sys.argv) + '\\n')\n" +
                 "assert sys.argv[1] == 'parse', sys.argv\n" +
                 "assert len(sys.argv) == 3, sys.argv\n" +
                 "raise SystemExit('BROKEN' in Path(sys.argv[2]).read_text())\n"}
        good = run_doccheck(files, "--require-examples")
        self.assertEqual(good.returncode, 0, good.stdout + good.stderr)
        self.assertIn("parse-only", good.stdout)
        self.assertIn("selected=1 executed=1 passed=1 failed=0 skipped=0", good.stdout)
        self.assertEqual(len(good.parse_calls), 2)
        for command in good.parse_calls:
            self.assertTrue(command[0].endswith('/target/test-release/tlx'), command)
            self.assertEqual(command[1], 'parse')
            self.assertEqual(len(command), 3)
        self.assertEqual([Path(c[2]).name for c in good.parse_calls], ['Probe.tla', 'Good.tla'])
        files["README.md"] = files["README.md"].replace("====", "BROKEN\n====")
        bad = run_doccheck(files, "--require-examples")
        self.assertEqual(bad.returncode, 1, bad.stdout + bad.stderr)
        self.assertIn("invalid TLA+ module", bad.stdout)
        self.assertIn("selected=1 executed=1 passed=0 failed=1 skipped=0", bad.stdout)


if __name__ == "__main__":
    unittest.main()
