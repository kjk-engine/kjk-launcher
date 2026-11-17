//! How the window's frame differs on each system. On macOS the launcher's
//! top bar is merged with the title bar, and the system's three window
//! buttons sit on it. On other systems the system's title bar stays, set
//! to dark. The pages know nothing of this.

use super::*;

/// The kinds of window frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    /// macOS: the top bar is the title bar, under the system's buttons.
    MacMerged,
    /// The system's own title bar, dark.
    System,
}

/// The frame for this system.
pub(super) fn kind() -> Kind {
    if cfg!(target_os = "macos") {
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
        Kind::System => builder,
    }
}

/// Asks the system for a dark title bar where the system draws it.
pub(super) fn start(ctx: &egui::Context) {
    if kind() == Kind::System {
        ctx.send_viewport_cmd(egui::ViewportCommand::SetTheme(egui::SystemTheme::Dark));
    }
}

/// The top bar's margins and the height its content is centered in.
pub(super) struct Bar {
    pub margin: egui::Margin,
    pub height: f32,
}

pub(super) fn bar(ctx: &egui::Context) -> Bar {
    let side = theme::SPACE_8 as i8;
    match kind() {
        Kind::MacMerged => {
            let fullscreen = ctx.input(|i| i.viewport().fullscreen) == Some(true);
            let left = if fullscreen {
                theme::SPACE_8
            } else {
                theme::MAC_BUTTONS
            };
            Bar {
                margin: egui::Margin {
                    left: left as i8,
                    right: side,
                    top: 0,
                    bottom: theme::SPACE_2 as i8,
                },
                height: theme::MAC_TITLE_BAR,
            }
        }
        Kind::System => Bar {
            margin: egui::Margin::symmetric(side, theme::SPACE_3 as i8),
            height: theme::MARK,
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
