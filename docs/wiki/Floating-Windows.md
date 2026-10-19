Floating windows appear above the tiled container tree. Each workspace has its
own floating layer.

Dialogs and fixed-size windows float automatically. Use `open-floating` in a
window rule to override that decision:

```kdl
window-rule {
    match app-id="firefox$" title="^Picture-in-Picture$"
    open-floating true
}
```

## Commands

The sway commands work as they do in sway:

| Command | Effect |
| --- | --- |
| `floating toggle` | move the focused window or container between the tree and the floating layer |
| `floating enable` / `floating disable` | set it explicitly |
| `focus mode_toggle` | switch focus between the floating layer and the tree |
| `focus floating` / `focus tiling` | focus a specific layer |
| `sticky enable` | keep a floating window visible across workspaces |
| `move position <x> <y>` | place it, in pixels or `ppt` |
| `move absolute position <x> <y>` | place it in output coordinates |
| `move position center` | centre it |
| `resize set <w> <h>` | size it |

swayward stores a floating position exactly where you put it, including
off-screen, as sway does. Only `move position pointer` corrects into bounds.

In KDL binds the equivalent actions are `toggle-window-floating`,
`switch-focus-between-floating-and-tiling` and `move-window-to-floating`,
inherited from niri.

While a floating window has focus, directional move and resize operations act
on that window rather than on the tree.

## Floating containers

A split, tabbed or stacked container can float too. Focus it with `focus
parent` (`Mod+A`) and run `floating toggle`: the whole container floats as one
group, with its children and layout intact.

- `Mod` and drag on any window in the group moves or resizes the whole group.
  Dragging the border between two children without `Mod` resizes them inside
  the group.
- `move scratchpad` from any window in the group stashes the whole group, and
  `scratchpad show` brings it back as one.
- `sticky enable` applies to whatever is focused. To make the group follow you
  across workspaces, focus the group itself, not one of its windows.

[Induction V](./Tree-School:-Floating-and-Scratchpad.md#float-a-whole-branch)
walks through it step by step.

Set `default-floating-position` in a [window
rule](./Configuration:-Window-Rules.md#default-floating-position) to choose an
initial position.
