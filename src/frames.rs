//! The arrangement on screen: nested splits, a tab strip per frame, draggable tabs.
//!
//! [`Layout`] owns what the arrangement *is*; this owns how it is drawn and how pointer
//! gestures turn into the next arrangement.

use egui::{
    Align, Color32, CornerRadius, CursorIcon, Id, LayerId, Layout as UiLayout, Order, Pos2, Rect,
    Response, Sense, Stroke, StrokeKind, Ui, UiBuilder, Vec2, pos2, vec2,
};

use crate::{
    DropSide, FrameId, Layout, LayoutNode, PaneId, SplitDirection,
    layout::DEFAULT_EDGE_SHARE,
    style::{FRAME_BORDER, FramesStyle},
};

/// A tab's title, and what it carries beside it.
#[derive(Clone, Debug)]
pub struct Tab {
    /// What the tab is called. Cut short with an ellipsis if it does not fit.
    pub title: String,
    /// A dot before the title, for a pane with something outstanding — a file with unsaved
    /// edits, a shell that finished while it was in the background.
    pub marker: bool,
    /// What the tab says when the pointer rests on it. The title is a good default when it
    /// might have been cut short.
    pub hover: Option<String>,
    /// Whether the tab offers a close mark, and answers a middle click.
    pub closable: bool,
}

impl Tab {
    /// A closable tab with a title and nothing else.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            marker: false,
            hover: None,
            closable: true,
        }
    }

    /// Put a dot before the title.
    #[must_use]
    pub fn with_marker(mut self, marker: bool) -> Self {
        self.marker = marker;
        self
    }

    /// What the tab says when the pointer rests on it.
    #[must_use]
    pub fn with_hover(mut self, hover: impl Into<String>) -> Self {
        self.hover = Some(hover.into());
        self
    }

    /// Whether this pane can be closed from its tab.
    #[must_use]
    pub fn closable(mut self, closable: bool) -> Self {
        self.closable = closable;
        self
    }
}

/// What an application has to say about its own panes for them to be drawn.
///
/// `P` is the pane type the [`Layout`] holds.
pub trait PaneView<P> {
    /// The tab this pane is shown by.
    fn tab(&mut self, id: PaneId, pane: &P) -> Tab;

    /// Draw the pane, in whatever space its frame has left below the tab strip.
    fn pane_ui(&mut self, ui: &mut Ui, id: PaneId, pane: &P);

    /// Draw a frame that has nothing in it — the state a workspace is left in when its last
    /// tab is closed. The default draws nothing.
    fn empty_frame_ui(&mut self, _ui: &mut Ui, _frame: FrameId) {}

    /// Draw whatever the application wants at the right-hand end of a tab strip, outside the
    /// new-tab button. `primary` marks the top-left frame, whose strip is the natural home for
    /// window-wide controls. The default draws nothing.
    fn tab_strip_end(&mut self, _ui: &mut Ui, _frame: FrameId, _primary: bool) {}
}

/// Something the user asked of the application while its arrangement was being drawn.
///
/// The arrangement itself is dealt with in place — a dropped tab has already moved by the time
/// [`Frames::show`] returns. These are the requests only the application can answer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FramesEvent {
    /// A tab's close mark was clicked, or it was middle-clicked.
    ///
    /// Nothing has closed yet: an application that has to ask first — a file with unsaved
    /// edits — can put the question up instead, and call [`Layout::close_pane`] when it has an
    /// answer.
    PaneCloseRequested(PaneId),
    /// The new-tab button on a frame's strip was clicked. What a new tab is, is the
    /// application's business.
    NewTabRequested(FrameId),
}

/// How close to the outer edge of the whole arrangement a dropped tab has to land to become a
/// column or row beside everything, rather than a split of the frame it happens to be over.
/// Stacked frames have no other way of saying "down the right of both".
const WORKSPACE_EDGE: f32 = 34.0;
/// How close to a frame's left or right edge a dropped tab has to land to split it there, as a
/// share of the frame's width.
const SIDE_EDGE_FRACTION: f32 = 0.22;
/// The same for the top and bottom edges, as a share of the frame's body. Deeper than the
/// sides: a frame is wider than it is tall, so an equal share of the height is a much shorter
/// band, and reaching a bottom split would mean dragging to the window's very edge.
const UP_DOWN_EDGE_FRACTION: f32 = 0.38;

/// How long a tab takes to walk to a new place in its strip. Long enough to be followed by
/// eye, short enough that the strip is never waiting on it.
const TAB_SLIDE: f32 = 0.12;

/// Space between the end of a tab's title and its close mark.
const TAB_CLOSE_GAP: f32 = 5.0;
const TAB_CLOSE_SIZE: f32 = 12.0;
const TAB_CLOSE_INSET: f32 = 4.0;
/// Where a tab's title starts.
const TAB_TEXT_INSET: f32 = 8.0;
/// Room before the title for the dot a marked tab carries.
const TAB_MARKER_SPACE: f32 = 11.0;

/// The widget: draws an arrangement, and turns pointer gestures into the next one.
///
/// Keep one of these beside the [`Layout`] it draws — it holds what a drag in flight knows,
/// and where everything was drawn, which is what makes a drop land on what the user was
/// actually looking at.
///
/// ```no_run
/// # use egui_frames::{Frames, Layout, PaneId, PaneView, Tab};
/// # struct MyApp { frames: Frames, layout: Layout<String> }
/// # impl PaneView<String> for MyApp {
/// #     fn tab(&mut self, _: PaneId, pane: &String) -> Tab { Tab::new(pane) }
/// #     fn pane_ui(&mut self, ui: &mut egui::Ui, _: PaneId, pane: &String) { ui.label(pane); }
/// # }
/// impl MyApp {
///     fn draw(&mut self, ui: &mut egui::Ui) {
///         // The view is the application itself, so both are lent out for the call.
///         let mut frames = std::mem::take(&mut self.frames);
///         let mut layout = std::mem::take(&mut self.layout);
///         for event in frames.show(ui, &mut layout, self) {
///             // close panes, open tabs
///         }
///         self.layout = layout;
///         self.frames = frames;
///     }
/// }
/// ```
pub struct Frames {
    style: FramesStyle,
    /// How much of the workspace a pane dropped against its outer edge takes.
    edge_share: f32,
    new_tab_button: bool,
    salt: Id,
    /// The tab being dragged, and where inside it the pointer picked it up — so the tab under
    /// the pointer keeps the spot it was grabbed by instead of snapping its corner to it.
    dragging: Option<PaneId>,
    grab_offset: Vec2,
    /// Where the dragged tab would land: the frame whose strip it is over, and how many of
    /// that strip's other tabs are to its left. Worked out at the end of a frame and read by
    /// the next one, which is what lets a strip draw the tab where it is going instead of
    /// where it came from — and so what makes the tabs around it move aside before the drop
    /// rather than after it.
    tab_landing: Option<(FrameId, usize)>,
    /// Where each frame, tab and pane body was drawn last time round.
    frame_rects: Vec<(FrameId, Rect)>,
    tab_rects: Vec<(FrameId, PaneId, Rect)>,
    pane_rects: Vec<(PaneId, Rect)>,
}

impl Default for Frames {
    fn default() -> Self {
        Self::new()
    }
}

impl Frames {
    /// A workspace styled for egui's dark theme. Give it a
    /// [`FramesStyle::from_visuals`](crate::FramesStyle::from_visuals) to follow the theme in
    /// play.
    pub fn new() -> Self {
        Self {
            style: FramesStyle::default(),
            edge_share: DEFAULT_EDGE_SHARE,
            new_tab_button: true,
            salt: Id::new("egui_frames"),
            dragging: None,
            grab_offset: Vec2::ZERO,
            tab_landing: None,
            frame_rects: Vec::new(),
            tab_rects: Vec::new(),
            pane_rects: Vec::new(),
        }
    }

    /// How it is drawn.
    #[must_use]
    pub fn with_style(mut self, style: FramesStyle) -> Self {
        self.style = style;
        self
    }

    /// How much of the workspace a pane dropped against its outer edge takes.
    #[must_use]
    pub fn with_edge_share(mut self, share: f32) -> Self {
        self.edge_share = share;
        self
    }

    /// Whether each tab strip offers a new-tab button. It reports
    /// [`FramesEvent::NewTabRequested`]; without one, an application opens tabs its own way.
    #[must_use]
    pub fn with_new_tab_button(mut self, show: bool) -> Self {
        self.new_tab_button = show;
        self
    }

    /// What sets this workspace's widgets apart from another's, for an application drawing more
    /// than one arrangement in the same context.
    #[must_use]
    pub fn with_id_salt(mut self, salt: impl std::hash::Hash + std::fmt::Debug) -> Self {
        self.salt = Id::new(salt);
        self
    }

    /// The style in play.
    pub fn style(&self) -> &FramesStyle {
        &self.style
    }

    /// The same, to change — which is how an application follows a theme switch.
    pub fn style_mut(&mut self) -> &mut FramesStyle {
        &mut self.style
    }

    /// The tab being dragged, if one is.
    pub fn dragged_pane(&self) -> Option<PaneId> {
        self.dragging
    }

    /// Where a frame was drawn.
    pub fn frame_rect(&self, frame: FrameId) -> Option<Rect> {
        self.frame_rects
            .iter()
            .find(|(drawn, _)| *drawn == frame)
            .map(|(_, rect)| *rect)
    }

    /// Where a pane's tab was drawn, for an application putting something of its own beside
    /// it — a context menu, a badge.
    pub fn tab_rect(&self, pane: PaneId) -> Option<Rect> {
        self.tab_rects
            .iter()
            .find(|(_, drawn, _)| *drawn == pane)
            .map(|(_, _, rect)| *rect)
    }

    /// Where a pane was drawn: the body of its frame, below the tabs. This is what anything
    /// floating over a pane — a find bar, an overlay — is placed against.
    pub fn pane_rect(&self, pane: PaneId) -> Option<Rect> {
        self.pane_rects
            .iter()
            .find(|(drawn, _)| *drawn == pane)
            .map(|(_, rect)| *rect)
    }

    /// Where everything drawn last time round sits, together.
    pub fn workspace_rect(&self) -> Option<Rect> {
        self.frame_rects
            .iter()
            .map(|(_, rect)| *rect)
            .reduce(|whole, rect| whole.union(rect))
    }

    /// Draw the arrangement into all the space the `ui` has left, and act on this frame's
    /// pointer: dragged tabs move, dropped ones split, and handles resize.
    ///
    /// Returns what only the application can answer — see [`FramesEvent`].
    pub fn show<P, V: PaneView<P>>(
        &mut self,
        ui: &mut Ui,
        layout: &mut Layout<P>,
        view: &mut V,
    ) -> Vec<FramesEvent> {
        // Frames and tabs record where they were drawn, so a drop lands on what the user was
        // actually looking at rather than on geometry recomputed from the tree.
        self.frame_rects.clear();
        self.tab_rects.clear();
        self.pane_rects.clear();
        let mut events = Vec::new();

        let background = self.style.background;
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(background))
            .show(ui, |ui| {
                let area = ui.available_rect_before_wrap();
                let root = layout.root().clone();
                self.draw_node(ui, layout, view, &mut events, &root, area, &[]);
            });

        if self.dragging.is_some() {
            self.draw_workspace_drop_hint(ui, layout);
        }

        // Worked out from what was just drawn, and read by the next frame. The tabs a dragged
        // one is being put between move aside for it as it goes, so the drop changes nothing
        // that was not already on screen.
        self.tab_landing = self.dragging.and_then(|dragged| {
            let at = ui.ctx().input(|input| input.pointer.hover_pos())?;
            self.landing_for(layout, dragged, at)
        });

        // A drag that ends anywhere resolves here, so releasing outside a frame simply cancels
        // rather than leaving the tab stuck to the pointer.
        let (released, at) = ui
            .ctx()
            .input(|input| (input.pointer.any_released(), input.pointer.latest_pos()));
        if self.dragging.is_some() && released {
            self.finish_drag(layout, at);
        }

        events
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "the recursion carries the tree, the space it is drawn in and the path to it"
    )]
    fn draw_node<P, V: PaneView<P>>(
        &mut self,
        ui: &mut Ui,
        layout: &mut Layout<P>,
        view: &mut V,
        events: &mut Vec<FramesEvent>,
        node: &LayoutNode,
        rect: Rect,
        path: &[usize],
    ) {
        match node {
            LayoutNode::Frame { frame } => self.draw_frame(ui, layout, view, events, *frame, rect),
            LayoutNode::Split {
                direction,
                children,
                sizes,
            } => {
                let horizontal = *direction == SplitDirection::Row;
                let (child_rects, usable) =
                    self.split_child_rects(rect, *direction, sizes, children.len());
                let mut resized: Option<Vec<f32>> = None;

                for (index, child) in children.iter().enumerate() {
                    let child_rect = child_rects[index];
                    let mut child_path = path.to_vec();
                    child_path.push(index);
                    self.draw_node(ui, layout, view, events, child, child_rect, &child_path);

                    if index + 1 < children.len() {
                        let handle = if horizontal {
                            Rect::from_min_size(
                                pos2(child_rect.max.x, rect.min.y),
                                vec2(self.style.divider_thickness, rect.height()),
                            )
                        } else {
                            Rect::from_min_size(
                                pos2(rect.min.x, child_rect.max.y),
                                vec2(rect.width(), self.style.divider_thickness),
                            )
                        };
                        if let Some(next) =
                            self.draw_divider(ui, handle, horizontal, path, index, sizes, usable)
                        {
                            resized = Some(next);
                        }
                    }
                }

                if let Some(sizes) = resized {
                    layout.set_split_sizes(path, &sizes);
                }
            }
        }
    }

    /// The grab handle between two frames. Returns the split's new shares while it is dragged.
    #[allow(
        clippy::too_many_arguments,
        reason = "one call site; a parameter struct here would only be destructured again"
    )]
    fn draw_divider(
        &self,
        ui: &mut Ui,
        rect: Rect,
        horizontal: bool,
        path: &[usize],
        index: usize,
        sizes: &[f32],
        usable: f32,
    ) -> Option<Vec<f32>> {
        // Named after where the handle is in the tree, not where it is on screen: a drag moves
        // the handle, and an id that moved with it would be a different widget on the next
        // frame — egui would drop the drag the moment the pointer left the handle's own width.
        let id = self.salt.with(("divider", path, index));
        let response = ui.interact(rect, id, Sense::drag());
        if response.hovered() || response.dragged() {
            ui.ctx().set_cursor_icon(if horizontal {
                CursorIcon::ResizeHorizontal
            } else {
                CursorIcon::ResizeVertical
            });
            ui.painter().rect_filled(
                rect.shrink2(if horizontal {
                    vec2(1.5, 0.0)
                } else {
                    vec2(0.0, 1.5)
                }),
                CornerRadius::same(1),
                self.style.accent,
            );
        }

        if !response.dragged() {
            return None;
        }
        let delta = if horizontal {
            response.drag_delta().x
        } else {
            response.drag_delta().y
        };
        if delta.abs() < f32::EPSILON || usable <= 0.0 {
            return None;
        }

        // Dragging a handle trades space between the two frames it sits between, leaving every
        // other frame in the split alone.
        let shift = delta / usable;
        let mut next = sizes.to_vec();
        if index + 1 >= next.len() {
            return None;
        }
        next[index] += shift;
        next[index + 1] -= shift;
        Some(next)
    }

    fn draw_frame<P, V: PaneView<P>>(
        &mut self,
        ui: &mut Ui,
        layout: &mut Layout<P>,
        view: &mut V,
        events: &mut Vec<FramesEvent>,
        frame: FrameId,
        rect: Rect,
    ) {
        self.frame_rects.push((frame, rect));
        let is_active = layout.active_frame() == frame;
        // The accent border says which frame the keyboard is talking to. A workspace with one
        // frame has nothing to tell it apart from, so it wears the ordinary border.
        let marked_active = is_active && layout.frame_count() > 1;
        let radius = self.style.corner_radius;
        ui.painter().rect_filled(rect, radius, self.style.frame_fill);
        ui.painter().rect_stroke(
            rect,
            radius,
            Stroke::new(
                1.0,
                if marked_active {
                    self.style.active_border
                } else {
                    self.style.border
                },
            ),
            StrokeKind::Inside,
        );

        let strip_rect =
            Rect::from_min_size(rect.min, vec2(rect.width(), self.style.tab_strip_height()));
        let body_rect = self.pane_body(rect);

        // Tabs sit inside the frame's border with the same margin on every side, so the strip
        // reads as an even band rather than a row pushed against the top-left corner.
        let margin = self.style.tab_margin;
        let tabs_rect = Rect::from_min_max(
            pos2(
                strip_rect.min.x + FRAME_BORDER + margin,
                strip_rect.min.y + FRAME_BORDER + margin,
            ),
            pos2(
                strip_rect.max.x - FRAME_BORDER - margin,
                strip_rect.max.y - margin,
            ),
        );
        let is_primary = layout.primary_frame() == frame;
        ui.scope_builder(UiBuilder::new().max_rect(tabs_rect), |ui| {
            ui.set_clip_rect(strip_rect);
            self.draw_tab_strip(ui, layout, view, events, frame, is_primary);
        });

        // Stop short of the frame's border on both sides: the border, active or not, stays the
        // outermost thing drawn on the frame.
        ui.painter().hline(
            (rect.min.x + FRAME_BORDER)..=(rect.max.x - FRAME_BORDER),
            strip_rect.max.y,
            Stroke::new(1.0, self.style.border),
        );

        match layout.frame(frame).and_then(crate::Frame::active_pane) {
            Some(pane) => {
                self.pane_rects.push((pane, body_rect));
                if let Some(payload) = layout.pane(pane) {
                    ui.scope_builder(UiBuilder::new().max_rect(body_rect), |ui| {
                        ui.set_clip_rect(body_rect);
                        view.pane_ui(ui, pane, payload);
                    });
                }
            }
            None => {
                ui.scope_builder(UiBuilder::new().max_rect(body_rect), |ui| {
                    ui.set_clip_rect(body_rect);
                    view.empty_frame_ui(ui, frame);
                });
            }
        }

        // Clicking anywhere in a frame makes it the one the keyboard talks to.
        //
        // This reads the pointer rather than registering a widget: a click-sensing widget the
        // size of the frame would sit on top of everything drawn inside it and swallow every
        // click meant for a tab or for the pane itself.
        let pressed_inside = ui.input(|input| {
            input.pointer.any_pressed()
                && input
                    .pointer
                    .interact_pos()
                    .is_some_and(|at| rect.contains(at))
        });
        if pressed_inside && !is_active {
            layout.set_active_frame(frame);
        }

        if self.dragging.is_some() {
            self.draw_drop_hint(ui, layout, rect, strip_rect);
        }
    }

    fn draw_tab_strip<P, V: PaneView<P>>(
        &mut self,
        ui: &mut Ui,
        layout: &mut Layout<P>,
        view: &mut V,
        events: &mut Vec<FramesEvent>,
        frame: FrameId,
        is_primary: bool,
    ) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = self.style.tab_gap;

            let Some(open) = layout.frame(frame) else {
                return;
            };
            let panes = self.tabs_in_drawn_order(frame, open.panes());
            let active = open.active_pane();
            let tabs: Vec<(PaneId, Tab)> = panes
                .into_iter()
                .filter_map(|pane| {
                    layout
                        .pane(pane)
                        .map(|payload| (pane, view.tab(pane, payload)))
                })
                .collect();

            // Right to left first: the application's own controls take the outer edge and the
            // new-tab button sits inside them, so both stay on screen however many tabs there
            // are — what is left over is what the tabs are drawn, and scrolled, in.
            ui.with_layout(UiLayout::right_to_left(Align::Center), |ui| {
                view.tab_strip_end(ui, frame, is_primary);
                if self.new_tab_button && self.draw_new_tab_button(ui).clicked() {
                    events.push(FramesEvent::NewTabRequested(frame));
                }

                ui.with_layout(UiLayout::left_to_right(Align::Center), |ui| {
                    let titles = self.title_widths(ui, &tabs);
                    // More tabs than the strip has room for scroll sideways under the
                    // trackpad rather than vanish off the end. No bar is drawn: a strip has
                    // no room for one, and the wheel is how the strip says it scrolls.
                    egui::ScrollArea::horizontal()
                        .id_salt(self.salt.with(("tab-scroll", frame)))
                        .scroll_bar_visibility(
                            egui::scroll_area::ScrollBarVisibility::AlwaysHidden,
                        )
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                // What a tab's place is measured against, so a strip that
                                // has been moved, resized or scrolled is not read as every
                                // tab in it having moved.
                                let origin = ui.min_rect().left();
                                for ((pane, tab), title_width) in tabs.iter().zip(titles) {
                                    self.draw_tab(
                                        ui,
                                        layout,
                                        events,
                                        frame,
                                        *pane,
                                        tab,
                                        active == Some(*pane),
                                        origin,
                                        title_width,
                                    );
                                }
                            });
                        });
                });
            });
        });
    }

    /// How wide each tab's one-line title gets this time round.
    ///
    /// Every title is guaranteed [`FramesStyle::max_tab_width`]; a strip with room to spare
    /// shares what is left, so a title that was cut short grows back the moment tabs close or
    /// the frame widens. Titles that want less than an even share keep their own width, and
    /// what they leave unused widens the longer ones.
    fn title_widths(&self, ui: &Ui, tabs: &[(PaneId, Tab)]) -> Vec<f32> {
        let style = &self.style;
        let wanted: Vec<f32> = tabs
            .iter()
            .map(|(_, tab)| {
                cut_to_fit(ui, &tab.title, style.font.clone(), style.text, f32::INFINITY)
                    .size()
                    .x
            })
            .collect();
        // What the strip spends beside the titles: each tab's insets, marker and close mark,
        // and the gaps between tabs. What is left after that is the titles' to share.
        let chrome: f32 = tabs
            .iter()
            .map(|(_, tab)| {
                let marker = if tab.marker { TAB_MARKER_SPACE } else { 0.0 };
                let close = if tab.closable {
                    TAB_CLOSE_GAP + TAB_CLOSE_SIZE + TAB_CLOSE_INSET
                } else {
                    TAB_TEXT_INSET
                };
                TAB_TEXT_INSET + marker + close
            })
            .sum();
        let gaps = style.tab_gap * tabs.len().saturating_sub(1) as f32;
        let room = ui.available_width() - chrome - gaps;
        let cap = shared_title_cap(&wanted, room, style.max_tab_width);
        wanted.into_iter().map(|want| want.min(cap)).collect()
    }

    /// Draw a tab at the place the strip gives it, walking there from wherever it was drawn
    /// last rather than appearing there.
    ///
    /// This is what makes the tabs a dragged one is being put between move out of its way
    /// instead of jumping: the strip answers where each tab belongs, and this walks it there
    /// over [`TAB_SLIDE`]. A tab that has not moved is drawn where it is with no work done,
    /// and one drawn for the first time starts where it belongs rather than sliding in from
    /// the left.
    ///
    /// The dragged tab is somewhere for a reason of its own — it is on the pointer — so it is
    /// stamped where the strip puts it instead of walking there, which is what stops it being
    /// seen sliding back from wherever it last sat.
    #[allow(
        clippy::too_many_arguments,
        reason = "a tab is drawn from its pane, its frame, and what the application named it"
    )]
    fn draw_tab<P>(
        &mut self,
        ui: &mut Ui,
        layout: &mut Layout<P>,
        events: &mut Vec<FramesEvent>,
        frame: FrameId,
        pane: PaneId,
        tab: &Tab,
        selected: bool,
        origin: f32,
        title_width: f32,
    ) {
        let id = self.salt.with(("tab-slide", pane));
        let belongs_at = ui.cursor().left() - origin;
        let drawn_at = if self.dragging == Some(pane) {
            ui.ctx().animate_value_with_time(id, belongs_at, 0.0)
        } else {
            ui.ctx().animate_value_with_time(id, belongs_at, TAB_SLIDE)
        };
        let offset = drawn_at - belongs_at;

        if offset.abs() < 0.5 {
            self.draw_tab_body(ui, layout, events, frame, pane, tab, selected, title_width);
            return;
        }

        // Drawn into a layer of its own so the shapes can be moved once they are made — the
        // same way the dragged tab is. Its clip is moved the other way first, so a tab on its
        // way between two places is still cut off at the strip it is in.
        let layer_id = LayerId::new(Order::Middle, id.with("sliding"));
        let clip = ui.clip_rect();
        ui.scope_builder(UiBuilder::new().layer_id(layer_id), |ui| {
            ui.set_clip_rect(clip.translate(vec2(-offset, 0.0)));
            self.draw_tab_body(ui, layout, events, frame, pane, tab, selected, title_width);
        });
        ui.ctx().transform_layer_shapes(
            layer_id,
            egui::emath::TSTransform::from_translation(vec2(offset, 0.0)),
        );
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "a tab is drawn from its pane, its frame, and what the application named it"
    )]
    fn draw_tab_body<P>(
        &mut self,
        ui: &mut Ui,
        layout: &mut Layout<P>,
        events: &mut Vec<FramesEvent>,
        frame: FrameId,
        pane: PaneId,
        tab: &Tab,
        selected: bool,
        title_width: f32,
    ) {
        let style = &self.style;
        // The title walks to the width the strip granted it rather than jumping there, the
        // same way a tab walks to its place. At full width the cut is lifted altogether, so
        // the whole title is never traded for an ellipsis by a rounding error.
        let granted = ui.ctx().animate_value_with_time(
            self.salt.with(("tab-title-width", pane)),
            title_width,
            TAB_SLIDE,
        );
        let full = cut_to_fit(ui, &tab.title, style.font.clone(), style.text, f32::INFINITY)
            .size()
            .x;
        let galley = cut_to_fit(
            ui,
            &tab.title,
            style.font.clone(),
            if selected {
                style.text
            } else {
                style.inactive_text
            },
            if granted >= full { f32::INFINITY } else { granted },
        );
        let marker_space = if tab.marker { TAB_MARKER_SPACE } else { 0.0 };
        let close_space = if tab.closable {
            TAB_CLOSE_GAP + TAB_CLOSE_SIZE + TAB_CLOSE_INSET
        } else {
            TAB_TEXT_INSET
        };
        let width = galley.size().x + marker_space + TAB_TEXT_INSET + close_space;
        let (rect, response) =
            ui.allocate_exact_size(vec2(width, style.tab_height), Sense::click_and_drag());
        let mut response = response.on_hover_cursor(CursorIcon::PointingHand);
        if let Some(hover) = &tab.hover {
            response = response.on_hover_text(hover);
        }
        self.tab_rects.push((frame, pane, rect));

        let dragging_this = self.dragging == Some(pane);
        let mut close_clicked = false;
        if ui.is_rect_visible(rect) {
            // A dragged tab rides the pointer, drawn above everything else and holding the spot
            // it was picked up by. Its slot in the strip stays behind as an outline, so the
            // strip's other tabs don't jump around underneath the drag.
            let pointer = ui.input(|input| input.pointer.hover_pos());
            let (painter, rect) = match (dragging_this, pointer) {
                (true, Some(at)) => {
                    ui.painter().rect_stroke(
                        rect,
                        CornerRadius::same(4),
                        Stroke::new(1.0, style.border),
                        StrokeKind::Inside,
                    );
                    (
                        ui.ctx()
                            .layer_painter(LayerId::new(Order::Foreground, self.salt.with("drag"))),
                        Rect::from_min_size(at - self.grab_offset, rect.size()),
                    )
                }
                _ => (ui.painter().clone(), rect),
            };

            let fill = if dragging_this || selected {
                style.active_tab_fill
            } else if response.hovered() {
                style.tab_fill
            } else {
                Color32::TRANSPARENT
            };
            painter.rect_filled(rect, CornerRadius::same(4), fill);
            if dragging_this {
                painter.rect_stroke(
                    rect,
                    CornerRadius::same(4),
                    Stroke::new(1.0, style.accent),
                    StrokeKind::Inside,
                );
            }
            if tab.marker {
                painter.circle_filled(
                    pos2(rect.min.x + TAB_TEXT_INSET + 3.0, rect.center().y),
                    3.0,
                    style.accent,
                );
            }
            let text_height = galley.size().y;
            painter.galley(
                pos2(
                    rect.min.x + TAB_TEXT_INSET + marker_space,
                    (rect.center().y - text_height / 2.0).round(),
                ),
                galley,
                style.text,
            );

            let close_rect = Rect::from_center_size(
                pos2(
                    rect.max.x - TAB_CLOSE_INSET - TAB_CLOSE_SIZE / 2.0,
                    rect.center().y,
                ),
                vec2(TAB_CLOSE_SIZE, TAB_CLOSE_SIZE),
            );
            let hovering_close =
                tab.closable && !dragging_this && pointer.is_some_and(|at| close_rect.contains(at));
            if tab.closable && !dragging_this && (response.hovered() || selected) {
                // Drawn rather than typeset: an ellipsis of a font's close glyphs is a heavy
                // emoji ✖, and a tab wants the thin ✕ a browser draws.
                draw_close_mark(
                    &painter,
                    close_rect.center(),
                    if hovering_close {
                        style.close_hover
                    } else {
                        style.inactive_text
                    },
                );
            }
            close_clicked = response.clicked() && hovering_close;
        }

        if close_clicked {
            events.push(FramesEvent::PaneCloseRequested(pane));
            return;
        }
        if response.clicked() {
            layout.focus_pane(pane);
        }
        // Middle-click closes a tab, as it does in a browser.
        if tab.closable && response.middle_clicked() {
            events.push(FramesEvent::PaneCloseRequested(pane));
        }
        if response.drag_started() {
            self.dragging = Some(pane);
            self.grab_offset = ui
                .input(|input| input.pointer.press_origin())
                .map(|at| at - rect.min)
                .unwrap_or_else(|| rect.size() / 2.0);
        }
        if response.dragged() {
            ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
        }
    }

    /// A `+` on a filled disc, at the end of the strip.
    fn draw_new_tab_button(&self, ui: &mut Ui) -> Response {
        let diameter = self.style.tab_height;
        let (rect, response) = ui.allocate_exact_size(vec2(diameter, diameter), Sense::click());
        if ui.is_rect_visible(rect) {
            let (fill, ink) = if response.hovered() {
                (self.style.active_tab_fill, self.style.text)
            } else {
                (self.style.tab_fill, self.style.inactive_text)
            };
            ui.painter()
                .circle_filled(rect.center(), diameter / 2.0, fill);
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "+",
                egui::FontId::proportional(diameter * 0.72),
                ink,
            );
        }
        response.on_hover_cursor(CursorIcon::PointingHand)
    }

    /// The area a frame hands its pane: below the tab strip, and clear of the frame's own
    /// border on every other side.
    ///
    /// Clear of it rather than up against it. A clip rect that stopped exactly on the border
    /// lets a glyph reaching the edge paint over the border pixel itself, which makes a long
    /// line of text look as though it had escaped its frame.
    fn pane_body(&self, rect: Rect) -> Rect {
        let inset = FRAME_BORDER + 1.0;
        Rect::from_min_max(
            pos2(rect.min.x + inset, rect.min.y + self.style.tab_strip_height()),
            pos2(rect.max.x - inset, rect.max.y - inset),
        )
    }

    /// Divide a split's area between its children, leaving room for the handles between them.
    /// Returns the child rects and the space the shares were taken from, which is what a drag
    /// on a handle converts pixels into shares with.
    fn split_child_rects(
        &self,
        rect: Rect,
        direction: SplitDirection,
        sizes: &[f32],
        count: usize,
    ) -> (Vec<Rect>, f32) {
        let horizontal = direction == SplitDirection::Row;
        let total = if horizontal { rect.width() } else { rect.height() };
        let gaps = self.style.divider_thickness * count.saturating_sub(1) as f32;
        let usable = (total - gaps).max(1.0);
        let even = 1.0 / count.max(1) as f32;

        let mut rects = Vec::with_capacity(count);
        let mut offset = 0.0;
        for index in 0..count {
            let extent = usable * sizes.get(index).copied().unwrap_or(even);
            rects.push(if horizontal {
                Rect::from_min_size(
                    pos2(rect.min.x + offset, rect.min.y),
                    vec2(extent, rect.height()),
                )
            } else {
                Rect::from_min_size(
                    pos2(rect.min.x, rect.min.y + offset),
                    vec2(rect.width(), extent),
                )
            });
            offset += extent + self.style.divider_thickness;
        }

        (rects, usable)
    }

    /// The outer edge a drop at this point lands against, if it is against one at all.
    ///
    /// This wins over the frame under the pointer: the band is narrow, and inside it the only
    /// thing the user can mean is "beside everything".
    fn workspace_drop_side<P>(&self, layout: &Layout<P>, at: Pos2) -> Option<DropSide> {
        let workspace = self.workspace_rect()?;
        if !workspace.contains(at) || layout.frame_count() < 2 {
            return None;
        }
        // A tab strip wins over the band down the outer edge. The band is deeper than a strip
        // is tall, so without this every strip along the top of the workspace sits inside the
        // top band — and a tab dropped on one could never join its tabs, only ever become a
        // row above everything. Aiming at a strip is precise and means one thing; the band is
        // a coarse gesture at the whole arrangement.
        if self.over_a_tab_strip(at) {
            return None;
        }

        let sides = [
            (DropSide::Left, at.x - workspace.min.x),
            (DropSide::Right, workspace.max.x - at.x),
            (DropSide::Top, at.y - workspace.min.y),
            (DropSide::Bottom, workspace.max.y - at.y),
        ];
        let (side, distance) = sides
            .into_iter()
            .min_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))?;
        (distance <= WORKSPACE_EDGE).then_some(side)
    }

    /// Where a dragged tab would land, if it would land among some frame's tabs at all.
    ///
    /// Middle against middle, and against the tabs as they are drawn now with the dragged one
    /// out of the reckoning — so carrying it over a tab cannot bounce between two answers.
    /// A drag that would split a frame, or that is over the band down the outer edge, lands
    /// among nobody's tabs and answers `None`.
    fn landing_for<P>(
        &self,
        layout: &Layout<P>,
        dragged: PaneId,
        at: Pos2,
    ) -> Option<(FrameId, usize)> {
        if self.workspace_drop_side(layout, at).is_some() {
            return None;
        }
        let (frame, frame_rect) = self
            .frame_rects
            .iter()
            .find(|(_, rect)| rect.contains(at))
            .copied()?;
        if drop_side(frame_rect, self.strip_rect_of(frame_rect), at)? != DropSide::Tabs {
            return None;
        }

        let index = self
            .tab_rects
            .iter()
            .filter(|(tab_frame, tab_pane, rect)| {
                *tab_frame == frame && *tab_pane != dragged && rect.center().x < at.x
            })
            .count();
        Some((frame, index))
    }

    /// The tabs of a frame in the order they are drawn, with a tab being dragged over this
    /// frame already moved to where it is being held — and taken out of whichever strip it
    /// came from, if that is a different one.
    ///
    /// Until the tab is over some frame's strip there is nowhere for it to be but where it
    /// came from, and taking it out of the strip for that stretch reads as a flicker.
    fn tabs_in_drawn_order(&self, frame: FrameId, panes: &[PaneId]) -> Vec<PaneId> {
        let (Some(dragged), Some((landing_frame, index))) = (self.dragging, self.tab_landing)
        else {
            return panes.to_vec();
        };

        let mut panes: Vec<PaneId> = panes.iter().copied().filter(|p| *p != dragged).collect();
        if landing_frame == frame {
            panes.insert(index.min(panes.len()), dragged);
        }
        panes
    }

    /// The strip along the top of a frame, which is where its tabs are drawn.
    fn strip_rect_of(&self, frame_rect: Rect) -> Rect {
        Rect::from_min_size(
            frame_rect.min,
            vec2(frame_rect.width(), self.style.tab_strip_height()),
        )
    }

    /// Whether a point is on some frame's tab strip.
    fn over_a_tab_strip(&self, at: Pos2) -> bool {
        self.frame_rects
            .iter()
            .any(|(_, rect)| self.strip_rect_of(*rect).contains(at))
    }



    /// The band down the edge of everything, shown while a dragged tab is over it.
    fn draw_workspace_drop_hint<P>(&self, ui: &mut Ui, layout: &Layout<P>) {
        let Some(at) = ui.input(|input| input.pointer.hover_pos()) else {
            return;
        };
        let (Some(side), Some(workspace)) =
            (self.workspace_drop_side(layout, at), self.workspace_rect())
        else {
            return;
        };

        let hint = match side {
            DropSide::Left => workspace.with_max_x(workspace.min.x + workspace.width() * 0.3),
            DropSide::Right => workspace.with_min_x(workspace.max.x - workspace.width() * 0.3),
            DropSide::Top => workspace.with_max_y(workspace.min.y + workspace.height() * 0.3),
            DropSide::Bottom => workspace.with_min_y(workspace.max.y - workspace.height() * 0.3),
            DropSide::Tabs => return,
        };
        let painter = ui
            .ctx()
            .layer_painter(LayerId::new(Order::Foreground, self.salt.with("edge-hint")));
        self.paint_hint(&painter, hint);
    }

    /// While a tab is being dragged over a frame, show where it would land.
    fn draw_drop_hint<P>(&self, ui: &mut Ui, layout: &Layout<P>, rect: Rect, strip_rect: Rect) {
        let Some(at) = ui.input(|input| input.pointer.hover_pos()) else {
            return;
        };
        if !rect.contains(at) || self.workspace_drop_side(layout, at).is_some() {
            return;
        }
        let Some(side) = drop_side(rect, strip_rect, at) else {
            return;
        };

        // Landing among the tabs needs nothing drawn: the strip has already opened a gap
        // where the tab will go and slid its neighbours aside for it, which says the same
        // thing in the place it is going to happen.
        if side == DropSide::Tabs {
            return;
        }

        let hint = match side {
            DropSide::Tabs => return,
            DropSide::Left => rect.with_max_x(rect.min.x + rect.width() * 0.5),
            DropSide::Right => rect.with_min_x(rect.min.x + rect.width() * 0.5),
            DropSide::Top => rect.with_max_y(rect.min.y + rect.height() * 0.5),
            DropSide::Bottom => rect.with_min_y(rect.min.y + rect.height() * 0.5),
        };
        self.paint_hint(ui.painter(), hint);
    }

    fn paint_hint(&self, painter: &egui::Painter, hint: Rect) {
        painter.rect_filled(
            hint.shrink(3.0),
            CornerRadius::same(5),
            self.style.accent.linear_multiply(0.18),
        );
        painter.rect_stroke(
            hint.shrink(3.0),
            CornerRadius::same(5),
            Stroke::new(1.5, self.style.accent),
            StrokeKind::Inside,
        );
    }

    fn finish_drag<P>(&mut self, layout: &mut Layout<P>, at: Option<Pos2>) {
        let Some(pane) = self.dragging.take() else {
            return;
        };
        let Some(at) = at else {
            return;
        };

        // Against the outer edge first: a drop there is about the whole arrangement, not about
        // whichever frame happens to reach that edge.
        if let Some(side) = self.workspace_drop_side(layout, at) {
            layout.move_pane_against_edge(pane, side, self.edge_share);
            return;
        }

        let Some((frame, frame_rect)) = self
            .frame_rects
            .iter()
            .find(|(_, rect)| rect.contains(at))
            .copied()
        else {
            return;
        };
        let strip_rect = self.strip_rect_of(frame_rect);
        let Some(side) = drop_side(frame_rect, strip_rect, at) else {
            return;
        };

        // Landing on a tab strip means landing in the gap the strip has been holding open, so
        // the tab stays exactly where it was last seen rather than moving again on release.
        let before = if side == DropSide::Tabs {
            let index = match self.tab_landing {
                Some((landing_frame, index)) if landing_frame == frame => index,
                // The pointer arrived on this strip on the very frame it was let go of, so
                // there was never a gap; where it is now is the best answer there is.
                _ => self
                    .landing_for(layout, pane, at)
                    .map_or(0, |(_, index)| index),
            };
            layout.frame(frame).and_then(|open| {
                open.panes()
                    .iter()
                    .copied()
                    .filter(|open_pane| *open_pane != pane)
                    .nth(index)
            })
        } else {
            None
        };

        layout.move_pane_to_frame(pane, frame, side, before);
    }
}

/// Which part of a frame a point is in, as far as a dropped tab is concerned.
fn drop_side(rect: Rect, strip_rect: Rect, at: Pos2) -> Option<DropSide> {
    if strip_rect.contains(at) {
        return Some(DropSide::Tabs);
    }
    if !rect.contains(at) {
        return None;
    }

    // Each distance is measured in units of its own edge's band, so a value below 1.0 means the
    // pointer is inside that band and the smallest value is the band it is deepest into. The
    // vertical ones start below the tab strip, which claims the top of the frame for itself.
    let side_band = (rect.width() * SIDE_EDGE_FRACTION).max(1.0);
    let body_top = strip_rect.max.y;
    let up_down_band = ((rect.max.y - body_top) * UP_DOWN_EDGE_FRACTION).max(1.0);

    let from_left = (at.x - rect.min.x) / side_band;
    let from_right = (rect.max.x - at.x) / side_band;
    let from_top = (at.y - body_top) / up_down_band;
    let from_bottom = (rect.max.y - at.y) / up_down_band;
    let nearest = from_left.min(from_right).min(from_top).min(from_bottom);

    if nearest > 1.0 {
        // Dropped well inside a frame: join its tabs rather than split it.
        return Some(DropSide::Tabs);
    }
    Some(if nearest == from_left {
        DropSide::Left
    } else if nearest == from_right {
        DropSide::Right
    } else if nearest == from_top {
        DropSide::Top
    } else {
        DropSide::Bottom
    })
}

/// A tab's close mark: two thin strokes, the size of the text beside them.
fn draw_close_mark(painter: &egui::Painter, center: Pos2, ink: Color32) {
    let reach = TAB_CLOSE_SIZE * 0.27;
    let stroke = Stroke::new(1.0, ink);
    painter.line_segment(
        [center + vec2(-reach, -reach), center + vec2(reach, reach)],
        stroke,
    );
    painter.line_segment(
        [center + vec2(reach, -reach), center + vec2(-reach, reach)],
        stroke,
    );
}

/// The widest any one title in a strip may be, given what each title wants and the room they
/// all share.
///
/// Filled up from the shortest title: one that wants less than an even share of what is left
/// keeps what it wants, and the width it leaves unused is split over the longer ones. When
/// everything fits there is no cap at all. The cap never drops below `floor` — a strip too
/// crowded to give every title that much scrolls instead.
fn shared_title_cap(wanted: &[f32], room: f32, floor: f32) -> f32 {
    let mut sorted = wanted.to_vec();
    sorted.sort_by(f32::total_cmp);
    let mut left = room;
    for (index, want) in sorted.iter().enumerate() {
        let share = left / (sorted.len() - index) as f32;
        if *want > share {
            return share.max(floor);
        }
        left -= want;
    }
    f32::INFINITY
}

/// A title laid out on one line, cut short with an ellipsis rather than wrapped.
fn cut_to_fit(
    ui: &Ui,
    text: &str,
    font: egui::FontId,
    color: Color32,
    max_width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::single_section(
        text.to_string(),
        egui::TextFormat {
            font_id: font,
            color,
            ..Default::default()
        },
    );
    job.wrap = egui::text::TextWrapping {
        max_width,
        max_rows: 1,
        break_anywhere: false,
        overflow_character: Some('…'),
    };
    ui.painter().layout_job(job)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A strip with room for everything cuts nothing, however unevenly long the titles are.
    #[test]
    fn titles_that_fit_are_not_capped() {
        assert_eq!(
            shared_title_cap(&[40.0, 300.0], 400.0, 170.0),
            f32::INFINITY
        );
    }

    /// Spare room goes to the titles that need it: a short title keeps its own width, and the
    /// long ones split what it leaves evenly between them.
    #[test]
    fn spare_room_is_shared_up_from_the_shortest_title() {
        assert_eq!(shared_title_cap(&[40.0, 500.0, 500.0], 400.0, 170.0), 180.0);
    }

    /// A strip too crowded to grant even the guaranteed width falls back to it, and the
    /// overflow is the scroll area's to deal with.
    #[test]
    fn a_crowded_strip_keeps_the_guaranteed_width() {
        assert_eq!(shared_title_cap(&[500.0, 500.0, 500.0], 300.0, 170.0), 170.0);
    }

    fn frame_of(width: f32, height: f32) -> (Frames, Rect, Rect) {
        let frames = Frames::new();
        let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(width, height));
        let strip = Rect::from_min_size(
            rect.min,
            vec2(width, frames.style().tab_strip_height()),
        );
        (frames, rect, strip)
    }

    /// A pane must not be able to paint on the border of the frame holding it, which is what
    /// makes a long line look as though it had spilled out of its box.
    #[test]
    fn a_pane_is_given_room_strictly_inside_its_frames_border() {
        let (frames, rect, _) = frame_of(600.0, 400.0);
        let body = frames.pane_body(rect);

        let border = rect.shrink(FRAME_BORDER);
        assert!(
            body.min.x > border.min.x && body.max.x < border.max.x && body.max.y < border.max.y,
            "the body {body:?} has to stay inside the border {border:?}"
        );
        assert!(
            body.min.y >= rect.min.y + frames.style().tab_strip_height(),
            "and start below the tab strip"
        );
    }

    /// A frame too small to hold anything must not hand out a body that is inside out.
    #[test]
    fn a_frame_with_no_room_left_hands_out_nothing_rather_than_a_negative_body() {
        let (frames, rect, _) = frame_of(3.0, 3.0);
        let body = frames.pane_body(rect);

        assert!(body.width() <= 0.0 || body.height() <= 0.0);
    }

    #[test]
    fn dropping_on_the_tab_strip_joins_its_tabs() {
        let (_, rect, strip) = frame_of(400.0, 300.0);
        assert_eq!(drop_side(rect, strip, pos2(200.0, 10.0)), Some(DropSide::Tabs));
    }

    #[test]
    fn dropping_near_an_edge_splits_on_that_side() {
        let (_, rect, strip) = frame_of(400.0, 300.0);
        assert_eq!(drop_side(rect, strip, pos2(10.0, 150.0)), Some(DropSide::Left));
        assert_eq!(
            drop_side(rect, strip, pos2(390.0, 150.0)),
            Some(DropSide::Right)
        );
        assert_eq!(
            drop_side(rect, strip, pos2(200.0, 295.0)),
            Some(DropSide::Bottom)
        );
    }

    #[test]
    fn dropping_in_the_middle_joins_the_tabs_rather_than_splitting() {
        let (_, rect, strip) = frame_of(400.0, 300.0);
        assert_eq!(
            drop_side(rect, strip, pos2(200.0, 150.0)),
            Some(DropSide::Tabs)
        );
    }

    #[test]
    fn a_bottom_split_is_reachable_without_dragging_to_the_very_edge() {
        // A wide frame: two thirds of the way down the body is already the bottom band, so the
        // drag doesn't have to travel to the window's edge to split downwards.
        let (_, rect, strip) = frame_of(1400.0, 900.0);
        let two_thirds_down = strip.max.y + (rect.max.y - strip.max.y) * 0.7;

        assert_eq!(
            drop_side(rect, strip, pos2(700.0, two_thirds_down)),
            Some(DropSide::Bottom)
        );
    }

    /// The band down the outer edge of the workspace is deeper than a tab strip is tall, so
    /// every strip along the top of the workspace sits inside the top band. Without the strip
    /// winning, a tab dropped on one of those headings could never join its tabs — it always
    /// became a row above everything, which is why reordering only worked from the middle of
    /// a frame.
    #[test]
    fn a_tab_strip_beats_the_band_down_the_outer_edge() {
        let mut frames = Frames::new();
        let strip_height = frames.style().tab_strip_height();
        assert!(
            strip_height < WORKSPACE_EDGE,
            "this is only worth guarding while a strip fits inside the band: {strip_height} \
             against {WORKSPACE_EDGE}"
        );

        // Two frames side by side, so the workspace edge is in play at all.
        let left = Rect::from_min_size(pos2(0.0, 0.0), vec2(500.0, 600.0));
        let right = Rect::from_min_size(pos2(500.0, 0.0), vec2(500.0, 600.0));
        frames.frame_rects = vec![(FrameId(1), left), (FrameId(2), right)];

        // Well inside the left frame's heading, and inside the top band with it.
        let on_the_heading = pos2(250.0, strip_height / 2.0);
        assert!(frames.over_a_tab_strip(on_the_heading));

        // A point in the body at the same depth as the band would still be an edge drop, so
        // the strip is doing the work here rather than the band having gone away.
        let below_the_headings = pos2(250.0, strip_height + 4.0);
        assert!(!frames.over_a_tab_strip(below_the_headings));
    }

    /// The tabs a dragged one is being put between move aside as it goes, so the drop changes
    /// nothing that was not already on screen. This is the order a strip draws while a drag is
    /// in flight, which is what those tabs slide towards.
    #[test]
    fn a_strip_draws_a_dragged_tab_where_it_is_being_held() {
        let mut frames = Frames::new();
        let here = FrameId(1);
        let (a, b, c) = (PaneId(1), PaneId(2), PaneId(3));

        // Nothing in flight: the strip is the strip.
        assert_eq!(frames.tabs_in_drawn_order(here, &[a, b, c]), [a, b, c]);

        // Picked up, but not yet over anyone's tabs — it stays where it came from, because
        // taking it out for that stretch reads as a flicker.
        frames.dragging = Some(a);
        assert_eq!(frames.tabs_in_drawn_order(here, &[a, b, c]), [a, b, c]);

        // Carried past the middle of both of the others.
        frames.tab_landing = Some((here, 2));
        assert_eq!(frames.tabs_in_drawn_order(here, &[a, b, c]), [b, c, a]);

        // Held over another frame's strip: this one closes up behind it, and that one opens.
        let there = FrameId(2);
        frames.tab_landing = Some((there, 0));
        assert_eq!(frames.tabs_in_drawn_order(here, &[a, b, c]), [b, c]);
        assert_eq!(frames.tabs_in_drawn_order(there, &[]), [a]);
    }

    /// Where a dragged tab lands is worked out middle against middle, against the tabs it
    /// would be put among with the dragged one taken out of the reckoning — the same rule the
    /// moontasks board uses, and what stops the answer bouncing as the pointer crosses a tab.
    #[test]
    fn a_landing_is_counted_from_the_tabs_it_would_be_put_among() {
        let mut frames = Frames::new();
        let here = FrameId(1);
        let (dragged, b, c) = (PaneId(1), PaneId(2), PaneId(3));
        let tab = |x: f32| Rect::from_min_size(pos2(x, 0.0), vec2(100.0, 18.0));

        frames.dragging = Some(dragged);
        frames.frame_rects = vec![(here, Rect::from_min_size(pos2(0.0, 0.0), vec2(600.0, 400.0)))];
        frames.tab_rects = vec![
            (here, dragged, tab(0.0)),
            (here, b, tab(110.0)),
            (here, c, tab(220.0)),
        ];

        let strip_y = frames.style().tab_strip_height() / 2.0;
        let landing = |x: f32| {
            frames
                .landing_for(&Layout::<()>::new(), dragged, pos2(x, strip_y))
                .map(|(_, index)| index)
        };

        // Left of everything, then past each middle in turn. The dragged tab's own slot is
        // never counted, so the answer never jumps by two.
        assert_eq!(landing(5.0), Some(0));
        assert_eq!(landing(150.0), Some(0));
        assert_eq!(landing(200.0), Some(1));
        assert_eq!(landing(400.0), Some(2));
    }

    #[test]
    fn dropping_outside_a_frame_is_not_a_drop() {
        let (_, rect, strip) = frame_of(400.0, 300.0);
        assert_eq!(drop_side(rect, strip, pos2(800.0, 150.0)), None);
    }

    #[test]
    fn split_children_tile_the_area_minus_the_handles() {
        let frames = Frames::new();
        let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 600.0));

        let (rects, usable) = frames.split_child_rects(area, SplitDirection::Row, &[0.65, 0.35], 2);

        let handle = frames.style().divider_thickness;
        assert_eq!(rects.len(), 2);
        assert!((usable - (area.width() - handle)).abs() < f32::EPSILON);
        let covered: f32 = rects.iter().map(Rect::width).sum();
        assert!(
            (covered - usable).abs() < 0.01,
            "columns should fill the usable width, got {covered} of {usable}"
        );
        assert!(rects.iter().all(|rect| rect.height() == area.height()));
        // The second column starts after the first one plus the handle between them.
        assert!((rects[1].min.x - (rects[0].max.x + handle)).abs() < 0.01);
    }

    #[test]
    fn a_column_split_divides_height_instead_of_width() {
        let frames = Frames::new();
        let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, 900.0));

        let (rects, usable) =
            frames.split_child_rects(area, SplitDirection::Column, &[0.5, 0.5], 2);

        assert!((usable - (area.height() - frames.style().divider_thickness)).abs() < f32::EPSILON);
        assert!(rects.iter().all(|rect| rect.width() == area.width()));
        assert!((rects[0].height() - rects[1].height()).abs() < 0.01);
    }

    #[test]
    fn missing_sizes_fall_back_to_an_even_split() {
        let frames = Frames::new();
        let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(300.0, 100.0));

        let (rects, _) = frames.split_child_rects(area, SplitDirection::Row, &[], 3);

        assert_eq!(rects.len(), 3);
        assert!((rects[0].width() - rects[2].width()).abs() < 0.01);
    }
}
