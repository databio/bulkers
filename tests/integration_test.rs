use std::fs;
use std::path::PathBuf;
use std::process::Command;
use tempfile::TempDir;

fn bulkers_bin() -> PathBuf {
    // Find the built binary
    let mut path = std::env::current_exe().unwrap();
    path.pop(); // Remove test binary name
    path.pop(); // Remove deps/
    path.push("bulkers");
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

#[test]
fn test_init_creates_config() {
    let tmp = TempDir::new().unwrap();
    let config_path = tmp.path().join("bulker_config.yaml");

    let output = Command::new(bulkers_bin())
        .args(["init", "-c", config_path.to_str().unwrap()])
        .output()
        .expect("failed to run bulkers init");

    assert!(output.status.success(), "init failed: {}", String::from_utf8_lossy(&output.stderr));
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
    assert!(content.contains("docker") || content.contains("singularity"));
}

#[test]
fn test_load_creates_executables() {
    let tmp = TempDir::new().unwrap();
    let config_path = tmp.path().join("bulker_config.yaml");

    // Init first
    let output = Command::new(bulkers_bin())
        .args(["init", "-c", config_path.to_str().unwrap()])
        .output()
        .expect("failed to run init");
    assert!(output.status.success(), "init failed: {}", String::from_utf8_lossy(&output.stderr));

    // Patch the config to set crate folder inside tmp
    let config_content = fs::read_to_string(&config_path).unwrap();
    let crate_folder = tmp.path().join("crates");
    let mut lines: Vec<String> = config_content.lines().map(|l| l.to_string()).collect();
    for line in &mut lines {
        if line.contains("default_crate_folder") {
            *line = format!("  default_crate_folder: {}", crate_folder.to_str().unwrap());
        }
    }
    fs::write(&config_path, lines.join("\n")).unwrap();

    // Create test manifest
    let manifest_path = create_test_manifest(tmp.path());

    // Load
    let output = Command::new(bulkers_bin())
        .args([
            "load",
            "-c", config_path.to_str().unwrap(),
            "-m", manifest_path.to_str().unwrap(),
            "test/demo:1.0",
        ])
        .output()
        .expect("failed to run load");

    assert!(output.status.success(), "load failed: {}\n{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout));

    // Verify executable scripts were created
    let crate_dir = crate_folder.join("test").join("demo").join("1.0");
    assert!(crate_dir.exists(), "crate dir not created: {}", crate_dir.display());
    assert!(crate_dir.join("cowsay").exists(), "cowsay executable not created");
    assert!(crate_dir.join("fortune").exists(), "fortune executable not created");
    assert!(crate_dir.join("_cowsay").exists(), "cowsay shell wrapper not created");
    assert!(crate_dir.join("_fortune").exists(), "fortune shell wrapper not created");
    assert!(crate_dir.join("ls").exists(), "ls host command symlink not created");

    // Verify executable content
    let cowsay_content = fs::read_to_string(crate_dir.join("cowsay")).unwrap();
    assert!(cowsay_content.starts_with("#!/bin/sh"), "missing shebang");
    assert!(cowsay_content.contains("docker run"), "missing docker run");
    assert!(cowsay_content.contains("nsheff/cowsay"), "missing image name");
    assert!(cowsay_content.contains("cowsay"), "missing command name");

    // Verify config was updated with crate entry
    let updated_config = fs::read_to_string(&config_path).unwrap();
    assert!(updated_config.contains("demo"), "config not updated with crate");
}

#[test]
fn test_load_then_list() {
    let tmp = TempDir::new().unwrap();
    let config_path = tmp.path().join("bulker_config.yaml");

    // Init
    Command::new(bulkers_bin())
        .args(["init", "-c", config_path.to_str().unwrap()])
        .output()
        .unwrap();

    // Patch crate folder
    let config_content = fs::read_to_string(&config_path).unwrap();
    let crate_folder = tmp.path().join("crates");
    let mut lines: Vec<String> = config_content.lines().map(|l| l.to_string()).collect();
    for line in &mut lines {
        if line.contains("default_crate_folder") {
            *line = format!("  default_crate_folder: {}", crate_folder.to_str().unwrap());
        }
    }
    fs::write(&config_path, lines.join("\n")).unwrap();

    // Load
    let manifest_path = create_test_manifest(tmp.path());
    Command::new(bulkers_bin())
        .args([
            "load",
            "-c", config_path.to_str().unwrap(),
            "-m", manifest_path.to_str().unwrap(),
            "test/demo:1.0",
        ])
        .output()
        .unwrap();

    // List
    let output = Command::new(bulkers_bin())
        .args(["list", "-c", config_path.to_str().unwrap()])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("test/demo:1.0"), "list output missing crate: {}", stdout);
}

#[test]
fn test_load_then_inspect() {
    let tmp = TempDir::new().unwrap();
    let config_path = tmp.path().join("bulker_config.yaml");

    Command::new(bulkers_bin())
        .args(["init", "-c", config_path.to_str().unwrap()])
        .output()
        .unwrap();

    let config_content = fs::read_to_string(&config_path).unwrap();
    let crate_folder = tmp.path().join("crates");
    let mut lines: Vec<String> = config_content.lines().map(|l| l.to_string()).collect();
    for line in &mut lines {
        if line.contains("default_crate_folder") {
            *line = format!("  default_crate_folder: {}", crate_folder.to_str().unwrap());
        }
    }
    fs::write(&config_path, lines.join("\n")).unwrap();

    let manifest_path = create_test_manifest(tmp.path());
    Command::new(bulkers_bin())
        .args([
            "load",
            "-c", config_path.to_str().unwrap(),
            "-m", manifest_path.to_str().unwrap(),
            "test/demo:1.0",
        ])
        .output()
        .unwrap();

    // Inspect
    let output = Command::new(bulkers_bin())
        .args(["inspect", "-c", config_path.to_str().unwrap(), "test/demo:1.0"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("cowsay"), "inspect missing cowsay: {}", stdout);
    assert!(stdout.contains("fortune"), "inspect missing fortune: {}", stdout);
}

#[test]
fn test_load_then_unload() {
    let tmp = TempDir::new().unwrap();
    let config_path = tmp.path().join("bulker_config.yaml");

    Command::new(bulkers_bin())
        .args(["init", "-c", config_path.to_str().unwrap()])
        .output()
        .unwrap();

    let config_content = fs::read_to_string(&config_path).unwrap();
    let crate_folder = tmp.path().join("crates");
    let mut lines: Vec<String> = config_content.lines().map(|l| l.to_string()).collect();
    for line in &mut lines {
        if line.contains("default_crate_folder") {
            *line = format!("  default_crate_folder: {}", crate_folder.to_str().unwrap());
        }
    }
    fs::write(&config_path, lines.join("\n")).unwrap();

    let manifest_path = create_test_manifest(tmp.path());
    Command::new(bulkers_bin())
        .args([
            "load",
            "-c", config_path.to_str().unwrap(),
            "-m", manifest_path.to_str().unwrap(),
            "test/demo:1.0",
        ])
        .output()
        .unwrap();

    // Verify crate dir exists
    let crate_dir = crate_folder.join("test").join("demo").join("1.0");
    assert!(crate_dir.exists());

    // Unload
    let output = Command::new(bulkers_bin())
        .args(["unload", "-c", config_path.to_str().unwrap(), "test/demo:1.0"])
        .output()
        .unwrap();

    assert!(output.status.success(), "unload failed: {}", String::from_utf8_lossy(&output.stderr));

    // Verify crate dir removed
    assert!(!crate_dir.exists(), "crate dir still exists after unload");

    // Verify removed from config
    let config_content = fs::read_to_string(&config_path).unwrap();
    assert!(!config_content.contains("1.0"), "crate still in config after unload");
}

#[test]
fn test_activate_echo_mode() {
    let tmp = TempDir::new().unwrap();
    let config_path = tmp.path().join("bulker_config.yaml");

    Command::new(bulkers_bin())
        .args(["init", "-c", config_path.to_str().unwrap()])
        .output()
        .unwrap();

    let config_content = fs::read_to_string(&config_path).unwrap();
    let crate_folder = tmp.path().join("crates");
    let mut lines: Vec<String> = config_content.lines().map(|l| l.to_string()).collect();
    for line in &mut lines {
        if line.contains("default_crate_folder") {
            *line = format!("  default_crate_folder: {}", crate_folder.to_str().unwrap());
        }
    }
    fs::write(&config_path, lines.join("\n")).unwrap();

    let manifest_path = create_test_manifest(tmp.path());
    Command::new(bulkers_bin())
        .args([
            "load",
            "-c", config_path.to_str().unwrap(),
            "-m", manifest_path.to_str().unwrap(),
            "test/demo:1.0",
        ])
        .output()
        .unwrap();

    // Activate with --echo
    let output = Command::new(bulkers_bin())
        .args([
            "activate",
            "-c", config_path.to_str().unwrap(),
            "--echo",
            "test/demo:1.0",
        ])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("export BULKERCRATE="), "missing BULKERCRATE export: {}", stdout);
    assert!(stdout.contains("export BULKERPATH="), "missing BULKERPATH export: {}", stdout);
    assert!(stdout.contains("export PATH="), "missing PATH export: {}", stdout);
    assert!(stdout.contains(&crate_folder.to_string_lossy().to_string()), "PATH doesn't contain crate folder: {}", stdout);
}

#[test]
fn test_envvars_add_remove() {
    let tmp = TempDir::new().unwrap();
    let config_path = tmp.path().join("bulker_config.yaml");

    Command::new(bulkers_bin())
        .args(["init", "-c", config_path.to_str().unwrap()])
        .output()
        .unwrap();

    // Add envvar
    let output = Command::new(bulkers_bin())
        .args(["envvars", "-c", config_path.to_str().unwrap(), "-a", "MY_VAR"])
        .output()
        .unwrap();
    assert!(output.status.success());

    let config_content = fs::read_to_string(&config_path).unwrap();
    assert!(config_content.contains("MY_VAR"), "MY_VAR not added to config");

    // Remove envvar
    let output = Command::new(bulkers_bin())
        .args(["envvars", "-c", config_path.to_str().unwrap(), "-r", "MY_VAR"])
        .output()
        .unwrap();
    assert!(output.status.success());

    let config_content = fs::read_to_string(&config_path).unwrap();
    assert!(!config_content.contains("MY_VAR"), "MY_VAR not removed from config");
}
