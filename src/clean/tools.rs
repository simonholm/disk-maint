//! Explicit cleanup of version stores, using independent active-version evidence.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub path: PathBuf,
    pub bytes: u64,
}

#[derive(Debug)]
pub struct Component {
    pub name: &'static str,
    pub stale: Vec<Entry>,
    pub issue: Option<String>,
}

pub struct Plan {
    home: PathBuf,
    pub components: Vec<Component>,
}

pub fn plan() -> Result<Plan, String> {
    let home = crate::home_dir().ok_or("HOME is unavailable; tool cleanup skipped")?;
    Ok(detect(&home))
}

fn command_path(name: &str) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    std::env::split_paths(&std::env::var_os("PATH")?).find_map(|dir| {
        let path = dir.join(name);
        let metadata = fs::metadata(&path).ok()?;
        (metadata.is_file() && metadata.permissions().mode() & 0o111 != 0).then_some(path)
    })
}

fn codex_version() -> Result<Option<String>, String> {
    let Some(path) = command_path("codex") else {
        return Ok(None);
    };
    let output = Command::new(path)
        .arg("--version")
        .output()
        .map_err(|e| format!("codex --version failed: {e}"))?;
    if !output.status.success() {
        return Err("codex --version failed".into());
    }
    let text = String::from_utf8(output.stdout).map_err(|_| "invalid codex --version output")?;
    let version = text
        .trim()
        .strip_prefix("codex-cli ")
        .filter(|v| version_name(v))
        .ok_or("unrecognized codex --version output")?;
    Ok(Some(version.to_string()))
}

fn detect(home: &Path) -> Plan {
    detect_with(home, command_path("claude").as_deref(), codex_version())
}

fn detect_with(home: &Path, claude: Option<&Path>, codex: Result<Option<String>, String>) -> Plan {
    let components = ["standalone", "app-server-daemon"]
        .into_iter()
        .map(|name| {
            let root = home.join(".codex/packages").join(name);
            component(name, || {
                let store = root.join("releases");
                safe_store(home, &store)?;
                let marker = root.join("auto-update-version");
                if !fs::symlink_metadata(&marker)
                    .map_err(|e| e.to_string())?
                    .is_file()
                {
                    return Err("auto-update-version must be a regular file".into());
                }
                let active = fs::read_to_string(marker)
                    .map_err(|e| format!("cannot read auto-update-version: {e}"))?;
                let active = active.trim();
                if !version_name(active) {
                    return Err("invalid auto-update-version".into());
                }
                let active_path = store.join(active);
                if !fs::symlink_metadata(&active_path)
                    .map_err(|e| format!("active release missing: {e}"))?
                    .is_dir()
                {
                    return Err("active release is not a real directory".into());
                }
                // A current pointer is supplementary evidence, never a fallback for the marker.
                let current = root.join("current");
                match fs::symlink_metadata(&current) {
                    Ok(_) => {
                        let resolved = fs::canonicalize(current)
                            .map_err(|e| format!("cannot resolve current pointer: {e}"))?;
                        if resolved != fs::canonicalize(&active_path).map_err(|e| e.to_string())? {
                            return Err("current pointer conflicts with auto-update-version".into());
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(format!("cannot inspect current pointer: {e}")),
                }
                if name == "standalone" {
                    match &codex {
                        Ok(Some(version))
                            if active != version && !active.starts_with(&format!("{version}-")) =>
                        {
                            return Err("codex --version conflicts with auto-update-version".into());
                        }
                        Err(error) => return Err(error.clone()),
                        _ => {}
                    }
                }
                stale_entries(&store, &active_path, true)
            })
        })
        .chain(std::iter::once(component("Claude", || {
            let store = home.join(".local/share/claude/versions");
            safe_store(home, &store)?;
            let active = fs::canonicalize(claude.ok_or("claude command not found")?)
                .map_err(|e| format!("cannot resolve claude command: {e}"))?;
            let canonical_store = fs::canonicalize(&store).map_err(|e| e.to_string())?;
            if active.parent() != Some(canonical_store.as_path())
                || !active.is_file()
                || !active
                    .file_name()
                    .and_then(|v| v.to_str())
                    .is_some_and(version_name)
            {
                return Err("claude command does not resolve to a version-store binary".into());
            }
            stale_entries(&store, &store.join(active.file_name().unwrap()), false)
        })))
        .collect();
    Plan {
        home: home.to_path_buf(),
        components,
    }
}

fn component(name: &'static str, detect: impl FnOnce() -> Result<Vec<Entry>, String>) -> Component {
    match detect() {
        Ok(stale) => Component {
            name,
            stale,
            issue: None,
        },
        Err(issue) => Component {
            name,
            stale: Vec::new(),
            issue: Some(issue),
        },
    }
}

// Do not traverse redirected stores or accept path-like marker values.
fn safe_store(home: &Path, store: &Path) -> Result<(), String> {
    let mut path = home.to_path_buf();
    for part in store
        .strip_prefix(home)
        .map_err(|e| e.to_string())?
        .components()
    {
        path.push(part);
        if !fs::symlink_metadata(&path)
            .map_err(|e| format!("cannot inspect {}: {e}", path.display()))?
            .is_dir()
        {
            return Err(format!("{} is not a real directory", path.display()));
        }
    }
    Ok(())
}

fn version_name(name: &str) -> bool {
    let base = name.split('-').next().unwrap_or("");
    let parts: Vec<_> = base.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".-_".contains(&b))
}

fn stale_entries(store: &Path, active: &Path, directories: bool) -> Result<Vec<Entry>, String> {
    let mut stale = Vec::new();
    for entry in fs::read_dir(store).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        if path == active
            || !entry.file_name().to_str().is_some_and(version_name)
            || !(if directories {
                kind.is_dir()
            } else {
                kind.is_file()
            })
        {
            continue;
        }
        stale.push(Entry {
            bytes: version_size(&path)?,
            path,
        });
    }
    stale.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(stale)
}

// Count all contents of releases, without Cargo-specific directory exclusions.
fn version_size(path: &Path) -> Result<u64, String> {
    let metadata = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if metadata.is_file() {
        return Ok(metadata.len());
    }
    if !metadata.is_dir() {
        return Ok(0);
    }
    fs::read_dir(path)
        .map_err(|e| e.to_string())?
        .map(|entry| version_size(&entry.map_err(|e| e.to_string())?.path()))
        .sum()
}

pub fn has_stale(plan: &Plan) -> bool {
    plan.components.iter().any(|c| !c.stale.is_empty())
}

pub fn render(plan: &Plan) -> String {
    let mut output = String::from("Tool versions\n\n");
    for (label, names, noun) in [
        (
            "Codex stale releases",
            &["standalone", "app-server-daemon"][..],
            "directories",
        ),
        ("Claude stale versions", &["Claude"][..], "versions"),
    ] {
        let entries: Vec<_> = plan
            .components
            .iter()
            .filter(|c| names.contains(&c.name))
            .flat_map(|c| &c.stale)
            .collect();
        if !entries.is_empty() {
            output.push_str(&format!(
                "{label:<22} {} ({} {noun})\n",
                crate::format_bytes(entries.iter().map(|e| e.bytes).sum()),
                entries.len()
            ));
        }
    }
    for c in &plan.components {
        if let Some(issue) = &c.issue {
            output.push_str(&format!("{}: cleanup skipped: {issue}\n", c.name));
        }
    }
    if has_stale(plan) {
        output.push_str("\n    safe to remove with\n    `disk-maint clean tools`\n");
    } else if plan.components.iter().all(|c| c.issue.is_none()) {
        output.push_str("No stale tool versions.\n");
    } else {
        output.push_str("No stale tool versions identified safely.\n");
    }
    output
}

pub fn execute(plan: &Plan) -> Result<String, String> {
    execute_with(plan, || detect(&plan.home))
}

fn execute_with(plan: &Plan, mut refresh: impl FnMut() -> Plan) -> Result<String, String> {
    let mut output = String::new();
    let mut total = 0;
    for component in &plan.components {
        for entry in &component.stale {
            // Refresh even after confirmation, and before every individual removal.
            let fresh = refresh();
            let Some(current) = fresh.components.iter().find(|c| c.name == component.name) else {
                continue;
            };
            let Some(stale) = current.stale.iter().find(|e| e.path == entry.path) else {
                output.push_str(&format!(
                    "Skipped {}: {}.\n",
                    entry.path.display(),
                    current
                        .issue
                        .as_deref()
                        .unwrap_or("no longer positively identified as stale")
                ));
                continue;
            };
            let result = if component.name == "Claude" {
                fs::remove_file(&stale.path)
            } else {
                fs::remove_dir_all(&stale.path)
            };
            if let Err(error) = result {
                return Err(format!(
                    "{output}failed to remove {}: {error}; reclaimed approximately {} so far",
                    stale.path.display(),
                    crate::format_bytes(total)
                ));
            }
            total += stale.bytes;
            output.push_str(&format!(
                "Removed {} {} ({}).\n",
                component.name,
                stale.path.file_name().unwrap().to_string_lossy(),
                crate::format_bytes(stale.bytes)
            ));
        }
    }
    output.push_str(&format!(
        "Reclaimed approximately {}.",
        crate::format_bytes(total)
    ));
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let home = std::env::temp_dir().join(format!(
                "disk-maint-tools-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&home).unwrap();
            let fixture = Self(home);
            for component in ["standalone", "app-server-daemon"] {
                let root = fixture.root(component);
                for version in ["0.160.0-linux", "0.159.0-linux"] {
                    fs::create_dir_all(root.join("releases").join(version)).unwrap();
                    fs::write(root.join("releases").join(version).join("binary"), b"codex")
                        .unwrap();
                }
                fs::write(root.join("auto-update-version"), "0.160.0-linux\n").unwrap();
                fs::write(root.join("install.lock"), "lock").unwrap();
            }
            let store = fixture.claude_store();
            fs::create_dir_all(&store).unwrap();
            for version in ["2.1.1", "2.1.2"] {
                fs::write(store.join(version), b"claude").unwrap();
            }
            symlink(store.join("2.1.2"), fixture.0.join("claude")).unwrap();
            fixture
        }
        fn root(&self, name: &str) -> PathBuf {
            self.0.join(".codex/packages").join(name)
        }
        fn claude_store(&self) -> PathBuf {
            self.0.join(".local/share/claude/versions")
        }
        fn plan(&self) -> Plan {
            detect_with(
                &self.0,
                Some(&self.0.join("claude")),
                Ok(Some("0.160.0".into())),
            )
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn identifies_active_and_stale_versions_and_sizes() {
        let f = Fixture::new();
        let p = f.plan();
        assert!(p.components.iter().all(|c| c.issue.is_none()));
        assert_eq!(
            p.components[0].stale[0].path,
            f.root("standalone").join("releases/0.159.0-linux")
        );
        assert_eq!(p.components[0].stale[0].bytes, 5);
        assert_eq!(
            p.components[2].stale[0].path,
            f.claude_store().join("2.1.1")
        );
        let output = render(&p);
        assert!(output.contains("10B (2 directories)"));
        assert!(output.contains("6B (1 versions)"));
        assert!(output.contains("disk-maint clean tools"));
    }

    #[test]
    fn release_sizes_include_build_and_other_contents_without_following_links() {
        let f = Fixture::new();
        let stale = f.root("standalone").join("releases/0.159.0-linux");
        fs::create_dir(stale.join("build")).unwrap();
        fs::write(stale.join("build/artifact"), b"1234567").unwrap();
        symlink(f.claude_store(), stale.join("external")).unwrap();
        assert_eq!(f.plan().components[0].stale[0].bytes, 12);
    }

    #[test]
    fn missing_invalid_and_nonexistent_markers_fail_safe() {
        let f = Fixture::new();
        let marker = f.root("standalone").join("auto-update-version");
        for contents in [
            "",
            "../0.160.0-linux",
            "0.160.0-linux\n0.159.0-linux",
            "0.999.0-linux",
        ] {
            fs::write(&marker, contents).unwrap();
            let p = f.plan();
            assert!(p.components[0].issue.is_some(), "{contents:?}");
            assert!(p.components[0].stale.is_empty());
            assert!(p.components[1].issue.is_none());
        }
        fs::remove_file(marker).unwrap();
        assert!(f.plan().components[0].issue.is_some());
    }

    #[test]
    fn codex_cli_and_current_conflicts_block_only_affected_component() {
        let f = Fixture::new();
        let p = detect_with(&f.0, Some(&f.0.join("claude")), Ok(Some("0.159.0".into())));
        assert!(
            p.components[0]
                .issue
                .as_ref()
                .unwrap()
                .contains("conflicts")
        );
        assert!(p.components[1].issue.is_none());
        symlink(
            f.root("app-server-daemon").join("releases/0.159.0-linux"),
            f.root("app-server-daemon").join("current"),
        )
        .unwrap();
        assert!(
            f.plan().components[1]
                .issue
                .as_ref()
                .unwrap()
                .contains("conflicts")
        );
        let p = detect_with(&f.0, None, Err("failed CLI check".into()));
        assert!(p.components[0].stale.is_empty());
    }

    #[test]
    fn claude_missing_broken_or_external_launcher_is_ambiguous() {
        let f = Fixture::new();
        for path in [
            None,
            Some(f.0.join("missing")),
            Some(f.root("standalone").join("install.lock")),
        ] {
            let p = detect_with(&f.0, path.as_deref(), Ok(None));
            assert!(p.components[2].issue.is_some());
            assert!(p.components[2].stale.is_empty());
        }
    }

    #[test]
    fn cleanup_preserves_active_metadata_unrelated_files_and_symlinks() {
        let f = Fixture::new();
        let releases = f.root("standalone").join("releases");
        fs::write(releases.join("metadata.json"), "metadata").unwrap();
        fs::create_dir(releases.join("incomplete-download")).unwrap();
        symlink(
            releases.join("0.160.0-linux"),
            releases.join("0.158.0-linux"),
        )
        .unwrap();
        fs::write(f.claude_store().join("install.lock"), "lock").unwrap();
        symlink(
            f.claude_store().join("2.1.2"),
            f.claude_store().join("2.1.0"),
        )
        .unwrap();
        let output = execute_with(&f.plan(), || f.plan()).unwrap();
        assert!(output.contains("Reclaimed approximately 16B"));
        assert!(output.contains("Removed standalone 0.159.0-linux"));
        for component in ["standalone", "app-server-daemon"] {
            let root = f.root(component);
            assert!(root.join("releases/0.160.0-linux/binary").exists());
            assert!(!root.join("releases/0.159.0-linux").exists());
            assert!(root.join("auto-update-version").exists());
            assert!(root.join("install.lock").exists());
        }
        assert!(releases.join("metadata.json").exists());
        assert!(releases.join("incomplete-download").exists());
        assert!(releases.join("0.158.0-linux").is_symlink());
        assert!(f.claude_store().join("2.1.2").exists());
        assert!(!f.claude_store().join("2.1.1").exists());
        assert!(f.claude_store().join("install.lock").exists());
        assert!(f.claude_store().join("2.1.0").is_symlink());
        assert!(render(&f.plan()).contains("No stale tool versions."));
    }

    #[test]
    fn rechecks_active_versions_after_plan_and_before_each_removal() {
        let f = Fixture::new();
        let p = f.plan();
        let mut calls = 0;
        let output = execute_with(&p, || {
            calls += 1;
            if calls == 1 {
                fs::write(
                    f.root("standalone").join("auto-update-version"),
                    "0.159.0-linux",
                )
                .unwrap();
                fs::remove_file(f.0.join("claude")).unwrap();
                symlink(f.claude_store().join("2.1.1"), f.0.join("claude")).unwrap();
            }
            detect_with(&f.0, Some(&f.0.join("claude")), Ok(None))
        })
        .unwrap();
        assert_eq!(calls, 3);
        assert!(output.contains("Skipped"));
        assert!(f.root("standalone").join("releases/0.159.0-linux").exists());
        assert!(f.root("standalone").join("releases/0.160.0-linux").exists());
        assert!(f.claude_store().join("2.1.1").exists());
        assert!(f.claude_store().join("2.1.2").exists());
    }

    #[test]
    fn metadata_disappearing_after_plan_prevents_deletion() {
        let f = Fixture::new();
        let p = f.plan();
        for component in ["standalone", "app-server-daemon"] {
            fs::remove_file(f.root(component).join("auto-update-version")).unwrap();
        }
        fs::remove_file(f.0.join("claude")).unwrap();
        let output = execute_with(&p, || f.plan()).unwrap();
        assert!(output.contains("Reclaimed approximately 0B"));
        assert!(f.root("standalone").join("releases/0.159.0-linux").exists());
        assert!(f.claude_store().join("2.1.1").exists());
    }

    #[test]
    fn redirected_store_and_active_symlink_fail_safe() {
        let f = Fixture::new();
        let active = f.root("standalone").join("releases/0.160.0-linux");
        fs::remove_dir_all(&active).unwrap();
        symlink("0.159.0-linux", &active).unwrap();
        assert!(f.plan().components[0].issue.is_some());
        let store = f.claude_store();
        let moved = f.0.join("redirected");
        fs::rename(&store, &moved).unwrap();
        symlink(&moved, &store).unwrap();
        assert!(f.plan().components[2].issue.is_some());
    }
}
