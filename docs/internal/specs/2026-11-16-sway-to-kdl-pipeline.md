# sway-to-kdl as a reader, lexer and KDL IR pipeline

Status: proposed
Date: 2026-11-16

No translator change lands with this document. It covers `contrib/sway-to-kdl`
only and refines the six-layer target pipeline from the task brief (reader,
lexer, directive AST, handlers, KDL IR, renderer), saying where it departs.
Line references are to `f9063b4c`. Sway references are to sway 1.12
(`8886939`).

## Problem

`Translator.parse` (`contrib/sway-to-kdl:99-467`) is one `if`/`elif` chain of
48 regex branches over the raw, variable-expanded line. The file makes 70
regex calls in total. Arguments are split by hand: `.split()` at `:213`,
`:292`, `:549`, `:589`, `:648`, `:1157`, `:1568` and `:1651`, and `re.split` on
`,` and `;` at `:1143` and `:1754`. Output is indented strings in lists and
dicts. Some of those strings are parsed back later (`smart_borders` at
`:313-318`, outer gaps at `:1393-1405`), and `render()` (`:1771-1858`) joins
them.

The bugs found in this week's review fall into the layers that are missing:

| Symptom | Missing layer |
| --- | --- |
| `output "Some Name With Spaces" scale 2` emits `output "Some"`. `title_format "a, b"` in a `for_window` emits `sway-for-window-command "title_format \"a"` plus a bogus report. In #42, `xcursor_theme 'Bibata' 32` keeps the quotes. | A lexer that follows sway's rules |
| `floating enable;` produced an empty command. `426ea5c7` patched it with a filter at `:1143-1146`. | An `argsep` equivalent |
| Two `resize set` commands in one `for_window` emit `default-column-width` twice (`:1243-1285`), and `swayward validate` rejects the duplicate. | An IR that knows which children are singular |
| `Mode_switch+a` and `Group2+a` pass as modifiers, and `Mod3` is refused. One table (`:14-35`) mixes modifiers with group names. | Sway's modifier list |
| `input type:touchpad { tap enabled }` silently emits an empty `touchpad {}`. A `{` on the next line produces four bogus reports. | A reader that follows sway's line rules |

## Reference semantics

Each layer below ports one of these rules from sway:

- **Lines.** `read_config` (`sway/config.c:732-869`) joins a trailing `\`
  with the next line unless the line starts with `#`, keeping the next line's
  leading whitespace (`getline_with_cont`, `:666-694`). It strips the result
  and skips blank lines and lines starting with `#`; a later `#` is not a
  comment.
- **Blocks.** A line opens a block when its last `split_args` token is `{`
  (`sway/commands.c:355-365`), or when the next nonblank line is exactly `{`
  (`detect_brace`, `config.c:696-717`). A last token of `}` closes the block.
  Inside a block, each line is prefixed with the block header
  (`expand_line`, `config.c:719-729,795-796`). That prefix is the whole block
  model: `mode "x" {` + `bindsym a nop` runs `mode "x" bindsym a nop`.
  `X { a }` on one line is not a block. Its last token is `}`, so sway takes
  it as an unmatched block end (`config.c:842-846`) and applies nothing.
- **Arguments.** `config_command` (`commands.c:343-431`) calls `split_args`. It
  then expands variables in the joined argument string and splits again.
  Finally it unescapes every argument after the first (after the second for
  `set`). Before that it strips quotes from an argument that starts with one,
  except under `exec`, `exec_always`, `mode`, `bind*`, `set` and `for_window`
  (`:409-423`). Those handlers get unescaped arguments with quotes kept;
  `mode` and `bind*` strip the name or key themselves (`mode.c:42`,
  `bind.c:373`). Handler lookup ignores case (`:145-149`).
- **Command lists.** `execute_command` (`commands.c:205-335`) parses criteria
  at the start of each `;` segment. It cuts the list with `argsep(";,")`,
  skips empty segments, and strips quotes from every command except
  `exec`, `exec_always` and `mode`.
- **Modifiers.** Sway's modifiers are Shift, Lock, Control, Ctrl, Mod1, Alt,
  Mod2, Mod3, Mod4, Super and Mod5, matched without case
  (`sway/input/keyboard.c:24-50`). Groups are separate. `Group1` to `Group4`
  match case-sensitively, and only one group may appear. `Mode_switch` sets
  group 2 and then still becomes a key (`sway/commands/bind.c:398-449`).
- **Duplicate bindings.** A later binding overwrites an earlier one
  (`bind.c:280-300`). The translator keeps the first (`:885-887`).

## The pipeline

Each layer has one job. Every function and type below lives in the same file.

### 1. Reader

```python
@dataclass(frozen=True)
class Pos:
    path: Path
    line: int            # first physical line; 0 = whole file (as warn() today)

@dataclass(frozen=True)
class Line:
    text: str              # logical line: continuation joined, stripped
    raw: tuple[str, ...]   # the physical lines it came from, unstripped
    pos: Pos

@dataclass
class Block:
    header: Line           # text without the trailing "{"
    body: list[Line | Block]
    raw: tuple[str, ...]   # header through closing line

def read_config(path: Path) -> list[Line | Block]   # sway's rules
class LegacyReader: ...                             # today's rules; C2 only
```

`read_config` implements the line and block rules above and loses nothing:
each node keeps its physical lines, so a report can echo source exactly. It
does no variable expansion and runs no includes. **This departs from the target pipeline,**
which put `include` and `set` in the reader. Sway runs both as directives, in
source order: `include $file` needs the variables defined above it, and an
included file's variables apply to the lines that follow. Both therefore live
in the front end, step 3.

**The reader also has no one-line blocks,** which the target pipeline listed.
Sway has no such construct, and a translator that accepts one translates
configs that sway rejects. `X { ... }` on one line becomes a report, which
today's output does not give.

**C2 reads with `LegacyReader`.** Today's segmentation differs from sway's,
the goldens record it, and no handler can undo a join the reader already made.
`LegacyReader` is today's loop (`:110-119`) and `block()` (`:681-689`) moved
verbatim: a cursor over physical lines with `next_line()` and
`take_block(first)`, building the same `Line` and `Block`. `read_config`
lands in C2 with unit tests and no caller. C3 switches `Frontend` to it and
deletes `LegacyReader`. Legacy behaviour lives in exactly two places:

- **`LegacyReader`:** a continuation is joined as `text[:-1] + " " +
  next.strip()` (sway keeps the next line's whitespace) and never inside a
  block body. A block opens only where one of today's opener regexes
  (`:121-192`, `:743-753`) matches the expanded line, so the front end calls
  `take_block` through a `LEGACY_OPENERS` table. The block's extent is a raw
  count of `{` and `}` on physical lines, quoted or commented ones included.
  Body lines are stripped physical lines, with blanks and comments kept.
  `Block.raw` is today's `block` list, expanded header first.
- **Handlers, as `legacy_*` helpers:** the `bar` echo `" | ".join(raw)`
  (`:138`), the `mode` handler's re-reading of its body through
  `LegacyReader` and its line offsets (`:737-756`), the unlocated report
  comments (Handlers), and every lexer and IR difference in C3's list.

### 2. Lexer

The lexer ports the five `common/stringop.c` primitives one for one:
`split_args` (`:92-143`), `strip_quotes` (`:26-50`), `argsep` (`:267-312`),
`unescape_string` (`:152-234`) and `join_args` (`:236-253`). It keeps their
quirks: the escape toggle, brackets one level deep, and `\0` truncation.
This follows the repo's convention for sway ports:
`swayward-ipc/src/command/lexer.rs:121-124` already ports `split_args` with a
source citation. Licensing of sway ports is a maintainer decision tracked
separately.

```python
def split_args(s: str) -> list[str]
def strip_quotes(s: str) -> str
def argsep(s: str, delims: str) -> tuple[str, str, str | None]  # segment, matched, rest
def unescape_string(s: str) -> str
def join_args(args: list[str]) -> str

@dataclass
class Command:
    criteria: str | None   # raw "[...]" token, valid for its ';' segment
    argv: list[str]

def split_command_list(s: str) -> list[Command]   # execute_command's loop
```

Unit tests import the script with `importlib.machinery.SourceFileLoader`, since
the file has no `.py` suffix. The case table comes from reading the C code.
For a one-off local check, compile `stringop.c` from the scratch sway checkout
against a small `main`, compare it with the Python port on the case table, and
record the result in the commit message. The check is not committed.

### 3. Front end and `Directive`

```python
@dataclass
class Directive:
    name: str          # argv[0] as written
    argv: list[str]    # after expansion, quotes kept
    args: list[str]    # config_command's view: all unescaped, quotes kept
                       # under exec*, mode, bind*, set, for_window
    text: str          # expanded logical text: today's string, used in messages
    pos: Pos
    block: Block | None

class Frontend:
    variables: dict[str, str]
    visited: set[Path]
    def run(self, path: Path, doc: Document) -> None
```

`Frontend.run` walks the reader output. It handles `set` and `include` itself
and builds one `Directive` for each other line. For a `Block`, it calls the
name's block handler if there is one. Otherwise it flattens the block with
sway's prefix rule and dispatches each line on its own. Criteria and command
lists are parsed on demand by `parse_criteria(token)` and
`split_command_list(...)`, not stored as fields. Only five handlers need them.

### 4. Handlers

```python
Handler = Callable[[Directive, Document], bool]   # False = "unhandled"
BlockHandler = Callable[[Block, list[Directive], Document], None]
HANDLERS: dict[str, Handler]
BLOCK_HANDLERS: dict[str, BlockHandler]   # input, output, mode, bar, for_window
```

**This also departs from the target pipeline:** handlers mutate a `Document`
instead of returning nodes. Several translations read earlier state:
`smart_borders toggle` at `:310-330`, outer-gap merging at `:1393-1410`, and
`titlebar_padding` against border thickness at `:1679-1709`. Sway's own
handlers mutate config state for the same reason.

The block handlers exist only where today's output groups lines from one
block. `input * {` turns into `keyboard` only when every item is `xkb_*` or a
repeat setting (`:1434-1440`). An output block becomes one node. A
`for_window [..] {` block becomes one rule (`:536-544`). A `bar` block becomes
one report.

Reports have one type. The manual item is always `{pos}: {reason}: {text}`
(`pos` is the bare path when `line` is 0). The stdout comment comes in two
shapes today, and C2 reproduces both:

```python
@dataclass(frozen=True)
class Report:
    pos: Pos
    reason: str
    text: str
    located: bool = True       # False: comment omits "{pos}: "

Document.report(r)          # top-level "// sway-to-kdl: ..." plus manual list
Node.report(r, doc)         # comment inside the node plus manual list
```

Top-level reports (`warn`, `:88-92`) and output-block reports
(`:1603-1646`) are located. Input-block reports (`:1483-1550`) and
`layer_effects` reports (`:1765-1767`) are not: the location appears only in
the manual list. Locating every comment is a C3 change. Comments that never
reach the manual list (`// sway-to-kdl: sway ...`, `SwayFX ...`, `satisfied`)
go through `note`. `main()` and its stderr format (`:1861-1875`) do not
change.

### 5. KDL IR

```python
Value = Union[str, int, float, bool, "Raw"]   # Raw: verbatim source token

@dataclass
class Node:
    name: str
    args: list[Value]
    props: list[tuple[str, Value]]     # ordered; repeats allowed, as today
    children: list[Node | Comment]
    inline: bool = False               # `{ fixed 100; }`, bind lines
    def add(self, child): ...          # repeatable
    def set(self, child): ...          # singular: drop same name, append
    def put(self, child): ...          # singular: replace in place
    def get(self, name): ...

SINGULAR: dict[str, frozenset[str]]    # parent -> singular child names
```

A module-level alias is evaluated at import, and `str | int` there needs
Python 3.10, so aliases use `typing.Union`. Annotations may use `|`: the file's
`from __future__ import annotations` (`:4`) keeps them unevaluated on 3.9.

`set` matches the remove-and-append order of `replace_layout_setting`
(`:1350-1356`), `replace_input_setting` (`:619-622`) and the `opacity` and
`open-floating` filters. `put` matches the dicts `blur_settings`
(`:1657-1678`) and `titlebar_colors` (`:656`). Binds stay a keyed map that
keeps the first and reports the rest (`:885-889`).

`SINGULAR` mirrors the `#[knuffel(child)]` fields, as distinct from
`children`, in `swayward-config/src` (for example `window_rule.rs:16-52`). A
test without Cargo reads those attributes from the structs the translator
emits and fails if the table drifts.

`Raw` marks the places that emit source text unchecked: `scale`,
`geometry-corner-radius`, `passes`, `offset`, and generic input values
(`:1554`). Each one is a gap in typed validation. They stay `Raw` until a fix
gives them a type.

`Document` replaces the state dicts in `__init__` (`:38-59`). It holds one
field per output group: `binds`, `modes`, `input`, `blur`, `layout`, a
`Titlebar` builder (rendered into `layout` in its fixed order, as at
`:1814-1842`), `body` (today's `sections`), `workspaces` and `manual`.

### 6. Renderer

`render(doc) -> str` is the only code that formats or quotes. Strings go
through `json.dumps(ensure_ascii=False)`, as `quote` does now (`:61`). Floats
go through `kdl_float` (`:946-949`). Indentation is four spaces per depth.
Groups keep today's order and blank lines. KDL-correct escaping of control
characters is a separate fix.

## Where today's code moves

| Today | Moves to |
| --- | --- |
| `parse` `:99-467` | `read_config`, `Frontend.run`, one handler per `elif` |
| `block` `:681-689` (brace counting) | `LegacyReader.take_block` in C2; `read_config` block stack in C3 |
| `translate_set_block`, `_bind_block`, `_exec_block`, `_for_window_block` `:470-534` | deleted; generic prefix flattening |
| `translate_for_window_commands` `:536` | `for_window` block handler |
| `translate_mode` `:730-768`, with its `self.binds` swap | `mode` block handler; the bind handler takes a target node |
| `expand` `:64-86` | `Frontend.expand`, verbatim |
| `translate_include` `:666-679` | `Frontend` (the refusal of command substitution stays) |
| `warn` / `satisfied` `:88-97` | `Document.report` / `Document.note` |
| `translate_bind` `:770-891` | `h_bind`, with options in `bind.c:353-382` order |
| `MODIFIER_ALIASES`, `translate_modifier`, `translate_key` `:14-35,893-944` | `SWAY_MODIFIERS`, `SWAY_GROUPS`, `KDL_KEY_NAMES`, `translate_key` |
| `translate_command`, `strip_outer_quotes` `:951-973` | rewrites over `Command` |
| `translate_window_criteria`, `render_window_match` `:1008-1087` | `parse_criteria` -> `match` node props |
| `translate_for_window` `:1134-1348` | `h_for_window`, plus a `FOR_WINDOW_COMMANDS` table keyed by command name |
| `split_amounts`, `fixed_resize_set`, `is_move_position_coordinates` | helpers over `argv` |
| `replace_layout_setting`, `replace_input_setting` | `Node.set` |
| `translate_gaps` `:1358-1412` | handler on an `outer-gaps` node; it no longer parses strings back |
| `translate_device_block`, `_input_block`, `_output_block` `:1414-1648` | `input` / `output` block handlers |
| `translate_swayfx`, `translate_layer_effects` `:1650-1769` | handlers registered by name; `layer_effects` uses `split_command_list` |
| `quote`, `kdl_float`, `render` | renderer |

Today the SwayFX names are tried only after every sway branch fails (`:467`).
None of them collides with a sway handler name, so one table preserves the
behaviour.

## Golden-output harness

**Committed goldens.** `contrib/test-sway-to-kdl.py` calls the script from 134
`self.translate(...)` sites and four direct `subprocess.run` sites
(`:1499-1562`). All of them go through one `run_translator(path, env)`. That
function compares stdout and stderr with
`contrib/sway-to-kdl-goldens/<test>.<n>.golden`, where `n` counts calls within
the test. Each file holds the input, stdout and stderr between marker lines.
The translator resolves source paths (`:100`), and the fixture test
(`test-sway-to-kdl.py:1493-1500`) passes checkout paths, so reports would
embed the author's checkout. Two prefixes are therefore normalised: the
test's temporary directory to `<tmp>` and `ROOT.resolve()` to `<repo>`, each
in both its given and its resolved spelling (macOS `/var` is a symlink).
`run_translator` fails if the normalised output still contains the real
`$HOME`, so no golden carries a machine path.
`SWAY_TO_KDL_UPDATE_GOLDENS=1` rewrites the goldens. A missing golden fails
the test, so every new test has to commit one.

The two committed fixtures in `tests/fixtures/config/` are also goldens. They
are unmodified copies of sway's and SwayFX's MIT-licensed `config.in`.
The tests run the script in a subprocess, so tracing goes through the same
helper: with `SWAY_TO_KDL_TRACE=<dir>` set, `run_translator` prefixes the
call with `python3 -m trace --count --missing --coverdir <dir>`. Before
freezing, add synthetic golden-only inputs for every `parse` branch the
suite does not reach.

**Corpus outputs stay local.** The 12 corpus entries are third-party. Their
manifest declares licences of `none` for seven of them (no grant, so they
cannot be redistributed), `GPL-3.0-only` for `codebam`, and `MIT` for four.
Translated output copies source text: criteria, commands, and the echoed
directive in every report. It therefore counts as a copy of the input, and
`docs/internal/sway-to-kdl-corpus-audit.md` already promises that "the
repository contains no config text from this corpus".

`contrib/sway-to-kdl-corpus` gains `--outputs DIR`, which writes
`<entry>.kdl` and `<entry>.stderr` with the sterile temporary root normalised.
Comparison is `diff -r`. The default `DIR` is under
`.cache/sway-to-kdl-corpus/` (gitignored, `.gitignore:3`). The runner does not
need `--swayward` for this, so it works on a machine without a binary.

**Acceptance rule:** zero golden diff, except documented bug fixes.
Concretely:

- C2 changes no file under `contrib/sway-to-kdl-goldens` relative to its
  parent, the main tip it is rebased onto. PRs that land between C1 and C2
  change goldens; C2 does not.
- The local corpus `diff -r` is empty.
- In C3, every changed golden hunk belongs to a fix named in the commit
  message, and each fix has a focused test that fails on C2.

## Landed history: three commits

Every commit we land is replayed by each `contrib/merging-rebase` onto a new
niri release (`AGENTS.md`, "History is rebase fuel"). Main therefore gets
three commits, not one per parser step:

1. **C1, `sway-to-kdl: freeze translator output as goldens`.** The harness,
   the goldens, the trace-driven extra inputs and the corpus `--outputs` flag.
   The translator does not change. This lands first and alone, so the goldens
   are produced by the old code on main.
2. **C2, `sway-to-kdl: translate through a reader, lexer and KDL IR`.** The
   whole pipeline, with zero golden diff. Wherever a layer that follows sway
   would change output, the handler calls a named `legacy_*` helper that
   reproduces the old result. Each helper is listed in a block comment.
3. **C3, `sway-to-kdl: follow sway's lexing, blocks and modifiers`**, only if
   needed. It deletes the `legacy_*` helpers, updates the goldens and adds
   focused tests. Expected contents: `read_config` replacing `LegacyReader`,
   `argsep` command lists, quoted selectors and names via `strip_quotes`,
   unescaped arguments (today `bindsym Mod4+a exec echo \"x\"` keeps both
   backslashes in `command`), located inline reports, last-wins singular children
   (`Node.add` starts raising on `SINGULAR` names), split modifier and group
   tables, reports for one-line blocks, continuation and next-line `{` inside
   blocks, and the off-by-one line numbers of nested blocks inside `mode`
   (`:745,756`). If one fix becomes contentious, its helper stays and the fix
   is dropped from C3. C3 is not split.

C2 is built on a working branch, `sway-to-kdl-pipeline`, in a worktree under
`~/hacking/swayward-wm/`, as step commits that each keep the suite and the
goldens green:

- **S1:** lexer and its unit tests; no caller changes yet.
- **S2:** `LegacyReader` and `Frontend`, feeding the unchanged `elif` chain,
  plus `read_config` and its unit tests, not yet called.
- **S3:** `Directive` and the handler tables, with each `elif` moved
  verbatim, still emitting strings.
- **S4:** IR and renderer, with the handlers switched from strings to nodes.
- **S5:** cleanup and an audit of the `legacy_*` helpers.

**Keeping a large C2 reviewable.** The maintainer reviews S1 to S5 one step
at a time on the branch; S3 is mostly moved code (`git diff --color-moved`).
The steps are then squashed into C2, and `git diff S5 C2` must be empty, so
every step review applies to the landed tree. The step branch stays on
`origin` as the review record, and C2's message names it. The golden zero
diff proves behaviour, so the reviewer does not re-derive it. The 103
existing tests are black box and stay unchanged. C3 is small by construction:
deleted helpers, golden hunks and tests.

## External pull requests

All three are open, one commit each, and mergeable today. Each touches code
that C2 rewrites entirely:

- **#38** adds a modifier-only check to `translate_bind` through the mixed
  table, so `bindsym Mode_switch exec x`, which sway binds, is reported as
  modifier-only.
- **#41** adds `keyword_resize_set` to `for_window`. It still uses
  `effects +=`, so a doubled `resize set` still emits duplicates.
- **#42** adds `seat` dispatch, `translate_seat`, a `cursor_settings` dict and
  a `cursor` group in `render()`. It removes only `"` from the theme name.

Proposed order: land C1; ask markstos to rebase the three onto it with
regenerated goldens (`SWAY_TO_KDL_UPDATE_GOLDENS=1`), so each PR's golden
diff shows its output change. If he prefers, the maintainer regenerates them
mechanically on his branch and he reviews the hunks. Land them; then start
the branch from that main, which freezes their behaviour into the goldens. C3 fixes their known defects through the shared layers
(`argsep`, `strip_quotes`, `Node.set`, the group table) rather than through
re-spins. If they are not ready when the branch starts, translator PRs wait
for C2, then rebase: only their implementation hunks (13, 50 and 90 lines)
move into handlers, their black-box tests carry over unchanged, and their
goldens are regenerated the same way. #44
(`validate` checks command strings) touches no translator code; it can change
the corpus `validate` column, not the output the goldens compare.

## Out of scope

- A Rust port, a new translator CLI, and behaviour changes beyond C3's list.
- New translations that the flat model makes easy, such as one-line
  `mode foo bindsym ...`.
- Behaviour decisions that are not bugs (see open questions): duplicate
  bindings, case-insensitive directive names (today only the 13
  `re.IGNORECASE` regexes), first versus last line of a continued line, and
  whether bind commands collapse spaces as `join_args` does.
- **A package layout.** The script ships as one file: `install -Dm755
  contrib/sway-to-kdl` in `swayward.spec.rpkg:163`, `flake.nix:205`
  (`patchShebangs` at `:65`), `contrib/PKGBUILD:160`, `contrib/build-deb:40`
  and `.github/workflows/release.yml:131`; `python3 contrib/sway-to-kdl` in
  `swayward-config/tests/translator.rs:48-60` and `ci.yml:181-188`. A package
  needs a module directory and an import path in five install rules. The file
  stays single, with sections in pipeline order, at about 2,100 lines.
  Reconsider at about 3,000, where a `zipapp` still installs one file. No
  syntax newer than 3.9 (no `match`, no dataclass `slots`, no `|` outside
  annotations), the floor the file already needs for `removeprefix`.

## Risks and what catches them

- **Untested branches change silently.** The C1 trace pass and the local
  corpus diff narrow the gap; the C2 message states that the proof covers
  exercised inputs only.
- **Echoed text, ordering or line numbers drift.** Reports echo
  `Directive.text` or `Block.raw`, not lexed tokens; the IR has both `set`
  and `put` orders;
  block handlers keep `block.header.pos` where today's code reports the
  opening line (`:1483-1646`). Goldens compare stdout and stderr bytes.
- **Variable expansion differs.** `expand` moves verbatim in C2. Sway's
  split, expand and re-split order enters C3 only if a golden shows a
  difference.
- **Helpers outlive C3.** C3 adds a test that no `legacy_` name remains
  beyond those the maintainer keeps.
- **Loadability.** Goldens do not load KDL. `swayward-config/tests/translator.rs`
  via `./contrib/fast-gate` in the dev container, and the corpus `--swayward`
  run where a binary exists, gate C2 and C3.

## Open questions

1. After C3, keep every golden, or only the fixtures and a curated set?
2. Should `Mode_switch` and `GroupN` bindings translate, given that the typed
   KDL accepts group names, or be reported? Sway's binding also contains
   `Mode_switch` as a key.
3. Should duplicate bindings keep the first or let the later one overwrite?
   Should directive names match without case?
4. Is 3.9 the right Python floor for the packaged targets?
