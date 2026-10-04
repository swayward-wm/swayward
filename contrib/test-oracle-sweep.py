#!/usr/bin/env python3
"""contrib/oracle-sweep against a fake oracle runner, through to --ledger."""
import json
import os
import subprocess
import tempfile
import time
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SWEEP = ROOT / "contrib" / "oracle-sweep"
DIFF = ROOT / "contrib" / "oracle-sweep-diff"
# Scratch output stays beside the repo, never in /tmp (AGENTS.md).
SCRATCH = ROOT.parent / "scratch"

# FAKE_MODE: ok writes a matching row and exits 0; mismatch writes a
# mismatching row and exits 1, as sway-ipc-run does; die writes nothing and
# exits 1 with no recognised message; crash writes a row but exits 3; hold
# waits for $FAKE_RELEASE to exist, then behaves as ok.
RUNNER = """#!/bin/sh
out=; while [ $# -gt 0 ]; do [ "$1" = --out ] && out=$2; shift; done
if [ "$FAKE_MODE" = hold ]; then
    while [ ! -e "$FAKE_RELEASE" ]; do sleep 0.05; done
    FAKE_MODE=ok
fi
case $FAKE_MODE in
ok) printf '[[result]]\\nscenario = "x"\\nrequest = "tree"\\nverdict = "match"\\n' >"$out" ;;
mismatch) printf '[[result]]\\nscenario = "x"\\nrequest = "tree"\\nverdict = "mismatch"\\n' >"$out"
    exit 1 ;;
die) echo "runner exited unsuccessfully"; exit 1 ;;
crash) printf '[[result]]\\nscenario = "x"\\nrequest = "tree"\\nverdict = "match"\\n' >"$out"
    exit 3 ;;
esac
"""


def git(*args, cwd=ROOT):
    return subprocess.run(["git", "-C", cwd, *args], text=True, capture_output=True,
                          check=True).stdout.strip()


class OracleSweepTest(unittest.TestCase):
    def setUp(self):
        SCRATCH.mkdir(exist_ok=True)
        self.tmp = tempfile.TemporaryDirectory(dir=SCRATCH)
        tmp = Path(self.tmp.name)
        self.oracle = tmp / "oracle"
        (self.oracle / "contrib").mkdir(parents=True)
        (self.oracle / "sway-ipc/random").mkdir(parents=True)
        (self.oracle / "sway-ipc/i3-derived").mkdir(parents=True)
        (self.oracle / "pins.toml").write_text(f'swayward = "{"0" * 40}"\n')
        runner = self.oracle / "contrib/sway-ipc-run"
        runner.write_text(RUNNER)
        runner.chmod(0o755)
        (self.oracle / "sway-ipc/random/sequences.json").write_text(
            json.dumps({"sequences": [{"seed": 1}, {"seed": 2}]}))
        (self.oracle / "sway-ipc/i3-derived/scenarios.json").write_text(
            json.dumps({"scenarios": [{"name": "a"}, {"name": "b"}]}))
        git("init", "-q", cwd=self.oracle)
        git("add", ".", cwd=self.oracle)
        git("-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "oracle",
            cwd=self.oracle)
        # A clone of this repo with nothing checked out: HEAD resolves in ROOT,
        # so --ledger accepts its sha, and the status stays clean.
        self.checkout = tmp / "swayward"
        subprocess.run(["git", "clone", "-q", "--shared", "--no-checkout", ROOT,
                        self.checkout], check=True)
        git("sparse-checkout", "set", "--no-cone", "/nothing", cwd=self.checkout)
        git("checkout", "-q", "--detach", git("rev-parse", "HEAD"), cwd=self.checkout)
        (self.checkout / ".git/info/exclude").write_text("/target\n")
        binary = self.checkout / "target/release/swayward"
        binary.parent.mkdir(parents=True)
        binary.write_text("")
        binary.chmod(0o755)
        self.out = tmp / "sweep"
        self.ledger = tmp / "progress.tsv"
        # A private lock, so the tests neither block on nor disturb a real sweep.
        self.lock = tmp / "oracle-sweep.lock"
        self.env = {**os.environ, "ORACLE_SWEEP_LOCK": str(self.lock),
                    "FAKE_RELEASE": str(tmp / "release")}

    def tearDown(self):
        self.tmp.cleanup()

    def sweep(self, mode, *flags, out=None):
        return subprocess.run(
            [SWEEP, *flags, self.oracle, out or self.out, self.checkout], text=True,
            capture_output=True, check=False, env={**self.env, "FAKE_MODE": mode})

    def ledger_lines(self):
        subprocess.run([DIFF, "--ledger", self.ledger, self.out, self.out],
                       capture_output=True, check=False)
        return self.ledger.read_text().splitlines() if self.ledger.exists() else []

    def dead(self):
        return sorted(json.loads((self.out / "status.json").read_text())["dead"].split())

    def test_fresh_dir_sweeps_and_appends_a_row(self):
        result = self.sweep("ok")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(self.dead(), [])
        self.assertEqual(len(self.ledger_lines()), 2)
        self.assertEqual(git("status", "--porcelain", cwd=self.oracle), "")

    def test_mismatches_are_not_a_dead_shard(self):
        result = self.sweep("mismatch")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(self.dead(), [])

    def test_reused_dir_is_refused(self):
        self.assertEqual(self.sweep("ok").returncode, 0)
        self.assertEqual(len(self.ledger_lines()), 2)
        before = (self.out / "status.json").read_text()
        result = self.sweep("die")
        self.assertEqual(result.returncode, 2)
        self.assertIn("holds a previous sweep", result.stdout)
        self.assertEqual((self.out / "status.json").read_text(), before)

    def test_reuse_clears_the_old_results(self):
        # The reviewer's repro: a good sweep, then every shard dies in the
        # same dir without a recognised log message.
        self.assertEqual(self.sweep("ok").returncode, 0)
        self.assertEqual(len(self.ledger_lines()), 2)
        result = self.sweep("die", "--reuse")
        self.assertEqual(result.returncode, 1)
        self.assertEqual(len(self.dead()), 6)
        self.assertEqual(list(self.out.glob("*.toml")), [])
        self.assertEqual(len(self.ledger_lines()), 2)

    def test_a_crashing_shard_is_dead_despite_its_output(self):
        result = self.sweep("crash")
        self.assertEqual(result.returncode, 1)
        self.assertIn("retrying dead shards", result.stdout)
        self.assertEqual(len(self.dead()), 6)
        self.assertEqual(self.ledger_lines(), [])

    def test_a_second_concurrent_sweep_exits_2_naming_the_holder(self):
        first = subprocess.Popen(
            [SWEEP, self.oracle, self.out, self.checkout], text=True,
            stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
            env={**self.env, "FAKE_MODE": "hold"})
        try:
            deadline = time.monotonic() + 30
            while f"pid={first.pid}" not in (self.lock.read_text()
                                              if self.lock.exists() else ""):
                self.assertLess(time.monotonic(), deadline, "first sweep never locked")
                time.sleep(0.05)
            second = self.sweep("ok", out=self.out.parent / "sweep2")
            self.assertEqual(second.returncode, 2, second.stdout + second.stderr)
            self.assertIn(f"pid={first.pid}", second.stdout)
            self.assertIn(f"swayward={git('rev-parse', 'HEAD')}", second.stdout)
            self.assertIn("started=", second.stdout)
            self.assertFalse((self.out.parent / "sweep2").exists())
        finally:
            Path(self.env["FAKE_RELEASE"]).touch()
            stdout, _ = first.communicate(timeout=60)
        self.assertEqual(first.returncode, 0, stdout)
        self.assertEqual(self.lock.read_text(), "")
        self.assertEqual(self.sweep("ok", out=self.out.parent / "sweep3").returncode, 0)

    def test_a_dead_holders_lock_is_reclaimed(self):
        self.lock.write_text("pid=999999999\nswayward=abc\nstarted=then\n")
        result = self.sweep("ok")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("reclaiming stale lock left by pid 999999999", result.stdout)


if __name__ == "__main__":
    unittest.main()
