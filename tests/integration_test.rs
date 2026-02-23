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

/// Helper: init a config and patch the crate folder to a temp dir.
fn init_config(tmp: &TempDir) -> (PathBuf, PathBuf) {
    let config_path = tmp.path().join("bulker_config.yaml");
    let crate_folder = tmp.path().join("crates");

    Command::new(bulkers_bin())
        .args(["config", "init", "-c", config_path.to_str().unwrap()])
        .output()
        .expect("failed to run config init");

    // Patch crate folder
    let config_content = fs::read_to_string(&config_path).unwrap();
    let mut lines: Vec<String> = config_content.lines().map(|l| l.to_string()).collect();
    for line in &mut lines {
        if line.contains("default_crate_folder") {
            *line = format!("  default_crate_folder: {}", crate_folder.to_str().unwrap());
        }
    }
    fs::write(&config_path, lines.join("\n")).unwrap();

    (config_path, crate_folder)
}

/// Helper: install a test crate from a local manifest.
fn install_test_crate(tmp: &TempDir, config_path: &std::path::Path) {
    let manifest_path = create_test_manifest(tmp.path());
    let output = Command::new(bulkers_bin())
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

    let output = Command::new(bulkers_bin())
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
    assert!(content.contains("docker") || content.contains("singularity"));
}

#[test]
fn test_crate_install_creates_executables() {
    let tmp = TempDir::new().unwrap();
    let (config_path, crate_folder) = init_config(&tmp);
    let manifest_path = create_test_manifest(tmp.path());

    // Install using local file path
    let output = Command::new(bulkers_bin())
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

    // The manifest file path is used as the cratefile, so it gets parsed as a registry path.
    // Since it's a local file, the crate name will be derived from the path.
    // Let's check the config to see what was stored.
    let updated_config = fs::read_to_string(&config_path).unwrap();
    assert!(updated_config.contains("path:"), "config not updated with crate entry");

    // Verify cowsay executable exists somewhere in the crate folder
    let has_cowsay = walkdir(crate_folder.clone(), "cowsay");
    assert!(has_cowsay, "cowsay executable not created in crate folder");
}

/// Walk a directory looking for a file with the given name.
fn walkdir(dir: PathBuf, name: &str) -> bool {
    if !dir.exists() { return false; }
    for entry in fs::read_dir(&dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            if walkdir(path, name) { return true; }
        } else if entry.file_name().to_string_lossy() == name {
            return true;
        }
    }
    false
}

#[test]
fn test_crate_list() {
    let tmp = TempDir::new().unwrap();
    let (config_path, _crate_folder) = init_config(&tmp);
    install_test_crate(&tmp, &config_path);

    // List
    let output = Command::new(bulkers_bin())
        .args(["crate", "list", "-c", config_path.to_str().unwrap()])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("test_manifest"), "list output missing crate: {}", stdout);
}

#[test]
fn test_crate_inspect() {
    let tmp = TempDir::new().unwrap();
    let (config_path, _crate_folder) = init_config(&tmp);
    install_test_crate(&tmp, &config_path);

    // Figure out the crate name from list output
    let list_output = Command::new(bulkers_bin())
        .args(["crate", "list", "-c", config_path.to_str().unwrap(), "--simple"])
        .output()
        .unwrap();
    let crate_name = String::from_utf8_lossy(&list_output.stdout).trim().to_string();

    // Inspect
    let output = Command::new(bulkers_bin())
        .args(["crate", "inspect", "-c", config_path.to_str().unwrap(), &crate_name])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("cowsay"), "inspect missing cowsay: {}", stdout);
    assert!(stdout.contains("fortune"), "inspect missing fortune: {}", stdout);
}

#[test]
fn test_crate_uninstall() {
    let tmp = TempDir::new().unwrap();
    let (config_path, _crate_folder) = init_config(&tmp);
    install_test_crate(&tmp, &config_path);

    // Get crate name
    let list_output = Command::new(bulkers_bin())
        .args(["crate", "list", "-c", config_path.to_str().unwrap(), "--simple"])
        .output()
        .unwrap();
    let crate_name = String::from_utf8_lossy(&list_output.stdout).trim().to_string();

    // Uninstall
    let output = Command::new(bulkers_bin())
        .args(["crate", "uninstall", "-c", config_path.to_str().unwrap(), &crate_name])
        .output()
        .unwrap();

    assert!(output.status.success(), "crate uninstall failed: {}", String::from_utf8_lossy(&output.stderr));

    // Verify removed from list
    let list_output = Command::new(bulkers_bin())
        .args(["crate", "list", "-c", config_path.to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&list_output.stdout);
    assert!(stdout.contains("No crates installed"), "crate still listed after uninstall: {}", stdout);
}

#[test]
fn test_activate_echo_mode() {
    let tmp = TempDir::new().unwrap();
    let (config_path, crate_folder) = init_config(&tmp);
    install_test_crate(&tmp, &config_path);

    // Get crate name
    let list_output = Command::new(bulkers_bin())
        .args(["crate", "list", "-c", config_path.to_str().unwrap(), "--simple"])
        .output()
        .unwrap();
    let crate_name = String::from_utf8_lossy(&list_output.stdout).trim().to_string();

    // Activate with --echo
    let output = Command::new(bulkers_bin())
        .args([
            "activate",
            "-c", config_path.to_str().unwrap(),
            "--echo",
            &crate_name,
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
fn test_config_get_set() {
    let tmp = TempDir::new().unwrap();
    let config_path = tmp.path().join("bulker_config.yaml");

    Command::new(bulkers_bin())
        .args(["config", "init", "-c", config_path.to_str().unwrap()])
        .output()
        .unwrap();

    // Get a value
    let output = Command::new(bulkers_bin())
        .args(["config", "get", "-c", config_path.to_str().unwrap(), "container_engine"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.trim() == "docker" || stdout.trim() == "singularity");

    // Set envvars
    let output = Command::new(bulkers_bin())
        .args(["config", "set", "-c", config_path.to_str().unwrap(), "envvars=HOME,DISPLAY,MY_VAR"])
        .output()
        .unwrap();
    assert!(output.status.success());

    // Get envvars back
    let output = Command::new(bulkers_bin())
        .args(["config", "get", "-c", config_path.to_str().unwrap(), "envvars"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("MY_VAR"), "MY_VAR not in envvars: {}", stdout);

    // Config show
    let output = Command::new(bulkers_bin())
        .args(["config", "show", "-c", config_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("container_engine"), "config show missing content: {}", stdout);
}
