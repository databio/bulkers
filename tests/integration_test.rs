use std::fs;
use std::path::PathBuf;
use std::process::Command;
use tempfile::TempDir;

fn bulker_bin() -> PathBuf {
    // Find the built binary
    let mut path = std::env::current_exe().unwrap();
    path.pop(); // Remove test binary name
    path.pop(); // Remove deps/
    path.push("bulker");
    path
}

fn create_test_manifest(dir: &std::path::Path) -> PathBuf {
    let manifest = r#"manifest:
  name: test-crate
  version: 1.0.0
  commands:
  - command: cowsay
    docker_image: nsheff/cowsay
    docker_command: cowsay
    docker_args: "-i"
  - command: fortune
    docker_image: nsheff/fortune
    docker_command: fortune
  host_commands:
  - ls
"#;
    let path = dir.join("test_manifest.yaml");
    fs::write(&path, manifest).unwrap();
    path
}

/// Helper: run bulker with XDG_CONFIG_HOME set to isolate manifest cache.
fn bulker_cmd(xdg_home: &std::path::Path) -> Command {
    let mut cmd = Command::new(bulker_bin());
    cmd.env("XDG_CONFIG_HOME", xdg_home);
    cmd
}

/// Helper: init a config in a temp dir.
fn init_config(tmp: &TempDir) -> PathBuf {
    let config_path = tmp.path().join("bulker_config.yaml");

    bulker_cmd(tmp.path())
        .args(["config", "init", "-c", config_path.to_str().unwrap()])
        .output()
        .expect("failed to run config init");

    config_path
}

/// Helper: install a test crate from a local manifest (populates manifest cache).
fn install_test_crate(tmp: &TempDir, config_path: &std::path::Path) {
    let manifest_path = create_test_manifest(tmp.path());
    let output = bulker_cmd(tmp.path())
        .args([
            "crate", "install",
            "-c", config_path.to_str().unwrap(),
            manifest_path.to_str().unwrap(),
        ])
        .output()
        .expect("failed to run crate install");
    assert!(output.status.success(), "crate install failed: {}\n{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout));
}

#[test]
fn test_config_init_creates_config() {
    let tmp = TempDir::new().unwrap();
    let config_path = tmp.path().join("bulker_config.yaml");

    let output = bulker_cmd(tmp.path())
        .args(["config", "init", "-c", config_path.to_str().unwrap()])
        .output()
        .expect("failed to run config init");

    assert!(output.status.success(), "config init failed: {}", String::from_utf8_lossy(&output.stderr));
    assert!(config_path.exists(), "config file not created");

    // Check templates were written
    let templates_dir = tmp.path().join("templates");
    assert!(templates_dir.exists(), "templates dir not created");
    assert!(templates_dir.join("docker_executable.tera").exists());
    assert!(templates_dir.join("start.sh").exists());
    assert!(templates_dir.join("zsh_start").join(".zshrc").exists());

    // Verify config content
    let content = fs::read_to_string(&config_path).unwrap();
    assert!(content.contains("container_engine"));
    assert!(content.contains("docker") || content.contains("apptainer"));
}

#[test]
fn test_crate_install_caches_manifest() {
    let tmp = TempDir::new().unwrap();
    let config_path = init_config(&tmp);
    let manifest_path = create_test_manifest(tmp.path());

    // Install using local file path -- now caches to manifest cache dir, not crate folder
    let output = bulker_cmd(tmp.path())
        .args([
            "crate", "install",
            "-c", config_path.to_str().unwrap(),
            manifest_path.to_str().unwrap(),
        ])
        .output()
        .expect("failed to run crate install");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "crate install failed: {}\n{}", stderr, stdout);
    assert!(stdout.contains("Cached:"), "install should report caching: {}", stdout);
}

#[test]
fn test_crate_list() {
    let tmp = TempDir::new().unwrap();
    let config_path = init_config(&tmp);
    install_test_crate(&tmp, &config_path);

    // List
    let output = bulker_cmd(tmp.path())
        .args(["crate", "list", "-c", config_path.to_str().unwrap()])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("test-crate") || stdout.contains("bulker/test-crate"),
        "list output missing crate: {}", stdout);
}

#[test]
fn test_crate_inspect() {
    let tmp = TempDir::new().unwrap();
    let config_path = init_config(&tmp);
    install_test_crate(&tmp, &config_path);

    // Inspect the cached crate (bulker/test-crate:1.0.0)
    let output = bulker_cmd(tmp.path())
        .args(["crate", "inspect", "-c", config_path.to_str().unwrap(), "bulker/test-crate:1.0.0"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "crate inspect failed: {}\n{}", stderr, stdout);
    assert!(stdout.contains("cowsay"), "inspect missing cowsay: {}", stdout);
    assert!(stdout.contains("fortune"), "inspect missing fortune: {}", stdout);
}

#[test]
fn test_activate_echo_mode() {
    let tmp = TempDir::new().unwrap();
    let config_path = init_config(&tmp);
    install_test_crate(&tmp, &config_path);

    // Activate with --echo using the cached crate name
    let output = bulker_cmd(tmp.path())
        .args([
            "activate",
            "-c", config_path.to_str().unwrap(),
            "--echo",
            "bulker/test-crate:1.0.0",
        ])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("export BULKERCRATE="), "missing BULKERCRATE export: {}", stdout);
    assert!(stdout.contains("export BULKERPATH="), "missing BULKERPATH export: {}", stdout);
    assert!(stdout.contains("export PATH="), "missing PATH export: {}", stdout);
    // With shimlinks, PATH contains /tmp/bulker_* shimlink dir
    assert!(stdout.contains("bulker_"), "PATH doesn't contain shimlink dir: {}", stdout);
}

#[test]
fn test_activate_local_manifest() {
    let tmp = TempDir::new().unwrap();
    let config_path = init_config(&tmp);
    let manifest_path = create_test_manifest(tmp.path());

    // Activate a local manifest file directly (no prior install needed)
    let output = bulker_cmd(tmp.path())
        .args([
            "activate",
            "-c", config_path.to_str().unwrap(),
            "--echo",
            manifest_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "activate local manifest failed: {}\n{}", stderr, stdout);
    assert!(stdout.contains("export PATH="), "missing PATH export: {}", stdout);
    assert!(stdout.contains("bulker_"), "PATH doesn't contain shimlink dir: {}", stdout);
}

#[test]
fn test_activate_force() {
    let tmp = TempDir::new().unwrap();
    let config_path = init_config(&tmp);
    install_test_crate(&tmp, &config_path);

    // Activate with --force should succeed with already-cached crate
    let output = bulker_cmd(tmp.path())
        .args([
            "activate",
            "-c", config_path.to_str().unwrap(),
            "--echo",
            "--force",
            "bulker/test-crate:1.0.0",
        ])
        .output()
        .unwrap();

    // --force will try to re-fetch from registry. The test-crate manifest likely
    // doesn't exist on the real registry, but we just check the flag is recognized.
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains("unrecognized"), "activate --force should be a recognized flag");
}

#[test]
fn test_crate_clean_specific() {
    let tmp = TempDir::new().unwrap();
    let config_path = init_config(&tmp);
    install_test_crate(&tmp, &config_path);

    // Clean the specific crate
    let output = bulker_cmd(tmp.path())
        .args(["crate", "clean", "-c", config_path.to_str().unwrap(), "bulker/test-crate:1.0.0"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "crate clean failed: {}", String::from_utf8_lossy(&output.stderr));
    assert!(stdout.contains("Removed:"), "clean should report removal: {}", stdout);

    // Verify no longer in list
    let list_output = bulker_cmd(tmp.path())
        .args(["crate", "list", "-c", config_path.to_str().unwrap()])
        .output()
        .unwrap();
    let list_stdout = String::from_utf8_lossy(&list_output.stdout);
    assert!(list_stdout.contains("No cached crates") || !list_stdout.contains("test-crate"),
        "crate still listed after clean: {}", list_stdout);
}

#[test]
fn test_crate_clean_all() {
    let tmp = TempDir::new().unwrap();
    let config_path = init_config(&tmp);
    install_test_crate(&tmp, &config_path);

    // Clean all cached crates
    let output = bulker_cmd(tmp.path())
        .args(["crate", "clean", "-c", config_path.to_str().unwrap(), "--all"])
        .output()
        .unwrap();

    assert!(output.status.success(), "crate clean --all failed: {}", String::from_utf8_lossy(&output.stderr));

    // Verify empty list
    let list_output = bulker_cmd(tmp.path())
        .args(["crate", "list", "-c", config_path.to_str().unwrap()])
        .output()
        .unwrap();
    let list_stdout = String::from_utf8_lossy(&list_output.stdout);
    assert!(list_stdout.contains("No cached crates"), "crates still listed after clean --all: {}", list_stdout);
}

#[test]
fn test_config_get_set() {
    let tmp = TempDir::new().unwrap();
    let config_path = tmp.path().join("bulker_config.yaml");

    bulker_cmd(tmp.path())
        .args(["config", "init", "-c", config_path.to_str().unwrap()])
        .output()
        .unwrap();

    // Get a value
    let output = bulker_cmd(tmp.path())
        .args(["config", "get", "-c", config_path.to_str().unwrap(), "container_engine"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.trim() == "docker" || stdout.trim() == "apptainer");

    // Set envvars
    let output = bulker_cmd(tmp.path())
        .args(["config", "set", "-c", config_path.to_str().unwrap(), "envvars=HOME,DISPLAY,MY_VAR"])
        .output()
        .unwrap();
    assert!(output.status.success());

    // Get envvars back
    let output = bulker_cmd(tmp.path())
        .args(["config", "get", "-c", config_path.to_str().unwrap(), "envvars"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("MY_VAR"), "MY_VAR not in envvars: {}", stdout);

    // Config show
    let output = bulker_cmd(tmp.path())
        .args(["config", "show", "-c", config_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("container_engine"), "config show missing content: {}", stdout);
}

#[test]
fn test_config_add_remove() {
    let tmp = TempDir::new().unwrap();
    let config_path = init_config(&tmp);

    // Add an envvar
    let output = bulker_cmd(tmp.path())
        .args(["config", "add", "-c", config_path.to_str().unwrap(), "envvars", "MY_CUSTOM_VAR"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Added 'MY_CUSTOM_VAR' to envvars"), "unexpected output: {}", stdout);

    // Verify it's there
    let output = bulker_cmd(tmp.path())
        .args(["config", "get", "-c", config_path.to_str().unwrap(), "envvars"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("MY_CUSTOM_VAR"), "added var not in envvars: {}", stdout);

    // Add duplicate should skip
    let output = bulker_cmd(tmp.path())
        .args(["config", "add", "-c", config_path.to_str().unwrap(), "envvars", "MY_CUSTOM_VAR"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("already in envvars"), "duplicate add should report already exists: {}", stdout);

    // Remove it
    let output = bulker_cmd(tmp.path())
        .args(["config", "remove", "-c", config_path.to_str().unwrap(), "envvars", "MY_CUSTOM_VAR"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Removed 'MY_CUSTOM_VAR' from envvars"), "unexpected output: {}", stdout);

    // Verify it's gone
    let output = bulker_cmd(tmp.path())
        .args(["config", "get", "-c", config_path.to_str().unwrap(), "envvars"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains("MY_CUSTOM_VAR"), "removed var still in envvars: {}", stdout);

    // Remove non-existent should not error
    let output = bulker_cmd(tmp.path())
        .args(["config", "remove", "-c", config_path.to_str().unwrap(), "envvars", "NONEXISTENT_VAR"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("not found in envvars"), "missing var should report not found: {}", stdout);
}

#[test]
fn test_config_add_rejects_scalar_key() {
    let tmp = TempDir::new().unwrap();
    let config_path = init_config(&tmp);

    // Trying to add to a scalar key should fail
    let output = bulker_cmd(tmp.path())
        .args(["config", "add", "-c", config_path.to_str().unwrap(), "container_engine", "podman"])
        .output()
        .unwrap();
    assert!(!output.status.success(), "add to scalar key should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not a list field"), "should mention not a list field: {}", stderr);
}

#[test]
fn test_host_exec_passthrough() {
    let output = Command::new(bulker_bin())
        .args(["host-exec", "/bin/echo", "hello", "world"])
        .output()
        .expect("failed to run host-exec");

    assert!(output.status.success(), "host-exec should succeed: {}",
        String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "hello world");
}

#[test]
fn test_activate_strict_echo_no_host_path() {
    let tmp = TempDir::new().unwrap();
    let config_path = init_config(&tmp);
    install_test_crate(&tmp, &config_path);

    // Activate with --strict --echo
    let output = bulker_cmd(tmp.path())
        .args([
            "activate",
            "-c", config_path.to_str().unwrap(),
            "--echo",
            "--strict",
            "bulker/test-crate:1.0.0",
        ])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "strict activate failed: {}\n{}", stderr, stdout);
    // In strict mode, PATH should only be the shimdir (no original PATH appended)
    let path_line = stdout.lines().find(|l| l.starts_with("export PATH=")).unwrap();
    assert!(!path_line.contains(":/"), "strict PATH should not contain original PATH segments: {}", path_line);
}
