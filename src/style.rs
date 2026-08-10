use egui::{Color32, CornerRadius, FontId, Visuals};

/// How an arrangement is drawn: the colors of a frame and its tabs, and the handful of sizes
/// the chrome is built from.
///
/// [`from_visuals`](FramesStyle::from_visuals) takes an egui theme and gets on with it; set the
/// fields yourself when the application has a palette of its own.
#[derive(Clone, Debug)]
pub struct FramesStyle {
    /// Behind the frames.
    pub background: Color32,
    /// Inside a frame.
    pub frame_fill: Color32,
    /// A frame's border, and the rule under its tab strip.
    pub border: Color32,
    /// The border of the frame the keyboard is talking to. Only drawn when there is more than
    /// one frame, since a lone frame has nothing to be told apart from.
    pub active_border: Color32,
    /// A hovered tab.
    pub tab_fill: Color32,
    /// The tab in front.
    pub active_tab_fill: Color32,
    /// A tab's title, and a hovered tab's close mark.
    pub text: Color32,
    /// The title of a tab that is not in front.
    pub inactive_text: Color32,
    /// Drop hints, the caret between tabs, the active border, and a tab's marker dot.
    pub accent: Color32,
    /// A close mark the pointer is on: the one click here that throws something away.
    pub close_hover: Color32,
    /// The tab titles, and anything else written on the chrome.
    pub font: FontId,
    /// How wide a tab's title is guaranteed before it is cut short with an ellipsis. A strip
    /// with room to spare lets its titles grow past this until nothing is cut; a strip too
    /// crowded to give every title this much scrolls instead.
    pub max_tab_width: f32,
    /// The height of a tab, which sets the height of the strip with it.
    pub tab_height: f32,
    /// The gap between the tabs and the edges of the strip, on all four sides.
    pub tab_margin: f32,
    /// The gap between one tab and the next.
    pub tab_gap: f32,
    /// How thick the grab handle between two frames is.
    pub divider_thickness: f32,
    /// The rounding of a frame.
    pub corner_radius: CornerRadius,
}

impl Default for FramesStyle {
    fn default() -> Self {
        Self::from_visuals(&Visuals::dark())
    }
}

impl FramesStyle {
    /// The style that suits an egui theme.
    pub fn from_visuals(visuals: &Visuals) -> Self {
        Self {
            background: visuals.panel_fill,
            frame_fill: visuals.window_fill,
            border: visuals.widgets.noninteractive.bg_stroke.color,
            active_border: visuals.selection.stroke.color,
            tab_fill: visuals.widgets.hovered.weak_bg_fill,
            active_tab_fill: visuals.widgets.active.weak_bg_fill,
            text: visuals.text_color(),
            inactive_text: visuals.weak_text_color(),
            accent: visuals.selection.stroke.color,
            close_hover: visuals.error_fg_color,
            font: FontId::proportional(12.0),
            max_tab_width: 170.0,
            tab_height: 18.0,
            tab_margin: 4.0,
            tab_gap: 3.0,
            divider_thickness: 5.0,
            corner_radius: CornerRadius::same(6),
        }
    }

    /// The height of a tab strip: the tabs, the margin above and below them, and the frame's
    /// own border above that.
    pub fn tab_strip_height(&self) -> f32 {
        FRAME_BORDER + self.tab_margin * 2.0 + self.tab_height
    }
}

/// A frame's border, one pixel drawn inside its rect.
pub(crate) const FRAME_BORDER: f32 = 1.0;
