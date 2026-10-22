A tabbed container gives its full content area to one child at a time. The
other children remain in the tree and keep their layout state, so tabbing is
a property of a *container*, not a mode the window is in.

Any container can be tabbed, not just a whole workspace. Tab a split that
holds two terminals and you get one tab containing both, still side by side
inside it.

## Commands

| Command | Effect |
| --- | --- |
| `layout tabbed` | make the container holding the focused node tabbed |
| `layout stacking` | stack instead: one child shown, titles listed vertically |
| `layout splith` / `layout splitv` | back to a side-by-side or top-to-bottom split |
| `layout toggle split` | flip between horizontal and vertical |
| `layout default` | return to the container's last split layout (splith or splitv) |
| `focus parent` | select the container holding the focused window |
| `focus child` | descend again |

Layout commands change the *parent* of the focused node, as in sway. To tab a
group, focus any window inside it and run `layout tabbed`: the group's children
become tabs. If you first run `focus parent` so the group itself is focused,
`layout tabbed` changes the container one level further up, and the group
becomes a single tab among its siblings.

Focus and move commands operate on the same nested tree in every layout.
`focus left` and `focus right` move between children of a tabbed container;
`focus up` and `focus down` move between children of a stacked container. In a
nested split, swayward first resolves the direction at the innermost container
with the matching axis.

## Tab indicator

The title strip identifies each tab and lets you click it to change focus.
Swayward disables niri's additional edge indicator by default because it
would duplicate the title strip.

You can enable the edge indicator in the [`tab-indicator` layout
section](./Configuration:-Layout.md#tab-indicator). You can change its side,
width, length, spacing, corner radius, and active, inactive, and urgent colours.
Set `hide-when-single-tab` to hide the indicator when a tabbed container has one
child.
