//! Frames tests - drawn headless through egui, against the public API.

use crate::{Frames, Layout, PaneId, PaneView, Tab};
use std::collections::HashMap;

/// Writes down the id of the ui each pane's body was drawn in.
#[derive(Default)]
struct IdsSeen(HashMap<PaneId, egui::Id>);

impl PaneView<&'static str> for IdsSeen {
    fn tab(&mut self, _pane: PaneId, name: &&'static str) -> Tab {
        Tab::new(*name)
    }

    fn pane_ui(&mut self, ui: &mut egui::Ui, pane: PaneId, _name: &&'static str) {
        self.0.insert(pane, ui.id());
    }
}

#[test]
fn tabs_of_one_frame_draw_under_their_own_ids() {
    let mut layout = Layout::with_pane("a.rs");
    let first = layout.active_pane().unwrap().0;
    let second = layout.add_pane(layout.primary_frame(), "b.rs", None);

    let ctx = egui::Context::default();
    let mut frames = Frames::new();
    let mut seen = IdsSeen::default();
    for pane in [first, second] {
        layout.focus_pane(pane);
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            frames.show(ui, &mut layout, &mut seen);
        });
        output.textures_delta.clear();
    }

    assert_eq!(seen.0.len(), 2, "both tabs drew their body");
    assert_ne!(seen.0[&first], seen.0[&second]);
}
