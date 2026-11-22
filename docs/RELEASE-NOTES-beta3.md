# beta3 release notes

Beta3 is the third beta on the 26.10 line, based on niri commit `9e72e491`
(93 commits after `v26.04`). It fixes several desktop bugs reported against
beta2, reworks the sway config translator, and makes packaged builds report
the exact commit they were built from.

## Changes you may notice on the desktop

- **Super no longer sticks in a launcher (#49).** With a bind on a bare
  modifier such as `Super_L`, a launcher opened while Super was held was never
  told Super had been released, so it dropped typed text. Swayward now sends
  every modifier change to the focused client, including changes from keys a
  binding consumed, as sway does.
- **Launchers keep the keyboard over empty outputs (#48, partly fixed).** With
  `focus_follows_mouse`, moving the pointer onto an empty area of another output
  no longer switches outputs while a launcher or other layer surface has the
  keyboard. Moving it onto a *window* on another output still gives that
  window the keyboard; see the known deviation below.
- **Popups near the right edge stay where the client put them (#47).** Popups
  of tiled windows away from the output origin, such as Firefox's extension
  popups, were pushed away from where the client placed them. Popups are now
  kept inside the output box, as in sway. A floating window's popups may
  now cover a bar's exclusive zone, which sway also allows.
- **`output <name> toggle` (#43).** The command is implemented.
- **Workspace rename frees assigned names (#34).** Renaming a workspace to a
  name that only has an output assignment no longer keeps it alive when it is
  empty. `workspace <name>` then creates the workspace on its assigned output,
  and the empty leftover no longer blocks `rename workspace 5 to 2`.
- **Command parsing edge cases follow sway.** An empty quoted argument is kept
  (`title_format ""` sets an empty format), `input <id> xkb_switch_layout`
  matches its subcommand ignoring case, and a bad `xkb_switch_layout` target
  no longer stops the rest of the command list.

## Configuration migration

- **`swayward-sway-to-kdl` follows sway's own parser.** It reads lines, line
  continuations and blocks the way sway does, matches directive names ignoring
  case, and accepts all of sway's modifier names (so `Mod3` binds; `Group1` to
  `Group4` translate). A later duplicate binding replaces the earlier one,
  and bind commands keep sway's spelling. Output directives with the same name
  are merged. Malformed input is reported with its line number instead of
  crashing the translator.
- **Seat cursor settings (#42).** `hide_cursor` and `xcursor_theme` on
  `seat *` or `seat seat0` become the KDL `cursor` block.
- **Modifier-only binds are reported (#38).** A `bindsym` or `bindcode` made
  only of modifiers, such as `bindsym Mod4 exec menu`, is reported rather than
  translated into a binding swayward would reject.
- **`for_window` fixes.** `inhibit_idle` translates as a window-rule command,
  and a trailing `;` or `,` no longer forces a manual report.
- **Output arguments are checked.** `output ... on` and `off`, which sway
  rejects, are reported: write `enable` or `disable`. Scales outside (0, 10],
  out-of-range positions and `output *` settings are reported too.
- **Includes.** A file whose path cannot be resolved, such as a symlink loop,
  is reported on its `include` line, and the pattern's other files are still
  read.
- **`swayward validate` checks sway command strings.** It parses the sway
  command in every bind and window rule and warns about any command sway would
  refuse. A command sway accepts but swayward has not implemented is reported
  as "not implemented in swayward". The config still validates;
  `swayward validate --strict` turns the warnings into a failure (#44).

See [Migrating from a sway config](SWAY_CONFIG_MIGRATION.md).

## Packaging and versions

- Packaged builds (RPM, Arch, Nix, release artifacts) report the commit they
  were built from instead of `unknown commit`. The release workflow's install
  checks fail if either binary reports `unknown commit`.
- `swaywardmsg --version` prints the same version string as `swayward`.
  `swayward --version` adds a `niri base:` line naming the niri commit the
  build is based on.
- The RPM, Arch and Debian packages and the release tarball ship
  `THIRD-PARTY.md`, sway's MIT notice for code ported from sway. The RPM,
  Arch and Nix license fields now include MIT.
- The release lifecycle (betas, release candidates, patch and feature
  releases) and the niri-based version scheme are documented in
  [Upstream maintenance](UPSTREAM.md#release-lifecycle).

## Known deviations added

In [Known deviations](KNOWN_DEVIATIONS.md):

- Launcher keyboard focus across outputs
- Modifier-only bindings
- Output identifier selectors
- Workspace layout after re-enabling an output

## Credits

Thanks to Mark Stosberg for the reports and pull requests behind #38 to #49:
the popup, output toggle, launcher modifier and focus fixes, and the
translator and validate work.

## Oracle

Compatibility counts were measured on swayward commit `fc39ac88`, the
source this tag carries unchanged, as snapshot `swayward-fc39ac88` of
[sway-ipc-oracle](https://github.com/swayward-wm/sway-ipc-oracle) at
[`92945a6e`](https://github.com/swayward-wm/sway-ipc-oracle/tree/92945a6ea5137be392f0db677b45b822c9bb505b)
(`92945a6ea5137be392f0db677b45b822c9bb505b`), the commit
[`tests/oracle.toml`](../tests/oracle.toml) pins. They are the figures on
the oracle's
[results page](https://github.com/swayward-wm/sway-ipc-oracle/blob/92945a6ea5137be392f0db677b45b822c9bb505b/docs/results.md),
generated with `contrib/i3-suite-summary`. The sway 1.12 column replays
sway's own captures as a self-check.

| Corpus | Scope | sway 1.12 self-check | swayward match | swayward mismatch | swayward unstable | swayward n.a. |
|---|---|---|---:|---:|---:|---:|
| Hand-picked state scenarios | 566 captures, 4,987 rows | 4,986 match, 1 mismatch | 4,970 | 17 | 0 | 0 |
| Events | 45 captures, 45 rows | 45 match | 45 | 0 | 0 | 0 |
| i3-derived states | 416 states, 4,160 rows | 4,160 match | 4,151 | 9 | 0 | 0 |
| Random sequences | 500 seeds × 20 steps | 500 match | 466 | 33 | 1 | 0 |
| Command fuzz | 195 cases | 195 match | 195 | 0 | 0 | 0 |
| Wire fuzz | 10 cases | 10 match | 6 | 4 | 0 | 0 |

Every swayward mismatch is classified in the oracle's `sway-ipc/classifications/`.

The i3 suite is context for where sway differs from i3, not a sway
compatibility measure. Stable filter over its 3,755 assertions:

| Compositor | pass | skip | fail | unreached | unstable | flaky | total |
|---|---:|---:|---:|---:|---:|---:|---:|
| swayward `fc39ac88` | 1,470 | 12 | 780 | 1,469 | 0 | 24 | 3,755 |

[Reading the i3 suite results](I3_SUITE_RESULTS.md) explains the outcomes.
