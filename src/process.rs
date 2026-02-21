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
