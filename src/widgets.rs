//! The design system's controls, drawn with the `theme` tokens: buttons,
//! status words, hairlines and section labels.

use crate::theme::{self, Text};
use eframe::egui::{self, CornerRadius, RichText, Stroke};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Ok,
    Warn,
    Error,
}

impl Status {
    fn color(self) -> egui::Color32 {
        match self {
            Status::Ok => theme::OK,
            Status::Warn => theme::WARN,
            Status::Error => theme::ERROR,
        }
    }
}

/// Text in one of the design system's styles and colors.
pub fn text(words: impl Into<String>, style: Text, color: egui::Color32) -> RichText {
    RichText::new(words).font(theme::font(style)).color(color)
}

/// The page's one primary button: an `INK` fill with an `ON_INK` label.
pub fn primary_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    let button = egui::Button::new(text(label, Text::Body, theme::ON_INK))
        .fill(theme::INK)
        .stroke(Stroke::NONE)
        .corner_radius(CornerRadius::same(theme::RADIUS_MD));
    add(ui, button)
}

/// A secondary button: a 1px `CONTROL_BORDER` outline and no fill.
pub fn secondary_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    let button = egui::Button::new(text(label, Text::Body, theme::INK))
        .fill(egui::Color32::TRANSPARENT)
        .stroke(Stroke::new(theme::HAIRLINE, theme::CONTROL_BORDER))
        .corner_radius(CornerRadius::same(theme::RADIUS_MD));
    add(ui, button)
}

/// A button that is only its words, such as "Show details".
pub fn text_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    let button = egui::Button::new(text(label, Text::Label, theme::INK)).frame(false);
    add(ui, button)
}

/// Adds a control, with a 2px `INK` outline 2px outside it while it has
/// the keyboard focus.
pub fn add(ui: &mut egui::Ui, widget: impl egui::Widget) -> egui::Response {
    let response = ui.add(widget);
    if response.has_focus() {
        let gap = theme::FOCUS_GAP + theme::FOCUS_WIDTH / 2.0;
        ui.painter().rect_stroke(
            response.rect.expand(gap),
            CornerRadius::same(theme::RADIUS_MD + gap as u8),
            Stroke::new(theme::FOCUS_WIDTH, theme::INK),
            egui::StrokeKind::Middle,
        );
    }
    response
}

/// An 8px dot in the status color, then the word in the label style.
pub fn status(ui: &mut egui::Ui, status: Status, word: &str) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = theme::SPACE_2;
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(theme::DOT, theme::DOT), egui::Sense::hover());
        ui.painter()
            .circle_filled(rect.center(), theme::DOT / 2.0, status.color());
        ui.label(text(word, Text::Label, theme::INK));
    });
}

/// A 1px `LINE` across the width.
pub fn hairline(ui: &mut egui::Ui) {
    let width = ui.available_width();
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(width, theme::HAIRLINE), egui::Sense::hover());
    ui.painter().rect_filled(rect, 0.0, theme::LINE);
}

/// A section's name in the label style, in `INK_FAINT`.
pub fn section_label(ui: &mut egui::Ui, words: &str) {
    ui.label(text(words, Text::Label, theme::INK_FAINT));
}

/// A `SURFACE` panel with `RADIUS_LG` corners, such as the update notice.
pub fn panel<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .fill(theme::SURFACE)
        .corner_radius(CornerRadius::same(theme::RADIUS_LG))
        .inner_margin(egui::Margin::same(theme::SPACE_4 as i8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

/// A progress line: a 2px `INK` line on a 2px `LINE` track.
pub fn progress(ui: &mut egui::Ui, fraction: f32) {
    let width = ui.available_width();
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(width, theme::PROGRESS), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 0.0, theme::LINE);
    let mut done = rect;
    done.set_width(rect.width() * fraction.clamp(0.0, 1.0));
    painter.rect_filled(done, 0.0, theme::INK);
}

/// A `SURFACE_RAISED` frame with `RADIUS_LG` corners, for menus and dialogs.
pub fn raised_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(theme::SURFACE_RAISED)
        .stroke(Stroke::new(theme::HAIRLINE, theme::LINE))
        .corner_radius(CornerRadius::same(theme::RADIUS_LG))
        .inner_margin(egui::Margin::same(theme::SPACE_3 as i8))
}

/// The version title in the display style with a chevron after it. A click
/// opens the version menu.
pub fn version_title(ui: &mut egui::Ui, title: &str) -> egui::Response {
    let galley =
        ui.painter()
            .layout_no_wrap(title.to_string(), theme::font(Text::Display), theme::INK);
    let gap = theme::SPACE_3;
    let size = galley.size() + egui::vec2(gap + theme::CHEVRON, 0.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let painter = ui.painter();
    painter.galley(rect.left_top(), galley, theme::INK);
    let half = theme::CHEVRON / 2.0;
    let middle = egui::pos2(rect.right() - half, rect.center().y);
    let stroke = Stroke::new(theme::FOCUS_WIDTH, theme::INK);
    painter.line_segment(
        [
            middle + egui::vec2(-half, -half / 2.0),
            middle + egui::vec2(0.0, half / 2.0),
        ],
        stroke,
    );
    painter.line_segment(
        [
            middle + egui::vec2(0.0, half / 2.0),
            middle + egui::vec2(half, -half / 2.0),
        ],
        stroke,
    );
    if response.has_focus() {
        let gap = theme::FOCUS_GAP + theme::FOCUS_WIDTH / 2.0;
        painter.rect_stroke(
            rect.expand(gap),
            CornerRadius::same(theme::RADIUS_MD),
            stroke,
            egui::StrokeKind::Middle,
        );
    }
    response
}

/// A row of a menu: words on the left in `style` (a tag in the mono style)
/// and a state word on the right. The row is highlighted under the pointer.
pub fn menu_row(ui: &mut egui::Ui, left: &str, style: Text, state: &str) -> egui::Response {
    let width = ui.available_width();
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width, theme::ROW), egui::Sense::click());
    let painter = ui.painter();
    if response.hovered() || response.has_focus() {
        painter.rect_filled(rect, CornerRadius::same(theme::RADIUS_SM), theme::LINE);
    }
    let inset = rect.shrink2(egui::vec2(theme::SPACE_2, 0.0));
    painter.text(
        inset.left_center(),
        egui::Align2::LEFT_CENTER,
        left,
        theme::font(style),
        theme::INK,
    );
    painter.text(
        inset.right_center(),
        egui::Align2::RIGHT_CENTER,
        state,
        theme::font(Text::Label),
        theme::INK_MUTED,
    );
    response
}
