#!/usr/bin/env python3
import importlib.machinery
import importlib.util
import re
import shutil
import subprocess
import tempfile
import tomllib
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SCRIPT = ROOT / "contrib" / "i3-suite-summary"
CENSUS = ROOT / "contrib" / "test-census"
COVERAGE = ROOT / "contrib" / "coverage-report"
ORACLE = ROOT / ".cache" / "sway-ipc-oracle"
WIKI = ROOT / "docs" / "wiki" / "Testing-and-Conformance.md"
RESULTS = ROOT / "docs" / "I3_SUITE_RESULTS.md"
README = ROOT / "README.md"
COMPAT_BEGIN = "<!-- i3-suite-summary:sway-compat "
COMPAT_END = "<!-- end i3-suite-summary:sway-compat -->"


def pinned_snapshot() -> str:
    with (ORACLE / "pins.toml").open("rb") as file:
        return tomllib.load(file)["snapshot"]["swayward"]


def load_script():
    loader = importlib.machinery.SourceFileLoader("i3_suite_summary", str(SCRIPT))
    spec = importlib.util.spec_from_loader(loader.name, loader)
    module = importlib.util.module_from_spec(spec)
    loader.exec_module(module)
    return module


def block(text, name):
    begin = text.index(f"<!-- i3-suite-summary:{name} ")
    return text[begin:text.index(f"<!-- end i3-suite-summary:{name} -->", begin)]


def run(*args):
    return subprocess.run(
        [*map(str, args)], text=True, capture_output=True, check=False, cwd=ROOT
    )


class I3SuiteSummaryTest(unittest.TestCase):
    def test_default_selects_and_discloses_snapshot(self):
        result = run(SCRIPT, ORACLE)
        self.assertEqual(result.returncode, 0, result.stderr)
        snapshot = pinned_snapshot()
        commit = snapshot.removeprefix("swayward-")
        self.assertIn(f"swayward snapshot: {snapshot} ({commit}", result.stdout)

    def test_explicit_snapshot_is_supported(self):
        result = run(SCRIPT, ORACLE, "--swayward", pinned_snapshot())
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_stable_filter_partitions_the_suite(self):
        result = run(SCRIPT, ORACLE)
        self.assertEqual(result.returncode, 0, result.stderr)
        lines = [l for l in result.stdout.splitlines() if l.startswith("i3 suite, ")]
        self.assertTrue(lines)
        for line in lines:
            figures, total = line.split(": ", 1)[1].rsplit(" (of ", 1)
            counted = sum(int(part.split()[0].replace(",", "")) for part in figures.split(", "))
            self.assertEqual(counted, int(total.rstrip(")").replace(",", "")), line)

    def test_summary_skips_pins_without_i3_results(self):
        # Oracle main pins compositors (swayfx, scroll, ...) that have sway
        # IPC results but no i3 suite run.
        with tempfile.TemporaryDirectory() as tmp:
            clone = Path(tmp) / "oracle"
            shutil.copytree(ORACLE, clone, symlinks=True)
            pins = clone / "pins.toml"
            text = pins.read_text()
            self.assertIn("[snapshot]\n", text)
            # A name no oracle pins, so the key cannot collide with a real one.
            pins.write_text(text.replace("[snapshot]\n", '[snapshot]\ntestfx = "testfx-0"\n', 1))
            result = run(SCRIPT, clone)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn("testfx-0", result.stdout)
        self.assertIn(f"i3 suite, {pinned_snapshot()}", result.stdout)

    def test_published_docs_match_the_pinned_oracle(self):
        result = run(SCRIPT, "--check")
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_hand_edit_to_a_generated_cell_fails(self):
        for doc in (WIKI, RESULTS):
            text = doc.read_text()
            begin = text.index("<!-- i3-suite-summary:sway-ipc ")
            row = text.index("| Random sequences |", begin)
            cells = text[row:text.index("\n", row)].split(" | ")
            cells[3] = str(int(cells[3].rstrip("†").replace(",", "")) + 1)
            edited = text[:row] + " | ".join(cells) + text[text.index("\n", row):]
            with tempfile.TemporaryDirectory() as tmp:
                copy = Path(tmp) / doc.name
                copy.write_text(edited)
                result = run(SCRIPT, "--check", copy)
                self.assertEqual(result.returncode, 1, doc)
                self.assertIn("'sway-ipc' differs", result.stderr)
                self.assertEqual(run(SCRIPT, "--write", copy).returncode, 0)
                self.assertEqual(copy.read_text(), text)

    def test_readme_table_is_the_run_counts(self):
        text = README.read_text()
        block = text[text.index(COMPAT_BEGIN):text.index(COMPAT_END)]
        rows = [l for l in block.splitlines() if l.startswith("| ") and "corpus" not in l]
        with (ORACLE / "pins.toml").open("rb") as file:
            snapshot = tomllib.load(file)["snapshot"]["swayward"]
        results = ORACLE / "sway-ipc" / "results"
        runs = []
        for path in sorted(results.glob(f"{snapshot}*.toml")):
            if path.stem == snapshot or path.stem.startswith(f"{snapshot}-"):
                with path.open("rb") as file:
                    runs.append(tomllib.load(file)["run"])
        self.assertEqual(len(rows), len(runs))
        cells = sorted(
            tuple(int(c.rstrip("†").replace(",", "")) for c in row.strip("| ").split(" | ")[1:3])
            for row in rows
        )
        self.assertEqual(cells, sorted((run["match"], run["mismatch"]) for run in runs))

    def test_readme_section_has_no_ratio_or_comparison(self):
        text = README.read_text()
        section = text[text.index("## How compatibility is measured"):]
        section = section[:section.index("\n## ", 1)]
        self.assertIn(COMPAT_BEGIN, section)
        self.assertNotRegex(
            section, r"(?i)more compliant|better tested|[0-9]+(\.[0-9]+)?%|total"
        )

    def test_hand_edit_to_a_readme_cell_fails(self):
        text = README.read_text()
        begin = text.index(COMPAT_BEGIN)
        row = text.index("| Event streams |", begin)
        end = text.index("\n", row)
        cells = text[row:end].split(" | ")
        cells[1] = str(int(cells[1].rstrip("†").replace(",", "")) + 1)
        edited = text[:row] + " | ".join(cells) + text[end:]
        with tempfile.TemporaryDirectory() as tmp:
            copy = Path(tmp) / README.name
            copy.write_text(edited)
            result = run(SCRIPT, "--check", copy)
            self.assertEqual(result.returncode, 1)
            self.assertIn("'sway-compat' differs", result.stderr)
            self.assertEqual(run(SCRIPT, "--write", copy).returncode, 0)
            self.assertEqual(copy.read_text(), text)

    def test_i3_suite_table_holds_swayward_only(self):
        # One compositor per table: no sway row beside swayward's.
        for doc in (WIKI, RESULTS):
            body = block(doc.read_text(), "i3-suite")
            rows = [l for l in body.splitlines() if l.startswith("| ") and "---" not in l]
            self.assertEqual(len(rows), 2, doc)  # header + swayward
            self.assertTrue(rows[1].startswith("| swayward "), doc)
            self.assertIn("results page", body)
            self.assertNotIn("are classified;", body)

    def test_wiki_drops_the_bucket_line(self):
        self.assertNotIn("Interpretive buckets", block(WIKI.read_text(), "i3-suite"))

    def test_untriaged_sentence_names_the_nonzero_corpora(self):
        summary = run(SCRIPT, ORACLE)
        self.assertEqual(summary.returncode, 0, summary.stderr)
        expected = {
            m["corpus"]: int(m["count"].replace(",", ""))
            for m in re.finditer(r"^  (?P<corpus>[\w-]+)†?: .*, (?P<count>[\d,]+) untriaged$",
                                 summary.stdout, re.M)
            if m["count"] != "0"
        }
        text = block(WIKI.read_text(), "sway-ipc")
        self.assertNotIn("untriaged mismatches |", text)
        if expected:
            line = next(l for l in text.splitlines() if l.startswith("Swayward mismatches with no row"))
            counts = [int(c.rstrip("†").replace(",", ""))
                      for c in re.findall(r" ([\d,]+†?)(?:,|\.$)", line)]
            self.assertEqual(sorted(counts), sorted(expected.values()))
        else:
            self.assertIn("Every swayward mismatch is classified", text)

    def test_untriaged_sentence_when_everything_is_classified(self):
        module = load_script()
        oracle = module.Oracle(ORACLE)
        oracle.untriaged = lambda suffix, result: 0
        text = module.render_sway_ipc(oracle)
        self.assertIn(
            "Every swayward mismatch is classified in the oracle's "
            "`sway-ipc/classifications/`.", text)
        self.assertNotIn("no row yet", text)
        oracle.untriaged = lambda suffix, result: 3 if suffix == "events" else 0
        text = module.render_sway_ipc(oracle)
        self.assertRegex(text, r"no row yet in the oracle's `sway-ipc/classifications/`: "
                               r"Events 3†?\.")

    def test_doc_without_a_block_fails(self):
        with tempfile.NamedTemporaryFile("w", suffix=".md") as file:
            file.write("no generated block\n")
            file.flush()
            result = run(SCRIPT, "--check", file.name)
        self.assertEqual(result.returncode, 1)
        self.assertIn("no generated i3-suite-summary block", result.stderr)

    def test_oracle_off_the_pin_is_refused(self):
        with tempfile.TemporaryDirectory() as tmp:
            clone = Path(tmp) / "oracle"
            shutil.copytree(ORACLE, clone, symlinks=True)
            subprocess.run(
                ["git", "-C", clone, "-c", "user.name=t", "-c", "user.email=t@t",
                 "commit", "-q", "--allow-empty", "-m", "off pin"],
                check=True,
            )
            result = run(SCRIPT, clone, "--check")
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertIn("tests/oracle.toml pins", result.stderr)


class CoverageDocTest(unittest.TestCase):
    def test_published_census_matches(self):
        result = run(COVERAGE, "--check-doc")
        self.assertEqual(result.returncode, 0, result.stdout)

    def test_hand_edit_to_the_census_fails(self):
        text = WIKI.read_text()
        with tempfile.TemporaryDirectory() as tmp:
            copy = Path(tmp) / WIKI.name
            copy.write_text(text.replace("| Files | 242 |", "| Files | 243 |", 1))
            result = run(COVERAGE, "--check-doc", copy)
            self.assertEqual(result.returncode, 1)
            self.assertIn("differs from the census", result.stdout)
            self.assertEqual(run(COVERAGE, "--write-doc", copy).returncode, 0)
            self.assertEqual(copy.read_text(), text)


class TestCensusDocTest(unittest.TestCase):
    def test_published_seed_table_matches(self):
        result = run(CENSUS, "--check-doc")
        self.assertEqual(result.returncode, 0, result.stdout)

    def test_hand_edit_to_a_seed_cell_fails(self):
        text = WIKI.read_text()
        row = text.index("| `proptest-regressions/layout/tests.txt` | ")
        end = text.index("\n", row)
        cells = text[row:end].split(" | ")
        cells[1] = str(int(cells[1].rstrip(" |")) + 1) + " |"
        edited = text[:row] + " | ".join(cells) + text[end:]
        self.assertNotEqual(edited, text)
        with tempfile.TemporaryDirectory() as tmp:
            copy = Path(tmp) / WIKI.name
            copy.write_text(edited)
            result = run(CENSUS, "--check-doc", copy)
            self.assertEqual(result.returncode, 1)
            self.assertIn("seed table differs", result.stdout)
            self.assertEqual(run(CENSUS, "--write-doc", copy).returncode, 0)
            self.assertEqual(copy.read_text(), text)


if __name__ == "__main__":
    unittest.main()
