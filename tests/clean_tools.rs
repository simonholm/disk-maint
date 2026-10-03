use std::fs;
use std::io::Write;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Fixture(std::path::PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let home = std::env::temp_dir().join(format!(
            "disk-maint-tools-cli-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(home.join("bin")).unwrap();
        for name in ["standalone", "app-server-daemon"] {
            let root = home.join(".codex/packages").join(name);
            for version in ["0.160.0-linux", "0.159.0-linux"] {
                fs::create_dir_all(root.join("releases").join(version)).unwrap();
                fs::write(root.join("releases").join(version).join("app"), "app").unwrap();
            }
            fs::write(root.join("auto-update-version"), "0.160.0-linux").unwrap();
        }
        let store = home.join(".local/share/claude/versions");
        fs::create_dir_all(&store).unwrap();
        for version in ["2.1.1", "2.1.2"] {
            fs::write(store.join(version), "#!/bin/sh\n").unwrap();
            fs::set_permissions(store.join(version), fs::Permissions::from_mode(0o755)).unwrap();
        }
        symlink(store.join("2.1.2"), home.join("bin/claude")).unwrap();
        Self(home)
    }
    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_disk-maint"));
        command.env("HOME", &self.0).env("PATH", self.0.join("bin"));
        command
    }
    fn assert_versions(&self, stale: bool) {
        for name in ["standalone", "app-server-daemon"] {
            let root = self.0.join(".codex/packages").join(name);
            assert!(root.join("releases/0.160.0-linux/app").exists());
            assert_eq!(root.join("releases/0.159.0-linux").exists(), stale);
            assert!(root.join("auto-update-version").exists());
        }
        let store = self.0.join(".local/share/claude/versions");
        assert!(store.join("2.1.2").exists());
        assert_eq!(store.join("2.1.1").exists(), stale);
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn clean_tools_yes_deletes_stale_versions_only() {
    let f = Fixture::new();
    let output = f
        .command()
        .args(["clean", "tools", "--yes"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Removed standalone 0.159.0-linux"));
    assert!(stdout.contains("Removed Claude 2.1.1"));
    assert!(stdout.contains("Reclaimed approximately"));
    assert!(!stdout.contains("Type 'yes'"));
    f.assert_versions(false);
}

#[test]
fn clean_tools_requires_exact_confirmation() {
    for answer in ["yes\n", "y\n", ""] {
        let f = Fixture::new();
        let mut child = f
            .command()
            .args(["clean", "tools"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(answer.as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success());
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(stdout.contains("Type 'yes' to continue"));
        if answer != "yes\n" {
            assert!(stdout.contains("Aborted"));
        }
        f.assert_versions(answer != "yes\n");
    }
}

#[test]
fn scan_reports_tool_versions_without_modifying_them() {
    let f = Fixture::new();
    let repos = f.0.join("repos");
    fs::create_dir(&repos).unwrap();
    let output = f
        .command()
        .arg("--root")
        .arg(&repos)
        .arg("scan")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Tool versions"));
    assert!(stdout.contains("Codex stale releases"));
    assert!(stdout.contains("Claude stale versions"));
    assert!(stdout.contains("disk-maint clean tools"));
    f.assert_versions(true);
}
