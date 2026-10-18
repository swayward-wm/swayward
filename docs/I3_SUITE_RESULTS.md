# Reading the i3 suite results

This page explains one part of swayward's test suite. [Testing and
conformance](https://github.com/swayward-wm/swayward/wiki/Testing-and-Conformance)
describes the whole suite.

The oracle runs i3's own tests unchanged against i3, sway, and swayward. This is
a demanding way to find regressions in the shared tree and IPC model. It is not
a sway compatibility percentage: many assertions require i3's X11 window
manager, config parser, bar, restart mechanism, or private tree shapes. Sway
1.12 does not pass most of them either.

Every figure on this page is generated from the committed results of the oracle
revision pinned in [`tests/oracle.toml`](../tests/oracle.toml). `fast-gate`
fails if a figure differs from those results. To regenerate after a repin:

```sh
./contrib/fetch-oracle
./contrib/i3-suite-summary --write
```

## Swayward's i3-suite outcomes

<!-- i3-suite-summary:i3-suite generated from the pinned oracle; do not edit by hand -->
Oracle [`cd64b835`](https://github.com/swayward-wm/sway-ipc-oracle/tree/cd64b8351f6c427dd8b3533c231c3f0ba42fad12), swayward snapshot `swayward-54c5acd9` (`54c5acd9`), stable filter over 3,755 assertions:

| pass | skip | fail | unreached | unstable | flaky | total |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1,453† | 11† | 800† | 1,469† | 0† | 22† | 3,755 |

Review: 2,280† non-pass rows are classified; 2,199† verified, 81† unverified, 81† of them unclassified.

† From a result listed in the oracle's `pending.toml`: it awaits regeneration, and the figure is its last measured value.
<!-- end i3-suite-summary:i3-suite -->

The table uses the oracle's stable filter, so its columns sum to the whole
suite:

- **fail** is an assertion that ran and did not pass;
- **unreached** is an assertion after an earlier abort, which never ran;
- **unstable** is an assertion whose outcome changed between two complete runs
  (`i3/unstable.toml`);
- **flaky** counts every assertion of a file whose TAP plan or abort point
  changed between runs (`i3/flaky.toml`).

A row's classification records why it does not pass: an i3-only premise, an X11
premise that does not cross swayward's `xwayland-satellite` boundary, a harness
artefact, or a finding to investigate. A finding is a compatibility bug or a
harness problem until shown otherwise; the classification does not excuse it.
The oracle's
[`i3/classifications/`](https://github.com/swayward-wm/sway-ipc-oracle/tree/main/i3/classifications)
holds the assertion-level review.

## Where sway and swayward differ

The oracle publishes one block per compositor snapshot, never one table across
compositors. Each block holds that snapshot's interpretive buckets, its own
comparable view (the rows its labels do not set aside as i3-only, X11-only or
harness), and the view's stated asymmetries. Read swayward's and sway's blocks
in the oracle's
[README](https://github.com/swayward-wm/sway-ipc-oracle#snapshots). Swayward's
docs link to them and copy no figures from them.

Many of swayward's remaining sway-relative gaps need X11 identity, window type,
urgency hints, requested geometry, or client geometry. xwayland-satellite
presents X11 clients as ordinary `xdg_toplevel` surfaces, without a separate
XID, class, instance, role, or window type, so those assertions cannot pass
through it. The oracle buckets them as `N-x11-sway`. The
[X11 window identity](KNOWN_DEVIATIONS.md#x11-window-identity) section and the
[Xwayland guide](https://github.com/swayward-wm/swayward/wiki/Xwayland) explain
the design. Work on standard protocols that can carry more of this metadata is
planned after beta 1.

## Use the sway corpus for sway compatibility

The oracle also compares IPC requests and events directly with sway 1.12. This
is the closer measure of sway compatibility because each scenario starts from
the same state and compares sway-shaped replies or event sequences.

<!-- i3-suite-summary:sway-ipc generated from the pinned oracle; do not edit by hand -->
Oracle [`cd64b835`](https://github.com/swayward-wm/sway-ipc-oracle/tree/cd64b8351f6c427dd8b3533c231c3f0ba42fad12), swayward snapshot `swayward-54c5acd9`. The sway 1.12 column replays sway's own captures as a self-check.

| Corpus | Scope | sway 1.12 self-check | swayward match | swayward mismatch | swayward unstable | swayward n.a. | untriaged mismatches |
|---|---|---|---:|---:|---:|---:|---:|
| Hand-picked state scenarios | 78 scenarios, 780 rows | 740 match, 40 n.a. | 749 | 31 | 0 | 0 | 31 |
| Events | 45 scenarios, 45 rows | 45 match | 44 | 1 | 0 | 0 | 1 |
| i3-derived states | 418 states, 4,180 rows | 4,180 match† | 4,118† | 62† | 0† | 0† | 62† |
| Random sequences | 500 seeds × 20 steps | 500 match | 329 | 171 | 0 | 0 | 171 |
| Command fuzz | 195 cases | 195 match | 195 | 0 | 0 | 0 | 0 |
| Wire fuzz | 10 cases | 10 match | 6 | 4 | 0 | 0 | 0 |

- sway 1.12, Hand-picked state scenarios: 40 rows not applicable: fixture client unavailable (missing wayland-scanner).

† From a result listed in the oracle's `pending.toml`: it awaits regeneration, and the figure is its last measured value.
<!-- end i3-suite-summary:sway-ipc -->

Read these as measurements by scenario, not as a ranking or one combined
percentage. The [sway compatibility guide](SWAY_COMPATIBILITY.md) describes the
implemented requests, commands, events, and known limits.
