use std::sync::{Mutex, MutexGuard};

/// Global mutex to serialize all tests that modify environment variables.
/// Since `std::env::set_var` affects the entire process, concurrent tests
/// that modify the same env vars (e.g. XDG_CONFIG_HOME) will race.
/// This mutex ensures only one such test runs at a time.
static ENV_MUTEX: Mutex<()> = Mutex::new(());

/// RAII guard that restores an environment variable on drop.
/// Handles both "was set to X" and "was not set" cases.
/// Holds a global mutex lock to prevent concurrent env var modifications.
pub(crate) struct EnvGuard {
    key: String,
    original: Option<String>,
    _lock: MutexGuard<'static, ()>,
}

impl EnvGuard {
    /// Save the current value of `key`, set it to `value`, and hold a global
    /// lock until this guard is dropped. This serializes all tests that modify
    /// environment variables.
    pub fn set(key: &str, value: impl AsRef<std::ffi::OsStr>) -> Self {
        let lock = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let original = std::env::var(key).ok();
        // SAFETY: We hold ENV_MUTEX so no other EnvGuard-using test is running.
        unsafe {
            std::env::set_var(key, value);
        }
        EnvGuard {
            key: key.to_string(),
            original,
            _lock: lock,
        }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        unsafe {
            match &self.original {
                Some(v) => std::env::set_var(&self.key, v),
                None => std::env::remove_var(&self.key),
            }
        }
    }
}

/// Build a minimal manifest with given imports (for tests).
pub(crate) fn make_manifest_with_imports(name: &str, imports: Vec<String>) -> crate::manifest::Manifest {
    crate::manifest::Manifest {
        manifest: crate::manifest::ManifestInner {
            name: Some(name.to_string()),
            version: None,
            commands: vec![crate::manifest::PackageCommand {
                command: name.to_string(),
                docker_image: format!("test/{}:latest", name),
                ..Default::default()
            }],
            host_commands: vec![],
            imports,
        },
    }
}
