# Output selectors: sway's stored output configs layered over KDL sections

Status: Parked (maintainer, 2026-11-20): large, risky parity change; pick up from here.
Date: 2026-11-20

No code lands with this document. Re-check every `path:line` against current
`main` before implementing; the citations are to the commits below.

Evidence base:

- sway 1.12 (`8886939 build: bump version to 1.12`). Citations of the form
  `config/output.c:241` are relative to the `sway/` directory of the sway
  repository. `common/` and `include/` paths are relative to the repository
  root.
- swayward `main` at `18ca1ea8`. Citations are `path:line` in this repo.
- sway-ipc-oracle `db931de8`: `sway-ipc/scenarios.toml`,
  `sway-ipc/fixtures/*.outputs.json` and `contrib/sway-ipc-run`.
- A first attempt, `9d6ea061`, which is not on `main` and is not landable
  (section 0).

## 0. Problem and the shape of the answer

Sway keeps every `output <selector> …` command as a **stored entry named
exactly as typed**, holding only the fields that command set. It computes each
output's configuration on demand by merging the entries whose name equals `*`,
the output's connector name or its identifier, in that order. Swayward has no
such store. A runtime command edits the first matching KDL `output` section in
place, or appends a section named as typed
(`src/swayward/config_reload.rs:508-549`). Every consumer then reads exactly one
section per output (`swayward-config/src/output.rs:150-156`).

The visible symptom is oracle row `output_identifier_unknown` (sway-ipc-oracle
`db931de8`): `output "Unknown Unknown Unknown" scale 1.5` leaves both headless
outputs at scale 1.0 on `main`, and the following identifier `toggle` fails
with "unknown output".

That row starts both outputs in the same state, so a fix can match it and
still be wrong. Any design must also meet these constraints, which the row
cannot see:

1. `toggle` takes its first match from sway's newest-first `all_outputs`
   (1.8), not from a sorted or hashed order, or unequal states toggle the
   wrong way.
2. `power toggle` is resolved once, disabled outputs included, before
   anything is changed (1.7), or results are mixed or partial.
3. A runtime command changes the result of the section first-match lookup
   actually uses, not some other matching section.
4. Offline and disabled outputs keep case-insensitive lookup (1.10).
5. A selector stays a stored name: it reaches outputs that appear later,
   obeys sway's supersede rules (1.3), and applies only on an exact-case
   match (1.10). Expanding it into today's connector names fails all three.

`9d6ea061` expands selectors into connector names and is not landable.

This spec keeps the KDL sections as swayward's config-file layer, unchanged. It
adds sway's entry list as a typed runtime layer, merged on top by sway's rules.

## 1. Sway 1.12's model

### 1.1 Vocabulary

- **Connector name**: `wlr_output->name`, e.g. `DP-1` or `HEADLESS-1`.
- **Identifier**: `"<make> <model> <serial>"`, where a missing field is written
  as `Unknown` (`output_get_identifier`, `config/output.c:31-38`). Headless
  outputs have none of the three fields, so every headless output, including
  sway's `FALLBACK`, has the identifier `Unknown Unknown Unknown`. The oracle
  fixture `output_identifier_unknown_scale.outputs.json` reports `make`,
  `model` and `serial` as `Unknown` for both outputs.
- **Wildcard**: `*`.
- **Entry**: one `struct output_config`, created by `new_output_config(name)`
  with every field at an "unset" sentinel: `enabled = -1`, `scale = -1`,
  `x = y = INT_MAX`, `width = height = refresh_rate = custom_mode = -1`,
  `power = -1`, and so on (`config/output.c:55-84`).

### 1.2 Parsing a runtime `output` command

`cmd_output` (`commands/output.c:36-129`) works in this order:

1. It refuses a selector equal to the fallback output's name, compared with
   `strcasecmp`: "Refusing to configure the no op output" (`:44-47`). The
   fallback is a headless output renamed `FALLBACK` (`server.c:690-693`).
2. `-` and `--` stand for the focused output's connector name and identifier.
   They are rejected while the config file is being read (`:50-71`).
3. Otherwise it creates an entry named exactly `argv[0]` (`:73`). The name is
   not looked up or canonicalised.
4. It runs each subcommand in turn. Subcommand names are looked up
   case-insensitively (`handler_compare` uses `strcasecmp`,
   `commands.c:145-159`). Each handler sets fields on the one pending entry, so
   a later subcommand of the same command overwrites an earlier one (for
   example `enable.c:9`, `disable.c:8`, `scale.c:14`).
5. If any subcommand fails, the pending entry is freed and nothing is stored
   (`:95-96`, `:125-128`). A command is therefore atomic.
6. `store_output_config(output)` runs (`:109`), then `request_modeset()`
   (`:116`), and the command returns `CMD_SUCCESS` (`:123`). A selector that
   matches no output still succeeds and is stored.

`mode` sets `custom_mode` (1 for `--custom`, otherwise 0), `width` and
`height`, and sets `drm_mode.type = 0`, which means "no modeline" and counts as
a set value (`commands/output/mode.c:15-24`, `:25-57`). It sets `refresh_rate`
**only** when the mode carries `@<rate>Hz` (`:35-45`). A later `mode` therefore
clears an earlier `modeline` during merge, and a `mode` without a rate leaves
an earlier rate in place. `modeline` sets only `drm_mode` (`mode.c:110`).

### 1.3 Storing: `store_output_config` (`config/output.c:241-294`)

```
wildcard = strcmp(oc->name, "*") == 0                              // :243
output   = wildcard ? NULL : all_output_by_name_or_id(oc->name)   // :244, first match, see 1.8
id       = identifier(output)                                      // :246-249
for old in config->output_configs (list order):
    if strcmp(old->name, oc->name) == 0: merge(old, oc); merged = true; continue   // :258-262
    if wildcard: supersede(old, oc); continue                                        // :266-269
    if output && strcmp(old->name, id) == 0
              && strcmp(oc->name, output->wlr_output->name) == 0:
        supersede(old, oc)                                                           // :273-276
if !merged: append oc                                                                // :289-293
```

- `merge(dst, src)` copies every field that `src` set (`:164-239`).
- `supersede(dst, src)` resets to unset every field of `dst` that `src` set
  (`:86-161`).
- Both work field by field. `width`, `height`, `refresh_rate`, `custom_mode`
  and `drm_mode` are separate fields (`:90-119`, `:168-194`), so "the mode" is
  not one value.
- A new entry with the same name, compared exactly, merges into the old one in
  place, so the old entry keeps its list position.
- A wildcard command clears the fields it sets from **every** non-wildcard
  entry, including entries for other outputs and for outputs that are absent.
- A command naming a connector, spelled exactly as `wlr_output->name`, clears
  its fields from the entry named by **that output's** identifier. Other
  outputs with the same identifier lose those values too.
- An identifier command supersedes nothing. It wins over a connector entry
  anyway, because identifiers merge last (1.4).
- If the output does not exist when the command runs, `output` is NULL and no
  connector-versus-identifier supersede happens.

### 1.4 Effective configuration and when it is applied

`find_output_config_from_list` (`config/output.c:703-732`) starts from a fresh
entry and merges every entry whose name `strcmp`-equals `"*"`, then the
connector name, then the identifier (`:717-728`). Within each tier, entries are
merged in list order. The precedence is therefore **identifier over connector
over wildcard over default**, each field separately. `find_output_config`
applies this to the stored list (`:734-738`).

`apply_output_configs` (`:1124-1153`) takes a list of entries, walks
`root->all_outputs`, skips the fallback (`:1135-1138`), resolves each output
against that list (`:1142`), sorts by priority (`:965-994`) and commits.
`apply_stored_output_configs` (`:1155-1158`) passes the stored list.

Disabling is decided per field:

- `output_config_is_disabling` is true when `enabled == 0` or `power == 0`
  (`:478-480`). `queue_output_config` then commits the wlr output as disabled
  and **returns before setting mode, transform or scale** (`:490-493`).
- `finalize_output_config` removes the output from the layout and calls
  `output_disable` only when `enabled == 0` (`:641-648`). With `power == 0`
  and `enabled != 0`, the output stays a sway output (`:677-679`) while its
  wlr output is off. That is the "powered off" state.

So stored mode, transform and scale are **deferred** while the output is
disabled or powered off: the wlr output keeps its previous live values, and
`GET_OUTPUTS` reports the live `wlr_output->scale` (`ipc-json.c:351-352`), not
the stored one. They take effect at the first modeset that finds the output
enabled and powered.

For a field no entry sets, the commit falls back to a default: the normal
transform, a computed scale that is 1 for any output with no physical size
(`:432-438`, `:526-534`), and the preferred mode **only if the output has a
mode list** (`:496-505`). An output without modes, which includes every headless
output (`output_identifier_unknown_scale.outputs.json` reports `modes: []`),
keeps its current mode when no entry sets one.

`request_modeset` arms a 10 ms timer that runs `apply_stored_output_configs`
(`desktop/output.c:402-417`). A new output is created on `handle_new_output`
(`desktop/output.c:572`), which also calls `request_modeset` (`:600`). Stored
entries, power included, therefore apply to outputs that appear later, by exact
name or identifier.

### 1.5 Config file, runtime commands and reload

The config file is read through the same `cmd_output` and
`store_output_config`. While `config->reading` is set, the modeset waits until
the end (`commands/output.c:111-114`). At startup the config is loaded
(`main.c:359`) before `server_start` (`main.c:366`), so the only output in
`all_outputs` is `FALLBACK`, and no config-file connector line supersedes an
identifier line. Runtime commands then append to and supersede the same list.
Because of the supersede rules, the most recent runtime command naming an
output that exists at that moment overrides the file's value for that output.

`reload` builds a fresh `struct sway_config` (`config.c:465-471`) whose
`output_configs` starts empty (`config.c:225`), loads only the file
(`config.c:508`), requests a modeset (`config.c:538-540`) and frees the old
config (`config.c:549`). **Every runtime output entry is discarded on
reload.** The next modeset applies the file's entries, with the defaults of 1.4
for fields the file leaves unset. A runtime disable, scale, position or power
change therefore reverts. A runtime mode change on an output with no mode list
does not revert unless the file sets a mode (1.4).

wlr-output-power-management requests become stored connector entries
(`handle_output_power_manager_set_mode`, `desktop/output.c:706-727`).

wlr-output-management applies (`output_manager_apply`,
`desktop/output.c:634-686`) work in two stages:

1. **Apply a temporary list.** Each configured head becomes an entry named by
   its connector (`output_config_for_config_head`, `:603-631`). The list passed
   to `apply_output_configs` is the stored entries followed by these head
   entries, unsuperseded (`:637-664`). Identifiers still merge last (1.4), so
   a stored identifier entry beats the head's value in the immediate result:
   with a stored identifier `scale 2`, a client asking for scale 1.5 on that
   output gets 2.
2. **Store on success.** Only after the commit succeeds is each head entry
   stored with `store_output_config` (`:667-670`), which supersedes the
   identifier entry's fields for that output and its sharers (1.3). The
   pending delayed modeset is then cancelled (`:678-683`) and no new one is
   requested, so the stored result shows at the next modeset, whatever
   triggers it.

A **disabled** head sets only `enabled = 0` and returns (`:610-613`). An
enabled head sets `enabled`, `width`, `height` and `refresh_rate` (from the
mode or custom mode), `x`, `y`, `transform`, `scale` and `adaptive_sync`
(`:615-631`); it does not set `custom_mode`, `drm_mode`, `power` or
`render_bit_depth`. Reload discards head entries like any other runtime entry.

### 1.6 `toggle` (`commands/output/toggle.c`)

1. A `*` selector returns `CMD_INVALID` "Cannot apply toggle to all outputs."
   (`:12-15`).
2. `sway_output = all_output_by_name_or_id(oc->name)`, the first match in
   `all_outputs` order, disabled outputs included (`:17`). Without a match the
   command returns `CMD_FAILURE` "Cannot apply toggle to unknown output %s"
   (`:19-22`).
3. `oc = find_output_config(sway_output)` merges the **stored** entries for
   that one output (`:24`). This reads configuration, not live state, and it
   does not include the command being built.
4. If `enabled != 0` (an unset `-1` counts as enabled), the pending entry gets
   `enabled = 0`, otherwise `1` (`:26-30`).
5. That one bit goes on the entry **named as typed**. Through 1.4 it reaches
   every output the selector names exactly.

The oracle comment on `output_identifier_unknown` (`scenarios.toml:307-312`)
restates this rule.

### 1.7 `power` / `dpms` toggle (`commands/output/power.c`, `dpms.c`)

`dpms` is an alias that logs a deprecation (`dpms.c:4-8`). For `power toggle`
(the argument is compared with `strcasecmp`, `power.c:16`):

1. A `*` selector returns `CMD_INVALID` "Cannot apply toggle to all outputs"
   (`:18-21`).
2. `sway_output` is the first match in `all_outputs` (`:23`), disabled outputs
   included. Without a match the command returns `CMD_FAILURE` "Cannot apply
   toggle to unknown output %s" (`:24-27`).
3. `current = true`, unless the output is a sway output whose wlr output is
   off, in which case `current = false` (`:15`, `:29-31`). So a **disabled**
   output reads as `current = true`, and toggling it stores `power = 0`.
4. `parse_boolean("toggle", current)` returns `!current`
   (`common/util.c:49-50`). The result is stored as `power` on the typed entry
   (`:34-38`), **resolved once**, and applies to every exact match through 1.4.

### 1.8 `all_outputs` order and lookups

- `output_match_name_or_id` returns true for `*` (compared with `strcmp`), and
  otherwise compares both the identifier and the connector name with
  **`strcasecmp`** (`desktop/output.c:43-53`). It has no special case for
  missing fields.
- `output_by_name_or_id` walks the enabled outputs in `root->outputs`
  (`:55-63`). `all_output_by_name_or_id` walks `root->all_outputs` (`:65-73`).
- `output_create` inserts every new output at the **head** of `all_outputs`
  (`tree/output.c:141`), so the list runs newest first. `output_disable`
  removes an output only from `root->outputs` (`tree/output.c:279-299`,
  `:293`). `begin_destroy` removes it from `all_outputs` when it is unplugged
  (`desktop/output.c:448`). `FALLBACK` is created at `server_init`
  (`server.c:690-693`), so it is always last.
- Oracle evidence of the order with `WLR_HEADLESS_OUTPUTS=2`
  (`contrib/sway-ipc-run:1036`): `GET_OUTPUTS` lists enabled outputs from
  `root->outputs`, then the disabled ones in `all_outputs` order
  (`ipc-server.c:678-705`). In `output_identifier_unknown_toggle.outputs.json`
  both outputs are disabled and appear as `HEADLESS-2`, `HEADLESS-1`. So
  `all_outputs` is `[HEADLESS-2, HEADLESS-1, FALLBACK]`. The first match for
  `Unknown Unknown Unknown` is **HEADLESS-2**, the higher-numbered and newer
  output. `output_identifier_unknown_scale.outputs.json` (`HEADLESS-2` first
  among enabled outputs) is consistent with this, because enabling also walks
  `all_outputs` (`config/output.c:1134`, `tree/output.c:156`).

### 1.9 Disabled and offline outputs

- A **disabled** output stays in `all_outputs` (1.8). Lookups for store,
  toggle and power find it, entries are stored and supersede as usual, and the
  effective config applies at the first modeset that enables and powers it
  (1.4). `output X enable` re-enables it unless a higher-precedence entry still
  says `enabled = 0`. A wildcard command reaches it, because `*` matches every
  output (`config/output.c:721`) and `apply_output_configs` walks all of them
  (`:1134`). `GET_OUTPUTS` lists it with `active`, `dpms` and `power` false
  (`ipc-json.c:426-428`).
- An **offline** (unplugged) output is in no list. Its entries are stored, no
  connector-versus-identifier supersede happens (1.3), `toggle` and
  `power toggle` fail with "unknown output", and the entries apply on hotplug
  (1.4).

### 1.10 Case sensitivity at each step

| Step | Comparison | Citation |
|---|---|---|
| Subcommand name | `strcasecmp` | `commands.c:148` |
| `FALLBACK` refusal | `strcasecmp` | `commands/output.c:44` |
| Lookup of the first match (store, toggle, power) | `strcasecmp` on connector and identifier; `strcmp` for `*` | `desktop/output.c:45-52` |
| Merge into an existing entry of the same name | `strcmp` | `config/output.c:258` |
| Connector supersedes identifier | `strcmp` on both names | `config/output.c:273-274` |
| Which entries apply to an output | `strcmp` | `config/output.c:725` |
| `toggle`, `on`, `off` and other boolean arguments | `strcasecmp` | `common/util.c:41-50`, `power.c:16` |

So **lookup ignores case and application does not**.
`output "unknown unknown unknown" scale 1.5` succeeds and changes nothing.
`output "unknown unknown unknown" toggle` finds HEADLESS-2, reads its config,
stores `enabled = 0` under the lowercase name, and also changes nothing.
`output dp-1 scale 2` is a silent no-op on a `DP-1` output.

### 1.11 IPC replies

`cmd_results_to_json` (`commands.c:509-528`) writes
`{"success": status == CMD_SUCCESS}`. When an error is present it adds
`"parse_error": status == CMD_INVALID` and `"error"`.

| Command | Reply |
|---|---|
| any non-toggle selector, matching or not | `[{"success":true}]` |
| `output * toggle`, `output * power toggle` | `success:false, parse_error:true`, "Cannot apply toggle to all outputs." (with a trailing period for `toggle`, without for `power`) |
| `toggle` or `power toggle` on a selector with no match | `success:false, parse_error:false`, "Cannot apply toggle to unknown output <typed>" |
| `output FALLBACK …` (any case) | `success:false, parse_error:false`, "Refusing to configure the no op output" |

### 1.12 Consequences that discriminate models

Under the oracle setup (sway config `output * mode 1270x1408`,
`contrib/sway-ipc-run:1073-1094`). C1-C8 follow from source; the
`output_identifier_unknown` and `output_toggle` captures confirm only the
equal-state and connector cases, and section 6 captures the rest.

- C1: `output "Unknown Unknown Unknown" scale 1.5` scales both outputs. A
  following `output HEADLESS-2 scale 2` gives HEADLESS-2 a scale of 2 and
  **HEADLESS-1 a scale of 1**: the connector command clears `scale` on the
  shared identifier entry (1.3), and HEADLESS-1 has no other scale.
- C2: `output HEADLESS-1 scale 2` followed by
  `output "Unknown Unknown Unknown" scale 1.5` leaves both at 1.5, because the
  identifier tier merges last.
- C3: disabling HEADLESS-2, then `output "Unknown Unknown Unknown" toggle`,
  **enables both**, because the first match, HEADLESS-2, has `enabled = 0`.
  Disabling HEADLESS-1 instead and toggling **disables both**.
- C4: `output HEADLESS-1 power off`, then an identifier `power toggle`, powers
  **both off**: HEADLESS-2, the first match, is on. A second toggle powers both
  on, since the identifier entry `power = 1` merges over HEADLESS-1's
  `power = 0`.
- C5: disabling HEADLESS-2, `output HEADLESS-2 scale 1.5`, then
  `output HEADLESS-2 power toggle` **succeeds** and stores `power = 0`. A later
  `output HEADLESS-2 enable` leaves it active but powered off **at its live
  scale 1**: the stored 1.5 is deferred (1.4). `output HEADLESS-2 power on`
  then applies 1.5.
- C6: `output * enable` re-enables a disabled output.
- C7: `reload` discards every runtime entry. Live state returns to the file's
  configuration and defaults, except a runtime mode on an output without a mode
  list when the file sets no mode (1.5). The oracle file sets `mode`, so the
  oracle cannot see that exception.
- C8: an identifier or connector entry stored before an output exists applies
  when it appears, power included.
- C9: `output HEADLESS-1 mode --custom 800x600@75Hz` followed by
  `output HEADLESS-1 mode --custom 1024x768` keeps 75 Hz, because the second
  command does not set `refresh_rate` (1.2).
- C10: with a stored identifier `scale 2`, an output-management apply of scale
  1.5 to HEADLESS-1 leaves it at 2 until the next modeset, which gives
  HEADLESS-1 1.5 and HEADLESS-2 the default 1 (1.5). With a stored identifier
  `scale 1.5`, a management apply that disables HEADLESS-2 leaves HEADLESS-1 at
  1.5, because a disabled head stores only `enabled = 0`.

## 2. Swayward's current model

### 2.1 Config data: KDL `output` sections

- `Config.outputs: Outputs` (`swayward-config/src/config.rs:11-12`) is
  `Outputs(pub Vec<Output>)` (`swayward-config/src/output.rs:13-14`). Each KDL
  `output "<name>" { … }` node is decoded and appended in file order
  (`swayward-config/src/decode.rs:114-117`). Fields are concrete values, not
  "unset" options: `off: bool`, `transform` defaults to `Normal`, and `scale`,
  `position`, `mode` and so on are `Option`s (`output.rs:50-80`). `mode` is
  one `Option<Mode>` holding size, refresh and the custom flag together. An
  unset `transform` or an unset refresh beside a set size cannot be
  represented, so sway's field-wise merge cannot be done on this type.
- `Outputs::find` and `find_mut` return the **first** section
  `name.matches(&section.name)` accepts (`output.rs:150-156`). A whole section
  wins. Nothing merges, connector and identifier sections are equal, and no
  wildcard exists. An `output "*"` section matches only a connector literally
  named `*`. The translator reports each of these losses
  (`contrib/sway-to-kdl:3056-3140`).
- `OutputName::matches` (`output.rs:196-243`) compares the connector ignoring
  ASCII case (`:197-200`). **If make, model and serial are all `None`, it never
  matches by identifier** (`:202-207`). Otherwise it compares
  `"make model serial"` with `Unknown` substituted, ignoring ASCII case
  (`:209-242`). The guard exists for the temporary all-None `OutputName` that
  `modify_output_config` builds from a typed string
  (`config_reload.rs:524-532`), so that it cannot match an identifier section.
  As a side effect, a headless output can never be matched by
  `Unknown Unknown Unknown`.
- `swayward.config_file_output_config` (`src/swayward.rs:258`) keeps the last
  file-loaded sections.

### 2.2 The runtime command path

`Command::Output { target, actions }`
(`swayward-ipc/src/command/parse/output.rs:56-91`) dispatches to
`output::configure` (`src/command/dispatch.rs:214`). The parser refuses
`toggle` and `power toggle` with `*` (`parse/output.rs:94-99`, `:287-296`). It
has no `-` or `--` alias and no `unplug` subcommand
(`parse/output.rs:43-55`).

`configure` (`src/command/output.rs:56-153`):

1. **toggle** (`:60-78`) uses `toggle_disables` (`:43-54`), which walks
   `backend.ipc_outputs()`. That is a `HashMap<OutputId, _>`
   (`src/backend/mod.rs:40`), so the order is arbitrary. It builds
   `OutputName::from_ipc_output` and calls `.matches(target)`, so the all-None
   guard applies and headless identifiers never match. It reads
   `config.outputs.find(&name).off` (`:53`). With no match it fails with sway's
   message (`:61-66`). The resolved bit replaces every `Toggle` in the command,
   so the last subcommand wins.
2. **targets** (`:86-95`): `*` expands to the **enabled** outputs in
   `global_space`. Anything else is a single target, the typed string.
3. **power** (`:105-131`) runs per target and per action. `output_by_name_match`
   searches `global_space`, enabled outputs only
   (`src/swayward/outputs.rs:1055-1059`, through `utils::output_matches_name`,
   `src/utils/mod.rs:321-324`, with the guard). A `Toggle` with no match fails
   **in the middle of the loop**, after earlier targets already changed
   (`:107-112`). `current` comes from `swayward.output_power[name]`. The key is
   the connector, or the **typed string** when nothing matched (`:113-127`).
4. **other actions** (`:133-143`) call `apply_transient_output_config(target)`
   (`config_reload.rs:551-646`). It goes through `modify_output_config`
   (`:508-549`), which resolves `target` to an `OutputName`: first the live
   output in `global_space`, then a connected but disabled TTY connector
   (`tty.rs:2643-2669`), then a temporary all-None
   `OutputName { connector: target }` (`:524-532`). It edits the **first
   matching KDL section** in place with `find_mut` (`:537`), or appends a
   section named as typed (`:538-546`). A `mode` action replaces the whole
   `Mode`, refresh included (`:563-576`). It then calls
   `reload_output_config()` (`:645`).
5. On/Off/Power actions raise an `output` event (`:96-104`, `:145-151`).

### 2.3 Consumers of the per-output config

Each call site reads one first-match section with `Outputs::find`:

- `src/backend/tty.rs:699` (max bpc), `:1110` (`should_be_off`), `:1285`
  (`connector_connected` mode and VRR), `:2463` (disconnect if `off`), `:2612`
  (connect if not `off`);
- `src/backend/headless.rs:259-265` (`off`, `mode`);
- `src/swayward/outputs.rs:20` (position), `:121` (`add_output` scale and
  transform), `:377` (hot corners);
- `src/swayward/render.rs:1105-1111` (on-demand VRR);
- `src/swayward/config_reload.rs:417` (`reload_output_config` scale,
  transform, backdrop and layout);
- `src/command/output.rs:53` (toggle).

`reload_output_config` (`config_reload.rs:417-453`) and `add_output`
(`outputs.rs:121-157`) apply scale and transform whenever they differ from the
section, whether or not the output is powered. Nothing defers them.

Three more paths are related. `src/swayward/cursor.rs:119-128` scans sections
for `focus-at-startup`. `src/protocols/output_management.rs:424-433` and
`:474-483` build a head's base config from `current_config.find`, where
`current_config` is a copy of `config.outputs` (`config_reload.rs:502-505`).
`src/handlers/mod.rs:927-930` replaces `config.outputs` wholesale with the
applied heads. D-Bus display config also goes through `modify_output_config`
(`src/dbus/mod.rs:76`).

### 2.4 Power on connect

A TTY output's power on connect comes from `output_power[connector]`,
defaulting to on (`tty.rs:1534-1541`). Nothing recomputes it from
configuration when an output appears.

### 2.5 TTY identity of duplicate monitors

When a newly connected monitor has the same make, model and serial as a known
one, the TTY backend clears all three on the new connector's `OutputName`
before storing it in `CrtcInfo` (`tty.rs:1062-1084`), because niri's layout
does not support duplicate names and panics on them. `known_crtc_name` returns
that stripped name (`tty.rs:197-225`). The second monitor's identifier is then
`Unknown Unknown Unknown`. Sway keeps each output's real properties
(`config/output.c:31-38`).

### 2.6 Reload

`reload_config` sets `output_config_changed` only when the file's sections
changed (`config_reload.rs:170-174`). Otherwise it **keeps the in-memory
sections, runtime edits included** (`:175-179`, `:264-266`).
`reset_sway_runtime_state_on_reload` clears `output_power` and powers every
output back on (`:373-399`, `:383-392`). The `reload` command validates and
then hands the file to the watcher (`src/command/session.rs:5-15`), which
delivers it to `reload_config` asynchronously.

### 2.7 What order swayward can offer for `all_outputs`

There is no creation-order list of all known outputs:

- `global_space.outputs()` and `sorted_outputs` (`src/swayward.rs:323`, built
  in `outputs.rs:33-61`) hold enabled outputs only, in placement order.
- `ipc_outputs` is a HashMap (`backend/mod.rs:40`). Its keys are `OutputId`s
  from a monotonic counter (`backend/mod.rs:45-50`).
- TTY assigns the `OutputId` when the DRM scan reports the connector connected
  (`tty.rs:1009`, stored in `CrtcInfo { id, name }`, `tty.rs:152-155`,
  `:1090`) and drops it on disconnect (`:1057`). That matches sway's "new wlr
  output on connect" up to the order inside one scan.
- Headless keeps `Headless::outputs: Vec<Output>` in creation order,
  **disabled outputs included** (`headless.rs:244-256`). Startup outputs are
  created in reverse, `headless-2` before `headless-1`
  (`headless.rs:95-105`), so that placement matches sway (`outputs.rs:33-48`).
  Headless connectors are named `headless-<n>` with a monotonic `n`
  (`headless.rs:107-124`), which mirrors wlroots' `HEADLESS-<n>` counter.

For headless, **descending `n`** reproduces sway's newest-first order both for
startup outputs (giving `[headless-2, headless-1]`, which matches 1.8) and for
`create_output`. Descending `OutputId` does not, because startup creation is
reversed.

### 2.8 Headless spelling

Swayward's headless connectors are `headless-N` (`headless.rs:123`). sway's are
`HEADLESS-N`. The oracle normalises output names (`normalize.toml:23`). It also
sends scenario commands verbatim: only `fake-N` is translated
(`contrib/sway-ipc-run:918-924`, `:1096-1097`, `:1132-1133`). So swayward
receives `output HEADLESS-2 …` for its `headless-2` output, while its own
`GET_OUTPUTS` reports `headless-2`. Its own tests type `headless-N` in 24
runtime `output` commands (`src/tests/ipc/outputs.rs` and five other files
under `src/tests/ipc/`). The oracle's swayward config declares
`output "headless-N" { mode custom=true "1270x1408@60"; scale 1; }`
(`contrib/sway-ipc-run:1112-1130`).

## 3. Gap table

| # | Sway rule | Swayward today | User-visible difference |
|---|---|---|---|
| G1 | Entry stored under the typed name and kept (1.2, 1.3) | Edits the first matching KDL section or appends one (`config_reload.rs:508-549`) | The selector's identity is lost. Effects depend on section order. |
| G2 | Effective config merges `*` < connector < identifier, field by field (1.4) | First matching section wins whole (`output.rs:150-156`) | `output "<id>" pos 0 0` after a connector section with `scale 2` silently drops one of the two, depending on file order. |
| G3 | A connector command supersedes its identifier entry; a wildcard supersedes all (1.3) | No supersede | After C1, swayward keeps HEADLESS-1 at 1.5 where sway reverts it to 1. |
| G4 | The identifier of a headless output is `Unknown Unknown Unknown` (1.1) | The all-None guard rejects it (`output.rs:202-207`) | `output "Unknown Unknown Unknown" scale 1.5` does nothing, and `… toggle` fails with "unknown output". Oracle row `output_identifier_unknown` mismatches on `main`. |
| G5 | `*` matches every output, including disabled and future ones (1.9) | Expands to enabled outputs (`command/output.rs:86-91`) | `output * enable` does not re-enable a disabled output (C6), and `output * scale 2` misses outputs added later. |
| G6 | `toggle` takes its bit from the first match in `all_outputs`, newest first (1.6, 1.8) | First match in HashMap order (`command/output.rs:44-53`) | With unequal states (C3), the result is arbitrary. |
| G7 | That bit is stored on the typed entry and applies to all exact matches (1.6) | Written to one section | An identifier toggle changes at most one output's section. |
| G8 | `power toggle` resolves once from the first match, disabled outputs included (1.7) | Resolved per target, enabled outputs only, failing mid-loop (`command/output.rs:105-131`) | C4 gives mixed results, and C5 fails with "unknown output" where sway succeeds. |
| G9 | `power` is a stored field that merges and supersedes by selector (1.3, 1.7) | `output_power[connector or typed string]` (`command/output.rs:113-127`) | An identifier `power off` affects only the first enabled match, or writes a dead key. |
| G10 | Lookup ignores case; storage and application use `strcmp` (1.10) | ASCII-case-insensitive everywhere | `output dp-1 scale 2` applies in swayward and is a no-op in sway, and likewise for lowercase identifiers. |
| G11 | An unmatched selector applies later only on an exact match (1.4) | Appended section matches later case-insensitively, by connector or by identifier for outputs with EDID | Applies to outputs sway would not configure. |
| G12 | Reload discards runtime entries (1.5) | Kept when the file's sections are unchanged (`config_reload.rs:175-179`) | After `reload`, a runtime-disabled output stays disabled in swayward (C7). |
| G13 | A command is atomic (1.2) | A power-toggle failure leaves earlier targets changed (`command/output.rs:107-112`) | Partial application. |
| G14 | Output-management and power-management requests are stored as connector entries, after a temporary apply (1.5) | Replaces `config.outputs` wholesale (`handlers/mod.rs:927-930`) | A difference in reload survival today, and a regression risk under any layered design (4.10). |
| G15 | Mode, transform and scale are deferred while disabled or powered off (1.4) | Applied directly (`config_reload.rs:417-453`, `outputs.rs:121-157`), and before the backend disable at `:494` | C5: swayward would report the stored scale on a powered-off output where sway reports the old live scale. A command that sets scale and disables changes the live scale in swayward only. |
| G16 | Mode size, refresh and custom flag are separate fields (1.2, 1.3) | `mode` replaces the whole `Mode` (`config_reload.rs:563-576`) | C9: a second `mode` without a rate drops the earlier rate. |
| G17 | `FALLBACK` refused; `-` and `--` aliases; `unplug` | Not implemented | Different replies for those inputs. Out of scope (4.14). |

## 4. Proposed design

### 4.1 Two layers

1. **File layer**: the KDL sections, unchanged, with niri's first-match
   semantics: `Outputs::find` and `OutputName::matches` with its guard. This is
   swayward's config language, not sway's config file.
2. **Runtime layer**: sway's `output_configs` list, holding only entries
   created by runtime `output` commands and by output-management applies. It is
   stored, superseded and resolved exactly as in 1.3-1.4.

The effective config of an output is the file layer's first matching section,
with the runtime layer's resolved entry merged on top.

This is a KDL-compatibility policy, not a reproduction of sway's single store.
For an output that exists when the command runs, putting the runtime layer on
top gives the same answer as sway's supersede rules (the most recent command
naming the output wins over the file, 1.3, 1.5), except where the file layer
would have been superseded or merged (4.14 items 1 and 2, B9, B10).

The alternative, one sway-shaped store that reload rebuilds from the KDL file,
is simpler semantically but changes KDL first-match behaviour for existing
configs. It is not an equal substitute while that behaviour is kept, and
whether to keep it is decision B9.

### 4.2 Types: new file `swayward-config/src/sway_output.rs`

The file is new, so it needs no divergence entry for its contents.

```rust
/// One stored `output <name> …` command, sway's `struct output_config`
/// restricted to the fields swayward's parser accepts. `None` = unset.
/// Mode fields are independent, as in sway (config/output.c:90-119, 168-194).
#[derive(Debug, Default, Clone, PartialEq)]
pub struct SwayOutputConfig {
    pub name: String,                       // exactly as typed
    pub enabled: Option<bool>,              // enable / disable / toggle
    pub size: Option<(u16, u16)>,           // width and height; every source sets both
    pub refresh: Option<f64>,               // only `mode …@<rate>Hz` and management heads
    pub custom_mode: Option<bool>,          // every `mode` command sets it (mode.c:15-21)
    pub modeline: Option<Option<Modeline>>, // modeline = Some(Some); mode = Some(None) (mode.c:23-24)
    pub position: Option<Position>,
    pub scale: Option<f64>,
    pub transform: Option<Transform>,
    pub vrr: Option<Option<Vrr>>,           // adaptive_sync
    pub max_bpc: Option<MaxBpc>,            // render_bit_depth
    pub power: Option<bool>,                // power / dpms
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct SwayOutputConfigs(Vec<SwayOutputConfig>);
```

Functions:

- `SwayOutputConfig::from_actions(name, &[OutputAction])` applies the actions
  in order, mirroring sway's sequential handlers. `Toggle` and
  `Power { Toggle }` are resolved before this call (4.4). `Mode` and
  `CustomMode` set `size`, `custom_mode` and `modeline = Some(None)`, and set
  `refresh` only when the parsed mode carries one. If the sway parser can emit
  `ModeToSet::Automatic` (sway has no such form), refuse it there rather than
  invent a mapping.
- `from_head(section, enabled) -> SwayOutputConfig` for output management
  (4.10). A disabled head sets only `enabled = Some(false)`. An enabled head
  sets `enabled`, `size`, `refresh`, `position`, `transform`, `scale` and
  `vrr`, and nothing else (`desktop/output.c:603-631`).
- `merge(&mut self, src)` and `supersede(&mut self, src)` mirror
  `config/output.c:164-239` and `:86-161`, field by field.
- `apply_to(&self, out: &mut Output)` writes every set field onto a KDL
  `Output`. `enabled` maps to `off = !enabled`. The mode fields are merged into
  the section's `Mode` one at a time: a set `size` with an unset `refresh`
  keeps the section's refresh, and so on.
- `identifier(name: &OutputName) -> String` is
  `name.format_make_model_serial()` (`output.rs:189-194`), which already
  substitutes `Unknown` like `output_get_identifier`, with no guard.
- `lookup_matches(name: &OutputName, target: &str) -> bool` is sway's
  `output_match_name_or_id`: `target == "*"`, or ASCII-case-insensitive
  equality with the connector or the identifier. It has no all-None guard. It
  is used only by the runtime path. `OutputName::matches` and
  `utils::output_matches_name` are not touched, so workspace output
  assignments, window rules and other niri consumers keep their behaviour.
- `same_selector(entry: &str, typed: &str) -> bool` is `entry == typed`, or
  both are exact spellings of one headless connector (4.11).
- `names_output(entry: &str, name: &OutputName) -> Tier` returns `Wildcard`,
  `Connector` or `Identifier` by **exact** comparison, or none. `Connector`
  accepts swayward's spelling and the wlroots spelling (4.11).
- `SwayOutputConfigs::store(&mut self, oc, first: Option<&OutputName>)` is a
  line-by-line port of `config/output.c:241-294`, with `same_selector` in place
  of the `strcmp` at `:258` and `names_output(.., first) == Connector` in place
  of `:274`.
- `SwayOutputConfigs::resolve(&self, name) -> SwayOutputConfig` is
  `find_output_config_from_list`: the wildcard tier, then the connector tier,
  then the identifier tier, each in list order.
- `SwayOutputConfigs::is_empty()`.
- `impl Config { pub fn output_config(&self, name: &OutputName) -> Option<Output> }`:
  it returns `None` when neither layer matches. Otherwise it starts from the
  first matching file section, cloned, or from
  `Output { name: connector, ..Default }`, and calls
  `self.sway_outputs.resolve(name).apply_to(..)`. This `impl` block lives in
  `sway_output.rs`. `output_power(&self, name) -> bool` returns the resolved
  `power`, defaulting to on.

`swayward-config/src/config.rs` gains one field,
`pub sway_outputs: SwayOutputConfigs`, which defaults to empty and is never
decoded from KDL. `swayward-config/src/lib.rs` gains `mod sway_output;` and a
`pub use`.

### 4.3 Order: swayward's `all_outputs`

Add `Backend::known_outputs_newest_first(&self) -> Vec<OutputName>` in
`src/backend/mod.rs`, dispatching to:

- **Headless** (`headless.rs`): `self.outputs`, which includes disabled
  outputs, each mapped to its `OutputName` from user data. Do not use
  `from_ipc_output`, because the IPC copy drops the serial (`headless.rs:195`).
  Sort by the `n` of `headless-<n>`, descending. Connectors that do not parse,
  which are test-only named outputs, follow in reverse creation order.
  Rationale: 2.7 and the oracle order in 1.8.
- **TTY** (`tty.rs`): connected connectors with a known CRTC, enabled or not,
  as `(CrtcInfo.id, known_crtc_name(..))`, sorted by `OutputId` descending.
  This matches sway's newest-first `output_create`, except within a single
  scan (4.14). The names are the stripped ones of 2.5, so a duplicate monitor
  appears with identifier `Unknown Unknown Unknown` (B7).
- **Winit**: its single output.

No new stored list is needed. All three backends already hold creation
information.

### 4.4 Command flow (`src/command/output.rs::configure`, rewritten)

This file is swayward's own, so it needs no ledger entry.

1. `known = backend.known_outputs_newest_first()`.
   `first = (target != "*").then(|| known.iter().find(|n| lookup_matches(n, &target)))`.
   This is sway's `all_output_by_name_or_id`.
2. **toggle**: when `first` is `None`, fail with
   `Cannot apply toggle to unknown output {target}`, as today. Otherwise
   `disables = !config.output_config(first).is_some_and(|o| o.off)`, which
   reads the stored configuration of the first match, both layers, not the
   pending command (`toggle.c:24-30`). Replace every `Toggle` with `Off` or
   `On`, as today (`:68-77`).
3. **power toggle**: when `first` is `None`, use the same failure.
   `current = !(first is in global_space && output_power[first.connector] == Some(false))`
   (`power.c:29-31`), so a disabled output reads as on. Replace every
   `Power { Toggle }` with `Power { Enable/Disable }` set to `!current`.
   Steps 2 and 3 run **before anything is stored**, so a failure leaves no
   partial state (G13).
4. `oc = SwayOutputConfig::from_actions(target, &actions)`. Then
   `config.sway_outputs.store(oc, first)`. A non-toggle selector that matches
   nothing is stored and succeeds (1.2).
5. **Power**: for every `n` in `known`, `p = config.output_power(n)`. Write
   `output_power[n.connector] = p`, removing the key when `p` is true and no
   entry set it. For enabled outputs whose value changed, call
   `backend.set_output_power` and queue a redraw when the output turns on
   (`:128-131`). Typed strings are never inserted as keys.
6. If the resolved `enabled` of any known output became `false`, call
   `state.ipc_suppress_workspace_moves()`, which keeps the intent of
   `:79-85`.
7. `state.reload_output_config()`. Its consumers now read `output_config`
   (4.5) and defer as in 4.6, and `backend.on_output_config_changed` enables
   or disables headless and TTY outputs from it (`config_reload.rs:494`).
8. Output events are unchanged (`:96-104`, `:145-151`).

`toggle_disables` is replaced by step 2. `apply_transient_output_config` is no
longer called from the sway path. It is left in place, unedited, in the
inherited `config_reload.rs`, to keep the rebase surface small.
`modify_output_config` stays for the D-Bus path.

### 4.5 Consumers read the effective config

Replace each `config.outputs.find(name)` in 2.3 with
`config.output_config(name)` (owned). That is `.cloned().unwrap_or_default()`
→ `.unwrap_or_default()`, and `Option<&Output>` → `.as_ref()`:

- `tty.rs:699`, `:1110`, `:1285`, `:2463`, `:2612`;
- `headless.rs:263`;
- `outputs.rs:20`, `:121`, `:377`;
- `render.rs:1110`;
- `config_reload.rs:417`.

Each swap is one or two lines. `render.rs:1110` runs per frame for VRR outputs.
Cloning one `Output` there is negligible, and `output_config` can
short-circuit to a borrowed section when `sway_outputs` is empty if a profile
ever shows the cost.

`cursor.rs:119-128` (`focus-at-startup`) is a startup-only file-layer query and
stays as it is.

Entries stored for offline outputs apply on hotplug, because
`connector_connected` (`tty.rs:1285`), `should_be_off` (`:1110`) and headless
`add_output` (`outputs.rs:121`) read the effective config (C8), except power
(4.14 item 4).

### 4.6 Deferred application while disabled or powered off

Lookup swaps alone keep G15. Sway skips mode, transform and scale for an output
whose resolved `enabled` is false or whose resolved `power` is false
(`config/output.c:478-493`), so:

- `reload_output_config` (`config_reload.rs:417-453`) and `add_output`
  (`outputs.rs:121-157`) leave the live scale and transform untouched for an
  output whose resolved `enabled` **or** `power` is false. The `enabled` guard
  matters even though disabled outputs leave `global_space`:
  `reload_output_config` updates scale and transform at `:417-453`, before the
  backend disables the output at `:494`, so `output X scale 2 disable` would
  otherwise change the live scale of an output sway leaves alone. The headless
  and TTY mode paths (`headless.rs:259-265`, `tty.rs:1285`) apply the same
  two guards to the mode.
- An output re-enabled with power on gets its stored mode, transform and
  scale at that point, including values set while it was disabled.
- An output being re-enabled while powered off enters the layout at its
  previous live scale, transform and mode. For headless, the Smithay `Output`
  in `Headless::outputs` survives disable (2.7), so its last values are still
  there. On TTY, a re-connected output has no previous live state; use the
  connector's current DRM mode and scale 1, which is what sway's wlr output
  holds at that point. Confirm by reading wlroots before relying on it.
- Turning power on (4.4 step 5) must run `reload_output_config` after
  updating `output_power`, so the deferred values apply then (C5).

This touches inherited code beyond the lookup swaps and is the main reason the
size estimate in 7.4 is soft.

### 4.7 Disabled and offline outputs

- Disabled outputs are in `known` (4.3), so `toggle`, `power toggle`,
  supersede and `*` reach them (C5, C6). `on_output_config_changed` re-adds an
  output when its effective `off` clears (`headless.rs:259-282`,
  `tty.rs:2595-2625`).
- Offline outputs: entries are stored, no supersede happens, toggles fail, and
  the entries apply on connect (4.5). This matches 1.9, except power on
  connect (4.14 item 4).

### 4.8 Reload

In `reload_config`, before `*old_config = config` (`config_reload.rs:263`),
add: `if !old_config.sway_outputs.is_empty() { output_config_changed = true; }`.
The swap itself drops the runtime layer, because a fresh `Config` has an empty
`sway_outputs`. This is sway's discard (`config.c:465-471`).
`reload_output_config` then reverts the outputs. `output_power` is already
cleared (`:383-392`). A failed reload returns before the swap and keeps the
runtime layer (`:72-83`), as sway keeps its old config when validation fails.

Swayward's headless outputs need no mode-list special case: the oracle
swayward config sets a mode for every headless output (2.8), and the exception
in C7 is not observable through the oracle.

The `preserved_output_config` branch (`:175-179`) stays. After 4.10 it
protects only D-Bus edits.

### 4.9 Power state beyond commands

`output_power` stays the render and backend source of truth (`render.rs:968`,
`tty.rs:1535`, `:1729`, `ipc/tree/outputs.rs:75`), recomputed from the runtime
layer after every store (4.4 step 5). Keys are never removed on unplug, so
`output_power_survives_replug_and_idle_wake`
(`src/tests/ipc/outputs.rs:674-697`) still holds for connector entries.

### 4.10 wlr-output-management

The layered design would otherwise regress. A runtime `output DP-1 scale 2`
followed by a wdisplays or kanshi apply of scale 1.5 to DP-1 works today,
because both write the same KDL section. With the runtime layer on top, the
client's value would be shadowed. Mirror sway's two stages (1.5):

- `config_reload.rs:502-505` gives output management the **effective**
  sections. For each `n` in `known`, it passes `output_config(n)` renamed to
  `n.connector`, followed by the file sections. The base that
  `output_management.rs:424-433` and `:474-483` read then includes runtime
  state.
- `handlers/mod.rs:927-930` `apply_output_config(config)` no longer replaces
  `config.outputs`. For each applied section it builds
  `SwayOutputConfig::from_head(section, !section.off)`, named by the head's
  connector. The mask matters: the section carries inherited fields from the
  base, and storing them for a disabled head would supersede shared identifier
  fields and change the head's sharers (C10). Then:
  1. Apply a temporary list: clone `sway_outputs`, append the head entries
     unsuperseded, swap the clone in, run `reload_output_config`, and swap the
     stored list back. A runtime identifier entry still beats the head's value
     in this result, as in sway.
  2. Store each head entry with `store(oc, Some(head))`. Do not reapply. The
     stored result shows at the next `reload_output_config`.

Swayward runs `reload_output_config` from more places than sway runs
modesets, so the window in which the live and stored results differ will not
match sway exactly. Matching sway here also means a kanshi apply can appear
not to take effect until something else triggers a modeset. Both are part of
sign-off B3; the simpler option, storing first and applying the stored result,
is not sway's behaviour and should be recorded as a deviation if chosen.

### 4.11 Case handling and the headless spelling

- Lookup (`lookup_matches`) ignores ASCII case, like `strcasecmp` in the C
  locale.
- Store-merge, supersede and resolve compare **exactly**, like `strcmp`. Sway's
  lowercase no-ops are reproduced (1.10, G10).
- For a swayward headless connector `headless-N`, the exact connector tier
  accepts **both** `headless-N` (what swayward's `GET_OUTPUTS` reports, so
  scripts that round-trip names keep working) and `HEADLESS-N` (wlroots'
  spelling, which the oracle sends verbatim, 2.8). `same_selector` treats the
  two as one name, so a later `headless-1 scale 3` merges into an earlier
  `HEADLESS-1` entry instead of creating a second entry that list order could
  reorder. `Headless-1` matches neither. On DRM there is one spelling, and the
  comparison is plain `strcmp`.
- The KDL layer keeps niri's case-insensitive matching.

Renaming swayward's headless connectors to `HEADLESS-N`, about 2,600
occurrences across `src/tests` and snapshots, would remove the alias. That
rename is out of scope.

### 4.12 IPC replies

| Case | Reply | Change |
|---|---|---|
| Any non-toggle selector, matching or not | `[{"success":true}]` | Unchanged |
| `* toggle`, `* power toggle` | Parse error from `parse/output.rs:94-99`, `:287-296` | Unchanged |
| Toggle or power toggle with no match among known outputs, disabled included | `success:false, parse_error:false`, "Cannot apply toggle to unknown output <typed>" | Now also matches headless identifiers, and disabled outputs for `power` |
| Power toggle on a disabled output | `success:true` | Was a failure |

### 4.13 Inherited files and the divergence ledger

Inherited files edited:

- `swayward-config/src/config.rs` (one field) and `swayward-config/src/lib.rs`
  (`mod` and `pub use`);
- `src/backend/mod.rs`, `src/backend/headless.rs`, `src/backend/tty.rs` (order
  helpers, `find` swaps, deferred mode);
- `src/swayward/outputs.rs`, `src/swayward/render.rs`,
  `src/swayward/config_reload.rs` (`find` swaps, deferred scale and transform,
  reload flag, management base);
- `src/handlers/mod.rs` (the management apply).

Not edited: `swayward-config/src/output.rs`, `src/utils/mod.rs`,
`src/protocols/output_management.rs` and `src/dbus/mod.rs`.

Add one `docs/data/divergence/output-runtime-selector-layer.toml` with one
`[[edit]]` listing those paths. `what`: sway's typed output entries are layered
over the KDL sections, consumers read the effective config, and mode, scale
and transform are deferred while an output is powered off. `why`: cite
`config/output.c:241-294`, `:490-493`, `:703-732` and `config.c:465-471`. Run
`./contrib/check-divergence`.

### 4.14 Deliberately not covered

1. **KDL sections keep niri semantics**: first match wins whole, no merge, no
   `*`, and the all-None guard stays (5, B9, B10). Making KDL sway-shaped is a
   separate decision.
2. **The file layer is never superseded.** A runtime connector command does not
   clear a KDL identifier section's field for other outputs sharing that
   identifier. The divergence is visible only with two outputs of identical
   identifier plus a KDL identifier section. Likewise, a runtime connector
   command issued while the output is unplugged still wins over a KDL
   identifier section on hotplug, whereas sway's file identifier entry would
   win (1.3).
3. `FALLBACK` refusal, the `-` and `--` aliases, and `unplug`, along with
   subcommands swayward does not parse (`bg`, `subpixel`, `scale_filter`,
   `max_render_time`, `allow_tearing`, `color_profile`, `hdr`).
4. **Hotplug power**: an output that appears after `output * power off` (or a
   stored identifier or connector `power off`) comes up powered on, because
   `output_power` is recomputed only in 4.4 step 5 and TTY connect reads it
   with a default of on (`tty.rs:1534-1541`). Sway applies stored power to new
   outputs (`desktop/output.c:600`, `config/output.c:490-493`, `:1134-1142`).
   Closing it means recomputing `output_power` for a new output before
   `tty.rs:1534-1541` and headless `add_output` read it. Not in the estimate;
   oracle row `output_selector_power_on_create` (6.2) measures it.
5. **Order within one TTY scan**: swayward assigns `OutputId`s in
   `DrmScanEvent` order (`tty.rs:994-1011`), while wlroots creates outputs in
   connector order. No oracle row can observe this, because the oracle is
   headless.
6. **D-Bus display config** (`dbus/mod.rs:76`) still edits KDL sections, so a
   runtime entry for the same field shadows it.
7. **Output events**: sway emits one `output` event per applied modeset
   (`config/output.c:1095`, `desktop/output.c:399`), and swayward only for on,
   off and power (`command/output.rs:96-104`). Unchanged.
8. **TTY identity**: duplicate monitors lose their identity (2.5), so B7 does
   not hold for them. Separately, identifier strings on DRM depend on
   swayward's EDID decoding producing the same make, model and serial strings
   as wlroots; not examined. Keeping the real identity for the runtime path
   alone (a second, unstripped name in `CrtcInfo`) would fix the first; it
   must not reach the layout, which panics on duplicate names.
9. `utils::output_matches_name` (workspace assignment, window rules,
   `map_to_output`) is unchanged. Changing it globally is not needed here and
   would need its own oracle evidence.

## 5. Behaviour changes for existing configs

| # | Change | Sway-faithful? | Sign-off |
|---|---|---|---|
| B1 | Runtime `output` commands no longer rewrite KDL sections. They live in the runtime layer. Today's results for a single selector per output are unchanged. | Yes | No |
| B2 | Every reload, whether the IPC `reload` or the file watcher on save, now discards runtime disable, scale and position changes. Today they survive while the file's output sections are unchanged (`config_reload.rs:175-179`). Saving an unrelated edit to the KDL file reverts `swaymsg output …` changes. Power already resets on every reload today: `reset_sway_runtime_state_on_reload` clears `output_power` and powers enabled outputs on (`:383-392`, 2.6), so power is unchanged by B2. | Yes for IPC `reload` (`config.c:465-471`). Sway has no file watcher, and swayward already treats watcher reloads as sway reloads for `output_power`, `set` variables and `for_window` (`:373-399`). | **Yes** |
| B3 | wlr-output-management applies (kanshi, wdisplays) become runtime connector entries, applied through a temporary list and stored afterwards (4.10): a runtime identifier entry can shadow the client's value until the next modeset, the stored entry then supersedes the identifier's fields, and reload discards it. A disabled head stores only its disable. Today applies replace `config.outputs` and survive reload while the file is unchanged. | Yes (`desktop/output.c:603-683`) | **Yes**, kanshi users; choose sway's two stages or a recorded deviation |
| B4 | Runtime selectors apply only by exact case. `swaymsg output dp-1 scale 2` becomes a silent no-op, and so does a lowercase identifier. | Yes (`config/output.c:725`) | **Yes**, surprising to users |
| B5 | `headless-N` and `HEADLESS-N` both address a headless output. | Deliberate adaptation (2.8) | Yes, record it in the ledger |
| B6 | `output * …` now applies to disabled outputs, and clears earlier per-output values of the same fields. It applies to outputs added later for every field except `power` (4.14 item 4). | Yes, except hotplug power (`config/output.c:264-269`, `:721`) | No |
| B7 | Identifier selectors match headless and no-EDID outputs (`Unknown …`) and every output sharing the identifier, including later ones. A connector command clears the shared identifier's field for all its sharers (C1). On TTY, a monitor whose make, model and serial duplicate an already connected one is matched only by its connector and by `Unknown Unknown Unknown` (2.5). | Yes, except TTY duplicates | No |
| B8 | `toggle` and `power toggle` by identifier resolve once from the newest matching output and apply to all matches. `power toggle` on a disabled output now succeeds. | Yes | No |
| B9 | A KDL file with **both** a connector section and an identifier section for one output is unchanged: the first section wins whole. Sway would merge them field by field with the identifier on top. A runtime command naming either selector now applies on top of whichever section wins, rather than editing that section. | No for the file layer, unchanged from today. The translator already reports it (`contrib/sway-to-kdl:3056-3140`). | **Yes**: confirm the KDL layer stays niri-shaped, or schedule a separate sway-merge change for KDL |
| B10 | A KDL section named `Unknown Unknown Unknown` still matches no headless or no-EDID output (the guard, `output.rs:202-207`), while the same runtime selector does. | No for the file layer, unchanged | Yes, note it in the KDL docs or remove the guard later |
| B11 | Typed `output_power` keys disappear. `GET_OUTPUTS` and `GET_TREE` power fields come from resolved entries. | Yes | No |
| B12 | Mode, scale and transform set while an output is disabled or powered off take effect when it is enabled and powered on (4.6). | Yes (`config/output.c:490-493`) | No |

## 6. Oracle evidence plan

### 6.1 Existing rows (oracle `db931de8`, `sway-ipc/scenarios.toml`)

All four have `outputs = 2`.

- `disabled_output_evacuation` (`:290-296`): `window:fixture-1`,
  `command:move workspace to output right`, `command:output HEADLESS-2 disable`,
  `capture:disabled_output_evacuation`. Connector disable.
- `output_toggle` (`:298-305`): `window:fixture-1`,
  `move workspace to output right`, `output HEADLESS-2 toggle`,
  `capture:output_toggle`, `output HEADLESS-2 toggle`,
  `capture:output_toggle_reenable`. Connector toggle in both directions.
- `output_identifier_unknown` (`:313-320`): `window:fixture-1`,
  `output "Unknown Unknown Unknown" scale 1.5`,
  `capture:output_identifier_unknown_scale`,
  `output "Unknown Unknown Unknown" toggle`,
  `capture:output_identifier_unknown_toggle`. Covers G4 and G7 with equal
  states only. It mismatches on `main`, and `KNOWN_DEVIATIONS.md` records it.
  Matching it is not enough: it cannot see the constraints in section 0.
- `output_config_live_changes` (`:322-329`): HEADLESS-1 scale, transform,
  position and custom mode, then HEADLESS-2 disable, combined subcommands, a
  disable and enable switch, re-enable, `output * power off`,
  `output * power on`. Covers single-selector state and wildcard power.

Captures record the cumulative command replies as well as tree, workspaces and
outputs (`contrib/sway-ipc-run:2443-2444`, `:2472-2478`).

### 6.2 New focused rows, to capture from pinned sway 1.12 before implementing

Common fields: `corpora = ["state"]`, `requires = ["sway-command-language"]`,
`not_applicable = true`, and a `reason` citing the i3 command table and the
sway file. Commands use literal `HEADLESS-N`, like the existing rows. The
predictions follow from section 1, and the capture decides.

```toml
# C3, order and toggle. First match = HEADLESS-2 (all_outputs newest first).
[[scenario]]
name = "output_selector_toggle_first_disabled"
outputs = 2
actions = ["window:fixture-1",
  "command:output HEADLESS-2 disable", "capture:output_selector_toggle_first_disabled_before",
  "command:output \"Unknown Unknown Unknown\" toggle", "capture:output_selector_toggle_first_disabled",      # both enabled
  "command:output \"Unknown Unknown Unknown\" toggle", "capture:output_selector_toggle_first_disabled_again"] # both disabled

[[scenario]]
name = "output_selector_toggle_first_enabled"
outputs = 2
actions = ["window:fixture-1",
  "command:output HEADLESS-1 disable",
  "command:output \"Unknown Unknown Unknown\" toggle", "capture:output_selector_toggle_first_enabled"]       # both disabled

# C1, C2: connector and identifier entries for the same output; supersede.
[[scenario]]
name = "output_selector_connector_supersedes_identifier"
outputs = 2
actions = ["window:fixture-1",
  "command:output \"Unknown Unknown Unknown\" scale 1.5", "capture:output_selector_id_scale",               # 1.5 / 1.5
  "command:output HEADLESS-2 scale 2", "capture:output_selector_connector_after_id",                         # H2 2, H1 1
  "command:output HEADLESS-1 scale 1.25",
  "command:output \"Unknown Unknown Unknown\" scale 1.75", "capture:output_selector_id_after_connector"]     # 1.75 / 1.75

# G10: exact-case application.
[[scenario]]
name = "output_selector_identifier_case"
outputs = 2
actions = ["window:fixture-1",
  "command:output \"unknown unknown unknown\" scale 1.5",
  "command:output \"unknown unknown unknown\" toggle", "capture:output_selector_identifier_case"]          # no change, both success

# C4: power toggle with mixed states, resolved once.
[[scenario]]
name = "output_selector_power_toggle_mixed"
outputs = 2
actions = ["window:fixture-1",
  "command:output HEADLESS-1 power off", "capture:output_selector_power_mixed_before",
  "command:output \"Unknown Unknown Unknown\" power toggle", "capture:output_selector_power_mixed_off",     # both off
  "command:output \"Unknown Unknown Unknown\" dpms toggle", "capture:output_selector_power_mixed_on"]       # both on

[[scenario]]
name = "output_selector_power_toggle_first_off"
outputs = 2
actions = ["window:fixture-1",
  "command:output HEADLESS-2 power off",
  "command:output \"Unknown Unknown Unknown\" power toggle", "capture:output_selector_power_first_off"]     # both on

# C5, C6, 1.9: selectors matching a disabled output; deferral; unknown-output replies.
[[scenario]]
name = "output_selector_disabled_output"
outputs = 2
actions = ["window:fixture-1",
  "command:output HEADLESS-2 disable",
  "command:output HEADLESS-2 scale 1.5", "capture:output_selector_disabled_scaled",                       # H2 disabled, scale 1 (1.5 deferred)
  "command:output HEADLESS-2 power toggle", "capture:output_selector_disabled_stored",                    # H2 disabled, success
  "command:output HEADLESS-2 enable", "capture:output_selector_disabled_enabled",                         # H2 active, power false, scale 1 (1.5 deferred)
  "command:output HEADLESS-2 power on", "capture:output_selector_disabled_powered",                       # H2 power true, scale 1.5
  "command:output HEADLESS-9 scale 2",
  "command:output HEADLESS-9 toggle",
  "command:output HEADLESS-9 power toggle", "capture:output_selector_unknown_replies"]                     # success, failure, failure

# G15: mode, transform and scale set while powered off are deferred.
[[scenario]]
name = "output_selector_powered_off_deferral"
outputs = 2
actions = ["window:fixture-1",
  "command:output HEADLESS-1 power off",
  "command:output HEADLESS-1 scale 1.5 transform 90", "capture:output_selector_powered_off_set",         # H1 power false, scale 1, transform normal
  "command:output HEADLESS-1 power on", "capture:output_selector_powered_off_on",                         # H1 scale 1.5, transform 90
  "command:output HEADLESS-1 scale 1.25 disable", "capture:output_selector_disabled_with_scale",          # H1 disabled, scale 1.5 (1.25 deferred)
  "command:output HEADLESS-1 enable", "capture:output_selector_disabled_with_scale_enabled"]              # H1 active, scale 1.25

# C9, G16: mode fields are independent.
[[scenario]]
name = "output_selector_mode_refresh_kept"
outputs = 2
actions = ["window:fixture-1",
  "command:output HEADLESS-1 mode --custom 800x600@75Hz",
  "command:output HEADLESS-1 mode --custom 1024x768", "capture:output_selector_mode_refresh_kept"]        # H1 1024x768, refresh 75000

[[scenario]]
name = "output_selector_wildcard_reaches_disabled"
outputs = 2
actions = ["window:fixture-1",
  "command:output HEADLESS-2 disable",
  "command:output * enable", "capture:output_selector_wildcard_enable",                                    # both enabled
  "command:output HEADLESS-1 scale 1.5",
  "command:output * scale 1.25", "capture:output_selector_wildcard_supersedes"]                            # 1.25 / 1.25

# C8: entries stored before the output exists. 1 output, so create_output
# yields HEADLESS-2 on either headless backend; the explicit mode keeps the
# new output's size identical (sway: `output * mode 1270x1408`; swayward:
# create_output defaults to 1920x1080, headless.rs:107-114).
[[scenario]]
name = "output_selector_applies_on_create"
outputs = 1
actions = ["window:fixture-1",
  "command:output HEADLESS-2 mode 1270x1408 scale 1.5",
  "command:output \"Unknown Unknown Unknown\" transform 90",
  "command:create_output", "capture:output_selector_applies_on_create"]                                     # H1 transform 90; H2 scale 1.5, transform 90

# 4.14 item 4: stored power reaches a new output. Expected to mismatch until
# hotplug power is closed; it measures the gap rather than gating the commit.
[[scenario]]
name = "output_selector_power_on_create"
outputs = 1
actions = ["window:fixture-1",
  "command:output * power off",
  "command:create_output", "capture:output_selector_power_on_create"]                                       # H1 and H2 power false

# C7: reload discards runtime entries.
[[scenario]]
name = "output_selector_reload_discards"
outputs = 2
actions = ["window:fixture-1",
  "command:output HEADLESS-2 disable",
  "command:output \"Unknown Unknown Unknown\" scale 1.5",
  "command:output HEADLESS-1 power off", "capture:output_selector_reload_before",
  "command:reload", "capture:output_selector_reload_after"]                                                 # both enabled, scale 1, power on
```

Rows that need two outputs: all except the two `*_on_create` rows, which start
with one and create the second.

No row covers output management: the runner has no wlr-output-management
action. C10 is proven by headless tests (7.2, T10) until one exists.

Risks to check while capturing:

- `create_output` has no existing row. If its placement or mode fields differ
  for reasons outside this spec, drop those rows and rely on the headless
  tests (7.2, T3).
- Swayward's `reload` is asynchronous (2.6). If the 0.15 s settle
  (`contrib/sway-ipc-run:2473`) proves flaky, insert a `command:nop` before the
  capture or extend the settle in the runner.
- Whether sway's headless backend honours a custom refresh is not checked. If
  `current_mode.refresh` does not move, `output_selector_mode_refresh_kept`
  shows nothing and C9 rests on U8.

Add the rows in an oracle worktree beside the operator's checkout, capture
against the pinned sway, and land them on oracle `main` as one oracle commit.
Do not move `pins.toml` or `tests/oracle.toml` (rule zero). The integrator
then sweeps unchanged swayward `main` on the new oracle commit to form the
baseline. A baseline is valid for one oracle commit.

## 7. Implementation plan

### 7.1 One commit

Title: `output: store runtime output configs by typed selector like sway`.

The body names the oracle commit and the rows: `output_identifier_unknown` and
every `output_selector_*` row above except `output_selector_power_on_create`,
unless 4.14 item 4 is closed in the same commit. Contents:

1. New `swayward-config/src/sway_output.rs` (4.2), plus the field and `mod`
   (`config.rs`, `lib.rs`).
2. `Backend::known_outputs_newest_first` (`backend/mod.rs`, `headless.rs`,
   `tty.rs`).
3. The `configure` rewrite and removal of `toggle_disables`
   (`src/command/output.rs`).
4. The 11 `find` → `output_config` swaps (4.5) and deferred application (4.6).
5. Reload flag and management base (`config_reload.rs`), and the two-stage
   management apply (`handlers/mod.rs`).
6. The divergence TOML (4.13).
7. Tests (7.2) and updates to the existing tests that inspected
   `config.outputs.0` after runtime commands (`src/tests/ipc/outputs.rs:162-173`,
   `:194-206`, `:221-240`), which switch to `output_config`.

### 7.2 Tests

Unit tests in `sway_output.rs`:

- U1: store merges an exact same name, and different case creates a second
  entry.
- U2: a wildcard supersedes only the fields it sets, on every non-wildcard
  entry.
- U3: a connector supersedes the first output's identifier entry only on an
  exact connector spelling and only when `first` is `Some`.
- U4: an identifier supersedes nothing, and resolve order is `*` < connector <
  identifier.
- U5: resolve is exact. `headless-1` and `HEADLESS-1` hit the connector tier,
  `Headless-1` does not, and `same_selector` merges the two spellings.
- U6: `identifier` substitutes `Unknown`, including a headless output with a
  serial.
- U7: `lookup_matches` ignores case, has no all-None guard, and accepts `*`.
- U8: `from_actions` is sequential; `mode` resets `modeline`, sets
  `custom_mode`, and leaves `refresh` unset without a rate, so a merged
  `800x600@75` then `1024x768` keeps 75. Supersede clears only the mode fields
  the source set.
- U9: `from_head` on a disabled head sets only `enabled`.

Headless runtime tests in `src/tests/ipc/outputs.rs`, using
`Fixture::add_output` (creation `1`, `2`, so the newest-first order is
`[headless-2, headless-1]`, as in sway):

- T1: C3 both ways. Disable `headless-2` and toggle the identifier: both
  enabled. Disable `headless-1` instead: both disabled.
- T2: C1 and C2: after a connector command, `headless-1` reverts, and the
  identifier wins after a connector command.
- T3: an identifier scale, then `fixture.add_output(3)`: the new output gets
  the scale. A connector command stored before `add_output(2)` applies on add.
- T4: lowercase identifier scale and toggle succeed and change nothing.
- T5: C4 resolves once. C5: `power toggle` on a disabled output succeeds, and
  after `enable` the output is in the layout with `output_power` false and its
  old scale; `power on` applies the stored scale.
- T6: C6, `output * enable` re-enables a disabled output, and `* scale`
  supersedes per-output scale.
- T7: KDL with `output "Unknown Unknown S1" { scale 2; }` first and
  `output "headless-1" { scale 3; }` second, through `add_output_with_serial`.
  `output headless-1 scale 1.5` gives 1.5.
- T8: KDL `output "HEADLESS-2" { off; }`. `output headless-2 enable` and
  `output HEADLESS-2 enable` both enable it.
- T9: `reload_config(Ok(same config))` after runtime disable, scale and power
  off reverts all three.
- T10: output management through `OutputManagementHandler::apply_output_config`:
  (a) runtime `output headless-1 scale 2`, then an apply of 1.5 gives 1.5;
  (b) runtime identifier `scale 2`, then an apply of 1.5 to `headless-1`
  leaves 2 until the next `reload_output_config`, which gives `headless-1` 1.5
  and `headless-2` 1; (c) runtime identifier `scale 1.5`, then an apply that
  disables `headless-2` leaves `headless-1` at 1.5.
- T11: a failing `power toggle` stores nothing, and the reply strings match
  1.11.
- T12: G15: power off, set scale and transform, check the live values are
  unchanged, power on, check they apply. Then `scale 1.25 disable` in one
  command and, separately, `scale 1.5` on an already disabled output: the live
  scale is unchanged until `enable`, which applies it.

Every test calls `layout.verify_invariants()` after enable and disable.

### 7.3 Gates and evidence

1. `cargo +nightly fmt --all` and `./contrib/check-divergence`.
2. `./contrib/fast-gate` (T0). `src/layout/` is untouched, so the slow layout
   gate is not triggered. The translator and harness paths are untouched, so
   the i3 ratchet is not triggered.
3. T1, cited rows: `contrib/targeted-oracle --baseline <integrator's sweep on
   the new oracle commit> --out-dir ~/hacking/swayward-wm/scratch/<name>
   --scenarios output_identifier_unknown,output_selector_… --run`. The rows
   must match.
4. Regression check: path-based targeted selection fails closed for
   `config_reload.rs`, the backends and `handlers/mod.rs`. Run
   `contrib/oracle-sweep <oracle-worktree> <out-dir>` on the commit, then
   `contrib/oracle-sweep-diff <baseline> <sweep>`. Expect 0 lost and 0
   missing. The existing rows in 6.1, the random corpora (which issue
   `output` commands) and the events corpus must not move.
5. Rebase onto the latest `origin/main` immediately before handing the commit
   over.

### 7.4 Size and risk

| Part | Lines (approx.) |
|---|---|
| `sway_output.rs` code | +190 |
| `sway_output.rs` unit tests | +170 |
| `command/output.rs` | +90 / −80 |
| Backend order helpers (3 files) | +45 |
| 11 consumer swaps | +15 / −15 |
| Deferred application (4.6) | +40 / −10 |
| `config_reload.rs` reload and management base | +15 / −2 |
| `handlers/mod.rs` two-stage apply | +30 / −2 |
| `config.rs` and `lib.rs` | +4 |
| Headless tests and existing-test updates | +300 / −30 |
| Divergence TOML | +12 |
| **Total** | **about +910 / −140 across 14 files** (10 inherited niri files) |

Treat the total as a lower bound. Deferred application and the management
apply are not lookup swaps, and the TTY parts of both have no headless
coverage.

Review risk is high:

- The TTY consumers (`tty.rs:699`, `:1110`, `:1285`, `:2463`, `:2612`), the
  TTY order helper and TTY deferral have no headless coverage. Review them by
  reading, and on hardware if possible.
- B2, B3 and B4 change behaviour users can see and need sign-off before the
  commit lands.
- Output management changes ownership semantics, and its only executable
  evidence is T10. No oracle row can drive wlr-output-management.
- B7 does not hold for duplicate TTY monitors (2.5), and nothing tests it.

### 7.5 Where each section 0 constraint is covered

| Constraint (section 0) | Design | Test | Row |
|---|---|---|---|
| 1. Newest-first first match | 4.3, 4.4 step 1 | T1 | `output_selector_toggle_first_{disabled,enabled}` |
| 2. Power toggle resolved once, disabled outputs included | 4.4 step 3, 4.4 step 5 | T5 | `output_selector_power_toggle_{mixed,first_off}`, `output_selector_disabled_output` |
| 3. Runtime command changes the section lookup uses | 4.1, 4.5: runtime layer on top of the effective section | T7 | KDL-only, so no row; the oracle cannot vary KDL |
| 4. Case-insensitive lookup for offline and disabled outputs | 4.4 step 1 (lookup ignores case), 4.11 | T8 | `output_selector_disabled_output` |
| 5. Selector stays a stored name: later outputs, supersede, exact case | 4.2 typed entries, 4.11 exact application | T3, T4 | `output_selector_applies_on_create`, `output_selector_identifier_case` |
