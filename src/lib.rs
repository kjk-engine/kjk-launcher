//! kjk-launcher: keeps installed versions of kjk side by side, checks
//! GitHub for a newer release when it starts, and installs one only when
//! the player asks. Saves and settings stay in kjk's own folder; the
//! launcher backs them up before a version starts for the first time.

pub mod backup;
pub mod cli;
pub mod config;
pub mod github;
pub mod install;
pub mod launch;
pub mod shot;
pub mod theme;
pub mod ui;
pub mod widgets;
