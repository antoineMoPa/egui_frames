//! Tabs, splits and draggable panes for [egui](https://github.com/emilk/egui).
//!
//! A workspace is a tree of splits whose leaves are *frames*. A frame holds a strip of tabs
//! and draws whichever of its panes is in front. Tabs can be dragged between frames, dropped
//! against a frame's edge to split it in two, or dropped against the outer edge of everything
//! to take a column of their own — the arrangement an editor or a terminal gives you, without
//! the editor or the terminal.
//!
//! Two types do the work:
//!
//! - [`Layout<P>`] is the arrangement: what is open, where, and which of it is in front. `P` is
//!   whatever a pane is in your application; the layout holds it and hands it back. It is plain
//!   data with no egui in it, so it can be stored, restored and tested on its own — with the
//!   `serde` feature, it round-trips through JSON.
//! - [`Frames`] draws that arrangement and turns pointer gestures into the next one.
//!
//! You supply a [`PaneView`], which names each pane's tab and draws its body:
//!
//! ```no_run
//! use egui_frames::{Frames, FramesEvent, Layout, PaneId, PaneView, Tab};
//!
//! struct Editor {
//!     frames: Frames,
//!     layout: Layout<String>,
//! }
//!
//! impl PaneView<String> for Editor {
//!     fn tab(&mut self, _pane: PaneId, path: &String) -> Tab {
//!         Tab::new(path).with_hover(path)
//!     }
//!
//!     fn pane_ui(&mut self, ui: &mut egui::Ui, _pane: PaneId, path: &String) {
//!         ui.label(format!("the contents of {path}"));
//!     }
//! }
//!
//! impl Editor {
//!     fn draw(&mut self, ui: &mut egui::Ui) {
//!         // The view is the application itself, so both are lent out for the call.
//!         let mut frames = std::mem::take(&mut self.frames);
//!         let mut layout = std::mem::take(&mut self.layout);
//!
//!         for event in frames.show(ui, &mut layout, self) {
//!             match event {
//!                 FramesEvent::PaneCloseRequested(pane) => {
//!                     layout.close_pane(pane);
//!                 }
//!                 FramesEvent::NewTabRequested(frame) => {
//!                     layout.add_pane(frame, "untitled".to_string(), None);
//!                 }
//!             }
//!         }
//!
//!         self.layout = layout;
//!         self.frames = frames;
//!     }
//! }
//! ```
//!
//! Run `cargo run --example panes` for a workspace to drag tabs around in.
//!
//! # What is left to the application
//!
//! Everything about what a pane *is*: where new ones go, what closing one costs, what a tab is
//! called, and whether a shell in one should keep running when its tab goes away. The
//! arrangement has opinions about none of that, which is what makes it reusable.

#![forbid(unsafe_code)]
#![warn(missing_docs, clippy::doc_markdown)]

mod frames;
mod id;
mod layout;
mod style;

pub use frames::{Frames, FramesEvent, PaneView, Tab};
pub use id::{FrameId, PaneId};
pub use layout::{
    DEFAULT_EDGE_SHARE, DropSide, Frame, Layout, LayoutNode, SplitDirection,
};
pub use style::FramesStyle;
