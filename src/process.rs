use nix::sys::signal::{Signal, kill, killpg};
use nix::unistd::Pid;
use std::sync::atomic::{AtomicI32, Ordering};
use std::thread;
use std::time::{Duration, Instant};

/// Global child PID for signal handler access.
pub static CHILD_PID: AtomicI32 = AtomicI32::new(-1);

/// Gracefully kill a process group with escalating signals.
///
/// Sends SIGINT, waits 1s; then SIGTERM, waits 1s; then SIGKILL, waits 0.5s.
/// Uses killpg to signal the entire process group (the child is the group leader
/// because we spawned it with setsid).
pub fn graceful_kill_group(pgid: Pid) {
    let escalation = [
        (Signal::SIGINT, Duration::from_secs(1)),
        (Signal::SIGTERM, Duration::from_secs(1)),
        (Signal::SIGKILL, Duration::from_millis(500)),
    ];

    for (signal, timeout) in &escalation {
        // Check if process group still exists
        if kill(pgid, None).is_err() {
            return; // process already gone
        }

        // Send signal to entire process group
        let _ = killpg(pgid, *signal);
        log::debug!("Sent {:?} to process group {}", signal, pgid);

        // Wait for process to die
        let start = Instant::now();
        while start.elapsed() < *timeout {
            if kill(pgid, None).is_err() {
                return; // dead
            }
            thread::sleep(Duration::from_millis(100));
        }
    }
}

/// Set up signal handler thread that forwards signals to the child process group.
pub fn setup_signal_forwarding() {
    use signal_hook::consts::{SIGINT, SIGTERM};
    use signal_hook::iterator::Signals;

    let mut signals = Signals::new([SIGINT, SIGTERM]).expect("Failed to register signal handlers");

    thread::spawn(move || {
        for _sig in signals.forever() {
            let pid = CHILD_PID.load(Ordering::SeqCst);
            if pid > 0 {
                graceful_kill_group(Pid::from_raw(pid));
            }
        }
    });
}

/// Spawn a child process in a new session with signal forwarding and wait for it.
/// Returns the child's exit code (or 1 if unavailable).
pub fn spawn_and_wait(program: &str, args: &[impl AsRef<std::ffi::OsStr>]) -> anyhow::Result<i32> {
    use anyhow::Context;
    use std::os::unix::process::CommandExt;

    setup_signal_forwarding();

    let child = unsafe {
        std::process::Command::new(program)
            .args(args)
            .pre_exec(|| {
                nix::unistd::setsid()
                    .map_err(|e| std::io::Error::from_raw_os_error(e as i32))?;
                Ok(())
            })
            .spawn()
            .with_context(|| format!("Failed to spawn: {}", program))?
    };

    let child_pid = child.id() as i32;
    CHILD_PID.store(child_pid, Ordering::SeqCst);

    let mut child = child;
    let status = child.wait().context("Failed to wait on child process")?;
    Ok(status.code().unwrap_or(1))
}

/// Like `spawn_and_wait` but runs via `/bin/sh -c`.
pub fn spawn_shell_and_wait(shell_command: &str) -> anyhow::Result<i32> {
    spawn_and_wait("/bin/sh", &["-c", shell_command])
}
