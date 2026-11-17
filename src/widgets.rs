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
    add(ui, outlined(label, Text::Body))
}

fn outlined(label: &str, style: Text) -> egui::Button<'static> {
    egui::Button::new(text(label, style, theme::INK))
        .fill(egui::Color32::TRANSPARENT)
        .stroke(Stroke::new(theme::HAIRLINE, theme::CONTROL_BORDER))
        .corner_radius(CornerRadius::same(theme::RADIUS_MD))
}

/// A compact secondary button for the end of a row or the side of a line
/// of text: label-size words, `COMPACT` high, `SPACE_3` at each side.
pub fn compact_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    let button = outlined(label, Text::Label).min_size(egui::vec2(0.0, theme::COMPACT));
    ui.scope(|ui| {
        ui.spacing_mut().button_padding = egui::vec2(theme::SPACE_3, 0.0);
        add(ui, button)
    })
    .inner
}

/// A compact number field, `COMPACT` high, that takes whole numbers in
/// `range`.
pub fn number_field(
    ui: &mut egui::Ui,
    value: &mut usize,
    range: std::ops::RangeInclusive<usize>,
) -> egui::Response {
    ui.scope(|ui| {
        let spacing = ui.spacing_mut();
        spacing.button_padding = egui::vec2(theme::SPACE_2, 0.0);
        spacing.interact_size = egui::vec2(theme::NUMBER_WIDTH, theme::COMPACT);
        add(ui, egui::DragValue::new(value).range(range))
    })
    .inner
}

/// A line of controls and words, all centred on one line `COMPACT` high.
pub fn line<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let size = egui::vec2(ui.available_width(), theme::COMPACT);
    let layout = egui::Layout::left_to_right(egui::Align::Center);
    ui.allocate_ui_with_layout(size, layout, add).inner
}

/// How far below the top of a line of text in `style` the middle of its
/// lower-case letters is. Text whose top is put this far above a line has
/// its x-height centred on that line.
pub fn x_middle(ui: &egui::Ui, style: Text) -> f32 {
    let galley = ui
        .painter()
        .layout_no_wrap("x".into(), theme::font(style), theme::INK);
    let Some(row) = galley.rows.first() else {
        return galley.size().y / 2.0;
    };
    match row.glyphs.first() {
        Some(x) => row.pos.y + x.pos.y + x.uv_rect.offset.y + x.uv_rect.size.y / 2.0,
        None => galley.size().y / 2.0,
    }
}

/// How wide `words` are in `style`.
pub fn width(ui: &egui::Ui, words: &str, style: Text) -> f32 {
    ui.painter()
        .layout_no_wrap(words.to_string(), theme::font(style), theme::INK)
        .size()
        .x
}

/// Paints `words` with the middle of their x-height on `middle_y`, from
/// `left`, and returns where they are.
pub fn paint_centred(
    ui: &egui::Ui,
    words: &str,
    style: Text,
    color: egui::Color32,
    left: f32,
    middle_y: f32,
) -> egui::Rect {
    let galley = ui
        .painter()
        .layout_no_wrap(words.to_string(), theme::font(style), color);
    let top = egui::pos2(left, middle_y - x_middle(ui, style));
    let rect = egui::Rect::from_min_size(top, galley.size());
    ui.painter().galley(top, galley, color);
    rect
}

/// Words in one column of a list line.
#[derive(Clone, Copy)]
pub struct Cell<'a> {
    pub words: &'a str,
    pub style: Text,
    pub color: egui::Color32,
}

impl<'a> Cell<'a> {
    pub fn new(words: &'a str, style: Text, color: egui::Color32) -> Cell<'a> {
        Cell {
            words,
            style,
            color,
        }
    }
}

/// Where each column of a list starts, so the columns line up down the
/// list: each one `SPACE_8` after the widest words of the column before.
pub fn columns(ui: &egui::Ui, rows: &[Vec<Cell>]) -> Vec<f32> {
    let count = rows.iter().map(Vec::len).max().unwrap_or(0);
    let mut starts = vec![0.0];
    for column in 0..count.saturating_sub(1) {
        let widest = rows
            .iter()
            .filter_map(|row| row.get(column))
            .map(|cell| {
                ui.painter()
                    .layout_no_wrap(cell.words.to_string(), theme::font(cell.style), cell.color)
                    .size()
                    .x
            })
            .fold(0.0, f32::max);
        starts.push(starts[column] + widest + theme::SPACE_8);
    }
    starts
}

/// One line of a list, `LIST_ROW` high: `cells` at the column `starts`,
/// centred on the line, and `right` adding its controls from the right end.
pub fn list_line(
    ui: &mut egui::Ui,
    starts: &[f32],
    cells: &[Cell],
    right: impl FnOnce(&mut egui::Ui),
) -> egui::Response {
    let width = ui.available_width();
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width, theme::LIST_ROW), egui::Sense::hover());
    let middle = rect.center().y;
    for (cell, start) in cells.iter().zip(starts) {
        paint_centred(
            ui,
            cell.words,
            cell.style,
            cell.color,
            rect.left() + start,
            middle,
        );
    }
    let layout = egui::Layout::right_to_left(egui::Align::Center);
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect).layout(layout), right);
    response
}

/// A state word at the right of a list line, such as "Selected".
pub fn state_word(ui: &mut egui::Ui, word: &str, color: egui::Color32) {
    ui.label(text(word, Text::Label, color));
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
    focus_outline(ui, &response);
    response
}

/// The focus outline of `add`, for a control drawn by hand. A control that
/// gets the focus from the keyboard is scrolled into view. The outline may
/// reach past the edge of a scrolling list, so it is drawn a little outside
/// the list's own clip.
pub fn focus_outline(ui: &egui::Ui, response: &egui::Response) {
    if response.gained_focus() {
        response.scroll_to_me(None);
    }
    if response.has_focus() {
        let gap = theme::FOCUS_GAP + theme::FOCUS_WIDTH / 2.0;
        let clip = ui.clip_rect().expand(gap + theme::FOCUS_WIDTH);
        ui.painter().with_clip_rect(clip).rect_stroke(
            response.rect.expand(gap),
            CornerRadius::same(theme::RADIUS_MD + gap as u8),
            Stroke::new(theme::FOCUS_WIDTH, theme::INK),
            egui::StrokeKind::Middle,
        );
    }
}

/// A check box with the focus outline.
pub fn checkbox(ui: &mut egui::Ui, value: &mut bool, label: &str) -> egui::Response {
    add(ui, egui::Checkbox::new(value, label))
}

/// A web link in the mono style, with the focus outline.
pub fn link(ui: &mut egui::Ui, url: &str) -> egui::Response {
    let words = text(url, Text::Mono, theme::INK);
    add(ui, egui::Hyperlink::from_label_and_url(words, url))
}

/// Mono text, such as a path or a command line, that wraps only after a
/// path separator or a space, so no name is cut in two. A name longer
/// than the whole line is cut where it must be.
pub fn wrapped_mono(ui: &mut egui::Ui, words: &str, color: egui::Color32) {
    let room = ui.available_width();
    let lines = wrap_at_separators(words, |line| width(ui, line, Text::Mono) <= room);
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for line in lines {
            ui.add(egui::Label::new(text(line, Text::Mono, color)).wrap());
        }
    });
}

/// Splits `words` into lines that each `fit`, breaking only after a `/`, a
/// `\` or a space. A piece that does not fit by itself gets a line of its
/// own.
pub fn wrap_at_separators(words: &str, fits: impl Fn(&str) -> bool) -> Vec<String> {
    let mut pieces = Vec::new();
    let mut start = 0;
    for (at, c) in words.char_indices() {
        if matches!(c, '/' | '\\' | ' ') {
            let end = at + c.len_utf8();
            pieces.push(&words[start..end]);
            start = end;
        }
    }
    if start < words.len() {
        pieces.push(&words[start..]);
    }
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    for piece in pieces {
        let longer = format!("{line}{piece}");
        if line.is_empty() || fits(longer.trim_end()) {
            line = longer;
        } else {
            lines.push(line.trim_end().to_string());
            line = piece.to_string();
        }
    }
    if !line.is_empty() {
        lines.push(line.trim_end().to_string());
    }
    lines
}

/// An 8px dot in the status color, then the word in the label style. In a
/// right-to-left layout the two are added in reverse, so the dot still
/// comes first.
pub fn status(ui: &mut egui::Ui, status: Status, word: &str) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = theme::SPACE_2;
        let dot = |ui: &mut egui::Ui| {
            let size = egui::vec2(theme::DOT, theme::DOT);
            let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
            ui.painter()
                .circle_filled(rect.center(), theme::DOT / 2.0, status.color());
        };
        let words = |ui: &mut egui::Ui| {
            ui.label(text(word, Text::Label, theme::INK));
        };
        if ui.layout().prefer_right_to_left() {
            words(ui);
            dot(ui);
        } else {
            dot(ui);
            words(ui);
        }
    });
}

/// A notice: the dot in the status color, then the words in the label
/// style, wrapped over as many lines as they need, and "Dismiss" at the
/// right end. Returns true when Dismiss is pressed.
pub fn notice(ui: &mut egui::Ui, status: Status, words: &str) -> bool {
    let mut dismissed = false;
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
        dismissed = text_button(ui, "Dismiss").clicked();
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
            ui.spacing_mut().item_spacing.x = theme::SPACE_2;
            let size = egui::vec2(theme::DOT, x_middle(ui, Text::Label) * 2.0);
            let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
            ui.painter()
                .circle_filled(rect.center(), theme::DOT / 2.0, status.color());
            ui.add(egui::Label::new(text(words, Text::Label, theme::INK)).wrap());
        });
    });
    dismissed
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
    focus_outline(ui, &response);
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
    focus_outline(ui, &response);
    response
}

/// A page's title in the title style.
pub fn page_title(ui: &mut egui::Ui, words: &str) {
    ui.label(text(words, Text::Title, theme::INK));
    ui.add_space(theme::SPACE_4);
}

/// A sentence in the body style, in `INK_MUTED`.
pub fn muted(ui: &mut egui::Ui, words: impl Into<String>) {
    ui.label(text(words, Text::Body, theme::INK_MUTED));
}

/// A row of a list: `left` stacks its lines on the left, and `right` adds
/// its controls from the right end, the first one rightmost. The left
/// lines wrap before the room kept for the controls.
pub fn row(ui: &mut egui::Ui, left: impl FnOnce(&mut egui::Ui), right: impl FnOnce(&mut egui::Ui)) {
    ui.add_space(theme::SPACE_3);
    ui.horizontal(|ui| {
        let width = (ui.available_width() - theme::ROW_CONTROLS).max(0.0);
        ui.vertical(|ui| {
            ui.set_max_width(width);
            left(ui);
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), right);
    });
    ui.add_space(theme::SPACE_3);
}

#[cfg(test)]
mod tests {
    use super::wrap_at_separators;

    #[test]
    fn paths_wrap_only_at_separators_and_spaces() {
        let fits = |n: usize| move |line: &str| line.chars().count() <= n;
        let cases: [(&str, usize, &[&str]); 5] = [
            ("/Users/me/kjk", 40, &["/Users/me/kjk"]),
            (
                "/Users/me/Library/Application Support/kjk",
                20,
                &["/Users/me/Library/", "Application Support/", "kjk"],
            ),
            (
                r"C:\Games\kjk launcher\kjk-launcher.exe",
                16,
                &[r"C:\Games\kjk", r"launcher\", "kjk-launcher.exe"],
            ),
            (
                "\"/opt/kjk-launcher\" --play %command%",
                20,
                &["\"/opt/kjk-launcher\"", "--play %command%"],
            ),
            // A name longer than the line keeps a line of its own.
            (
                "/a/averyveryverylongname/b",
                8,
                &["/a/", "averyveryverylongname/", "b"],
            ),
        ];
        for (words, room, expected) in cases {
            assert_eq!(wrap_at_separators(words, fits(room)), expected, "{words}");
        }
    }
}
