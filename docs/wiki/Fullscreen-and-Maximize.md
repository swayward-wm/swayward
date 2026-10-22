swayward supports fullscreen, maximized, and windowed-fullscreen states.

## Maximize

Maximize is inherited from niri. Sway has no maximize command or binding, and
it ignores an application's maximize request. Swayward honours the request:

- A maximized tiled window keeps its container and its place in the tree.
  Swayward sets the `maximized` state, asks the application for the size of
  the workspace's working area, and hides the window's border. Unmaximizing
  returns it to its slot's size.
- A maximized floating window moves into the tiled tree and maximizes there.
  Unmaximizing floats it again.

Two window rules affect maximize at open:

- `open-maximized-to-edges true` opens a matching window in the maximized
  state, as if it had asked; `false` refuses the application's initial request.
- `open-maximized` is niri's full-width column rule. The parser accepts it,
  but it has no effect: the tree, not the rule, sets a tiled window's width.

The niri bind actions `maximize-window-to-edges` and `maximize-column` are
available but unbound in the shipped config. A taskbar's maximize request
through the wlr foreign-toplevel protocol does nothing, as in sway.

## Fullscreen

A fullscreen window covers its output, including normal layer-shell panels and
floating windows. Overlay-layer surfaces can still appear above it. The window
remains in its workspace and returns to its prior tree position or floating
geometry when fullscreen ends.

`fullscreen` (`Mod+F` in the shipped config) acts on the focused node, so a
focused container goes fullscreen as a whole. This includes a floating group
and a window inside one.

`fullscreen global` covers only the window's own output; sway spans every
output. See [Known deviations: global fullscreen on several
outputs](https://github.com/swayward-wm/swayward/blob/main/docs/KNOWN_DEVIATIONS.md#global-fullscreen-on-several-outputs).

Use `open-fullscreen true` to start a matching window fullscreen. Set it to
`false` to reject an application's initial fullscreen request.

## Windowed fullscreen

Windowed fullscreen tells the application that it is fullscreen without making
its tile cover the output. This is useful for browser presentations that must
hide their UI while remaining visible beside other windows.

The `toggle-windowed-fullscreen` action controls this state.
