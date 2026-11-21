# Release and oracle launch checklist

The release lifecycle in [`docs/UPSTREAM.md`](../UPSTREAM.md#release-lifecycle) defines five release kinds: beta, rc, final `.0`, patch, and feature release. [Release kinds](#release-kinds) says what each kind needs. The numbered steps after it are the full T4 path, written for beta1; for another release, substitute that release's version, human name and notes file, and skip the launch-only steps 11 to 13.

## Release kinds

### Version strings

`contrib/package-version` derives the Debian, RPM and Arch forms from the Cargo version, and `contrib/check-release-consistency` checks them. The release workflow takes the Cargo version without the `swayward-v` prefix.

| Kind | Cargo, release workflow | Tag | `SWAYWARD_VERSION` | Debian, RPM | Arch `pkgver` (`_tagver` = Cargo) |
|---|---|---|---|---|---|
| beta | `YY.MM.0-beta.N` | `swayward-vYY.MM.0-beta.N` | `betaN` | `YY.MM.0~beta.N` | `YY.MM.0betaN` |
| rc | `YY.MM.0-rc.N` | `swayward-vYY.MM.0-rc.N` | `rcN` | `YY.MM.0~rc.N` | `YY.MM.0rcN` |
| final | `YY.MM.0` | `swayward-vYY.MM.0` | `YY.MM.0` | `YY.MM.0` | `YY.MM.0` |
| patch, feature | `YY.MM.N` | `swayward-vYY.MM.N` | `YY.MM.N` | `YY.MM.N` | `YY.MM.N` |
| patch or feature rc | `YY.MM.N-rc.1` | `swayward-vYY.MM.N-rc.1` | `rc1` | `YY.MM.N~rc.1` | `YY.MM.Nrc1` |

`SWAYWARD_VERSION` is defined in `swayward-ipc/src/version.rs` and re-exported by `src/utils/mod.rs`, which also defines `NIRI_BASE`. `swayward -V` and `GET_VERSION.human_readable` print `swayward <SWAYWARD_VERSION> (<git describe>)`; `swayward --version` adds `niri base: <NIRI_BASE>`, which must match `docs/FORK-BASE.md`; the `GET_VERSION` numeric fields stay `1.0.0`. Land the bump as in step 3.

### Every release

- Release notes name the exact niri base (for example "based on niri 26.12.1") and the tag the published compatibility counts were measured on. A feature release's notes also say it carries features. Write them as `docs/RELEASE-NOTES-<version>.md` for every release kind, in the step 8 metadata commit (for a final, in the version-bump commit), and copy them into the GitHub release body (step 10).
- `docs/FORK-BASE.md` and `NIRI_BASE` at the tag name the exact niri base, including a niri point release whose fixes the line has backported.
- The tag is signed and never moved. The Prepare release workflow runs on it (step 10).

### Where commits land

| Change | `main` | `release/YY.MM` |
|---|---|---|
| development, sway parity | lands | only through a feature release (`git cherry-pick -x`) |
| version bump | for a beta or rc1 | for rc2 and later, a final, a patch or a feature release |
| niri tag `vYY.MM` | absorbed by merging-rebase, after the current line's rc1; starts line `YY.MM` | never rebased onto niri |
| niri point release, e.g. `v26.12.1` | absorbed by merging-rebase if newer than its base | relevant fixes cherry-picked for a patch release |
| beta, rc1 | tagged here | fast-forwarded to the tag |
| blocker fix after rc1, patch fix | cherry-picked, or forward-ported from the branch | lands |
| rc2 and later, `.0`, patch, feature release | | tagged here |

Feature releases must use `git cherry-pick -x`, so each copy names its origin on `main`. For every other cherry-pick (blocker fixes, patch fixes, forward-ports, niri backports) `-x` is recommended.

### Release-branch T4 overrides

rc2 and later, a risky patch rc and a feature-release rc run the full T4 path on `release/YY.MM`. Fixes, the version bump and the release metadata all land on `release/YY.MM`, never on `main`. Read the steps with these overrides:

- **Step 3:** commit the bump on `release/YY.MM` and push it there as the last source commit. `main` keeps its own version.
- **Step 4.2:** confirm that no blocker fix for the line remains open and that the step 3 bump is the last source commit on `origin/release/YY.MM`.
- **Step 4.3:** skip it. A release branch is never rewritten.
- **Step 4.4:** record `export SWAYWARD_SHA=$(git -C "$SWAYWARD" rev-parse origin/release/YY.MM)`.
- **Step 4.5:** check ancestry with `git -C "$SWAYWARD" merge-base --is-ancestor "$SWAYWARD_SHA" origin/release/YY.MM`.
- **Step 4 gate:** from here until the tag, `release/YY.MM` (not `main`) changes only in `tests/oracle.toml`, generated docs and release metadata.
- **Steps 7 and 8:** the repin and metadata commits land on `release/YY.MM`. The repin does not move `main`'s `tests/oracle.toml`, which repins at its own line's T4.
- **Step 10:** check `test "$(git rev-parse HEAD)" = "$(git rev-parse origin/release/YY.MM)"` instead of `origin/main`; the `$SWAYWARD_SHA` source checks apply unchanged. The branch push in step 10 is a no-op, because the tag is already on the branch.

### Publication without T4

A final `YY.MM.0`, a final `YY.MM.N` after its rc, and a patch release whose fixes change no measured behaviour skip steps 4 to 7: nothing is frozen, regenerated or repinned. Steps 1 and 11 to 13 do not apply, and of step 2 only its `VERSION` export does. Instead:

1. Export the release version as step 2 does, so steps 3, 9 and 10 get it: `export VERSION=<Cargo version>`, for example `export VERSION=26.10.0`.
2. Commit the version bump (step 3, with the release-branch override above) and `docs/RELEASE-NOTES-<version>.md` on `release/YY.MM`.
3. Run T0 (`./contrib/fast-gate`) and the step 9 command gate on HEAD.
4. Run the kind's own check:
   - **Final after an rc:** the only change since the last rc is the bump and the notes file:

     ```sh
     export LAST_RC=swayward-vYY.MM.N-rc.K
     git diff --name-only "$LAST_RC" HEAD
     ```

     Expect only the step 3 files (`Cargo.toml`, `Cargo.lock`, workspace manifest pins, `swayward-ipc/src/version.rs`, `contrib/PKGBUILD`, `contrib/.SRCINFO`, public examples) and `docs/RELEASE-NOTES-<version>.md`.

     The notes publish the last rc's counts and name `$LAST_RC` as the measurement.
   - **Patch without a T4:** every fix since the previous tag has its T0 and T1 evidence, and T1 showed no oracle row change against a baseline sweep of the line's last measured tag. The notes name that tag as the measurement and list the fixes.
5. Step 10 with these changes: confirm `HEAD` equals `origin/release/YY.MM`; replace the `$SWAYWARD_SHA` source checks with step 4's check; tag, run Prepare release, and publish after maintainer review.

The oracle pin and `tests/oracle.toml` do not move.

### Beta, `YY.MM.0-beta.N`

Optional: for 26.10 and for a line with a large change.

- **Cut from:** `main`.
- **Gates:** T0 on every commit, T1 on behaviour changes, T2 sweeps by the integrator, then a full T4: steps 1 to 10.
- **Oracle:** freeze, pin and regenerate (steps 4 to 7); publish the counts in the notes.
- **Branch:** step 10 fast-forwards `release/YY.MM` to the tag. Create and protect the branch at the line's first tag.

### rc1, `YY.MM.0-rc.1`

- **Entry criteria**, all required before step 4:
  - every open issue and backlog row is triaged as a blocker for the line or for later;
  - zero open blockers and no open stability regression;
  - the nightly live IPC soak (`live-soak.yml`) green on 3 consecutive nights;
  - on a line with betas, 2 weeks since the last beta with no new blocker (a line without betas soaks on rc1 instead; see the final);
  - user docs current: `docs/KNOWN_DEVIATIONS.md`, `docs/SWAY_CONFIG_MIGRATION.md`, `docs/wiki/`;
  - the stable packaging channels are decided.
- **Proptests:** after step 4 records `$SWAYWARD_SHA`, the 200,000-case proptest CI job (`ci.yml`, `randomized and slow tests`) is green on `$SWAYWARD_SHA`. If step 4 is repeated, check the new commit again; step 9 confirms it before the tag.
- **Cut from:** `main`, by a full T4 (steps 1 to 10). The T4 is clean: `pending.toml` empty, two runs agree, and the notes carry the public counts.
- **Branch:** step 10 fast-forwards `release/YY.MM` to rc1. From here the branch stops following `main`.
- **niri:** rc1 comes before the next niri merging-rebase lands on `main`. If niri tags earlier, prepare the rebase but do not push it until rc1 is tagged.

### rc2 and later, `YY.MM.0-rc.N`

- **Accepts:** blocker fixes only: crashes, data loss, security, regressions since the previous rc, packaging breakage. No behaviour or sway-parity change. Each fix lands on `release/YY.MM` with T0 and T1 and is forward-ported to `main`.
- **Gates:** a full T4 with the [release-branch overrides](#release-branch-t4-overrides).

### Final, `YY.MM.0`

- **Cut from:** `release/YY.MM`: the last rc's tree plus only the version-bump commit, which may also carry `docs/RELEASE-NOTES-YY.MM.0.md`.
- **Soak:** a line without betas has had rc1 out for 2 weeks with no new blocker.
- **Gates:** [publication without T4](#publication-without-t4), final check. The final reuses the last rc's measurement.
- **Oracle:** no regeneration, no `pins.toml` move and no `tests/oracle.toml` change; the pin stays at the last rc's commit, which `.0` descends from.

### Patch release, `YY.MM.N` (N > 0)

- **Cut from:** `release/YY.MM` only. Fixes and the version bump land on `release/YY.MM`. It normally carries regression, crash and security fixes (cherry-picked from `main`, or fixed on the branch first and forward-ported) and niri point-release backports. The previous line takes security fixes only.
- **Gates:** T0 and T1 for every fix, on `release/YY.MM`. Pass `contrib/targeted-oracle` a baseline sweep of the line's last measured tag.
- **Oracle:** if T1 shows a fix changes oracle-measured behaviour, run a full T4 with the [release-branch overrides](#release-branch-t4-overrides), then pin and publish. Otherwise follow [publication without T4](#publication-without-t4), patch check.
- **Risky patch** (for example a large niri backport): cut `YY.MM.N-rc.1` first. Like every rc it is a full T4 with the release-branch overrides. Then cut `YY.MM.N` as a final after its rc.

### Feature release, `YY.MM.N` (N > 0)

- **Cut from:** `release/YY.MM`, with the reviewed features taken from `main` by `git cherry-pick -x`.
- **Gates:** always through `YY.MM.N-rc.1`, a full T4 with the [release-branch overrides](#release-branch-t4-overrides), then `YY.MM.N` as a final after its rc: the last rc's tree plus the version bump, reusing its measurement.
- **Notes:** say the release carries features.

## Full T4 path

Written for beta1. Use this checklist from top to bottom. Stop when a gate fails. Do not tag or publish either repository until the checklist reaches the step that names that action.

The launch-critical task chain is `oracle-swayward-snapshot-repin`, `oracle-readme-cross-compositor-table`, `docs-manual-review`, `oracle-final-squash`, `extract-compliance-harness`, `oracle-courtesy-notes`, `launch-six-rules-gate`, `cut-beta1-release`, and `launch-announce-and-beta`. Complete each task before the step that names it.

Paths below assume these checkouts:

```sh
export SWAYWARD=$HOME/hacking/swayward-wm/swayward
export ORACLE=$HOME/hacking/swayward-wm/oracle
export SCRATCH=$HOME/hacking/swayward-wm/scratch/maintainer
mkdir -p "$SCRATCH"
```

Use a clean oracle worktree beside `$ORACLE` for measurements. Do not edit `$ORACLE` itself. Keep build output and logs under `$SCRATCH`, not `/tmp`.

## 1. Confirm the extracted oracle requirements

**Owner: oracle maintainer**

Before the final measurement, confirm that the standalone oracle has the i3, sway, and swayward black-box adapters, all six result families, pinned source revisions, and clean-checkout reproduction instructions. `extract-compliance-harness` remains open until `oracle-final-squash` closes.

Gate: every `extract-compliance-harness` blocker except `oracle-final-squash` is closed.

## 2. Resolve the beta version contract

**Owner: maintainer**

Complete `launch-beta-version-contract` before editing release metadata. The maintainer chose Option A on 2026-09-29: the human name is `beta1` and the ordered version is `26.10.0-beta.1`.

The exact values:

- signed Git tag: `swayward-v26.10.0-beta.1`;
- `swayward -V` and `GET_VERSION.human_readable`: `swayward beta1 (<git describe>)`; `swayward --version` adds `niri base: <NIRI_BASE>`; the `GET_VERSION` numeric fields stay `1.0.0`;
- Cargo workspace version: `26.10.0-beta.1`;
- Debian and RPM: `26.10.0~beta.1`; Arch `pkgver`: `26.10.0beta1` with `_tagver=26.10.0-beta.1`;
- release workflow input: `26.10.0-beta.1`;
- `swayward-ipc` is not published for beta1 (`publish = false`).

Gate: `contrib/check-release-consistency`, its test, the release runbook, and version tests all encode the same decision. Export the decided version as `VERSION`.

## 3. Land the version bump

**Owner: release worker, reviewed by maintainer**

The bump changes `src`, `swayward-ipc`, and `swayward-config`, so it lands before the freeze in step 4 and the measured commit reports the beta version.

Follow `docs/wiki/Development:-Releasing-swayward.md`. Update the version source, `Cargo.toml`, `Cargo.lock`, workspace dependency pins, `contrib/PKGBUILD`, and public examples. In an Arch container, regenerate rather than hand-edit `.SRCINFO`:

```sh
cd "$SWAYWARD/contrib"
makepkg --printsrcinfo > .SRCINFO
```

Run:

```sh
cd "$SWAYWARD"
./contrib/check-release-consistency "swayward-v$VERSION"
./contrib/test-release-consistency
```

Gate: version metadata agrees, `.SRCINFO` is generated, and `./contrib/fast-gate` passes. Commit the bump and land it on `main` as the last source commit.

## 4. Freeze the swayward source commit

**Owner: maintainer and integrator**

Never pin an intermediate commit. The commit recorded here is the only swayward commit the oracle pins for beta1, so it must survive every later history change.

1. Drain the integration queue.
2. Confirm that no launch-blocking fix remains open and that the step 3 version bump is the last source commit on `main`.
3. Squash before freeze, never after. If the maintainer wants a shorter history, squash the commits on swayward `main` above the latest release tag now (never a commit at or below it, and only while nothing else is in flight), back up the pre-squash history as a bundle under `$SCRATCH`, and push with `--force-with-lease`. A squash after this step orphans the pinned commit and invalidates every result that names it.
4. Record the commit that the oracle will measure:

   ```sh
   git -C "$SWAYWARD" fetch origin
   git -C "$SWAYWARD" status --short
   export SWAYWARD_SHA=$(git -C "$SWAYWARD" rev-parse origin/main)
   printf '%s\n' "$SWAYWARD_SHA" | tee "$SCRATCH/swayward-snapshot-sha"
   ```

5. Confirm that the commit is on `main` and reachable from a fresh clone:

   ```sh
   git clone --quiet https://github.com/swayward-wm/swayward "$SCRATCH/fresh-clone"
   git -C "$SCRATCH/fresh-clone" cat-file -e "$SWAYWARD_SHA^{commit}"
   git -C "$SWAYWARD" merge-base --is-ancestor "$SWAYWARD_SHA" origin/main
   rm -rf "$SCRATCH/fresh-clone"
   ```

Gate: the squash, if any, is pushed; the working tree is clean; the queue is empty; and the integrator confirms `$SWAYWARD_SHA` as the snapshot commit. From here until the tag, `main` changes only in `tests/oracle.toml`, generated docs, and release metadata that does not touch `src`, `swayward-ipc`, or `swayward-config`. Check with `git diff --quiet "$SWAYWARD_SHA" HEAD -- src swayward-ipc swayward-config`. If another behavior change lands, return to this step and discard any unpublished measurement of the old commit.

## 5. Repin and regenerate the oracle

**Owner: oracle snapshot worker**

Create a worktree from current oracle `origin/main`:

```sh
git -C "$ORACLE" fetch origin
git -C "$ORACLE" worktree add \
  $HOME/hacking/swayward-wm/oracle.worktrees/launch-repin \
  -b launch-repin origin/main
export ORACLE_WORKTREE=$HOME/hacking/swayward-wm/oracle.worktrees/launch-repin
```

In `pins.toml`, set `swayward` to `$SWAYWARD_SHA` and rename the snapshot to `swayward-${SWAYWARD_SHA:0:8}`. Rename all old swayward result and classification files to that snapshot name. Build the pinned image from the committed pin:

```sh
cd "$ORACLE_WORKTREE"
git add pins.toml i3 sway-ipc
git commit -m 'pins: select the beta1 swayward snapshot'
podman build -t sway-ipc-oracle-launch -f Containerfile .
podman image inspect sway-ipc-oracle-launch --format '{{.Id}}' \
  | tee "$SCRATCH/oracle-image-id"
```

Run one oracle job at a time on this shared machine. Cap every container at 4 GiB with no swap. Follow `docs/reproducing.md`. Generate all swayward outputs:

- the complete i3 suite, with two consecutive agreeing runs;
- state scenarios;
- event scenarios;
- the i3-derived corpus;
- all 500 random seeds;
- command fuzz at the committed CI budget;
- wire fuzz at the committed CI budget.

Record the oracle commit, image ID, wall time, and non-metadata diff for every unit. Where the corpus requires repeated stability runs, run it three times. Do not edit generated result files.

Review `i3/classifications/swayward-<sha8>.toml`. Remove a finding or boundary reason from every assertion that now passes. Recheck the evidence and citation for every remaining non-pass.

Update the oracle README from the generated files. Present each compositor snapshot independently. Do not restore a cross-compositor scoreboard. Always publish pass, skip, and fail together.

Gate:

```sh
cd "$ORACLE_WORKTREE"
./contrib/validate
git diff --check
git status --short
```

Commit the complete repin. Push only its oracle branch. Do not force-push that branch after results exist because result metadata records `oracle_commit`.

## 6. Finish and squash the oracle

**Owner: maintainer**

Complete `docs-manual-review` and `oracle-readme-cross-compositor-table` first. Confirm that `oracle-final-squash` has no other open blocker.

Back up the pre-squash history outside the repository:

```sh
cd "$ORACLE_WORKTREE"
git fetch origin
git bundle create "$SCRATCH/sway-ipc-oracle-prelaunch.bundle" --all
git bundle verify "$SCRATCH/sway-ipc-oracle-prelaunch.bundle"
```

Squash only the oracle commits above `ORACLE_BASE`, the newest oracle commit pinned by any swayward release tag, so every tag's pinned oracle sha survives. It is not the pin of the most recent tag by date: with two supported lines, a later 26.10 patch can pin an older oracle commit than 26.12 does. Read `tests/oracle.toml` at every `swayward-v*` tag, keep the pins that are ancestors of oracle `origin/main`, and pick the one that descends from all the others; stop if the kept pins are not on one line of history. Dry-run the squash in a disposable branch before changing `main`:

```sh
git -C "$SWAYWARD" fetch --tags origin
PINS=$(for tag in $(git -C "$SWAYWARD" tag --list 'swayward-v*'); do
  git -C "$SWAYWARD" show "${tag}:tests/oracle.toml" | sed -n 's/^commit = "\(.*\)"$/\1/p'
done | sort -u | while read -r pin; do
  git merge-base --is-ancestor "$pin" origin/main 2>/dev/null && echo "$pin"
done)
export ORACLE_BASE=$(printf '%s\n' "$PINS" | {
  base=
  while read -r pin; do
    if [ -z "$base" ] || git merge-base --is-ancestor "$base" "$pin"; then base=$pin; fi
  done
  echo "$base"
})
test -n "$ORACLE_BASE"
# Every kept pin must be an ancestor of ORACLE_BASE; any STOP line means stop.
printf '%s\n' "$PINS" | while read -r pin; do
  git merge-base --is-ancestor "$pin" "$ORACLE_BASE" || echo "STOP: $pin is not below $ORACLE_BASE"
done
git merge-base --is-ancestor "$ORACLE_BASE" HEAD
export ORACLE_TREE=$(git rev-parse HEAD^{tree})
export ORACLE_SQUASH=$(printf '%s\n\n%s\n%s\n' \
  'sway-ipc-oracle: publish the conformance harness' \
  "Uses i3's unchanged BSD-3-Clause test suite with full credit." \
  'Originated in swayward and remains useful independently.' \
  | git commit-tree "$ORACLE_TREE" -p "$ORACLE_BASE")
git branch launch-squash-dry-run "$ORACLE_SQUASH"
test "$(git rev-parse launch-squash-dry-run^{tree})" = "$ORACLE_TREE"
git switch launch-squash-dry-run
./contrib/validate
git switch launch-repin
git branch -D launch-squash-dry-run
```

Gate: the bundle verifies, the tree hashes match, and validation passes on the disposable squash commit.

When the maintainer approves the exact commit message, recreate the squash commit, move oracle `main` to it, and push with `--force-with-lease`. Oracle `main` is otherwise protected and never rewritten: lift its protection for this push only (`gh api -X DELETE repos/swayward-wm/sway-ipc-oracle/branches/main/protection`) and restore it right after the push. This is an irreversible publication step. Do not tag the oracle and do not create an oracle GitHub release.

After the push, restore oracle `main` protection first, even if a later check fails:

```sh
printf '%s' '{"required_status_checks":null,"enforce_admins":true,"required_pull_request_reviews":null,"restrictions":null,"allow_force_pushes":false,"allow_deletions":false}' \
  | gh api -X PUT "repos/swayward-wm/sway-ipc-oracle/branches/main/protection" --input -
gh api "repos/swayward-wm/sway-ipc-oracle/branches/main/protection" --jq '.allow_force_pushes.enabled'  # false
git fetch origin
test "$(git rev-list --count "$ORACLE_BASE..origin/main")" -eq 1
./contrib/validate
```

Record the new oracle squash SHA as `ORACLE_SHA`. Close `oracle-final-squash`, then close `extract-compliance-harness` with the final SHA and reproducibility evidence.

## 7. Pin swayward to the final oracle commit

**Owner: swayward integrator**

Start from current swayward `main`. Update `tests/oracle.toml` to `$ORACLE_SHA`. Update every public link that contains the old oracle SHA or old swayward snapshot name.

Regenerate the published i3 summary from the final oracle checkout:

```sh
cd "$SWAYWARD"
./contrib/fetch-oracle
./contrib/i3-suite-summary --write
./contrib/i3-suite-summary --check
./contrib/coverage-report --check
./contrib/check-internal-links
./contrib/check-divergence
git diff --check
```

Gate: every generated block names `$ORACLE_SHA`, all public links use the final snapshot name, and coverage reports zero violations. Commit this as one swayward commit. Do not push from a worker worktree; the integrator cherry-picks it.

## 8. Prepare the release metadata commit

**Owner: release worker, reviewed by maintainer**

The version bump already landed in step 3. This commit only adds release metadata that depends on the final oracle pin from step 7. If any edit below would change `src`, `swayward-ipc`, or `swayward-config`, return to step 4.

Write `docs/RELEASE-NOTES-beta1.md` so it names `$ORACLE_SHA`, the final swayward snapshot name, and the current user-visible changes. Then confirm that the bump from step 3 still holds:

```sh
cd "$SWAYWARD"
./contrib/check-release-consistency "swayward-v$VERSION"
./contrib/test-release-consistency
git diff --quiet "$SWAYWARD_SHA" HEAD -- src swayward-ipc swayward-config
```

Gate: release metadata agrees and the release notes name the final oracle pin. Commit the release metadata. Do not amend it after the final audit.

## 9. Run the final launch gate

**Owner: integrator; maintainer reviews the evidence**

Run the full project gate in `swayward-dev`, with no concurrent Cargo command in this worktree:

```sh
distrobox enter swayward-dev -- bash -lc "
  cd '$SWAYWARD' &&
  CARGO_BUILD_JOBS=4 cargo +nightly fmt --all -- --check &&
  CARGO_BUILD_JOBS=4 cargo clippy --all --all-targets &&
  CARGO_BUILD_JOBS=4 cargo test --all &&
  ./contrib/coverage-report --check &&
  ./contrib/check-divergence &&
  ./contrib/check-internal-links &&
  ./contrib/test-release-consistency &&
  ./contrib/check-release-consistency 'swayward-v$VERSION'
"
```

For rc1, also confirm that the 200,000-case proptest CI job (`ci.yml`, `randomized and slow tests`) is green on `$SWAYWARD_SHA`:

```sh
gh run list --workflow ci.yml --commit "$SWAYWARD_SHA" --json conclusion,status
```

For changes under `src/layout/`, also run both slow gates with `CARGO_BUILD_JOBS=4`, `RUN_SLOW_TESTS=1`, and `PROPTEST_CASES=20000`, as specified in `AGENTS.md`. For changes to the translator or i3 harness paths listed there, also run the i3 ratchet: `RUN_SLOW_TESTS=1 cargo test -p swayward --lib i3_conformance_non_green_passes_do_not_drop`.

Run `./contrib/sync-github-wiki --check-published` with network access. Search `docs/wiki` for `Since: next release`; only the releasing guide may contain the check's literal text.

Apply the six oracle launch rules and record evidence on `launch-six-rules-gate`:

1. The README states the oracle's swayward origin.
2. Results are not presented as a scoreboard.
3. Every public count includes pass, skip, and fail or match, mismatch, unstable, and not applicable.
4. A clean checkout reproduces the pinned i3 and sway evidence.
5. The i3 and sway courtesy notes are ready for the maintainer.
6. The oracle documents a useful sway-only workflow.

Gate: every command passes at the exact release commit, all six rules hold, and the working tree is clean.

## 10. Create the beta tag and draft release

**Owner: maintainer**

Tag HEAD after the step 7 repin and the step 8 release metadata, not `$SWAYWARD_SHA`. HEAD differs from the measured commit only in `tests/oracle.toml`, generated docs, and release metadata, so the tagged tree has the final pin and the same source the oracle measured. Confirm that the audited commit is still `origin/main`:

```sh
cd "$SWAYWARD"
git fetch origin
test "$(git rev-parse HEAD)" = "$(git rev-parse origin/main)"
git status --short                    # must print nothing
git diff --quiet "$SWAYWARD_SHA" HEAD -- src swayward-ipc swayward-config
git diff --stat "$SWAYWARD_SHA" HEAD  # pin, generated docs, release metadata only
./contrib/check-release-consistency "swayward-v$VERSION"
```

Create and push the signed tag:

```sh
git tag -s "swayward-v$VERSION" -m "swayward $VERSION"
git push origin "swayward-v$VERSION"
export RELEASE_BRANCH=release/$(printf '%s' "$VERSION" | cut -d. -f1,2)
git push origin "swayward-v$VERSION^{commit}:refs/heads/$RELEASE_BRANCH"
```

A release tag is never moved. Every swayward sha the oracle pins is reachable
from a release tag (T4 pins the frozen RC sha and tags on top of it). Swayward
`main` may be rewritten only above the latest release tag on it, with
`--force-with-lease` when nothing else is in flight; never rewrite a commit at
or below that tag. `$RELEASE_BRANCH` keeps its own commits and is never
rewritten; `main` may still rewrite its own commits above its latest release
tag even if `$RELEASE_BRANCH` shares them. Turn on branch protection for oracle
`main` and for `$RELEASE_BRANCH` so that neither accepts a force-push or
deletion, and confirm it:

```sh
protect() {
  printf '%s' '{"required_status_checks":null,"enforce_admins":true,"required_pull_request_reviews":null,"restrictions":null,"allow_force_pushes":false,"allow_deletions":false}' \
    | gh api -X PUT "repos/$1/branches/$2/protection" --input -
  gh api "repos/$1/branches/$2/protection" --jq '.allow_force_pushes.enabled'  # false
}
protect swayward-wm/sway-ipc-oracle main
protect swayward-wm/swayward "$RELEASE_BRANCH"
```

From here on, the oracle pins only tagged or `main` commits, and oracle `main` is rewritten only by the next T4 squash, above the oracle commit this tag pins.

Dispatch **Prepare release** on that tag and use the version without the `swayward-v` prefix:

```sh
gh workflow run release.yml \
  --ref "swayward-v$VERSION" \
  -f version="$VERSION"
gh run list --workflow release.yml --limit 1
```

Gate: the validation, Ubuntu, Debian, Fedora, Arch, and clean-install jobs pass, and each clean-install job's `swayward --version` and `swaywardmsg --version` name the tag rather than `unknown commit` (the RPM spec, the PKGBUILD and `release.yml` supply the commit; see `docs/UPSTREAM.md`, Versioning). Download the artifacts, verify checksums, review generated notes, and add the bounded beta limitations from `docs/RELEASE-NOTES-beta1.md`. Publish the draft only after maintainer review.

The release body is the user-facing notes: drop the `DRAFT` heading and the
maintainer-review paragraph from `docs/RELEASE-NOTES-beta1.md` when copying
them, and keep its References section out of the body or behind a link.

Right after publishing, flip the pages that describe beta1 as unpublished, in
one docs-only commit on `main`:

- `README.md`, "Install": "There is no public build yet" and "The first beta
  will put these … packages".
- `docs/wiki/Getting-Started.md`, "Install": the same two sentences and "Once
  the beta is published".
- `docs/wiki/FAQ.md`, "Which distributions have packages?": "No public beta
  release exists yet" and "Until beta 1 is published".
- `docs/RELEASE-NOTES-beta1.md`: remove the `DRAFT` heading.

Each should say beta1 is on the releases page. Then run
`./contrib/sync-github-wiki --check` and `./contrib/check-internal-links`.

## 11. Send courtesy notes and wait

**Owner: maintainer**

After the final oracle commit is public, send the reviewed notes from `oracle-courtesy-notes` to the i3 and sway maintainers. The notes give notice rather than ask permission. Record send dates and factual corrections without publishing private replies.

Gate: wait at least one week before the public announcement. Fix and credit any confirmed citation, fixture, or harness error before continuing.

## 12. Announce the oracle and invite beta reports

**Owner: maintainer**

Complete `launch-announce-and-beta`. Announce `sway-ipc-oracle` first. State that it came from swayward and that swayward is its first adopter. Link the beta release as the compositor the oracle was built to test.

Use `docs/TONE.md` for every published sentence. Keep claims dry, publish all outcome categories together, and ask for reports rather than stars. Do not compare project quality or present conformance results as a ranking.

Gate: the maintainer approves the final post, the courtesy interval has elapsed, the release artifacts remain available, and the issue-reporting path is clear.

## 13. Defer COPR and AUR

COPR and AUR are not beta1 blockers. GitHub release artifacts are the beta distribution channel.

Reopen `deploy-copr-fedora` and `deploy-aur` only after real users have installed the GitHub artifacts. At that time:

- build the RPM through COPR and verify installation and paths;
- build the Arch package, regenerate `.SRCINFO`, require no `namcap` errors, and publish with the maintainer's AUR account.

Do not advertise either repository before it contains a verified package.
