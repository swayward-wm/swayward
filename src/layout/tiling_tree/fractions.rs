//! Sway's second fraction axis.
//!
//! Sway keeps a `width_fraction` and a `height_fraction` on every container. A
//! horizontal split arranges with the width fractions, a vertical one with the
//! height fractions, and tabbed or stacked layouts touch neither
//! (`apply_horiz_layout`, `apply_vert_layout`, `apply_tabbed_layout`,
//! sway/tree/arrange.c:15-212). A split's `percents` hold the fractions on its
//! own axis; `cross_percents` keeps each child's fraction on the other one, so
//! a split whose axis flips arranges with the fractions sway would use, not
//! with the ones it just stopped using.

use super::*;

impl<W: LayoutElement> TilingTree<W> {
    /// The axis, `SplitH` or `SplitV`, that `id`'s `percents` are fractions on.
    pub(super) fn percent_axis(&self, id: NodeId) -> Option<Layout> {
        match self.split_layout(id)? {
            layout @ (Layout::SplitH | Layout::SplitV) => Some(layout),
            Layout::Tabbed | Layout::Stacked => Some(
                self.percent_axes
                    .get(&id)
                    .copied()
                    .unwrap_or(Layout::SplitH),
            ),
        }
    }

    /// Sets a split's layout, carrying its children's fractions to the new axis.
    pub(super) fn set_split_layout(&mut self, id: NodeId, layout: Layout) {
        let Some(from) = self.percent_axis(id) else {
            return;
        };
        if let Some(TreeNode::Split {
            layout: current, ..
        }) = self.nodes.get_mut(&id).map(|node| &mut node.value)
        {
            *current = layout;
        }
        self.adopt_percent_axis(id, from);
    }

    /// `id`'s `percents` are fractions on `from`; make them fractions on the
    /// axis its layout arranges. A tabbed or stacked split remembers `from`.
    /// On a flip each child takes its stored fraction on the new axis and
    /// keeps the old one. An unset fraction takes the average of the set
    /// ones, all unset split evenly, and the result is normalized, as sway's
    /// next arrange does (sway/tree/arrange.c:20-52 and 105-137).
    pub(super) fn adopt_percent_axis(&mut self, id: NodeId, from: Layout) {
        let to = match self.split_layout(id) {
            Some(layout @ (Layout::SplitH | Layout::SplitV)) => layout,
            Some(Layout::Tabbed | Layout::Stacked) => {
                self.percent_axes.insert(id, from);
                return;
            }
            None => return,
        };
        self.percent_axes.remove(&id);
        if to == from {
            return;
        }
        let Some(TreeNode::Split {
            children, percents, ..
        }) = self.nodes.get_mut(&id).map(|node| &mut node.value)
        else {
            return;
        };
        for (child, percent) in children.iter().zip(percents.iter_mut()) {
            let cross = self
                .cross_percents
                .insert(*child, (from, *percent))
                .filter(|(axis, _)| *axis == to);
            *percent = cross.map_or(0., |(_, cross)| cross);
        }
        let (set, total) = percents
            .iter()
            .filter(|percent| **percent > 0.)
            .fold((0usize, 0.), |(count, total), percent| {
                (count + 1, total + percent)
            });
        let unset = if set == 0 { 1. } else { total / set as f64 };
        for percent in percents.iter_mut() {
            if *percent <= 0. {
                *percent = unset;
            }
        }
        let sum: f64 = percents.iter().sum();
        for percent in percents.iter_mut() {
            *percent /= sum;
        }
    }

    /// `replacement` takes `id`'s slot with both of its fractions
    /// (`container_replace`, sway/tree/container.c:1471-1491). The slot's
    /// `percents` entry already carries the one on the parent's axis.
    pub(super) fn take_over_fractions(&mut self, id: NodeId, replacement: NodeId) {
        match self.cross_percents.get(&id).copied() {
            Some(cross) => self.cross_percents.insert(replacement, cross),
            None => self.cross_percents.remove(&replacement),
        };
    }
}
