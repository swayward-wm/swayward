# Beta1 and oracle launch checklist

Use this checklist from top to bottom. Stop when a gate fails. Do not tag or publish either repository until the checklist reaches the step that names that action.

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

Complete `launch-beta-version-contract` before editing release metadata. The current decisions conflict: the public version is `beta1`, while the release workflow requires a numeric Semantic Version such as `26.9.0-beta.1`, and Cargo/package metadata still says `26.4.0`.

Record one exact value for each item:

- the signed Git tag;
- `swayward --version` and `GET_VERSION.human_readable`;
- the Cargo workspace version;
- Debian, RPM, and Arch package versions;
- the release workflow input;
- whether `swayward-ipc` is published for beta1.

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
3. Squash before freeze, never after. If the maintainer wants a shorter history, squash swayward `main` now, back up the pre-squash history as a bundle under `$SCRATCH`, and push with `--force-with-lease`. A squash after this step orphans the pinned commit and invalidates every result that names it.
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

Dry-run the one-root tree in a disposable branch before changing `main`:

```sh
export ORACLE_TREE=$(git rev-parse HEAD^{tree})
export ORACLE_ROOT=$(printf '%s\n\n%s\n%s\n' \
  'sway-ipc-oracle: publish the conformance harness' \
  "Uses i3's unchanged BSD-3-Clause test suite with full credit." \
  'Originated in swayward and remains useful independently.' \
  | git commit-tree "$ORACLE_TREE")
git branch launch-squash-dry-run "$ORACLE_ROOT"
test "$(git rev-parse launch-squash-dry-run^{tree})" = "$ORACLE_TREE"
git switch launch-squash-dry-run
./contrib/validate
git switch launch-repin
git branch -D launch-squash-dry-run
```

Gate: the bundle verifies, the tree hashes match, and validation passes on the disposable root commit.

When the maintainer approves the exact root commit message, recreate the root commit, move oracle `main` to it, and push with `--force-with-lease`. This is an irreversible publication step. Do not tag the oracle and do not create an oracle GitHub release.

After the push:

```sh
git fetch origin
test "$(git rev-list --count origin/main)" -eq 1
./contrib/validate
```

Record the new oracle root SHA as `ORACLE_SHA`. Close `oracle-final-squash`, then close `extract-compliance-harness` with the final SHA and reproducibility evidence.

## 7. Pin swayward to the final oracle root

**Owner: swayward integrator**

Start from current swayward `main`. Update `tests/oracle.toml` to `$ORACLE_SHA`. Update every public link that contains the old oracle SHA or old swayward snapshot name.

Regenerate the published i3 summary from the final oracle checkout:

```sh
cd "$SWAYWARD"
./contrib/i3-suite-summary "$ORACLE_WORKTREE"
./contrib/fetch-oracle
./contrib/coverage-report --check
./contrib/check-internal-links
./contrib/check-divergence
git diff --check
```

Gate: the summary names `$ORACLE_SHA`, all public links use the final snapshot name, and coverage reports zero violations. Commit this as one swayward commit. Do not push from a worker worktree; the integrator cherry-picks it.

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

For changes under `src/layout/`, also run both slow gates with `CARGO_BUILD_JOBS=4`, `RUN_SLOW_TESTS=1`, and `PROPTEST_CASES=20000`, as specified in `AGENTS.md`.

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
```

History is final at the tag. Turn on branch protection for swayward `main` and oracle `main` so that neither accepts a force-push or deletion, and confirm it:

```sh
for repo in swayward-wm/swayward swayward-wm/sway-ipc-oracle; do
  printf '%s' '{"required_status_checks":null,"enforce_admins":true,"required_pull_request_reviews":null,"restrictions":null,"allow_force_pushes":false,"allow_deletions":false}' \
    | gh api -X PUT "repos/$repo/branches/main/protection" --input -
  gh api "repos/$repo/branches/main/protection" --jq '.allow_force_pushes.enabled'  # false
done
```

From here on, the oracle pins only tagged or `main` commits, and neither `main` is squashed or rebased again.

Dispatch **Prepare release** on that tag and use the version without the `swayward-v` prefix:

```sh
gh workflow run release.yml \
  --ref "swayward-v$VERSION" \
  -f version="$VERSION"
gh run list --workflow release.yml --limit 1
```

Gate: the validation, Ubuntu, Debian, Fedora, Arch, and clean-install jobs pass. Download the artifacts, verify checksums, review generated notes, and add the bounded beta limitations from `docs/RELEASE-NOTES-beta1.md`. Publish the draft only after maintainer review.

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
