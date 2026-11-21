swayward is a permanent fork of niri. It keeps niri's whole history, replaces
the layout engine and the IPC layer, and expects to absorb upstream work for
as long as both projects exist.

See [`docs/FORK-BASE.md`](https://github.com/swayward-wm/swayward/blob/main/docs/FORK-BASE.md) for the current base and
[`docs/DIVERGENCE.md`](DIVERGENCE.md) for every edit to an inherited file.

## The rule

**Release lines live on protected branches. main follows niri by merging-rebase.**

- Every release line has a branch, `release/<YY.MM>` (the first is
  `release/26.10`). It is protected: no force-push, no deletion. Releases are
  tagged there (`swayward-v<version>`). The branch follows `main` until the
  line's first release candidate, then takes only the fixes described in
  [Release lifecycle](#release-lifecycle). Users, packagers and Nix inputs
  follow a tag or a `release/*` branch, never `main`. Once tagged, a release branch keeps its
  own commits and is never rewritten; `main` may still rewrite its own commits
  above its latest release tag even if a release branch shares them.
- `main` is where development and niri catch-up happen. A merging-rebase push
  fast-forwards, so nobody's checkout breaks; `main` may be rewritten only
  above the latest release tag on it, with `--force-with-lease` when nothing
  else is in flight. It absorbs each niri release by **merging-rebase**
  instead of a merge.

### Merging-rebase

This is the workflow git-for-windows uses to stay on top of git. Each time
niri tags a release, swayward's commits are rebased onto the new tag, and a
marker commit joins the old history back in:

    marker   tree   = the niri tag's tree
             parent 1 = the niri tag
             parent 2 = the previous main tip
             message  "Start the merging-rebase to <tag>"

The rebased commits are replayed on top of the marker. So:

- `main` fast-forwards: the old tip is an ancestor, through parent 2.
- `<newest marker>..main` is exactly swayward's patch stack on top of niri,
  which keeps the divergence readable and reviewable.
- `git log --first-parent` stays clean.

Run it with the script:

    contrib/merging-rebase v26.12            # dry run: the base and stack size
    contrib/merging-rebase --apply v26.12    # rebase locally; never pushes

The rebase onto the tag is where conflicts happen. Resolve them, run
`git rebase --continue`, then rerun the script with `--resume <rebased HEAD>`
to create the marker. The script enables `rerere`, so a resolution replays if
the same rebase is repeated.

Before pushing:

1. Read the range-diff the script prints (it is also in the marker message):
   `git range-diff <old marker>..<old tip> <new marker>..main`.
2. Run `./contrib/fast-gate`. A rebase touches `src/layout/`, so it also runs
   the slow layout tests.
3. Update `docs/FORK-BASE.md` to the new tag and `NIRI_BASE` in
   `src/utils/mod.rs` to match it (`swayward --version` prints it, and a test
   fails until the two agree), and add any new inherited-file edits to
   `docs/data/divergence/`.
4. `git push origin main`. A merging-rebase push is a fast-forward, so no
   force is needed.

Things that behave differently with this history shape:

- `git bisect --first-parent`, otherwise bisect can step into an older copy
  of the patch stack.
- `git describe --first-parent`, or explicit tags.
- "N commits ahead of niri" counts every older copy of the stack. Count the
  patch stack as `<newest marker>..main`.
- To undo before pushing, `git reset --hard <old tip>`. After pushing, fix
  forwards when you can; `main` may be rewritten only above the latest release
  tag on it, with `--force-with-lease` when nothing else is in flight (squash,
  fold a correction into the commit it corrects). Every swayward sha the
  oracle pins is reachable from a release tag (T4 pins the frozen RC sha and
  tags on top of it), so never rewrite a commit at or below that tag. A
  frozen RC counts as a release tag. A release tag is never moved. T4
  squashes only oracle commits above the newest oracle commit pinned by any
  swayward release tag; oracle `main` is otherwise never rewritten.

### Upstream comes in at niri releases

We follow niri **release tags**, not upstream `main`. Tracking `main` would
mean absorbing upstream's in-progress refactors one commit at a time, each
with its own conflict against our layout engine. A release is a point
upstream has already stabilised and tested. The expensive part of each
rebase is judgement about our divergence, not the number of hunks.

When resolving a conflict:

1. Read the upstream release notes first. Anything touching layout, IPC or
   window management is where our divergence lives.
2. Resolve in favour of **sway's behaviour**, not niri's. When a conflict is
   about which model wins, sway wins, and the reason goes in
   `docs/KNOWN_DEVIATIONS.md` with a citation into sway's source.
3. Expect breakage that the tests find only afterwards. An earlier upstream
   merge produced three integration failures that appeared in no individual
   commit, and one resolution silently deleted five tests until
   `cargo test --all` caught it.

### History shape

The fork base is upstream commit `9e72e491` (2026-09-11), 93 commits after
`v26.04`; rebasing back onto `v26.04` was rejected because it would have
discarded upstream fixes we depend on (a screenshot-UI deadlock with a
tablet, multi-GPU copy formats, GLES2 blur, and others). The first
merging-rebase starts from that commit. `main` may be rewritten only above
the latest release tag on it, to keep the replayed stack small; release tags
never move, and every oracle-pinned swayward sha is reachable from one.

## Release lifecycle

T4 below is the full oracle regeneration defined in
[`AGENTS.md`](https://github.com/swayward-wm/swayward/blob/main/AGENTS.md):
every result regenerated against a frozen swayward commit, `pending.toml`
empty, and two runs agreeing. T0 is the fast gate and T1 the targeted oracle
run for a behaviour change.

### A release line is a niri base

A swayward release line `YY.MM` is based on niri's `vYY.MM` tag. The line
starts when `main` absorbs that tag by merging-rebase. Its versions are
`YY.MM.N` and its branch is `release/YY.MM`.

The 26.10 line is the one exception. It predates this scheme and is based on
niri commit `9e72e491`, 93 commits after `v26.04` (see
[`FORK-BASE.md`](https://github.com/swayward-wm/swayward/blob/main/docs/FORK-BASE.md)).
It keeps the 26.10 name through its release candidates, 26.10.0 and its patch
releases.

`N` is swayward's release counter on that base, starting at 0. It is not
niri's point number: swayward 26.12.3 is a release of the 26.12 line, and its
niri base may be niri 26.12.1. The release notes, `FORK-BASE.md` at the tag,
and the second line of `swayward --version` name the exact niri base (see
[Versioning](#versioning)).

### Stages of a line

1. **Betas**, `YY.MM.0-beta.N`, are optional. The 26.10 line has them, and so
   does a line that carries a large change. Each beta is cut from `main` by a
   full T4.
2. **Release candidates**, `YY.MM.0-rc.N`. Every rc is a full T4.
3. **`YY.MM.0`** is the last rc's tree plus only the version bump and its
   release notes. It reuses that rc's oracle measurement: nothing is
   regenerated, the oracle pin stays at the rc's commit, and the notes name
   the rc tag as the measurement.

Until rc1, `release/YY.MM` follows `main`. At rc1 it stops following `main`.
Until `YY.MM.0`, the line accepts blocker fixes only:

- crashes;
- data loss;
- security issues;
- regressions since the previous rc;
- packaging breakage.

Each blocker fix lands on `release/YY.MM` and is forward-ported to `main`. No
behaviour change or sway-parity change lands between rc1 and `YY.MM.0`. After
`YY.MM.0`, changes reach the line only as patch or feature releases.

### Entering rc1

rc1 is cut when all of these hold:

- every open issue and backlog item is triaged as a blocker for the line or as
  later work;
- zero blockers are open;
- no stability regression is open;
- the nightly live soak is green on 3 consecutive nights;
- the 200,000-case proptest CI job is green on the frozen rc commit;
- on a line with betas, the last beta has soaked for 2 weeks with no new
  blocker. A line without betas soaks on rc1 instead: `YY.MM.0` ships only
  after rc1 has been out for 2 weeks with no new blocker;
- T4 is clean: `pending.toml` empty, two runs agree, and the public counts
  are in the release notes;
- the user docs are current: known deviations, the sway config migration
  guide, and the wiki;
- the stable packaging channels are decided.

### Patch releases

A patch release, `YY.MM.N` with `N` above 0, is cut from `release/YY.MM`
only. Release branches are never rebased onto niri.

A patch release normally carries:

- regression, crash and security fixes, cherry-picked from `main`, or fixed on
  the branch first and forward-ported to `main` (`git cherry-pick -x` is
  recommended);
- niri point-release backports: the relevant upstream fixes from, for example,
  niri `v26.12.1` are cherry-picked onto `release/26.12`. If that point tag is
  newer than `main`'s base, `main` absorbs it by merging-rebase.

A patch release has no prerelease suffix by default. An unusually risky one,
such as a large niri backport, goes through `YY.MM.N-rc.1` first; like every
rc, that is a full T4.

Each fix needs T0 and T1. A patch release needs a full T4 only when a fix
changes oracle-measured behaviour. Otherwise the line's pinned measurement
stands, and the release notes say which tag it was measured on.

### Feature releases

A feature can reach a line on the same niri base only as a swayward feature
release, `YY.MM.N`. The feature must be cherry-picked from `main` onto
`release/YY.MM` with `git cherry-pick -x`. The release goes through
`YY.MM.N-rc.1` with a full T4; `YY.MM.N` is then the last rc's tree plus the
version bump and reuses its measurement. Its release notes say that it carries
features. Any other feature waits for the next niri-based line.

### What main carries

Once rebased, `main` carries the next niri base. After the next niri tag,
`main` starts the next line: optional betas, then rcs, then `.0`. The current
line's rc1 comes before that merging-rebase lands on `main`: if niri tags
earlier, the rebase is prepared but waits, unpushed, until rc1 is tagged. Between
niri releases, work on `main` reaches the current line only through a
reviewed feature release; otherwise it waits for the next line.

### Supported lines

Two lines get fixes at once:

| Line | Fixes |
|---|---|
| current stable line | every patch-release class above |
| previous line | security fixes only |

A line reaches end of life when the line after the next one ships its `.0`.
When 26.12.0 ships, for example, 26.12 becomes the current stable line and
26.10 gets security fixes only; 26.10 reaches end of life when the line after
26.12 ships its `.0`.

### Which ref to follow

Users, packagers and Nix inputs follow `swayward-v*` tags, or `release/*` for
the latest fixes on a line. `main` is never recommended to users. A COPR devel
build of `main` may be published, labelled as unreleased.

### Compatibility numbers

Every rc and every `.0` is pinned in the oracle, and its counts are published.
A `.0` keeps its last rc's pin and counts.
A patch release is pinned and published only if it changes measured
behaviour. A public compatibility number always names the tag it was measured
on.

## Versioning

The version is `YY.MM.N`: `YY.MM` names the niri base (see
[Release lifecycle](#release-lifecycle)), and `N` counts swayward releases on
that base. Prereleases carry a SemVer suffix and a human name:

| Kind | Cargo version and tag | Human name | Debian and RPM | Arch `pkgver` |
|---|---|---|---|---|
| beta | `26.10.0-beta.1`, `swayward-v26.10.0-beta.1` | `beta1` | `26.10.0~beta.1` | `26.10.0beta1` |
| rc | `26.12.0-rc.1`, `swayward-v26.12.0-rc.1` | `rc1` | `26.12.0~rc.1` | `26.12.0rc1` |
| release | `26.12.0`, `swayward-v26.12.0` | `26.12.0` | `26.12.0` | `26.12.0` |
| patch or feature rc | `26.12.3-rc.1`, `swayward-v26.12.3-rc.1` | `rc1` | `26.12.3~rc.1` | `26.12.3rc1` |

Each packaging form sorts a prerelease before its release. The Arch package
keeps the Cargo version in `_tagver`.

`swayward_ipc::version::version()` renders the human name followed by the
build commit. `get_version` over IPC reports that string, `"variant":
"swayward"`, and the numeric fields `1.0.0`, which name the sway protocol
family rather than the swayward release.

`swayward --version` adds the niri base as a second line, such as
`niri base: 9e72e491 (93 commits after v26.04)`, from `NIRI_BASE` in
`src/utils/mod.rs`, which a test keeps equal to `FORK-BASE.md`.
`swayward -V` prints the first line only; `swaywardmsg --version` prints
`swaywardmsg <name> (<commit>)`.

The commit is `git describe` of the build tree. A build without `.git` reads
`unknown commit` unless the packager sets `SWAYWARD_BUILD_COMMIT`; the RPM
spec, the PKGBUILD and the flake set it to the source commit.

A release is tagged `swayward-v<version>`. The prefix keeps our tags apart
from niri's `vYY.MM` tags, which the repository fetches for every upstream
merge.

## Never send AI-generated pull requests upstream

Fixes that belong to niri are reported as issues, or rewritten by hand by
someone who can stand behind them. See
[`CONTRIBUTING.md`](https://github.com/swayward-wm/swayward/blob/main/CONTRIBUTING.md).
