//! How the window's frame differs on each system. On macOS the launcher's
//! top bar is merged with the title bar, and the system's three window
//! buttons sit on it. On Windows the window has no system frame: the top
//! bar draws the window buttons at its right end, a drag on it moves the
//! window, and the edges resize it. On Linux the system's title bar stays,
//! set to dark. `KJK_LAUNCHER_FRAME=windows` shows the Windows frame on any
//! system. The pages know nothing of this.

use super::*;

/// The kinds of window frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    /// macOS: the top bar is the title bar, under the system's buttons.
    MacMerged,
    /// Windows: no system frame. The top bar has the window buttons.
    WindowsMerged,
    /// The system's own title bar, dark.
    System,
}

/// The frame for this system.
pub(super) fn kind() -> Kind {
    let forced = std::env::var("KJK_LAUNCHER_FRAME").is_ok_and(|v| v == "windows");
    if forced || cfg!(windows) {
        Kind::WindowsMerged
    } else if cfg!(target_os = "macos") {
        Kind::MacMerged
    } else {
        Kind::System
    }
}

/// The window's settings for the frame.
pub(super) fn viewport(builder: egui::ViewportBuilder) -> egui::ViewportBuilder {
    match kind() {
        Kind::MacMerged => builder
            .with_fullsize_content_view(true)
            .with_titlebar_shown(false)
            .with_title_shown(false),
        Kind::WindowsMerged => builder.with_decorations(false),
        Kind::System => builder,
    }
}

/// Asks the system for a dark title bar where the system draws it.
pub(super) fn start(ctx: &egui::Context) {
    if kind() == Kind::System {
        ctx.send_viewport_cmd(egui::ViewportCommand::SetTheme(egui::SystemTheme::Dark));
    }
}

/// The room kept free at each end of the top bar.
pub(super) struct Bar {
    pub left: f32,
    pub right: f32,
}

pub(super) fn bar(ctx: &egui::Context) -> Bar {
    let side = theme::SPACE_8;
    match kind() {
        Kind::MacMerged => {
            let fullscreen = ctx.input(|i| i.viewport().fullscreen) == Some(true);
            let left = if fullscreen { side } else { theme::MAC_BUTTONS };
            Bar { left, right: side }
        }
        Kind::WindowsMerged => Bar {
            left: side,
            right: 0.0,
        },
        Kind::System => Bar {
            left: side,
            right: side,
        },
    }
}

/// Lets a drag on an empty part of the top bar move the window, and a
/// double-click on it toggle maximized. Call it before the bar's controls
/// are added, so they take the clicks on top of it.
pub(super) fn bar_background(ui: &mut egui::Ui, rect: egui::Rect) {
    if kind() == Kind::System {
        return;
    }
    let response = ui.interact(
        rect,
        ui.scope_id().with("title bar"),
        egui::Sense::click_and_drag(),
    );
    if response.double_clicked() {
        let maximized = ui.input(|i| i.viewport().maximized) == Some(true);
        ui.ctx()
            .send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
    } else if response.drag_started_by(egui::PointerButton::Primary) {
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }
}

fn maximized(ctx: &egui::Context) -> bool {
    ctx.input(|i| i.viewport().maximized) == Some(true)
}

/// The window buttons at the right end of the top bar, on Windows: close
/// first in a right-to-left layout, so they read minimize, maximize or
/// restore, and close.
pub(super) fn window_buttons(ui: &mut egui::Ui) {
    if kind() != Kind::WindowsMerged {
        return;
    }
    let ctx = ui.ctx().clone();
    ui.spacing_mut().item_spacing.x = 0.0;
    if window_button(ui, Glyph::Close).clicked() {
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
    let maximized = maximized(&ctx);
    let glyph = if maximized {
        Glyph::Restore
    } else {
        Glyph::Maximize
    };
    if window_button(ui, glyph).clicked() {
        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
    }
    if window_button(ui, Glyph::Minimize).clicked() {
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Glyph {
    Minimize,
    Maximize,
    Restore,
    Close,
}

/// One window button: a `WINDOW_BUTTON` area with a thin glyph. Under the
/// pointer the area is `SURFACE_RAISED`, or `ERROR` for close.
fn window_button(ui: &mut egui::Ui, glyph: Glyph) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(theme::WINDOW_BUTTON, egui::Sense::click());
    let painter = ui.painter();
    let mut ink = theme::INK_MUTED;
    if response.hovered() {
        let (fill, hovered_ink) = if glyph == Glyph::Close {
            (theme::ERROR, theme::INK_BRIGHT)
        } else {
            (theme::SURFACE_RAISED, theme::INK)
        };
        painter.rect_filled(rect, 0.0, fill);
        ink = hovered_ink;
    }
    let stroke = egui::Stroke::new(theme::HAIRLINE, ink);
    let c = rect.center();
    let half = theme::WINDOW_GLYPH / 2.0;
    let square = |center: egui::Pos2, side: f32| {
        egui::Rect::from_center_size(center, egui::vec2(side, side))
    };
    match glyph {
        Glyph::Minimize => {
            painter.hline(c.x - half..=c.x + half, c.y, stroke);
        }
        Glyph::Maximize => {
            painter.rect_stroke(
                square(c, theme::WINDOW_GLYPH),
                0.0,
                stroke,
                egui::StrokeKind::Middle,
            );
        }
        Glyph::Restore => {
            let side = theme::WINDOW_GLYPH - theme::RESTORE_OFFSET;
            let shift = egui::vec2(theme::RESTORE_OFFSET, -theme::RESTORE_OFFSET) / 2.0;
            let back = square(c + shift, side);
            let front = square(c - shift, side);
            // The back square shows only above and right of the front one.
            painter.hline(back.x_range(), back.top(), stroke);
            painter.vline(back.right(), back.y_range(), stroke);
            painter.rect_stroke(front, 0.0, stroke, egui::StrokeKind::Middle);
        }
        Glyph::Close => {
            let corner = egui::vec2(half, half);
            let other = egui::vec2(half, -half);
            painter.line_segment([c - corner, c + corner], stroke);
            painter.line_segment([c - other, c + other], stroke);
        }
    }
    response
}

/// On Windows, while the window is not maximized: a 1px `LINE` border
/// around it, and a `RESIZE_EDGE` zone at each edge and corner that shows
/// the resize pointer and starts a resize on a press.
pub(super) fn edges(ctx: &egui::Context) {
    if kind() != Kind::WindowsMerged || maximized(ctx) {
        return;
    }
    let rect = ctx.content_rect();
    ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::unique("window border"),
    ))
    .rect_stroke(
        rect,
        0.0,
        egui::Stroke::new(theme::HAIRLINE, theme::LINE),
        egui::StrokeKind::Inside,
    );
    let Some(at) = ctx.input(|i| i.pointer.hover_pos()) else {
        return;
    };
    let Some((direction, cursor)) = resize_zone(rect, at) else {
        return;
    };
    ctx.set_cursor_icon(cursor);
    if ctx.input(|i| i.pointer.primary_pressed()) {
        ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(direction));
    }
}

/// The resize direction and pointer for a point near the window's edge.
fn resize_zone(
    rect: egui::Rect,
    at: egui::Pos2,
) -> Option<(egui::ResizeDirection, egui::CursorIcon)> {
    use egui::{CursorIcon as C, ResizeDirection as D};
    let zone = theme::RESIZE_EDGE;
    let west = at.x - rect.left() < zone;
    let east = rect.right() - at.x < zone;
    let north = at.y - rect.top() < zone;
    let south = rect.bottom() - at.y < zone;
    Some(match (north, south, west, east) {
        (true, _, true, _) => (D::NorthWest, C::ResizeNwSe),
        (true, _, _, true) => (D::NorthEast, C::ResizeNeSw),
        (_, true, true, _) => (D::SouthWest, C::ResizeNeSw),
        (_, true, _, true) => (D::SouthEast, C::ResizeNwSe),
        (true, ..) => (D::North, C::ResizeVertical),
        (_, true, ..) => (D::South, C::ResizeVertical),
        (.., true, _) => (D::West, C::ResizeHorizontal),
        (.., true) => (D::East, C::ResizeHorizontal),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resize_zones_at_each_edge_and_corner() {
        use egui::ResizeDirection as D;
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        let cases = [
            ((2.0, 2.0), Some(D::NorthWest)),
            ((798.0, 2.0), Some(D::NorthEast)),
            ((2.0, 598.0), Some(D::SouthWest)),
            ((798.0, 598.0), Some(D::SouthEast)),
            ((400.0, 2.0), Some(D::North)),
            ((400.0, 598.0), Some(D::South)),
            ((2.0, 300.0), Some(D::West)),
            ((798.0, 300.0), Some(D::East)),
            ((400.0, 300.0), None),
        ];
        for ((x, y), expected) in cases {
            let found = resize_zone(rect, egui::pos2(x, y)).map(|(d, _)| d);
            assert_eq!(found, expected, "at {x}, {y}");
        }
    }
}
