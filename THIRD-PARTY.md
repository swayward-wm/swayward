# Third-party code

swayward is licensed under GPL-3.0-or-later (see [`LICENSE`](LICENSE)). Some of
its code is ported from [sway](https://github.com/swaywm/sway) 1.12: the
algorithm and structure of a sway function translated into Rust or Python,
rather than written independently to match sway's behaviour. sway is
MIT-licensed, and its licence requires the notice below in all copies or
substantial portions of its code. The table names the main ports; sway's
notice applies to all code in swayward ported or adapted from sway.

## Ported from sway

| swayward file | What is ported | sway source |
|---|---|---|
| `swayward-ipc/src/command/lexer.rs` | `sway_split_args` and `sway_argc` (`split_args`) | `common/stringop.c` |
| `swayward-ipc/src/command/parse/arity.rs` | `checkarg` | `sway/commands.c` |
| `swayward-ipc/src/command/variables.rs` | `expand_variables` (`do_var_replacement`) | `sway/config.c` |
| `swayward-ipc/src/bin/swaywardmsg/summary.rs` | swaymsg's human summaries (`pretty_print_*`) and `success` | `swaymsg/main.c` |
| `src/input/backend_ext.rs` | `describe_libinput_device` | `sway/ipc-json.c` |
| `src/layout/floating_tree.rs` | `adjust_floating_box` (`resize_adjust_floating`) | `sway/commands/resize.c` |
| `src/layout/workspace/output_priority.rs` | `raise_output_priority` (`workspace_output_raise_priority`) | `sway/tree/workspace.c` |
| `src/layout/tiling_tree/resize.rs` | `is_internal_edge` (`edge_is_external`) | `sway/input/seatop_default.c` |
| `src/layout/tiling_tree/normalize.rs` | `squash_for_move` (`workspace_squash`, `container_squash`) | `sway/tree/workspace.c`, `sway/tree/container.c` |
| `contrib/sway-to-kdl` (installed as `swayward-sway-to-kdl`) | `split_args`, `strip_quotes`, `argsep`, `unescape_string`, `join_args` | `common/stringop.c` |
| | `parse_boolean` | `common/util.c` |
| | `read_config` with `getline_with_cont`, `detect_brace` and `expand_line` | `sway/config.c` |
| | `split_command_list` (the command loop of `execute_command`) and `checkarg` | `sway/commands.c` |
| | `criteria_length` (the extent scan of `criteria_parse`) | `sway/criteria.c` |
| | `split_key_combo` (the key-combo loop of `cmd_bindsym_or_bindcode`) | `sway/commands/bind.c` |

## sway's licence

```
Copyright (c) 2016-2017 Drew DeVault

Permission is hereby granted, free of charge, to any person obtaining a copy of
this software and associated documentation files (the "Software"), to deal in
the Software without restriction, including without limitation the rights to
use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies
of the Software, and to permit persons to whom the Software is furnished to do
so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```
