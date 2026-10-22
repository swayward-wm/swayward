> **Inherited from niri.** This is an additive feature swayward keeps from
> niri. Sway has no equivalent and no sway client can observe it, so nothing
> here affects sway compatibility. Safe to ignore entirely if you want
> swayward to behave like sway.

### Overview

The `gestures {}` section configures the workspace overview's pointer
gestures. A vertical four-finger touchpad swipe opens and closes the overview
and has no settings. Configure the overview's appearance in [miscellaneous
configuration](./Configuration:-Miscellaneous.md). The Mod+drag window
gestures, which sway also has, are on the [gestures page](./Gestures.md).

These are the defaults, with hot corners off as the shipped config sets them:

```kdl
gestures {
    dnd-edge-workspace-switch {
        trigger-height 50
        delay-ms 100
        max-speed 1500
    }

    hot-corners {
        off
        // top-left
        // top-right
        // bottom-left
        // bottom-right
    }
}
```

### `hot-corners`

Moving the pointer into a corner of an output toggles the overview. With
`off`, no corner does. Otherwise list the corners that should; a
`hot-corners {}` block that lists none, or omitting the section, enables only
the top-left corner. A resize drag does not trigger a hot corner, but a move
drag does, so you can carry a window into the overview. Touch input never
triggers one.

An `output` section can set its own `hot-corners {}`, which replaces the
`gestures` value for that output.

### `dnd-edge-workspace-switch`

While the overview is open, dragging a window or a drag-and-drop payload near
the top or bottom edge of the working area scrolls between workspaces.

- `trigger-height`: height in logical pixels of the edge zone. Default 50.
- `delay-ms`: how long the pointer must stay in the zone before scrolling
  starts, so a drag across outputs does not scroll by accident. Default 100.
- `max-speed`: scroll speed in logical pixels per second at the very edge.
  Default 1500.

The zone excludes layer-shell exclusive zones such as a bar.

### `dnd-edge-view-scroll`

The parser still accepts niri's `dnd-edge-view-scroll`, but nothing reads it:
it scrolled niri's horizontal strip, which the nested tree does not have.
