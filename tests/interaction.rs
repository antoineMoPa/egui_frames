//! The gestures, driven through a real egui context: clicks, drags and drops on tabs.

use std::sync::{Arc, Mutex};

use egui_frames::{DropSide, Frames, FramesEvent, Layout, PaneId, PaneView, Tab};
use egui_kittest::{Harness, kittest::Queryable};

/// A workspace of named panes, and whatever it reported while it was drawn.
struct Workspace {
    frames: Frames,
    layout: Layout<String>,
    events: Vec<FramesEvent>,
    /// Whether the tabs of the active frame wear the chord that raises them, the way the
    /// application vendoring this crate stamps cmd+1..cmd+9 on the frame the keyboard is in.
    /// A frame that becomes active grows its tabs by that label, which is what makes the
    /// strip move under a pointer that has only just gone down on it.
    shortcuts: bool,
    /// The tabs wearing a chord this time round, worked out before the arrangement is drawn —
    /// the layout is lent to `show` for the length of the draw, so a tab cannot look it up.
    shortcut_panes: Vec<PaneId>,
    /// The tab whose title is open for retyping, the way an application answers a double
    /// click on a tab it lets be renamed.
    editing: Option<PaneId>,
}

impl PaneView<String> for Workspace {
    fn tab(&mut self, pane: PaneId, name: &String) -> Tab {
        let tab = Tab::new(name).editing(self.editing == Some(pane));
        match self
            .shortcut_panes
            .iter()
            .position(|stamped| *stamped == pane)
        {
            Some(index) => tab.with_indicator(format!("cmd+{}", index + 1)),
            None => tab,
        }
    }

    fn pane_ui(&mut self, ui: &mut egui::Ui, _pane: PaneId, name: &String) {
        ui.label(name.as_str());
    }

    fn tab_editor_ui(&mut self, ui: &mut egui::Ui, _pane: PaneId, name: &String) {
        ui.label(format!("editing {name}"));
    }
}

impl Workspace {
    fn draw(&mut self, ui: &mut egui::Ui) {
        if self.shortcuts {
            let active = self.layout.active_frame();
            self.shortcut_panes = self
                .layout
                .frame(active)
                .map(|open| open.panes().to_vec())
                .unwrap_or_default();
        }
        let mut frames = std::mem::take(&mut self.frames);
        let mut layout = std::mem::take(&mut self.layout);
        self.events = frames.show(ui, &mut layout, self);
        self.layout = layout;
        self.frames = frames;
    }
}

/// A workspace of the given arrangement, drawn in a window-sized harness. `step_dt` is how
/// much time one drawn frame takes, which matters to any gesture held across several of them:
/// egui only calls a press a click if the button comes back up inside its click window.
fn harness_over(workspace: &Arc<Mutex<Workspace>>, step_dt: f32) -> Harness<'static> {
    let drawn = Arc::clone(workspace);
    Harness::builder()
        .with_size(egui::vec2(900.0, 600.0))
        .with_step_dt(step_dt)
        .build_ui(move |ui| {
            drawn.lock().expect("expected the workspace").draw(ui);
        })
}

/// What a workspace is before anything is arranged in it.
fn empty_workspace() -> Workspace {
    Workspace {
        frames: Frames::new(),
        layout: Layout::new(),
        events: Vec::new(),
        shortcuts: false,
        shortcut_panes: Vec::new(),
        editing: None,
    }
}

/// The panes named, side by side as tabs of one frame.
fn workspace(panes: &[&str]) -> (Arc<Mutex<Workspace>>, Harness<'static>, Vec<PaneId>) {
    let mut state = empty_workspace();
    let frame = state.layout.active_frame();
    let ids = panes
        .iter()
        .map(|name| state.layout.add_pane(frame, (*name).to_string(), None))
        .collect();

    let workspace = Arc::new(Mutex::new(state));
    let harness = harness_over(&workspace, DEFAULT_STEP_DT);
    (workspace, harness, ids)
}

/// A drawn frame's worth of time, as long as kittest's own default: the tests that watch a
/// tab walk to a new place want it long enough that an animation is over in a step or two.
const DEFAULT_STEP_DT: f32 = 1.0 / 4.0;

/// A drawn frame's worth of time for the tests that hold a button down across several of
/// them, where a real pointer would be down for a fraction of a second all told.
const CLICK_STEP_DT: f32 = 1.0 / 60.0;

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

/// Dropping a tab on another frame's heading reorders it into that frame.
///
/// The band down the outer edge of the workspace is deeper than a heading is tall, so every
/// heading along the top of the workspace sits inside the top band. While the band won, this
/// drop turned the tab into a row above everything instead — which is why reordering only
/// worked from the middle of a frame.
#[test]
fn dropping_a_tab_on_another_frames_heading_joins_that_frames_tabs() {
    let (workspace, mut harness, panes) = workspace(&["review", "shell"]);
    harness.run();

    // Split first, so there are two frames and the outer edge is in play at all.
    let first = workspace.lock().unwrap().layout.active_frame();
    let frame_rect = workspace
        .lock()
        .unwrap()
        .frames
        .frame_rect(first)
        .expect("expected the frame to have been drawn");
    let from = tab_center(&workspace, panes[1]);
    let to = egui::pos2(frame_rect.max.x - 12.0, frame_rect.center().y);
    press(&mut harness, from, true);
    for at in [from + egui::vec2(30.0, 20.0), to] {
        harness.input_mut().events.push(egui::Event::PointerMoved(at));
        harness.step();
    }
    press(&mut harness, to, false);
    harness.run();
    assert_eq!(workspace.lock().unwrap().layout.frame_count(), 2);

    // Now carry the right-hand pane's tab back onto the left frame's heading, which is inside
    // the workspace's top band.
    let left = workspace
        .lock()
        .unwrap()
        .layout
        .frame_of(panes[0])
        .expect("expected the first pane to be in a frame");
    let left_rect = workspace
        .lock()
        .unwrap()
        .frames
        .frame_rect(left)
        .expect("expected the left frame to have been drawn");
    let heading = egui::pos2(
        left_rect.center().x,
        left_rect.min.y + workspace.lock().unwrap().frames.style().tab_strip_height() / 2.0,
    );

    let from = tab_center(&workspace, panes[1]);
    press(&mut harness, from, true);
    for at in [from + egui::vec2(-30.0, 6.0), heading] {
        harness.input_mut().events.push(egui::Event::PointerMoved(at));
        harness.step();
    }
    press(&mut harness, heading, false);
    harness.run();

    let state = workspace.lock().unwrap();
    assert_eq!(
        state.layout.frame_count(),
        1,
        "the tab joined the other frame rather than becoming a row of its own"
    );
    assert_eq!(
        state.layout.frame_of(panes[1]),
        Some(left),
        "and it landed in the frame whose heading it was dropped on"
    );
    assert_eq!(state.layout.pane_count(), 2, "nothing was lost doing it");
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

/// A title cut short while the strip was crowded grows back once the other tabs close and
/// their room re-becomes available.
#[test]
fn a_cut_title_grows_back_when_tabs_close() {
    let long = "a ridiculously long shell title that is cut short while the strip is crowded";
    let (workspace, mut harness, panes) = workspace(&[long; 6]);
    harness.run();

    let width_of = |workspace: &Arc<Mutex<Workspace>>, pane| {
        workspace
            .lock()
            .expect("expected the workspace")
            .frames
            .tab_rect(pane)
            .expect("expected the tab to have been drawn")
            .width()
    };
    let crowded = width_of(&workspace, panes[0]);

    for pane in &panes[1..] {
        workspace
            .lock()
            .expect("expected the workspace")
            .layout
            .close_pane(*pane);
    }
    // Long enough for the title to finish walking out to the width it was granted.
    for _ in 0..30 {
        harness.step();
    }

    let alone = width_of(&workspace, panes[0]);
    assert!(
        alone > crowded + 50.0,
        "a lone tab should have grown into the freed strip: {crowded} then {alone}"
    );
}

/// A close mark in a frame the keyboard is not in closes its tab on the first click.
///
/// The press that lands on the mark is also what makes that frame the active one, and an
/// application that marks the active frame's tabs — with the chord that raises each of them,
/// as this one does — widens every tab in the strip. The mark walks out from under the
/// pointer while the button is still down, and the click belongs to the mark it went down on
/// rather than to whatever has slid into its place.
#[test]
fn a_close_mark_in_an_inactive_frame_closes_on_the_first_click() {
    let mut state = empty_workspace();
    state.shortcuts = true;
    let here = state.layout.active_frame();
    state.layout.add_pane(here, "review".to_string(), None);
    let beside = state
        .layout
        .add_pane_beside(here, DropSide::Right, "shell".to_string());
    let there = state.layout.frame_of(beside).expect("expected the frame");
    let aside = state.layout.add_pane(there, "notes".to_string(), None);
    state.layout.set_active_frame(here);

    let workspace = Arc::new(Mutex::new(state));
    let mut harness = harness_over(&workspace, CLICK_STEP_DT);
    harness.run();

    // The mark at the right-hand end of the tab, where the user sees it before pressing: the
    // pointer holds that spot for the whole click.
    let at = {
        let state = workspace.lock().expect("expected the workspace");
        assert_ne!(
            state.layout.active_frame(),
            there,
            "the tab being closed is in the frame the keyboard is not in"
        );
        let tab = state
            .frames
            .tab_rect(aside)
            .expect("expected the tab to have been drawn");
        egui::pos2(tab.max.x - 10.0, tab.center().y)
    };
    press(&mut harness, at, true);
    // The tabs of the frame the press landed in take their chords and grow by them, which is
    // over in a few frames — well inside the time a button stays down for a click.
    for _ in 0..10 {
        harness.step();
    }
    press(&mut harness, at, false);

    let state = workspace.lock().expect("expected the workspace");
    assert_eq!(
        state.events,
        vec![FramesEvent::PaneCloseRequested(aside)],
        "the tab whose close mark was pressed is the one asked to close"
    );
}

/// A double click on a tab is reported and nothing more: what it asks for — a rename, say —
/// is the application's to answer.
#[test]
fn double_clicking_a_tab_tells_the_application() {
    // Drawn a frame at a time at a real frame's pace: egui only calls two clicks a double
    // when the second lands within a fraction of a second of the first.
    let (workspace, _, panes) = workspace(&["review", "shell"]);
    let mut harness = harness_over(&workspace, CLICK_STEP_DT);
    harness.run();

    let at = tab_center(&workspace, panes[1]);
    press(&mut harness, at, true);
    press(&mut harness, at, false);
    press(&mut harness, at, true);
    press(&mut harness, at, false);

    let state = workspace.lock().unwrap();
    assert_eq!(
        state.events,
        vec![FramesEvent::TabDoubleClicked(panes[1])],
        "the second click of a double is reported as one"
    );
    assert_eq!(
        state.layout.frame(state.layout.active_frame()).unwrap().active_pane(),
        Some(panes[1]),
        "and the first of the two has already brought the pane forward"
    );
}

/// A tab that is editing is drawn as the application's editor, in the tab's own place.
#[test]
fn an_editing_tab_is_drawn_as_the_applications_editor() {
    let (workspace, mut harness, panes) = workspace(&["review", "shell"]);
    harness.run();
    let before = workspace
        .lock()
        .unwrap()
        .frames
        .tab_rect(panes[1])
        .expect("expected the tab to have been drawn");

    workspace.lock().unwrap().editing = Some(panes[1]);
    harness.run();

    assert!(
        harness.query_by_label("editing shell").is_some(),
        "the editor should be drawn in the tab"
    );
    let editing = workspace
        .lock()
        .unwrap()
        .frames
        .tab_rect(panes[1])
        .expect("expected the editing tab to have been drawn");
    assert!(
        editing.min.x == before.min.x && editing.width() > before.width(),
        "the editor takes the tab's place, widened to hold what is typed: {before:?} -> {editing:?}"
    );
}
