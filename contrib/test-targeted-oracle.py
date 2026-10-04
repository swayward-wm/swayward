#!/usr/bin/env python3
import importlib.machinery
import importlib.util
import json
import os
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SCRIPT = ROOT / "contrib" / "targeted-oracle"
# Scratch output stays beside the repo, never in /tmp (AGENTS.md).
OUT = ROOT.parent / "scratch" / "targeted-oracle-test"


def load_module():
    loader = importlib.machinery.SourceFileLoader("targeted_oracle", str(SCRIPT))
    module = importlib.util.module_from_spec(importlib.util.spec_from_loader(loader.name, loader))
    loader.exec_module(module)
    return module


class TargetedOracleTest(unittest.TestCase):
    def invoke(self, *paths: str):
        return subprocess.run(
            [SCRIPT, "--out-dir", OUT, *paths], text=True, capture_output=True, check=False
        )

    def test_test_only_change_selects_nothing(self):
        result = self.invoke("src/tests/ipc/wire.rs")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("no oracle rows selected", result.stdout)

    def test_layout_change_selects_release_random_and_focused_rows(self):
        result = self.invoke("src/layout/mod.rs")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("target/release/swayward", result.stdout)
        self.assertIn("--seeds 0,50", result.stdout)
        self.assertIn("--scenario grouped_scratchpad_events", result.stdout)
        self.assertNotIn("target/debug", result.stdout)

    def test_events_only_scenario_is_not_run_as_state(self):
        result = self.invoke("src/layout/mod.rs")
        state = next(line for line in result.stdout.splitlines() if "--corpus state" in line)
        self.assertNotIn("grouped_scratchpad_events", state)

    def test_baseline_comparison_separates_new_known_and_fixed(self):
        module = load_module()
        OUT.parent.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=OUT.parent) as tmp:
            now, before = Path(tmp, "now.toml"), Path(tmp, "before.toml")
            row = '[[result]]\nscenario = "{}"\nrequest = "tree"\nverdict = "{}"\n'
            before.write_text(row.format("a", "match") + row.format("b", "mismatch")
                              + row.format("c", "mismatch"))
            now.write_text(row.format("a", "mismatch") + row.format("b", "mismatch")
                           + row.format("c", "match"))
            new, known, fixed = module.compare(module.verdicts(now), module.verdicts(before))
            seed = '[[result]]\nseed = {}\nverdict = "{}"\n'
            before.write_text(seed.format(7, "match") + seed.format(8, "match"))
            now.write_text(seed.format(7, "mismatch") + seed.format(8, "match"))
            seeds = module.compare(module.verdicts(now), module.verdicts(before))
        self.assertEqual(seeds, ([(7,)], [], []))
        self.assertEqual(new, [("a", "tree")])
        self.assertEqual(known, [("b", "tree")])
        self.assertEqual(fixed, [("c", "tree")])

    def test_unknown_production_path_fails_closed(self):
        result = self.invoke("src/new-production-module.rs")
        self.assertEqual(result.returncode, 2)
        self.assertIn("lack targeted oracle coverage", result.stderr)



ROW = '[[result]]\nseed = {}\nverdict = "{}"\n'
FAKE_RUNNER = """#!/usr/bin/env python3
import os, pathlib, sys
out = pathlib.Path(sys.argv[sys.argv.index("--out") + 1])
out.write_text(os.environ["FAKE_RESULT"])
"""


class FakeOracle:
    """An oracle git checkout whose sway-ipc-run writes $FAKE_RESULT."""

    def __init__(self, root: Path):
        self.root = root / "oracle"
        (self.root / "contrib").mkdir(parents=True)
        (self.root / "sway-ipc" / "results").mkdir(parents=True)
        runner = self.root / "contrib" / "sway-ipc-run"
        runner.write_text(FAKE_RUNNER)
        runner.chmod(0o755)
        self.pins = '[snapshot]\nswayward = "swayward-snap"\n'
        (self.root / "pins.toml").write_text(self.pins)
        # Seeds 7 and 8 both match in the pinned snapshot.
        (self.root / "sway-ipc" / "results" / "swayward-snap-random.toml").write_text(
            ROW.format(7, "match") + ROW.format(8, "match"))
        git = ["git", "-C", self.root, "-c", "user.name=t", "-c", "user.email=t@t"]
        subprocess.run([*git[:3], "init", "-q"], check=True)
        subprocess.run([*git, "add", "."], check=True)
        subprocess.run([*git, "commit", "-qm", "fixture"], check=True)
        self.sha = subprocess.check_output([*git[:3], "rev-parse", "HEAD"], text=True).strip()

    def sweep(self, root: Path, rows: str, oracle: str | None = None, dead: str = "") -> Path:
        sweep = root / "sweep"
        sweep.mkdir(exist_ok=True)
        # Two shards, as contrib/oracle-sweep writes them.
        (sweep / "random-0.toml").write_text(rows.split("\n[[")[0] + "\n")
        (sweep / "random-1.toml").write_text("[[" + rows.split("\n[[", 1)[1])
        (sweep / "status.json").write_text(json.dumps(
            {"swayward": "0" * 40, "oracle": oracle or self.sha, "wall_min": 1, "dead": dead}))
        return sweep


class RollingBaselineTest(unittest.TestCase):
    def setUp(self):
        OUT.parent.mkdir(parents=True, exist_ok=True)
        self.tmp = tempfile.TemporaryDirectory(dir=OUT.parent)
        self.root = Path(self.tmp.name)
        self.oracle = FakeOracle(self.root)

    def tearDown(self):
        self.tmp.cleanup()

    def run_targeted(self, *args: str, result: str):
        return subprocess.run(
            [SCRIPT, "--oracle", self.oracle.root, "--binary", self.root / "swayward",
             "--out-dir", self.root / "out", "--run", *args],
            text=True, capture_output=True, check=False,
            env=os.environ | {"FAKE_RESULT": result},
        )

    def test_row_matching_in_baseline_and_mismatching_now_exits_1_only_with_flag(self):
        # Seed 7 mismatches in the snapshot but matches in the rolling baseline;
        # now it mismatches. Only the rolling baseline sees the regression.
        (self.oracle.root / "sway-ipc" / "results" / "swayward-snap-random.toml").write_text(
            ROW.format(7, "mismatch") + ROW.format(8, "match"))
        subprocess.run(["git", "-C", self.oracle.root, "-c", "user.name=t", "-c", "user.email=t@t",
                        "commit", "-qam", "snapshot"], check=True)
        self.oracle.sha = subprocess.check_output(
            ["git", "-C", self.oracle.root, "rev-parse", "HEAD"], text=True).strip()
        sweep = self.oracle.sweep(self.root, ROW.format(7, "match") + ROW.format(8, "match"))
        now = ROW.format(7, "mismatch") + ROW.format(8, "match")

        without = self.run_targeted("--seeds", "7,8", result=now)
        self.assertEqual(without.returncode, 0, without.stdout + without.stderr)
        self.assertIn("known random 7", without.stdout)

        rolling = self.run_targeted("--seeds", "7,8", "--baseline", str(sweep), result=now)
        self.assertEqual(rolling.returncode, 1, rolling.stdout + rolling.stderr)
        self.assertIn("NEW random 7", rolling.stdout)
        self.assertIn(f"vs {sweep}", rolling.stdout)
        # The scratch pin is restored.
        self.assertEqual((self.oracle.root / "pins.toml").read_text(), self.oracle.pins)

    def test_baseline_mismatch_stays_known(self):
        sweep = self.oracle.sweep(self.root, ROW.format(7, "mismatch") + ROW.format(8, "match"))
        result = self.run_targeted("--seeds", "7,8", "--baseline", str(sweep),
                                   result=ROW.format(7, "mismatch") + ROW.format(8, "match"))
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("0 new, 1 known", result.stdout)

    def test_row_absent_from_baseline_is_refused_or_new(self):
        # The reviewer's probe: a finished sweep whose random shards lack seed 7.
        sweep = self.root / "sweep"
        sweep.mkdir()
        (sweep / "random-1.toml").write_text(ROW.format(8, "match"))
        (sweep / "status.json").write_text(json.dumps(
            {"swayward": "0" * 40, "oracle": self.oracle.sha, "wall_min": 1, "dead": ""}))
        now = ROW.format(7, "mismatch")

        refused = self.run_targeted("--seeds", "7", "--baseline", str(sweep), result=now)
        self.assertEqual(refused.returncode, 2, refused.stdout + refused.stderr)
        self.assertIn("absent from baseline", refused.stderr)
        self.assertIn("random 7", refused.stderr)
        self.assertNotIn("known random 7", refused.stdout)

        allowed = self.run_targeted("--seeds", "7", "--baseline", str(sweep), "--allow-new-rows",
                                    result=now)
        self.assertEqual(allowed.returncode, 1, allowed.stdout + allowed.stderr)
        self.assertIn("NEW random 7", allowed.stdout)
        self.assertNotIn("known random 7", allowed.stdout)

    def test_baseline_from_another_oracle_commit_is_refused(self):
        sweep = self.oracle.sweep(self.root, ROW.format(7, "match") + ROW.format(8, "match"),
                                  oracle="f" * 40)
        result = self.run_targeted("--seeds", "7", "--baseline", str(sweep), result=ROW.format(7, "match"))
        self.assertEqual(result.returncode, 2)
        self.assertIn("valid for one oracle commit", result.stderr)

    def test_baseline_with_dead_shard_is_refused(self):
        sweep = self.oracle.sweep(self.root, ROW.format(7, "match") + ROW.format(8, "match"),
                                  dead="random-1")
        result = self.run_targeted("--seeds", "7", "--baseline", str(sweep), result=ROW.format(7, "match"))
        self.assertEqual(result.returncode, 2)
        self.assertIn("dead shards", result.stderr)


class SelectorTest(unittest.TestCase):
    """--scenarios and --seeds run only the named rows (needs ./contrib/fetch-oracle)."""

    ORACLE = ROOT / ".cache" / "sway-ipc-oracle"

    def setUp(self):
        if not (self.ORACLE / "sway-ipc" / "scenarios.toml").exists():
            self.skipTest("oracle not fetched")

    def invoke(self, *args: str):
        return subprocess.run(
            [SCRIPT, "--oracle", self.ORACLE, "--out-dir", OUT, *args],
            text=True, capture_output=True, check=False,
        )

    def test_seeds_select_only_those_seeds(self):
        result = self.invoke("--seeds", "29,47", "--seeds", "101")
        self.assertEqual(result.returncode, 0, result.stderr)
        lines = result.stdout.splitlines()
        self.assertEqual(len(lines), 1, result.stdout)
        self.assertIn("random", lines[0])
        self.assertIn("--seeds 29,47,101", lines[0])

    def test_scenarios_route_to_their_corpora(self):
        derived = json.loads((self.ORACLE / "sway-ipc" / "i3-derived" / "scenarios.json").read_text())
        name = derived["scenarios"][0]["name"]
        result = self.invoke("--scenarios", f"fullscreen,one_window,grouped_scratchpad_events,{name}")
        self.assertEqual(result.returncode, 0, result.stderr)
        lines = result.stdout.splitlines()
        state = next(line for line in lines if "--corpus state" in line)
        events = next(line for line in lines if "--corpus events" in line)
        i3d = next(line for line in lines if " i3-derived " in line)
        self.assertEqual(len(lines), 3, result.stdout)
        self.assertIn("--scenario fullscreen --scenario one_window --out", state)
        self.assertIn("--scenario fullscreen --scenario grouped_scratchpad_events "
                      "--scenario one_window --out", events)
        self.assertIn(f"--scenario {name} --out", i3d)
        self.assertNotIn("--seeds", result.stdout)

    def test_unknown_scenario_fails(self):
        result = self.invoke("--scenarios", "no_such_scenario")
        self.assertEqual(result.returncode, 2)
        self.assertIn("unknown oracle scenario: no_such_scenario", result.stderr)

    def test_selectors_replace_paths(self):
        result = self.invoke("--seeds", "1", "src/layout/mod.rs")
        self.assertEqual(result.returncode, 2)
        self.assertIn("replace path selection", result.stderr)


if __name__ == "__main__":
    unittest.main()
