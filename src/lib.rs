//! kjk-launcher: keeps installed versions of kjk side by side, checks
//! GitHub for a newer release when it starts, and installs one only when
//! the player asks. Saves and settings stay in kjk's own folder; the
//! launcher backs them up before a version starts for the first time.

pub mod backup;
pub mod cli;
pub mod config;
pub mod github;
pub mod install;
pub mod kjk_report;
pub mod kjk_settings;
pub mod launch;
pub mod run;
pub mod shot;
pub mod theme;
pub mod ui;
pub mod versions;
pub mod widgets;
