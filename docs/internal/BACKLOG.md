# Maintainer backlog

Migrated from the `mu -w swayward` workstream (last live triage
2026-10-28). That workstream is an archive; this file is the source of truth
for unfinished work.

**Re-verify on current `main` before picking anything up.** Many residual
titles cite scratch dirs and oracle rows that may no longer exist. Do not
promote a residual into `KNOWN_DEVIATIONS.md` or a user-facing issue until it
reproduces.

## How to file new work

| Kind | Where |
| --- | --- |
| Deliberate / refused sway behaviour (user-visible) | [`docs/KNOWN_DEVIATIONS.md`](../KNOWN_DEVIATIONS.md); oracle cited deviation |
| Confirmed user-visible bug still true on `main` | GitHub issue (or a row under [Open bugs](#open-bugs-candidates--re-verify-first) once verified) |
| Diff-family / oracle / harness engineering | [Engineering leftovers](#engineering-leftovers) below |
| Launch / announce / templates | [Launch ops](#launch-ops) below |
| COPR / AUR / post-beta features | [Post-beta](#post-beta) below |
| Decided not to fix, logged so it is not re-filed | [Won't fix (recorded)](#wont-fix-recorded) below |

Public docs must not link here. See [`README.md`](README.md).

## Launch ops

Beta1 and beta2 tags already shipped. Remaining announcement work:

| Item | Notes |
| --- | --- |
| Gap / migration issue templates + pinned issue | Was `beta-report-machinery`. `.github/ISSUE_TEMPLATE` still only `bug_report.md` + `config.yml`. |
| Announce plan (where to post, beta etiquette) | Was `launch-announce-and-beta`. |
| Interest / beta-tester strategy | Was `beta1-interest-strategy`. |
| Launch sequence | Was `launch-beta1`: swaywardlock r/unixporn → oracle post (+ sway crash reports) → swayward announcement. |
| GitHub Support: purge closed-PR refs that still serve `docs/internal/TONE.md` | Nice-to-have, not a release gate. Was `github-support-purge-pr-refs`. |

Also see [`launch-checklist.md`](launch-checklist.md) (parts are pre-beta1; trust the release notes for what already shipped).

## Open bugs (candidates — re-verify first)

Likely user-visible or command-gap. Confirm on current `main` + oracle/i3 harness before filing issues or fixing.

| Item | Issue | Hint |
| --- | --- | --- |
| `rename workspace 5 to 2` rejected where sway accepts | [#34](https://github.com/swayward-wm/swayward/issues/34) | i3 522; was `swayward-rename-workspace-assigned` |
| Floating split after scratchpad show: wrong view height | [#35](https://github.com/swayward-wm/swayward/issues/35) | Was `swayward-floating-split-scratchpad-height` |
| Focus after `[con_mark] move workspace to output` matching two containers | [#36](https://github.com/swayward-wm/swayward/issues/36) | i3 543; was `swayward-move-ws-multi-match-focus` |
| Implement `ipc_kill_timeout` (drop IPC clients that stop reading) | [#37](https://github.com/swayward-wm/swayward/issues/37) | Was `swayward-ipc-kill-timeout` |
| `output <name> toggle` re-enable: re-created workspace is `splitv` where sway gives `splith` (portrait output) | — | Oracle row `output_toggle_reenable` (sway-ipc-oracle `088ce65c`). Sway creates the workspace while the output's cached size is 0x0 (`sway/tree/workspace.c:219`, `sway/tree/output.c:441-450`, `sway/config/output.c:677-678,1085-1092`); swayward keeps the real size through disable (`src/backend/headless.rs:283-295`). Declared in `KNOWN_DEVIATIONS.md`. Low impact. Was `toggle-reenable-layout` |
| Output command failure does not continue the list: `output NOPE toggle bogus` returns `CMD_INVALID` and stops, where sway returns `CMD_FAILURE` "Cannot apply toggle to unknown output NOPE" and runs the rest; sway also refuses `output FALLBACK ...` with `CMD_FAILURE` | — | Parked from beta3; re-verify before work. Sway: `sway/commands.c:295-299`, `sway/commands/output/toggle.c:17-21`, `sway/commands/output.c:83-97` (continuation), `output.c:44-47` (FALLBACK). Needs the output selector redesign first: identifier selectors such as `"Unknown Unknown Unknown"` cannot be matched today (see [`specs/2026-11-20-output-selectors.md`](specs/2026-11-20-output-selectors.md)). A fix must keep criteria replies (`No matching node.`) and cached criteria targets correct. Needs an oracle row. Was `beta3-cmd-continue`, earlier `pr43-cmd-failure-continue` |
| `exec` tail tokenizing still differs from sway after `bfa738e3` (the empty-token case now matches): a newline in an exec tail runs two commands where sway joins one line; Unicode whitespace counts in the quote-strip argc and in `--no-startup-id` detection, where sway splits on ASCII only (`common/stringop.c:92-142`); upper-case `EXEC` keeps quotes where sway's `strcmp` path unquotes (`sway/commands.c:265-274`); `set $e ""; exec $e` fails arity (see `pr44-expanded-empty-args`) | — | Parked from beta3; re-verify before work. `swayward-ipc/src/command/parse/rules.rs` `parse_exec`. No oracle row observes the spawned argv: evidence is a headless test in `src/tests/` that reads it. Was `beta3-exec-split`, earlier `rt-exec-split` |
| `swap` of sticky or fullscreen floating containers diverges | — | i3 291-swap.t 128, 129, 134. Sway re-fullscreens con2 and focuses it (`sway/tree/container.c:1884-1889,1199-1206`); sticky floaters follow a workspace switch (`sway/input/seat.c:1209-1220`). Needs an oracle row. Was `swap-sticky-fullscreen` |
| `workspace N gaps …` (translated) makes N eagerly created and persistent; sway does not | — | `contrib/sway-to-kdl` emits `workspace "N" { layout … }` with no assignment; `src/layout/sway_workspace.rs` treats it as declared. Sway: `sway/tree/workspace.c:314-331`. See [`specs/2026-09-16-workspace-order-model.md`](specs/2026-09-16-workspace-order-model.md). Was `ws-gaps-persistent` |
| Rename during a pending workspace switch: stale switch index after re-sort; rename onto a stale empty name rejected during overview DnD (cosmetic) | — | `sort_sway_workspaces` updates only `active_workspace_idx` (`src/layout/monitor.rs`); `finish_workspace_switch` ignores `DndScroll`. Seen on `2aec3ab5`. Was `f34-followup` |
| A `release=true` binding consumes its key; sway forwards the press and the release to the client | — | `should_intercept_key` (`src/input/keyboard_bindings.rs:63-78,136`) vs `sway/input/keyboard.c:474-490,548-561,574-582`. Not observable over IPC; needs a headless protocol test. Was `pr49-release-bind-forward` |
| X11-only criteria are refused before sway's checks of their value: `[class] kill` answers "X11-only criterion 'class' is unsupported" where sway answers "Token 'class' requires a value" (likewise a non-numeric `id`) | — | `apply_pair` in `swayward-ipc/src/criteria.rs` vs `sway/criteria.c:635-642,677-681`. Wire reply only: `swayward validate` already classifies these as invalid. Needs an oracle row. Was `pr44-x11-criteria-value` |
| Empty arguments are lost when a command is rebuilt from expanded words: with any variable set, `set $unused ""; assign [app_id=foo] ""; nop` fails assign's arity check and `set $empty ""; exec $empty` loses the argument; sway keeps argc | — | `parse_one_with_variables` joins expanded words into `expanded_input` (`swayward-ipc/src/command/parse/mod.rs`) vs `sway/sway/commands/assign.c:11-12,65`. Pre-existing (the lexer dropped `""` before `pr44-land`). Needs an oracle row. Was `pr44-expanded-empty-args` |
| Numeric arguments mishandle an empty word that sway reads as 0 through strtol/atoi/strtof: `border pixel ""` is accepted without a width, where sway applies width 0; `titlebar_border_thickness ""`, `workspace 1 gaps inner ""` and `output * scale ""` are rejected | — | `swayward-ipc/src/command/parse/move_resize.rs` (border), `settings.rs` (titlebar_border_thickness), `workspace.rs` (gaps), `output.rs` (scale) vs `sway/sway/commands/titlebar_border_thickness.c:15-17`, `border.c:91-92`, `workspace.c:79-80`, `output/scale.c:14-15`. Pre-existing (the lexer dropped `""` before `pr44-land`). Needs an oracle row. Was `pr44-empty-numeric-args` |
| Popup and move leftovers from niri: 8 px popup padding preference; interactive move keeps the window-width restriction | — | `src/handlers/xdg_shell/configure.rs:484-526`, `src/layout/mod.rs:1833-1842`. Sway has neither. Was `popup-parity-leftovers` |
| Oracle runner has no layer-shell client, so no row can observe layer keyboard focus (B0 of PR #48) | [#48](https://github.com/swayward-wm/swayward/pull/48) | sway-ipc-oracle: a `wlr-layer-shell` fixture client, a capability flag, `layer`/`layer_unmap` actions, rows captured from pinned sway. Prerequisite for the next two rows. `scratch/design-48-v2.md` §7 |
| GET_TREE marks a node focused and GET_SEATS reports its id while a layer holds the keyboard; sway marks none and reports `focus: 0` (B1 of PR #48) | [#48](https://github.com/swayward-wm/swayward/pull/48) | `sway/ipc-json.c:811,1222-1229`, `sway/input/seat.c:1283-1288,1431-1440`. Already differs with a layer on one output. Fix in `src/ipc/server/query_state.rs::describe_live_tree`; needs B0's row first. `scratch/design-48-v2.md` §4 |
| An exclusive layer loses the keyboard when the seat focuses a window on another output; sway keeps it (B2 of PR #48, the fuzzel report) | [#48](https://github.com/swayward-wm/swayward/pull/48) | Seat-level exclusive owner chosen by recency (`sway/desktop/layer_shell.c:103-139,305-312`); seat-focus side effects under the layer: urgency, focus stack, `window::focus` (`sway/input/seat.c:1255-1264`); the owner's popups on another output. ~250-300 prod lines over ~9 files, medium-high risk; needs B0 and B1. Declared in `KNOWN_DEVIATIONS.md`. `scratch/design-48-v2.md` §5 |

## Hygiene (small, clear)

| Item | Hint |
| --- | --- |
| AGENTS.md vs `ci.yml` on 200k proptest cadence | Was `ci-proptest-cadence-truth` |
| `contrib/dev-run.sh` stray `/proc/PID/cmdline` at teardown | Was `devrun-stray-proc-error` |
| Nightly oracle sweep job (skip unless `main` moved) | Was `nightly-sway-ipc-sweep` |
| Capture-check empty log when a test launches sway | Was `capture-check-launch-with-config` |
| Border inner-arc follow-ups | Was `border-arc-followups` |
| Floating-group pointer test gaps | Was `fcg-pointer-group-test-gaps` |
| Flake candidate: `tests::xdg_shell::client_interactive_resize_requires_floating` | Was `flake_candidate_tests_xdg_shell_client` |
| Scratchpad stale-entry doc/oracle follow-ups | Was `scratchpad-stale-followups` |
| Divergence ledger cites missing oracle row (focused-flag) | Was `low_diff_fam_parity_f2_focused_flag` |
| Regression test doc cites missing oracle row (hinted cross-output) | Was `low_diff_fam_v3_hinted_cross_output_move` |
| Output toggle nits: `ipc_outputs().lock().unwrap()` → snapshot; unreachable `OutputAction::Toggle` arm | `lock().unwrap()` in `toggle_disables`, `src/command/output.rs:43-50`; unreachable `Toggle` arm in `apply_transient_output_config`, `src/swayward/config_reload.rs:561`. Was `pr43-nits` |

## Engineering leftovers

Not user docs. Re-run or close; do not paste into `KNOWN_DEVIATIONS.md`.

### Campaigns

| Item | Hint |
| --- | --- |
| Differential campaign (`random-v2` / G16) | Was `differential-campaign` |
| Post-beta IPC-shape compliance backlog (NON-BLOCKER) | Was `post-beta-compliance-backlog` |

### Diff-family residuals

Was: `diff-fam-parity-f8l-stale-split-share-residual-1`,
`diff-fam-v3-floated-fs-sibling-residual-1`,
`diff-fam-f3-focus-order-residual-6`,
`diff-fam-f3-focus-order-residual-8`,
`diff_fam_f3_focus_order_residual_7`,
`diff-fam-parity-f2-focused-flag-residual-4`,
`diff-fam-v3-fullscreen-move-to-workspace-switches`,
`diff-fam-v3-floating-fullscreen-sibling-focus-residual-2`,
`diff-fam-v3-for-window-fs-floating-group-member-residual-1`,
`diff-fam-v3-hinted-floater-kill-focus-residual-1`,
`diff-fam-v3-urgent-criteria-move-fullscreen-scratchpad`,
`diff-fam-v3-urgent-focused-parent-residual-4`,
`diff_fam_v3_output_disable_workspace`,
`diff-fam-v3-layout-default-floated-split-command`.

Triage note (2026-10-28): scratch evidence under `scratch/g16` and
`scratch/beta-triage/` had already rotted.

### Oracle / provenance

| Item | Hint |
| --- | --- |
| Recapture sway tree fixtures inside pinned image | Was `oracle-recapture-host-fixtures` |
| Swayward adapter merges repeated workspace assignments | i3 297; was `oracle-swayward-adapter-dup-workspaces` |
| Validate i3 coverage exemption by scenario, not capture name | Was `oracle-validate-coverage-exemption-by-scenario` |
| Random corpus replayable on i3 | Was `oracle-i3-random-real-replay` |
| sway-ipc applicability gaps (hinted windows / size hints) | Was `sway-ipc-applicability-gaps` |
| Provenance: classify deps by kind, not output path | Was `provenance-classify-by-kind` |
| Map `src/swayward/config_reload.rs` and `src/utils/mod.rs` in `tests/oracle-targets.toml` | `contrib/targeted-oracle` exits 2 on them (unmapped). Until mapped, use a full sweep diff against the rolling baseline. Was `oracle-targets-map` |
| Oracle `contrib/tests/pr-check` self-test fails in the pinned container | `PermissionError` case raises `KeyError: 'resolved'` (likely running as root); passes in distrobox. Was `oracle-prcheck-root` |

## Post-beta

Parked on purpose until after the beta launch path. Several are already
declared in `KNOWN_DEVIATIONS.md` (e.g. global fullscreen spans one output).

| Item | Hint |
| --- | --- |
| Post-beta1 umbrella (refused until after beta) | Was `post-beta1-list` |
| Output selector redesign: sway's stored `output` entries layered over KDL sections | Parked (large, risky). Design: [`specs/2026-11-20-output-selectors.md`](specs/2026-11-20-output-selectors.md). Oracle row `output_identifier_unknown` (sway-ipc-oracle `db931de8`) mismatches on `main`; declared in `KNOWN_DEVIATIONS.md`. `9d6ea061` is not landable |
| Global fullscreen spans every output | Declared deviation; was `feature-global-fullscreen-span-outputs` |
| Floating-group residual random seeds | Was `fcg-random-residual-group-seeds` |
| Parity S2 output layout geometry | Was `parity-s2-output-layout-geometry` |
| Parity F9 / F8s / F11–F12 residuals | Was `parity-f9-arranged-tiled-rect`, `parity-f8s-percent-rounding`, `parity-f11-f12-tree-structure` |
| Per-view `allow_tearing` / `max_render_time` | Was `per-view-allow-tearing-render-path`, `per-view-max-render-time-deadline` |
| xwayland-satellite upstream protocol PRs | Was `satellite-upstream-standard-protocols` |
| Touchpad GET_INPUTS oracle row on real hardware | Was `touchpad-libinput-hardware-oracle-row` |
| Fedora COPR | Not a beta gate; GitHub RPMs ship. Was `deploy-copr-fedora` |
| COPR devel build of `main`, labelled unreleased | Allowed by the release lifecycle in `docs/UPSTREAM.md`; no such channel exists yet |
| AUR | Not a beta gate; Arch pkg on GitHub release. Was `deploy-aur` |

## Won't fix (recorded)

Decided; logged so the numbers and refusals are not a surprise.

| Item | Hint |
| --- | --- |
| i3 291-swap.t: 35 intentional skips (X11 absolute rectangles, i3 floating wrapper) | 28 `x11_absolute_geometry` rect skips (97-108, 115-118, 121-124, 130-133, 135-138) that native `xdg_toplevel` cannot place; 7 `i3_floating_wrapper` skips (109-113, 120, 125), sway has no wrapper. Documented skips in `tests/i3/coverage.toml` (`291-swap.t`: 96 pass, 39 skip, 3 open fails tracked as `swap-sticky-fullscreen`). Was `i3-291-known-fails` |
| sway-to-kdl lexer edge cases: NUL in args, exotic delimiters, ~40k commands on one line | NUL unreachable (`split_args` truncates); only `;,` and `,` delimiters reach it; quadratic only at ~40k commands per line. Quotes kept in argv by design (`sway/commands.c:264-272`). Was `kdl-lexer-wontfix` |
| sway-to-kdl: keyword and one-dimension `resize set` forms (#41) | Reported, not translated: floating state cannot be inferred from config. `floating enable, resize set 800 600` translates. Revisit only if a sound rule appears. Was `kdl-resize-set-forms` |

## mu archive

Workstream name: `swayward`. Tasks closed `superseded` on migration with a
pointer here. Do not dispatch from that workstream; open a fresh
`<project>-<purpose>` workstream when a crew actually runs.
