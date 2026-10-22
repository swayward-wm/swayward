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

## i3-suite outcomes

The table holds swayward only. Sway 1.12's figures, from the same oracle
revision and stable filter, are in its own block on the oracle's results page.

<!-- i3-suite-summary:i3-suite generated from the pinned oracle; do not edit by hand -->
Oracle [`27560327`](https://github.com/swayward-wm/sway-ipc-oracle/tree/27560327227afd9ff05ea40f3c527b7333e714a6), swayward snapshot `swayward-1fd3c25b`, stable filter over 3,755 assertions:

| Compositor | pass | skip | fail | unreached | unstable | flaky | total |
|---| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| swayward `1fd3c25b` | 1,470 | 12 | 780 | 1,469 | 0 | 24 | 3,755 |

swayward review: 2,261 non-pass rows; 2,191 verified, 70 unverified, 70 of them unclassified.

Each compositor, sway 1.12 included, has its own block on the oracle's [results page](https://github.com/swayward-wm/sway-ipc-oracle/blob/27560327227afd9ff05ea40f3c527b7333e714a6/docs/results.md).
<!-- end i3-suite-summary:i3-suite -->

The table uses the oracle's stable filter, so each row's columns sum to the
whole suite:

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

The table above gives the raw outcomes only. The oracle's
[results](https://github.com/swayward-wm/sway-ipc-oracle/blob/main/docs/results.md)
page holds one block per compositor snapshot, each with its interpretive
buckets, its comparable view (the rows its labels do not set aside as i3-only,
X11-only or harness), and the view's stated asymmetries. Swayward's docs link
to those blocks and copy no figures from them.

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
Oracle [`27560327`](https://github.com/swayward-wm/sway-ipc-oracle/tree/27560327227afd9ff05ea40f3c527b7333e714a6), swayward snapshot `swayward-1fd3c25b`. The sway 1.12 column replays sway's own captures as a self-check.

| Corpus | Scope | sway 1.12 self-check | swayward match | swayward mismatch | swayward unstable | swayward n.a. |
|---|---|---|---:|---:|---:|---:|
| Hand-picked state scenarios | 559 captures, 4,917 rows | 4,916 match, 1 mismatch | 4,908 | 9 | 0 | 0 |
| Events | 45 captures, 45 rows | 45 match | 45 | 0 | 0 | 0 |
| i3-derived states | 416 states, 4,160 rows | 4,160 match | 4,148 | 12 | 0 | 0 |
| Random sequences | 500 seeds × 20 steps | 500 match | 466 | 34 | 0 | 0 |
| Command fuzz | 195 cases | 195 match | 195 | 0 | 0 | 0 |
| Wire fuzz | 10 cases | 10 match | 6 | 4 | 0 | 0 |

Every swayward mismatch is classified in the oracle's `sway-ipc/classifications/`.
<!-- end i3-suite-summary:sway-ipc -->

Read these as measurements by scenario, not as a ranking or one combined
percentage. The [sway compatibility guide](SWAY_COMPATIBILITY.md) describes the
implemented requests, commands, events, and known limits.
