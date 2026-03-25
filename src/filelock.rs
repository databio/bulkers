//! Advisory file locking via flock(2).

use anyhow::{Context, Result};
use nix::fcntl::{Flock, FlockArg};
use std::fs::{File, OpenOptions};
use std::path::Path;

/// An advisory lock on a sidecar `.lock` file. The lock is held until this
/// guard is dropped (which closes the file descriptor, releasing the lock).
pub struct FileLock {
    _flock: Flock<File>,
}

impl FileLock {
    /// Acquire an exclusive advisory lock on `path`. Creates the file if needed.
    /// Blocks until the lock is available.
    pub fn acquire(path: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .open(path)
            .with_context(|| format!("Failed to open lock file: {}", path.display()))?;
        let flock = Flock::lock(file, FlockArg::LockExclusive)
            .map_err(|(_file, errno)| {
                anyhow::anyhow!("Failed to acquire lock: {}: {}", path.display(), errno)
            })?;
        Ok(FileLock { _flock: flock })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn test_filelock_mutual_exclusion() {
        let tmpdir = tempfile::tempdir().unwrap();
        let lock_path = tmpdir.path().join("test.lock");
        let counter = Arc::new(Mutex::new(Vec::new()));

        let mut handles = vec![];
        for i in 0..2 {
            let lock_path = lock_path.clone();
            let counter = Arc::clone(&counter);
            handles.push(std::thread::spawn(move || {
                let _lock = FileLock::acquire(&lock_path).unwrap();
                // While holding the lock, record entry and exit
                counter.lock().unwrap().push(format!("enter-{}", i));
                std::thread::sleep(std::time::Duration::from_millis(50));
                counter.lock().unwrap().push(format!("exit-{}", i));
                // Lock released on drop
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        let events = counter.lock().unwrap();
        // With mutual exclusion, one thread's enter-exit pair must complete
        // before the other's enter. Check that entries alternate properly.
        assert_eq!(events.len(), 4);
        // First event is an enter, second is exit of same thread
        let first_id: String = events[0].replace("enter-", "");
        assert_eq!(events[1], format!("exit-{}", first_id));
        let second_id: String = events[2].replace("enter-", "");
        assert_eq!(events[3], format!("exit-{}", second_id));
        assert_ne!(first_id, second_id);
    }
}
