use super::Layout;
use crate::layout::tile::Tile;
use crate::layout::LayoutElement;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(pub(crate) u64);

/// Per-container state that belongs to a split and travels with it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SplitMeta {
    /// The last SplitH/SplitV layout, restored by `layout toggle split` and `layout default`.
    pub previous_layout: Option<Layout>,
    /// A `title_format` other than the default `%title`.
    pub title_format: Option<String>,
    pub sticky: bool,
    /// The linear axis `percents` holds shares along, once known.
    pub fraction_axis: Option<super::Layout>,
    /// Each child's share along the other linear axis. Sway keeps a width and
    /// a height fraction on every container and lays a split out with the one
    /// along its axis (`apply_horiz_layout`/`apply_vert_layout`,
    /// sway/tree/arrange.c:15-170), so the other survives an axis change.
    pub latent_shares: Vec<(NodeId, f64)>,
}

#[derive(Debug)]
pub enum TreeNode<W: LayoutElement> {
    Split {
        layout: Layout,
        children: Vec<NodeId>,
        percents: Vec<f64>,
        meta: SplitMeta,
    },
    Leaf {
        tile: Box<Tile<W>>,
    },
}

#[derive(Debug)]
pub(crate) struct Node<W: LayoutElement> {
    pub(crate) parent: Option<NodeId>,
    pub(crate) value: TreeNode<W>,
}
