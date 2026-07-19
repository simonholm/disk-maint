use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn clean_shared_yes_deletes_configured_shared_target() {
    let temp = test_dir("disk-maint-it-clean-shared-yes");
    let shared = temp.join("shared-target");
    fs::create_dir_all(&shared).unwrap();
    fs::write(shared.join("artifact"), "artifact").unwrap();
    cargo_project_with_shared_target(&temp);

    let output = Command::new(env!("CARGO_BIN_EXE_disk-maint"))
        .arg("--root")
        .arg(&temp)
        .arg("clean")
        .arg("shared")
        .arg("--yes")
        .env("CARGO_TARGET_DIR", &shared)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("The shared Cargo target directory will be removed"));
    assert!(stdout.contains("Deleted shared Cargo target directory."));
    assert!(!shared.exists());

    fs::remove_dir_all(temp).unwrap();
}

#[test]
fn clean_shared_prompts_before_deleting_configured_shared_target() {
    let temp = test_dir("disk-maint-it-clean-shared-confirm");
    let shared = temp.join("shared-target");
    fs::create_dir_all(&shared).unwrap();
    fs::write(shared.join("artifact"), "artifact").unwrap();
    cargo_project_with_shared_target(&temp);

    let mut child = Command::new(env!("CARGO_BIN_EXE_disk-maint"))
        .arg("--root")
        .arg(&temp)
        .arg("clean")
        .arg("shared")
        .env("CARGO_TARGET_DIR", &shared)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    child.stdin.as_mut().unwrap().write_all(b"yes\n").unwrap();
    let output = child.wait_with_output().unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Type 'yes' to continue"));
    assert!(stdout.contains("Deleted shared Cargo target directory."));
    assert!(!shared.exists());

    fs::remove_dir_all(temp).unwrap();
}

#[test]
fn clean_shared_absent_prints_noop_message() {
    let temp = test_dir("disk-maint-it-clean-shared-absent");
    let shared = temp.join("shared-target");
    cargo_project_with_shared_target(&temp);

    let output = Command::new(env!("CARGO_BIN_EXE_disk-maint"))
        .arg("--root")
        .arg(&temp)
        .arg("clean")
        .arg("shared")
        .env("CARGO_TARGET_DIR", &shared)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        "No shared Cargo target directory found. No files will be deleted."
    );

    fs::remove_dir_all(temp).unwrap();
}

fn cargo_project_with_shared_target(root: &std::path::Path) {
    let project = root.join("example");
    fs::create_dir_all(project.join("src")).unwrap();
    fs::create_dir_all(root.join(".cargo")).unwrap();
    fs::write(
        project.join("Cargo.toml"),
        "[package]\nname = \"example\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    fs::write(project.join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(
        root.join(".cargo/config.toml"),
        format!(
            "[build]\ntarget-dir = \"{}\"\n",
            root.join("shared-target").display()
        ),
    )
    .unwrap();
}

fn test_dir(name: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).unwrap();
    path
}
