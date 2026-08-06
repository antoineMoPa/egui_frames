//! What the arrangement *is*: a tree of splits whose leaves are frames, and a frame's tabs.
//!
//! Nothing here draws anything or reads a pointer, which is what makes an arrangement
//! something an application can store, restore and reason about on its own.

use std::collections::HashMap;

use crate::{FrameId, PaneId};

/// How much of a split's space the smallest child is allowed to be squeezed down to.
const MIN_SPLIT_FRACTION: f32 = 0.1;

/// How much of the workspace a new column or row against its edge takes by default.
pub const DEFAULT_EDGE_SHARE: f32 = 0.35;

/// Which way a split divides the space it was given.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "lowercase"))]
pub enum SplitDirection {
    /// Children side by side, dividing the width.
    Row,
    /// Children stacked, dividing the height.
    Column,
}

/// Where a pane lands when it is dropped on a frame: on its tab strip, or against one of its
/// edges, which splits that frame in two.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DropSide {
    /// Among the frame's tabs.
    Tabs,
    /// A new frame to the left of it.
    Left,
    /// A new frame to the right of it.
    Right,
    /// A new frame above it.
    Top,
    /// A new frame below it.
    Bottom,
}

impl DropSide {
    fn direction(self) -> Option<SplitDirection> {
        match self {
            Self::Tabs => None,
            Self::Left | Self::Right => Some(SplitDirection::Row),
            Self::Top | Self::Bottom => Some(SplitDirection::Column),
        }
    }

    /// Whether a new frame goes before (0) or after (1) the frame it was dropped on.
    fn offset(self) -> usize {
        match self {
            Self::Left | Self::Top => 0,
            _ => 1,
        }
    }
}

/// The arrangement's tree: frames at the leaves, splits in between.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(tag = "kind", rename_all = "lowercase"))]
pub enum LayoutNode {
    /// One frame.
    Frame {
        /// Which frame.
        frame: FrameId,
    },
    /// A row or column of children, and the share of the space each one takes.
    Split {
        /// Which way the space is divided.
        direction: SplitDirection,
        /// The children, in the order they are laid out.
        children: Vec<LayoutNode>,
        /// One share per child, summing to one.
        sizes: Vec<f32>,
    },
}

impl LayoutNode {
    /// Every frame of this subtree, in the order it is laid out.
    pub fn frames(&self) -> Vec<FrameId> {
        match self {
            Self::Frame { frame } => vec![*frame],
            Self::Split { children, .. } => children.iter().flat_map(Self::frames).collect(),
        }
    }
}

/// One box of the arrangement: a tab strip, and whichever of its panes is in front.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Frame {
    id: FrameId,
    panes: Vec<PaneId>,
    active_pane: Option<PaneId>,
}

impl Frame {
    /// This frame's name.
    pub fn id(&self) -> FrameId {
        self.id
    }

    /// Its panes, in the order their tabs are drawn.
    pub fn panes(&self) -> &[PaneId] {
        &self.panes
    }

    /// The pane in front, which is the one whose body the frame draws.
    pub fn active_pane(&self) -> Option<PaneId> {
        self.active_pane
    }

    fn new(id: FrameId) -> Self {
        Self {
            id,
            panes: Vec::new(),
            active_pane: None,
        }
    }
}

/// An arrangement of panes: what is open, where, and which of it is in front.
///
/// `P` is whatever a pane is in your application — an enum of kinds, a document handle, an
/// index into your own state. The arrangement holds it and hands it back when it is drawn;
/// none of the layout logic looks inside it.
///
/// Every method that changes the arrangement keeps it coherent: a frame emptied of its last
/// pane is taken out of the tree, the shares of a split always add up, and the workspace
/// always has at least one frame to drop a pane on.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Layout<P> {
    root: LayoutNode,
    frames: HashMap<FrameId, Frame>,
    panes: HashMap<PaneId, P>,
    active_frame: FrameId,
    /// Names are handed out from here rather than from a global counter, so a restored
    /// arrangement carries on where it left off instead of colliding with what it restored.
    next_id: u64,
}

impl<P> Default for Layout<P> {
    fn default() -> Self {
        Self::new()
    }
}

impl<P> Layout<P> {
    /// An arrangement of one empty frame.
    pub fn new() -> Self {
        let frame = FrameId(1);
        Self {
            root: LayoutNode::Frame { frame },
            frames: HashMap::from([(frame, Frame::new(frame))]),
            panes: HashMap::new(),
            active_frame: frame,
            next_id: 2,
        }
    }

    /// An arrangement of one frame holding one pane.
    pub fn with_pane(pane: P) -> Self {
        let mut layout = Self::new();
        let frame = layout.active_frame;
        layout.add_pane(frame, pane, None);
        layout
    }

    /// The tree, for an application that wants to walk the arrangement itself.
    pub fn root(&self) -> &LayoutNode {
        &self.root
    }

    /// Every frame, in the order it is laid out: left to right, top to bottom.
    pub fn frame_ids(&self) -> Vec<FrameId> {
        self.root.frames()
    }

    /// One frame, if it is part of this arrangement.
    pub fn frame(&self, frame: FrameId) -> Option<&Frame> {
        self.frames.get(&frame)
    }

    /// How many frames there are.
    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    /// The top-left frame, whose tab strip is the one an application usually hangs its own
    /// controls off.
    pub fn primary_frame(&self) -> FrameId {
        self.frame_ids().first().copied().unwrap_or(self.active_frame)
    }

    /// The frame the keyboard is talking to.
    pub fn active_frame(&self) -> FrameId {
        self.active_frame
    }

    /// Hand the keyboard to a frame.
    pub fn set_active_frame(&mut self, frame: FrameId) {
        if self.frames.contains_key(&frame) {
            self.active_frame = frame;
        }
    }

    /// Hand the keyboard to the next frame, wrapping round at the end.
    ///
    /// Frames come back in the order they are laid out, so this walks the workspace the way it
    /// looks rather than the way its tree happens to be built.
    pub fn focus_next_frame(&mut self) -> FrameId {
        let frames = self.frame_ids();
        let at = frames
            .iter()
            .position(|frame| *frame == self.active_frame)
            .unwrap_or(0);
        if let Some(next) = frames.get((at + 1) % frames.len().max(1)) {
            self.active_frame = *next;
        }
        self.active_frame
    }

    /// Every open pane. The order is not meaningful; a frame's own [`panes`](Frame::panes) is.
    pub fn panes(&self) -> impl Iterator<Item = (PaneId, &P)> {
        self.panes.iter().map(|(id, pane)| (*id, pane))
    }

    /// How many panes are open, across every frame.
    pub fn pane_count(&self) -> usize {
        self.panes.len()
    }

    /// Whether nothing at all is open.
    pub fn is_empty(&self) -> bool {
        self.panes.is_empty()
    }

    /// One pane, if it is open.
    pub fn pane(&self, pane: PaneId) -> Option<&P> {
        self.panes.get(&pane)
    }

    /// The same, to change.
    pub fn pane_mut(&mut self, pane: PaneId) -> Option<&mut P> {
        self.panes.get_mut(&pane)
    }

    /// Whether a pane is open.
    pub fn contains(&self, pane: PaneId) -> bool {
        self.panes.contains_key(&pane)
    }

    /// The pane in front of the active frame — the one the keyboard is talking to.
    pub fn active_pane(&self) -> Option<(PaneId, &P)> {
        let pane = self.frames.get(&self.active_frame)?.active_pane?;
        Some((pane, self.panes.get(&pane)?))
    }

    /// The first pane the predicate accepts, for "is this already open?" questions.
    pub fn find_pane(&self, mut wanted: impl FnMut(&P) -> bool) -> Option<(PaneId, &P)> {
        self.panes
            .iter()
            .find(|(_, pane)| wanted(pane))
            .map(|(id, pane)| (*id, pane))
    }

    /// The frame holding a pane.
    pub fn frame_of(&self, pane: PaneId) -> Option<FrameId> {
        self.frames
            .values()
            .find(|frame| frame.panes.contains(&pane))
            .map(Frame::id)
    }

    /// The frame that panes like this one already live in: `preferred` if it holds one, else
    /// wherever the others are. This is what keeps shells with shells and documents with
    /// documents as new panes open.
    pub fn frame_holding(
        &self,
        preferred: FrameId,
        mut wanted: impl FnMut(&P) -> bool,
    ) -> Option<FrameId> {
        let mut holds = |frame: FrameId| {
            self.frames.get(&frame).is_some_and(|frame| {
                frame
                    .panes
                    .iter()
                    .any(|pane| self.panes.get(pane).is_some_and(&mut wanted))
            })
        };

        if holds(preferred) {
            return Some(preferred);
        }
        self.frame_ids().into_iter().find(|frame| holds(*frame))
    }

    /// Whether the tree, the frames and the panes agree with each other.
    ///
    /// A restored arrangement is only worth drawing if they do; a half-written or outdated one
    /// is better thrown away than rendered.
    pub fn is_coherent(&self) -> bool {
        let frame_ids = self.frame_ids();
        !frame_ids.is_empty()
            && frame_ids.len() == self.frames.len()
            && frame_ids.iter().all(|frame| self.frames.contains_key(frame))
            && self.frames.contains_key(&self.active_frame)
            && self
                .frames
                .values()
                .all(|frame| frame.panes.iter().all(|pane| self.panes.contains_key(pane)))
    }

    /// Keep the arrangement's shape and hand back everything that was in it.
    ///
    /// This is how a stored arrangement is reused across runs: its splits say where the user
    /// put their columns and rows, which is worth keeping, while its panes named documents and
    /// processes that this run has to open for itself. The frames come back empty, ready to be
    /// refilled — and [`drop_empty_frames`](Layout::drop_empty_frames) takes away whatever
    /// nothing landed in.
    pub fn take_panes(&mut self) -> Vec<P> {
        for frame in self.frames.values_mut() {
            frame.panes.clear();
            frame.active_pane = None;
        }
        std::mem::take(&mut self.panes).into_values().collect()
    }

    /// Put a pane in a frame, before one of its tabs or at the end of the strip. It becomes
    /// the pane in front, and its frame becomes the active one.
    pub fn add_pane(&mut self, frame: FrameId, pane: P, before: Option<PaneId>) -> PaneId {
        let id = PaneId(self.take_id());
        self.put_pane(frame, id, pane, before);
        id
    }

    /// Put a pane in a frame of its own, beside an existing frame.
    pub fn add_pane_beside(&mut self, frame: FrameId, side: DropSide, pane: P) -> PaneId {
        let id = PaneId(self.take_id());
        self.put_pane_beside(frame, side, id, pane);
        id
    }

    /// Put a pane in a frame that runs the whole width or height of the workspace, against one
    /// of its outer edges — beside everything already open, rather than beside one frame.
    ///
    /// `share` is how much of the workspace the new column or row takes; the frames already
    /// there give up an equal part of themselves to it. [`DEFAULT_EDGE_SHARE`] is a sensible
    /// one.
    pub fn add_pane_against_edge(&mut self, side: DropSide, share: f32, pane: P) -> PaneId {
        let id = PaneId(self.take_id());
        self.put_pane_against_edge(side, share, id, pane);
        id
    }

    /// Everything [`add_pane`](Layout::add_pane) does, for a pane that already has a name —
    /// which is what a move is, so a dragged tab stays the same tab.
    fn put_pane(&mut self, frame: FrameId, id: PaneId, pane: P, before: Option<PaneId>) {
        let Some(target) = self.frames.get_mut(&frame) else {
            return;
        };

        let at = before
            .and_then(|before| target.panes.iter().position(|pane| *pane == before))
            .unwrap_or(target.panes.len());
        target.panes.insert(at, id);
        target.active_pane = Some(id);

        self.panes.insert(id, pane);
        self.active_frame = frame;
    }

    fn put_pane_beside(&mut self, frame: FrameId, side: DropSide, id: PaneId, pane: P) {
        let Some(direction) = side.direction() else {
            self.put_pane(frame, id, pane, None);
            return;
        };

        let new_frame = FrameId(self.take_id());
        let root = std::mem::replace(&mut self.root, LayoutNode::Frame { frame: new_frame });
        self.root = insert_frame_beside(root, frame, new_frame, direction, side.offset());
        self.frames.insert(new_frame, Frame::new(new_frame));

        self.put_pane(new_frame, id, pane, None);
    }

    fn put_pane_against_edge(&mut self, side: DropSide, share: f32, id: PaneId, pane: P) {
        let Some(direction) = side.direction() else {
            let frame = self.active_frame;
            self.put_pane(frame, id, pane, None);
            return;
        };
        let share = share.clamp(MIN_SPLIT_FRACTION, 1.0 - MIN_SPLIT_FRACTION);
        let new_frame = FrameId(self.take_id());
        let leaf = LayoutNode::Frame { frame: new_frame };
        let at_end = side.offset() == 1;

        let root = std::mem::replace(&mut self.root, LayoutNode::Frame { frame: new_frame });
        self.root = match root {
            // A split already running the right way takes one more child, and the frames
            // already there give up an equal part of themselves to it.
            LayoutNode::Split {
                direction: existing,
                mut children,
                sizes,
            } if existing == direction => {
                let mut sizes: Vec<f32> =
                    sizes.into_iter().map(|size| size * (1.0 - share)).collect();
                if at_end {
                    children.push(leaf);
                    sizes.push(share);
                } else {
                    children.insert(0, leaf);
                    sizes.insert(0, share);
                }
                LayoutNode::Split {
                    direction,
                    children,
                    sizes,
                }
            }
            root => {
                let (children, sizes) = if at_end {
                    (vec![root, leaf], vec![1.0 - share, share])
                } else {
                    (vec![leaf, root], vec![share, 1.0 - share])
                };
                LayoutNode::Split {
                    direction,
                    children,
                    sizes,
                }
            }
        };

        self.frames.insert(new_frame, Frame::new(new_frame));
        self.put_pane(new_frame, id, pane, None);
    }

    /// Bring a pane to the front of its frame, and hand that frame the keyboard.
    pub fn focus_pane(&mut self, pane: PaneId) {
        let Some(frame) = self.frame_of(pane) else {
            return;
        };
        if let Some(frame) = self.frames.get_mut(&frame) {
            frame.active_pane = Some(pane);
        }
        self.active_frame = frame;
    }

    /// Close a pane and hand it back, taking its frame with it if that leaves it empty.
    ///
    /// The last frame stays even when empty, so the workspace always has a drop target.
    pub fn close_pane(&mut self, pane: PaneId) -> Option<P> {
        let frame_id = self.frame_of(pane)?;
        let only_frame_left = self.frames.len() == 1;

        let frame = self.frames.get_mut(&frame_id)?;
        frame.panes.retain(|open| *open != pane);
        if frame.active_pane == Some(pane) {
            frame.active_pane = frame.panes.last().copied();
        }
        let frame_is_empty = frame.panes.is_empty();
        let closed = self.panes.remove(&pane);

        if frame_is_empty && !only_frame_left {
            self.remove_frame(frame_id);
        }
        closed
    }

    /// Drop frames that ended up with nothing in them, keeping the last one.
    ///
    /// A restored arrangement contributes its splits, and whatever this run does not put back
    /// into them would otherwise be drawn as an empty frame.
    pub fn drop_empty_frames(&mut self) {
        let empty: Vec<FrameId> = self
            .frames
            .values()
            .filter(|frame| frame.panes.is_empty())
            .map(Frame::id)
            .collect();

        for frame in empty {
            // The last frame stays even when empty, so there is always a drop target.
            if self.frames.len() == 1 {
                break;
            }
            self.remove_frame(frame);
        }
    }

    /// Move a pane onto another frame's tab strip, or against one of its edges — which puts
    /// the pane in a brand new frame beside it.
    pub fn move_pane_to_frame(
        &mut self,
        pane: PaneId,
        frame: FrameId,
        side: DropSide,
        before: Option<PaneId>,
    ) {
        let Some(source) = self.frame_of(pane) else {
            return;
        };
        let source_pane_count = self.frames.get(&source).map_or(0, |frame| frame.panes.len());

        if side == DropSide::Tabs {
            if source == frame {
                self.reorder_pane(frame, pane, before);
                return;
            }
            let Some(taken) = self.close_pane(pane) else {
                return;
            };
            self.put_pane(frame, pane, taken, before);
            return;
        }

        // A lone tab dropped on its own frame's edge would just rebuild the same frame.
        if source == frame && source_pane_count == 1 {
            return;
        }
        let Some(taken) = self.close_pane(pane) else {
            return;
        };
        self.put_pane_beside(frame, side, pane, taken);
    }

    /// Move an open pane into a frame against one of the workspace's outer edges.
    pub fn move_pane_against_edge(&mut self, pane: PaneId, side: DropSide, share: f32) {
        // A lone pane moved against the edge it already fills would only rebuild what is there.
        if self.frames.len() == 1 && self.panes.len() == 1 {
            return;
        }
        let Some(taken) = self.close_pane(pane) else {
            return;
        };
        self.put_pane_against_edge(side, share, pane, taken);
    }

    /// Set the shares of one split, named by the path from the root that reaches it: the index
    /// of the child taken at each step. The empty path is the root itself.
    ///
    /// No child is left below a tenth of the split, and the shares are rescaled to add up.
    pub fn set_split_sizes(&mut self, path: &[usize], sizes: &[f32]) {
        let root = std::mem::replace(&mut self.root, LayoutNode::Frame { frame: FrameId(0) });
        self.root = set_split_sizes(root, path, sizes);
    }

    fn reorder_pane(&mut self, frame: FrameId, pane: PaneId, before: Option<PaneId>) {
        if let Some(frame) = self.frames.get_mut(&frame) {
            frame.panes.retain(|open| *open != pane);
            let at = before
                .and_then(|before| frame.panes.iter().position(|pane| *pane == before))
                .unwrap_or(frame.panes.len());
            frame.panes.insert(at, pane);
            frame.active_pane = Some(pane);
        }
        self.active_frame = frame;
    }

    fn remove_frame(&mut self, frame: FrameId) {
        self.frames.remove(&frame);
        let root = std::mem::replace(&mut self.root, LayoutNode::Frame { frame });
        self.root = remove_frame_node(root, frame);
        let remaining = self.frame_ids();
        if !remaining.contains(&self.active_frame)
            && let Some(first) = remaining.first()
        {
            self.active_frame = *first;
        }
    }

    fn take_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
}

fn remove_frame_node(node: LayoutNode, frame: FrameId) -> LayoutNode {
    let LayoutNode::Split {
        direction,
        children,
        sizes,
    } = node
    else {
        return node;
    };

    let at = children
        .iter()
        .position(|child| matches!(child, LayoutNode::Frame { frame: id } if *id == frame));

    let Some(at) = at else {
        return LayoutNode::Split {
            direction,
            children: children
                .into_iter()
                .map(|child| remove_frame_node(child, frame))
                .collect(),
            sizes,
        };
    };

    let mut children = children;
    let mut sizes = sizes;
    children.remove(at);
    if at < sizes.len() {
        sizes.remove(at);
    }
    if children.len() == 1 {
        return children.remove(0);
    }

    LayoutNode::Split {
        direction,
        children,
        sizes: rescale_to_one(sizes),
    }
}

fn insert_frame_beside(
    node: LayoutNode,
    target: FrameId,
    new_frame: FrameId,
    direction: SplitDirection,
    offset: usize,
) -> LayoutNode {
    let leaf = LayoutNode::Frame { frame: new_frame };

    match node {
        LayoutNode::Frame { frame } => {
            if frame != target {
                return LayoutNode::Frame { frame };
            }
            let existing = LayoutNode::Frame { frame };
            let children = if offset == 0 {
                vec![leaf, existing]
            } else {
                vec![existing, leaf]
            };
            LayoutNode::Split {
                direction,
                children,
                sizes: vec![0.5, 0.5],
            }
        }
        LayoutNode::Split {
            direction: node_direction,
            children,
            sizes,
        } => {
            let at = children
                .iter()
                .position(|child| matches!(child, LayoutNode::Frame { frame } if *frame == target));

            let Some(at) = at.filter(|_| node_direction == direction) else {
                return LayoutNode::Split {
                    direction: node_direction,
                    children: children
                        .into_iter()
                        .map(|child| {
                            insert_frame_beside(child, target, new_frame, direction, offset)
                        })
                        .collect(),
                    sizes,
                };
            };

            // Same direction as the surrounding split: the new frame becomes another sibling
            // and takes half of the space the target had.
            let mut children = children;
            let mut sizes = sizes;
            let half = sizes.get(at).copied().unwrap_or(0.5) / 2.0;
            if let Some(size) = sizes.get_mut(at) {
                *size = half;
            }
            children.insert(at + offset, leaf);
            sizes.insert(at + offset, half);

            LayoutNode::Split {
                direction: node_direction,
                children,
                sizes,
            }
        }
    }
}

fn set_split_sizes(node: LayoutNode, path: &[usize], new_sizes: &[f32]) -> LayoutNode {
    let LayoutNode::Split {
        direction,
        children,
        sizes,
    } = node
    else {
        return node;
    };

    if path.is_empty() {
        return LayoutNode::Split {
            direction,
            children,
            sizes: rescale_to_one(
                new_sizes
                    .iter()
                    .map(|size| size.max(MIN_SPLIT_FRACTION))
                    .collect(),
            ),
        };
    }

    let (index, rest) = (path[0], &path[1..]);
    LayoutNode::Split {
        direction,
        children: children
            .into_iter()
            .enumerate()
            .map(|(at, child)| {
                if at == index {
                    set_split_sizes(child, rest, new_sizes)
                } else {
                    child
                }
            })
            .collect(),
        sizes,
    }
}

fn rescale_to_one(sizes: Vec<f32>) -> Vec<f32> {
    let total: f32 = sizes.iter().sum();
    if total <= 0.0 {
        let share = 1.0 / sizes.len().max(1) as f32;
        return sizes.iter().map(|_| share).collect();
    }
    sizes.into_iter().map(|size| size / total).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Panes are whatever the application says they are; a name is enough to tell them apart.
    fn layout() -> Layout<&'static str> {
        Layout::with_pane("review")
    }

    fn pane_of(layout: &Layout<&'static str>, name: &str) -> PaneId {
        layout
            .find_pane(|pane| *pane == name)
            .unwrap_or_else(|| panic!("expected a {name} pane"))
            .0
    }

    #[test]
    fn an_arrangement_starts_as_one_frame_holding_one_pane() {
        let layout = layout();

        assert_eq!(layout.frame_ids().len(), 1);
        assert_eq!(layout.pane_count(), 1);
        assert!(layout.is_coherent());
    }

    #[test]
    fn a_new_arrangement_has_a_frame_to_drop_things_on_and_nothing_in_it() {
        let layout: Layout<&str> = Layout::new();

        assert_eq!(layout.frame_ids().len(), 1);
        assert!(layout.is_empty());
        assert!(layout.is_coherent());
    }

    #[test]
    fn names_are_never_handed_out_twice() {
        let mut layout = layout();
        let frame = layout.active_frame();
        let first = layout.add_pane(frame, "shell", None);
        let second = layout.add_pane(frame, "shell", None);

        assert_ne!(first, second);
        assert_eq!(layout.pane_count(), 3);
    }

    #[test]
    fn dropping_a_pane_on_an_edge_splits_the_frame() {
        let mut layout = layout();
        let frame = layout.active_frame();
        let shell = layout.add_pane(frame, "shell", None);

        layout.move_pane_to_frame(shell, frame, DropSide::Right, None);

        assert_eq!(layout.frame_ids().len(), 2);
        assert!(matches!(layout.root(), LayoutNode::Split { .. }));
        assert!(layout.is_coherent());
    }

    #[test]
    fn a_lone_tab_dropped_on_its_own_edge_changes_nothing() {
        let mut layout = layout();
        let frame = layout.active_frame();
        let review = pane_of(&layout, "review");

        layout.move_pane_to_frame(review, frame, DropSide::Bottom, None);

        assert_eq!(layout.frame_ids().len(), 1);
        assert!(layout.contains(review));
    }

    #[test]
    fn a_pane_dropped_against_the_workspace_takes_a_column_beside_everything() {
        // Two frames stacked one above the other, then a third pane against the right edge.
        let mut layout = layout();
        let frame = layout.active_frame();
        let stacked = layout.add_pane(frame, "shell", None);
        layout.move_pane_to_frame(stacked, frame, DropSide::Bottom, None);
        assert!(matches!(
            layout.root(),
            LayoutNode::Split {
                direction: SplitDirection::Column,
                ..
            }
        ));

        let moved = layout.add_pane(frame, "second shell", None);
        layout.move_pane_against_edge(moved, DropSide::Right, DEFAULT_EDGE_SHARE);

        let LayoutNode::Split {
            direction,
            children,
            ..
        } = layout.root()
        else {
            panic!("expected the workspace to be split");
        };
        assert_eq!(*direction, SplitDirection::Row, "the column runs down the side");
        assert_eq!(children.len(), 2);
        let LayoutNode::Frame { frame: column } = &children[1] else {
            panic!("expected the new column to be one frame");
        };
        assert_eq!(
            layout.frame(*column).expect("expected the column").panes(),
            [moved],
            "the moved pane should be the whole of the new right-hand column"
        );
        assert!(layout.is_coherent());
    }

    #[test]
    fn a_right_column_takes_its_share_from_the_existing_columns() {
        let mut layout = layout();

        layout.add_pane_against_edge(DropSide::Right, DEFAULT_EDGE_SHARE, "shell");

        let LayoutNode::Split { sizes, .. } = layout.root() else {
            panic!("expected the root to be a split");
        };
        assert_eq!(sizes.len(), 2);
        assert!((sizes.iter().sum::<f32>() - 1.0).abs() < 1e-5);
        assert!((sizes[1] - DEFAULT_EDGE_SHARE).abs() < 1e-5);
    }

    #[test]
    fn a_second_column_rescales_the_earlier_ones() {
        let mut layout = layout();
        layout.add_pane_against_edge(DropSide::Right, DEFAULT_EDGE_SHARE, "shell");

        layout.add_pane_against_edge(DropSide::Right, DEFAULT_EDGE_SHARE, "second shell");

        let LayoutNode::Split { sizes, children, .. } = layout.root() else {
            panic!("expected the root to be a split");
        };
        assert_eq!(children.len(), 3);
        assert!((sizes.iter().sum::<f32>() - 1.0).abs() < 1e-5);
    }

    #[test]
    fn closing_the_last_pane_of_a_split_frame_removes_that_frame() {
        let mut layout = layout();
        let frame = layout.active_frame();
        let shell = layout.add_pane_beside(frame, DropSide::Right, "shell");
        assert_eq!(layout.frame_ids().len(), 2);

        assert_eq!(layout.close_pane(shell), Some("shell"));

        assert_eq!(layout.frame_ids().len(), 1);
        assert!(layout.is_coherent());
    }

    #[test]
    fn closing_the_last_pane_keeps_one_empty_frame_as_a_drop_target() {
        let mut layout = layout();
        let review = pane_of(&layout, "review");

        layout.close_pane(review);

        assert_eq!(layout.frame_ids().len(), 1);
        assert!(layout.is_empty());
        assert!(layout.is_coherent());
    }

    #[test]
    fn reordering_tabs_within_a_frame_keeps_every_pane() {
        let mut layout = layout();
        let frame = layout.active_frame();
        let review = pane_of(&layout, "review");
        let shell = layout.add_pane(frame, "shell", None);

        layout.move_pane_to_frame(shell, frame, DropSide::Tabs, Some(review));

        assert_eq!(
            layout.frame(frame).expect("expected the frame").panes(),
            [shell, review]
        );
        assert!(layout.is_coherent());
    }

    #[test]
    fn a_tab_dragged_to_another_frames_strip_arrives_where_it_was_dropped() {
        let mut layout = layout();
        let frame = layout.active_frame();
        let first = layout.add_pane_beside(frame, DropSide::Right, "first");
        let column = layout.active_frame();
        let second = layout.add_pane(column, "second", None);
        let moving = pane_of(&layout, "review");

        layout.move_pane_to_frame(moving, column, DropSide::Tabs, Some(second));

        assert_eq!(
            layout.frame(column).expect("expected the column").panes(),
            [first, moving, second]
        );
        assert_eq!(layout.frame_ids().len(), 1, "the emptied frame went with it");
    }

    /// A move is not a close and an open: applications key their own state on a pane's name,
    /// so a dragged tab has to come out of the gesture as the same pane it went in as.
    #[test]
    fn a_moved_pane_keeps_its_name() {
        let mut layout = layout();
        let frame = layout.active_frame();
        let shell = layout.add_pane(frame, "shell", None);

        layout.move_pane_to_frame(shell, frame, DropSide::Right, None);
        assert!(layout.contains(shell), "after a split");

        layout.move_pane_against_edge(shell, DropSide::Bottom, DEFAULT_EDGE_SHARE);
        assert!(layout.contains(shell), "and after a move against the edge");

        let review_frame = layout.frame_of(pane_of(&layout, "review")).expect("a frame");
        layout.move_pane_to_frame(shell, review_frame, DropSide::Tabs, None);
        assert!(layout.contains(shell), "and after joining another strip");
        assert_eq!(layout.pane_count(), 2, "and there is still only one of it");
    }

    #[test]
    fn split_sizes_never_collapse_a_pane_to_nothing() {
        let mut layout = layout();
        let frame = layout.active_frame();
        layout.add_pane_beside(frame, DropSide::Right, "shell");

        layout.set_split_sizes(&[], &[0.0, 1.0]);

        let LayoutNode::Split { sizes, .. } = layout.root() else {
            panic!("expected the root to be a split");
        };
        assert!(sizes[0] >= MIN_SPLIT_FRACTION * 0.9);
        assert!((sizes.iter().sum::<f32>() - 1.0).abs() < 1e-5);
    }

    #[test]
    fn frame_holding_keeps_panes_of_a_kind_together() {
        let mut layout = layout();
        let review_frame = layout.active_frame();
        layout.add_pane_beside(review_frame, DropSide::Right, "shell");
        let shell_frame = layout.active_frame();

        assert_eq!(
            layout.frame_holding(review_frame, |pane| *pane == "shell"),
            Some(shell_frame)
        );
        assert_eq!(
            layout.frame_holding(review_frame, |pane| *pane == "review"),
            Some(review_frame)
        );
    }

    #[test]
    fn the_keyboard_walks_the_frames_in_the_order_they_are_laid_out() {
        let mut layout = layout();
        let first = layout.active_frame();
        layout.add_pane_beside(first, DropSide::Right, "shell");
        let second = layout.active_frame();

        layout.set_active_frame(first);
        assert_eq!(layout.focus_next_frame(), second);
        assert_eq!(layout.focus_next_frame(), first, "and round again");
    }

    #[test]
    fn a_kept_shape_comes_back_as_empty_frames() {
        let mut layout = layout();
        let frame = layout.active_frame();
        layout.add_pane_beside(frame, DropSide::Right, "shell");
        assert_eq!(layout.frame_ids().len(), 2);

        let taken = layout.take_panes();

        assert_eq!(taken.len(), 2, "the panes are handed back to their owner");
        assert_eq!(layout.frame_ids().len(), 2, "the splits are the point");
        assert!(layout.is_empty());
        assert!(
            layout
                .frame_ids()
                .iter()
                .filter_map(|frame| layout.frame(*frame))
                .all(|frame| frame.panes().is_empty() && frame.active_pane().is_none())
        );
        assert!(layout.is_coherent());
    }

    #[test]
    fn a_restored_shape_keeps_only_the_frames_something_landed_in() {
        let mut layout = layout();
        let frame = layout.active_frame();
        layout.add_pane_beside(frame, DropSide::Right, "shell");
        layout.take_panes();

        let primary = layout.primary_frame();
        layout.add_pane(primary, "review", None);
        layout.drop_empty_frames();

        assert_eq!(layout.frame_count(), 1, "the empty column should be gone");
        assert_eq!(layout.pane_count(), 1);
        assert!(layout.is_coherent());
    }

    #[test]
    fn the_last_frame_stays_even_with_nothing_in_it() {
        let mut layout = layout();
        layout.take_panes();

        layout.drop_empty_frames();

        assert_eq!(layout.frame_ids().len(), 1, "a workspace needs a drop target");
    }

    /// A restored arrangement carries on naming things where the stored one left off: a name
    /// handed out twice would put two frames in one box.
    #[test]
    fn names_from_a_restored_arrangement_are_not_handed_out_again() {
        let mut stored = layout();
        let frame = stored.active_frame();
        stored.add_pane_beside(frame, DropSide::Right, "shell");
        let known: Vec<FrameId> = stored.frame_ids();
        stored.take_panes();

        let refilled = stored.add_pane_beside(stored.primary_frame(), DropSide::Bottom, "review");
        let new_frame = stored.frame_of(refilled).expect("expected a frame");

        assert!(!known.contains(&new_frame), "{new_frame} was already taken");
        assert!(stored.is_coherent());
    }

    #[test]
    fn a_frame_that_was_never_there_is_not_a_place_to_put_a_pane() {
        let mut layout = layout();
        let missing = FrameId(999);

        layout.add_pane(missing, "shell", None);

        assert_eq!(layout.pane_count(), 1, "nothing was added");
        assert!(layout.is_coherent());
    }
}

#[cfg(all(test, feature = "serde"))]
mod serde_tests {
    use super::*;

    #[test]
    fn an_arrangement_survives_a_round_trip_through_json() {
        let mut layout = Layout::with_pane("review".to_string());
        let frame = layout.active_frame();
        layout.add_pane_beside(frame, DropSide::Right, "shell".to_string());

        let encoded = serde_json::to_string(&layout).expect("expected the layout to encode");
        let decoded: Layout<String> =
            serde_json::from_str(&encoded).expect("expected the layout to decode");

        assert!(decoded.is_coherent());
        assert_eq!(decoded.frame_ids(), layout.frame_ids());
        assert_eq!(decoded.active_frame(), layout.active_frame());
        assert_eq!(decoded.pane_count(), 2);
    }

    #[test]
    fn an_arrangement_whose_frames_disagree_with_its_tree_is_not_coherent() {
        let layout = Layout::with_pane("review".to_string());
        let encoded = serde_json::to_string(&layout).expect("expected the layout to encode");
        // What a truncated or outdated stored value looks like: a tree with no frames behind it.
        let broken = encoded.replace(&format!("\"{}\":", layout.primary_frame().0), "\"404\":");

        let decoded: Layout<String> =
            serde_json::from_str(&broken).expect("expected the layout to decode");

        assert!(!decoded.is_coherent());
    }
}
