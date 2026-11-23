//! Running a helper program for a check (`kjk installs`):
//! without a console window on Windows, and stopped when it takes too long.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// How long a check may take before it is stopped.
#[cfg(not(test))]
pub const TIMEOUT: Duration = Duration::from_secs(10);
#[cfg(test)]
pub const TIMEOUT: Duration = Duration::from_secs(1);

/// What `program` printed when it ran with `args` and exited with success.
/// `None` when it could not start, failed, or ran longer than `TIMEOUT`.
pub fn output(program: &Path, args: &[&str]) -> Option<String> {
    output_with(program, args, &[])
}

/// `output` for kjk: with the variables of `config::kjk_env`, so a kjk
/// whose folder was moved finds it.
pub fn kjk_output(program: &Path, args: &[&str]) -> Option<String> {
    output_with(program, args, &crate::config::kjk_env())
}

fn output_with(program: &Path, args: &[&str], env: &[(&str, PathBuf)]) -> Option<String> {
    let mut command = Command::new(program);
    command
        .args(args)
        .envs(env.iter().map(|(name, value)| (name, value)))
        // A program that waits for input waits until it is stopped.
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    no_window(&mut command);
    let mut child = command.spawn().ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        stdout.read_to_string(&mut text).ok().map(|_| text)
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if started.elapsed() < TIMEOUT => {
                std::thread::sleep(Duration::from_millis(20));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let text = reader.join().ok().flatten();
    status.filter(|s| s.success()).and(text)
}

/// Starts the program without a console window on Windows.
#[cfg(windows)]
pub fn no_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
pub fn no_window(_command: &mut Command) {}
