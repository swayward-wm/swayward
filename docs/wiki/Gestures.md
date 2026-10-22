swayward supports interactive move and resize gestures for windows, as sway's
`floating_modifier` does.

## Mouse

Hold <kbd>Mod</kbd> and drag with the left mouse button to move a window. Hold
<kbd>Mod</kbd> and drag with the right mouse button to resize it. Right-click
while moving to switch between the tiled tree and the floating layer.

`Mod` is the key `input { mod-key }` names. To use a different modifier for
these drags, or to swap the buttons, set
[`floating-modifier`](./Configuration:-Input.md#floating-modifier).

## Touch and tablet

Hold <kbd>Mod</kbd> and drag a window with a finger or tablet pen to perform an
interactive move. The [insert hint](./Configuration:-Layout.md#insert-hint)
shows the destination in the tiled tree.

## Overview gestures

The workspace overview is inherited from niri, and sway has no equivalent. A
vertical four-finger touchpad swipe opens and closes it. A `toggle-overview`
bind provides keyboard access, and
[gesture configuration](./Configuration:-Gestures.md) covers hot corners and
the overview's drag-and-drop edge scrolling. Swayward does not include niri's
horizontal viewport gesture because the nested tree has no scrolling viewport.

Configure device behaviour in [Input](./Configuration:-Input.md).
