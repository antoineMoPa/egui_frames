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

/// Two frames side by side, the left one active, drawn with `window_over_right` floating a
/// window over the right one, then pressed at the middle of the right one.
fn press_over_the_right_frame(
    window_over_right: bool,
) -> (crate::FrameId, crate::FrameId, crate::FrameId) {
    let mut layout = Layout::with_pane("left");
    let left = layout.primary_frame();
    let right_pane = layout.add_pane_beside(left, crate::DropSide::Right, "right");
    let right = layout.frame_of(right_pane).unwrap();
    layout.set_active_frame(left);

    let ctx = egui::Context::default();
    let mut frames = Frames::new();
    let mut seen = IdsSeen::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
    let at = egui::pos2(600.0, 220.0);
    let mut time = 0.0;
    for events in [
        vec![],
        vec![],
        vec![],
        vec![egui::Event::PointerMoved(at)],
        vec![egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Default::default(),
        }],
    ] {
        let input = egui::RawInput {
            screen_rect: Some(screen),
            time: Some(time),
            events,
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| {
            frames.show(ui, &mut layout, &mut seen);
            if window_over_right {
                egui::Window::new("picker")
                    .fixed_pos(egui::pos2(500.0, 200.0))
                    .fixed_size(egui::vec2(200.0, 200.0))
                    .show(ui.ctx(), |ui| {
                        ui.label("a model");
                    });
            }
        });
        output.textures_delta.clear();
        time += 0.5;
    }
    (layout.active_frame(), left, right)
}

#[test]
fn a_press_in_a_frame_makes_it_active() {
    let (active, _, right) = press_over_the_right_frame(false);
    assert_eq!(active, right);
}

#[test]
fn a_press_on_a_window_floating_over_a_frame_leaves_the_active_frame_alone() {
    let (active, left, _) = press_over_the_right_frame(true);
    assert_eq!(active, left);
}
