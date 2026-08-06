//! A workspace to drag tabs around in: `cargo run --example panes`.
//!
//! Drop a tab on another strip to join it, near a frame's edge to split that frame, or right
//! against the edge of the window to take a column of your own.

use egui_frames::{Frames, FramesEvent, FramesStyle, Layout, PaneId, PaneView, Tab};

/// What a pane is here: a name, and a note to type into. In a real application this is your
/// document handle, your session, your terminal.
struct Note {
    title: String,
    body: String,
}

struct Demo {
    frames: Frames,
    layout: Layout<Note>,
    opened: usize,
    /// Set light or dark once, on the first frame, from whatever the desktop asked for.
    styled: bool,
    /// An edit made while a pane was lent out for drawing, applied once it is back.
    edited: Option<(PaneId, String)>,
}

impl PaneView<Note> for Demo {
    fn tab(&mut self, _pane: PaneId, note: &Note) -> Tab {
        // The dot marks a note with something typed into it — the "unsaved" a tab usually says.
        Tab::new(&note.title)
            .with_marker(!note.body.is_empty())
            .with_hover(&note.title)
    }

    fn pane_ui(&mut self, ui: &mut egui::Ui, pane: PaneId, note: &Note) {
        let mut body = note.body.clone();
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            ui.add(
                egui::TextEdit::multiline(&mut body)
                    .hint_text("type something")
                    .desired_width(ui.available_width() - 8.0)
                    .desired_rows(6),
            );
        });
        // The pane is lent out for drawing, so an edit is applied once the borrow is over.
        if body != note.body {
            self.edited = Some((pane, body));
        }
    }

    fn empty_frame_ui(&mut self, ui: &mut egui::Ui, _frame: egui_frames::FrameId) {
        ui.centered_and_justified(|ui| {
            ui.label(
                egui::RichText::new("nothing open — the + opens a note")
                    .color(ui.visuals().weak_text_color()),
            );
        });
    }

    fn tab_strip_end(&mut self, ui: &mut egui::Ui, _frame: egui_frames::FrameId, primary: bool) {
        if primary {
            ui.label(
                egui::RichText::new(format!("{} open", self.layout.pane_count()))
                    .size(10.0)
                    .color(ui.visuals().weak_text_color()),
            );
        }
    }
}

impl eframe::App for Demo {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if !self.styled {
            *self.frames.style_mut() = FramesStyle::from_visuals(ui.visuals());
            self.styled = true;
        }

        // The view is this application, so the widget and the arrangement are both lent out.
        let mut frames = std::mem::take(&mut self.frames);
        let mut layout = std::mem::take(&mut self.layout);
        let events = frames.show(ui, &mut layout, self);
        self.layout = layout;
        self.frames = frames;

        for event in events {
            match event {
                FramesEvent::PaneCloseRequested(pane) => {
                    self.layout.close_pane(pane);
                }
                FramesEvent::NewTabRequested(frame) => {
                    self.opened += 1;
                    let note = Note {
                        title: format!("note {}", self.opened),
                        body: String::new(),
                    };
                    self.layout.add_pane(frame, note, None);
                }
            }
        }

        if let Some((pane, body)) = self.edited.take()
            && let Some(note) = self.layout.pane_mut(pane)
        {
            note.body = body;
        }
    }
}

fn main() -> eframe::Result {
    let layout = Layout::with_pane(Note {
        title: "note 1".to_string(),
        body: "drag my tab somewhere".to_string(),
    });

    eframe::run_native(
        "egui_frames",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default().with_inner_size([1000.0, 640.0]),
            ..Default::default()
        },
        Box::new(|_creation| {
            Ok(Box::new(Demo {
                frames: Frames::new(),
                layout,
                opened: 1,
                styled: false,
                edited: None,
            }))
        }),
    )
}
