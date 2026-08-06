//! The gestures, driven through a real egui context: clicks, drags and drops on tabs.

use std::sync::{Arc, Mutex};

use egui_frames::{Frames, FramesEvent, Layout, PaneId, PaneView, Tab};
use egui_kittest::Harness;

/// A workspace of named panes, and whatever it reported while it was drawn.
struct Workspace {
    frames: Frames,
    layout: Layout<String>,
    events: Vec<FramesEvent>,
}

impl PaneView<String> for Workspace {
    fn tab(&mut self, _pane: PaneId, name: &String) -> Tab {
        Tab::new(name)
    }

    fn pane_ui(&mut self, ui: &mut egui::Ui, _pane: PaneId, name: &String) {
        ui.label(name.as_str());
    }
}

impl Workspace {
    fn draw(&mut self, ui: &mut egui::Ui) {
        let mut frames = std::mem::take(&mut self.frames);
        let mut layout = std::mem::take(&mut self.layout);
        self.events = frames.show(ui, &mut layout, self);
        self.layout = layout;
        self.frames = frames;
    }
}

/// Two panes in one frame, drawn in a window-sized harness.
fn workspace(panes: &[&str]) -> (Arc<Mutex<Workspace>>, Harness<'static>, Vec<PaneId>) {
    let mut layout = Layout::new();
    let frame = layout.active_frame();
    let ids = panes
        .iter()
        .map(|name| layout.add_pane(frame, (*name).to_string(), None))
        .collect();

    let workspace = Arc::new(Mutex::new(Workspace {
        frames: Frames::new(),
        layout,
        events: Vec::new(),
    }));

    let drawn = Arc::clone(&workspace);
    let harness = Harness::builder()
        .with_size(egui::vec2(900.0, 600.0))
        .build_ui(move |ui| {
            drawn.lock().expect("expected the workspace").draw(ui);
        });

    (workspace, harness, ids)
}

fn tab_center(workspace: &Arc<Mutex<Workspace>>, pane: PaneId) -> egui::Pos2 {
    workspace
        .lock()
        .expect("expected the workspace")
        .frames
        .tab_rect(pane)
        .expect("expected the tab to have been drawn")
        .center()
}

fn press(harness: &mut Harness<'_>, at: egui::Pos2, pressed: bool) {
    harness.input_mut().events.extend([
        egui::Event::PointerMoved(at),
        egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        },
    ]);
    harness.step();
}

#[test]
fn clicking_a_tab_brings_it_to_the_front() {
    let (workspace, mut harness, panes) = workspace(&["review", "shell"]);
    harness.run();
    assert_eq!(
        workspace.lock().unwrap().layout.active_pane().map(|(id, _)| id),
        Some(panes[1]),
        "the pane opened last starts in front"
    );

    let at = tab_center(&workspace, panes[0]);
    press(&mut harness, at, true);
    press(&mut harness, at, false);
    harness.run();

    let state = workspace.lock().unwrap();
    assert_eq!(
        state.layout.active_pane().map(|(id, _)| id),
        Some(panes[0]),
        "clicking a tab is what brings its pane forward"
    );
    assert!(state.events.is_empty(), "and asks nothing of the application");
}

/// The gesture the whole widget exists for: a tab dragged out of a strip and dropped against
/// the right-hand edge of its own frame, which splits the frame in two.
#[test]
fn dragging_a_tab_to_a_frames_edge_splits_the_frame() {
    let (workspace, mut harness, panes) = workspace(&["review", "shell"]);
    harness.run();
    let frame_before = workspace.lock().unwrap().layout.active_frame();

    let from = tab_center(&workspace, panes[1]);
    let frame_rect = workspace
        .lock()
        .unwrap()
        .frames
        .frame_rect(frame_before)
        .expect("expected the frame to have been drawn");
    // Well down the right-hand side of the body, which is the "split here" band.
    let to = egui::pos2(frame_rect.max.x - 12.0, frame_rect.center().y);

    press(&mut harness, from, true);
    for at in [from + egui::vec2(30.0, 20.0), to] {
        harness.input_mut().events.push(egui::Event::PointerMoved(at));
        harness.step();
    }
    press(&mut harness, to, false);
    harness.run();

    let state = workspace.lock().unwrap();
    assert_eq!(state.layout.frame_count(), 2, "the frame was split in two");
    assert_eq!(state.layout.pane_count(), 2, "and nothing was lost doing it");
    assert!(
        state.layout.contains(panes[1]),
        "a dragged pane keeps the name it was known by"
    );
    assert_ne!(
        state.layout.frame_of(panes[0]),
        state.layout.frame_of(panes[1]),
        "the two panes ended up in frames of their own"
    );
    assert!(state.frames.dragged_pane().is_none(), "the drag is over");
    assert!(state.layout.is_coherent());
}

/// A drag released outside every frame cancels, rather than leaving the tab on the pointer.
#[test]
fn a_tab_dropped_outside_the_workspace_stays_where_it_was() {
    let (workspace, mut harness, panes) = workspace(&["review", "shell"]);
    harness.run();
    let frame = workspace.lock().unwrap().layout.active_frame();

    let from = tab_center(&workspace, panes[1]);
    let outside = egui::pos2(4000.0, 4000.0);
    press(&mut harness, from, true);
    harness.input_mut().events.push(egui::Event::PointerMoved(outside));
    harness.step();
    press(&mut harness, outside, false);
    harness.run();

    let state = workspace.lock().unwrap();
    assert_eq!(state.layout.frame_count(), 1);
    assert_eq!(state.layout.frame_of(panes[1]), Some(frame));
    assert!(state.frames.dragged_pane().is_none(), "the drag is over");
}

#[test]
fn clicking_a_close_mark_asks_the_application_to_close_that_pane() {
    let (workspace, mut harness, panes) = workspace(&["review", "shell"]);
    harness.run();

    // The close mark sits at the right-hand end of the tab in front.
    let tab = workspace
        .lock()
        .unwrap()
        .frames
        .tab_rect(panes[1])
        .expect("expected the tab to have been drawn");
    let at = egui::pos2(tab.max.x - 10.0, tab.center().y);
    press(&mut harness, at, true);
    press(&mut harness, at, false);

    let state = workspace.lock().unwrap();
    assert_eq!(
        state.events,
        vec![FramesEvent::PaneCloseRequested(panes[1])],
        "closing is the application's to do, so it is asked rather than told"
    );
    assert_eq!(
        state.layout.pane_count(),
        2,
        "and nothing closed behind its back"
    );
}

#[test]
fn the_new_tab_button_asks_the_application_for_a_tab() {
    let (workspace, mut harness, _) = workspace(&["review"]);
    harness.run();
    let frame = workspace.lock().unwrap().layout.active_frame();
    let frame_rect = workspace
        .lock()
        .unwrap()
        .frames
        .frame_rect(frame)
        .expect("expected the frame to have been drawn");

    // The button is the last thing on the strip, against its right-hand edge.
    let strip_height = workspace.lock().unwrap().frames.style().tab_strip_height();
    let at = egui::pos2(
        frame_rect.max.x - 5.0 - 9.0,
        frame_rect.min.y + strip_height / 2.0 + 0.5,
    );
    press(&mut harness, at, true);
    press(&mut harness, at, false);

    assert_eq!(
        workspace.lock().unwrap().events,
        vec![FramesEvent::NewTabRequested(frame)]
    );
}
