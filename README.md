# egui_frames

Tabs, splits and draggable panes for [egui](https://github.com/emilk/egui).

A workspace is a tree of splits whose leaves are *frames*. A frame holds a strip of tabs and
draws whichever pane is in front. Tabs drag between frames, drop against a frame's edge to split
it in two, or drop against the outer edge of everything to take a column of their own — the
arrangement an editor or a terminal gives you, without the editor or the terminal.

- **`Layout<P>`** is the arrangement. `P` is whatever a pane is in your application; the layout
  holds it and hands it back. No egui in it, so it can be stored, restored and tested on its
  own — with the `serde` feature, it round-trips through JSON.
- **`Frames`** draws that arrangement, and turns pointer gestures into the next one.

```rust
impl PaneView<String> for Editor {
    fn tab(&mut self, _pane: PaneId, path: &String) -> Tab {
        Tab::new(path).with_hover(path)
    }

    fn pane_ui(&mut self, ui: &mut egui::Ui, _pane: PaneId, path: &String) {
        ui.label(format!("the contents of {path}"));
    }
}

// once per frame — the view is the application itself, so both are lent out for the call
let mut frames = std::mem::take(&mut self.frames);
let mut layout = std::mem::take(&mut self.layout);
for event in frames.show(ui, &mut layout, self) {
    match event {
        FramesEvent::PaneCloseRequested(pane) => { layout.close_pane(pane); }
        FramesEvent::NewTabRequested(frame) => { layout.add_pane(frame, doc, None); }
    }
}
self.layout = layout;
self.frames = frames;
```

Nothing is closed or opened behind your back: the widget reports what the user asked for, and
your application decides what it costs.

```sh
cargo run --example panes
```

## What is left to you

Everything about what a pane *is*: where new ones go, what a tab is called, what closing one
costs, and whether whatever is running inside it should outlive its tab. The arrangement has
opinions about none of that, which is what makes it reusable.

## License

MIT
