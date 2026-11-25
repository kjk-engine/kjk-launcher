//! kjk-launcher: keeps installed versions of kjk side by side, checks
//! GitHub for a newer release when it starts, and installs one only when
//! the player asks. Saves and settings stay in kjk's own folder; the
//! launcher backs them up before a version starts for the first time.

pub mod args;
pub mod backup;
pub mod cli;
pub mod config;
pub mod folders;
pub mod github;
pub mod install;
pub mod kjk_report;
pub mod kjk_settings;
pub mod launch;
pub mod lighting;
pub mod log;
pub mod notes;
pub mod run;
pub mod selfupdate;
pub mod shot;
pub mod size;
pub mod theme;
pub mod ui;
pub mod versions;
pub mod widgets;
