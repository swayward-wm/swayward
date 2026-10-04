#!/usr/bin/env python3
import json
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SCRIPT = ROOT / "contrib" / "oracle-sweep-diff"
# Scratch output stays beside the repo, never in /tmp (AGENTS.md).
SCRATCH = ROOT.parent / "scratch"

ROW = '[[result]]\nscenario = "{}"\nrequest = "tree"\nverdict = "{}"\n'
SEED_MATCH = '[[result]]\nseed = {}\nverdict = "match"\n'
SEED_MISMATCH = '[[result]]\nseed = {}\nverdict = "mismatch"\nrequest = "tree"\n'


class OracleSweepDiffTest(unittest.TestCase):
    def sweep(self, root: Path, name: str, files: dict[str, str]) -> Path:
        directory = root / name
        directory.mkdir()
        for file, text in files.items():
            (directory / file).write_text(text)
        return directory

    def invoke(self, before: dict, after: dict):
        SCRATCH.mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(dir=SCRATCH) as tmp:
            a = self.sweep(Path(tmp), "a", before)
            b = self.sweep(Path(tmp), "b", after)
            return subprocess.run([SCRIPT, a, b], text=True, capture_output=True, check=False)

    def invoke_ledger(self, status: dict | None, sweeps: int = 1):
        """Run --ledger `sweeps` times on one finished sweep; return the last
        result and the ledger's lines."""
        SCRATCH.mkdir(exist_ok=True)
        before = {"state.toml": ROW.format("x", "match") + ROW.format("y", "mismatch"),
                  "random-0.toml": SEED_MATCH.format(1)}
        after = {"state.toml": ROW.format("x", "match") + ROW.format("y", "match"),
                 "random-0.toml": SEED_MATCH.format(1), "random-1.toml": SEED_MISMATCH.format(2),
                 "i3d-0.toml": ROW.format("d", "match"), "events.toml": ROW.format("e", "mismatch")}
        with tempfile.TemporaryDirectory(dir=SCRATCH) as tmp:
            a = self.sweep(Path(tmp), "a", before)
            b = self.sweep(Path(tmp), "b", after)
            if status is not None:
                (b / "status.json").write_text(json.dumps(status))
            ledger = Path(tmp, "progress.tsv")
            for _ in range(sweeps):
                result = subprocess.run([SCRIPT, "--ledger", ledger, a, b],
                                        text=True, capture_output=True, check=False)
            lines = ledger.read_text().splitlines() if ledger.exists() else []
            return result, lines

    @staticmethod
    def git(*args: str) -> str:
        return subprocess.run(["git", "-C", ROOT, *args], text=True, capture_output=True,
                              check=True).stdout.strip()

    def status(self, dead: str = "") -> dict:
        return {"swayward": self.git("rev-parse", "HEAD"), "oracle": "021ed5f" + "0" * 33,
                "wall_min": 10, "dead": dead}

    def test_identical_sweeps_exit_zero(self):
        files = {"state.toml": ROW.format("x", "match"), "random-0.toml": SEED_MATCH.format(1)}
        result = self.invoke(files, files)
        self.assertEqual(result.returncode, 0, result.stdout)
        self.assertIn("lost: 0", result.stdout)

    def test_random_rows_key_by_seed_whatever_the_verdict(self):
        # A mismatching random row carries a request key; a matching one does not.
        result = self.invoke(
            {"random-0.toml": SEED_MATCH.format(1) + SEED_MISMATCH.format(2)},
            {"random-1.toml": SEED_MISMATCH.format(1) + SEED_MATCH.format(2)},
        )
        self.assertEqual(result.returncode, 1)
        self.assertIn("lost: 1\n  random 1", result.stdout)
        self.assertIn("gained: 1\n  random 2", result.stdout)
        self.assertIn("missing: 0", result.stdout)

    def test_missing_row_fails(self):
        result = self.invoke(
            {"state.toml": ROW.format("x", "match") + ROW.format("y", "match")},
            {"state.toml": ROW.format("x", "match")},
        )
        self.assertEqual(result.returncode, 1)
        self.assertIn("missing: 1\n  state y tree", result.stdout)

    def test_valid_sweep_appends_one_row_keyed_by_tree_and_oracle(self):
        result, lines = self.invoke_ledger(self.status())
        # Seed 2 is new in after, so the diff exits 1, but the sweep is valid.
        self.assertEqual(result.returncode, 1, result.stderr)
        header, row = lines[0].split("\t"), dict(zip(lines[0].split("\t"), lines[1].split("\t")))
        self.assertEqual(len(lines), 2)
        self.assertEqual(header[:4], ["date", "swayward_sha", "tree", "oracle_sha"])
        self.assertEqual(row["tree"], self.git("rev-parse", "HEAD^{tree}")[:8])
        self.assertEqual(row["swayward_sha"], self.git("rev-parse", "HEAD")[:8])
        self.assertEqual(row["oracle_sha"], "021ed5f")
        self.assertEqual([row[c] for c in ("random", "i3d", "state", "events")],
                         ["1/2", "1/1", "2/2", "0/1"])
        self.assertEqual((row["lost"], row["gained"], row["wall_min"]), ("0", "1", "10"))
        self.assertIn(f"SWEEP tree={row['tree']} sha={row['swayward_sha']} oracle=021ed5f "
                      "random 1/2 i3d 1/1 state 2/2 events 0/1 lost 0 gained 1 wall 10",
                      result.stdout)

    def test_second_sweep_appends_without_a_second_header(self):
        _, lines = self.invoke_ledger(self.status(), sweeps=2)
        self.assertEqual(len(lines), 3)
        self.assertTrue(lines[0].startswith("date\t"))
        self.assertFalse(lines[2].startswith("date\t"))

    def test_dead_shard_sweep_appends_nothing(self):
        result, lines = self.invoke_ledger(self.status(dead="random-1 i3d-0"))
        self.assertEqual(result.returncode, 2)
        self.assertIn("dead shards: random-1 i3d-0", result.stderr)
        self.assertNotIn("SWEEP", result.stdout)
        self.assertEqual(lines, [])

    def test_unfinished_sweep_appends_nothing(self):
        result, lines = self.invoke_ledger(None)
        self.assertEqual(result.returncode, 2)
        self.assertIn("did not finish", result.stderr)
        self.assertEqual(lines, [])


if __name__ == "__main__":
    unittest.main()
