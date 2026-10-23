swayward is a permanent fork of niri. It keeps niri's whole history, replaces
the layout engine and the IPC layer, and expects to absorb upstream work for
as long as both projects exist.

See [`docs/FORK-BASE.md`](https://github.com/swayward-wm/swayward/blob/main/docs/FORK-BASE.md) for the current base and
[`docs/DIVERGENCE.md`](DIVERGENCE.md) for every edit to an inherited file.

## The rule

**Release lines live on protected branches. main follows niri by merging-rebase.**

- Every release line has a branch, `release/<YY.MM>` (the first is
  `release/26.10`). It is protected: no force-push, no deletion. Releases are
  tagged there (`swayward-v<version>`), and fixes for that line, such as a
  beta.2, land there. Users, packagers and Nix inputs that want a stable ref
  follow a tag or a `release/*` branch.
- `main` is where development and niri catch-up happen. It only ever
  fast-forwards, so nobody's checkout breaks, but it absorbs each niri release
  by **merging-rebase** instead of a merge.

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
3. Update `docs/FORK-BASE.md` to the new tag, and add any new inherited-file
   edits to `docs/data/divergence/`.
4. `git push origin main`. It is a fast-forward, so no force is needed.

Things that behave differently with this history shape:

- `git bisect --first-parent`, otherwise bisect can step into an older copy
  of the patch stack.
- `git describe --first-parent`, or explicit tags.
- "N commits ahead of niri" counts every older copy of the stack. Count the
  patch stack as `<newest marker>..main`.
- To undo before pushing, `git reset --hard <old tip>`. After pushing, fix
  forwards: `main` is never force-pushed.

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

### History: before beta1

Until beta1, swayward's commits were rewritten freely and upstream came in
by merge. The fork base is upstream commit `9e72e491` (2026-09-11), 93 commits
after `v26.04`; rebasing back onto `v26.04` was rejected because it would
have discarded upstream fixes we depend on (a screenshot-UI deadlock with a
tablet, multi-GPU copy formats, GLES2 blur, and others). The first
merging-rebase starts from that commit.

## Versioning

swayward uses niri's date scheme, `YY.MM` with a patch component, for its own
releases. Prereleases carry a SemVer suffix and a human name:

    26.10.0-beta.1   # Cargo.toml and the tag swayward-v26.10.0-beta.1
    beta1            # swayward --version and GET_VERSION human_readable

`src/utils/version()` renders the human name followed by the build commit.
`get_version` over IPC reports that string, `"variant": "swayward"`, and the
numeric fields `1.0.0`, which name the sway protocol family rather than the
swayward release.

The version says which swayward release this is, not which niri release it is
based on. `FORK-BASE.md` records the niri base.

A release is tagged `swayward-v<version>`. The prefix keeps our tags apart
from niri's `vYY.MM` tags, which the repository fetches for every upstream
merge.

## Never send AI-generated pull requests upstream

Fixes that belong to niri are reported as issues, or rewritten by hand by
someone who can stand behind them. See
[`CONTRIBUTING.md`](https://github.com/swayward-wm/swayward/blob/main/CONTRIBUTING.md).
