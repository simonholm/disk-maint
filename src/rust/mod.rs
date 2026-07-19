use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::format_bytes;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustProject {
    pub name: String,
    pub path: PathBuf,
    pub source_bytes: u64,
    pub target_bytes: u64,
    pub workspace_members: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoBuildArtifacts {
    pub projects: Vec<RustProject>,
    pub shared_target: Option<CargoTargetDir>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoTargetDir {
    pub path: PathBuf,
    pub bytes: u64,
}

impl RustProject {
    pub fn reclaimable_bytes(&self) -> u64 {
        self.target_bytes
    }
}

pub fn report(root: &Path) -> Result<String, String> {
    let projects = discover_projects(root)?;
    Ok(render_projects(&projects))
}

pub fn discover_build_artifacts(root: &Path) -> Result<CargoBuildArtifacts, String> {
    let projects = discover_projects(root)?;
    let shared_target = discover_shared_target_dir_from_projects(&projects)?
        .map(|path| target_dir(&path))
        .transpose()?
        .flatten();

    Ok(CargoBuildArtifacts {
        projects,
        shared_target,
    })
}

pub fn discover_shared_target_dir(root: &Path) -> Result<Option<PathBuf>, String> {
    let projects = discover_projects(root)?;
    discover_shared_target_dir_from_projects(&projects)
}

fn discover_shared_target_dir_from_projects(
    projects: &[RustProject],
) -> Result<Option<PathBuf>, String> {
    for project in projects {
        let project_path = project
            .path
            .canonicalize()
            .map_err(|error| format!("failed to resolve {}: {error}", project.path.display()))?;
        let manifest = project_path.join("Cargo.toml");
        let Some(target_dir) = cargo_metadata_target_dir(&manifest)? else {
            continue;
        };
        if target_dir != project_path.join("target") {
            return Ok(Some(target_dir));
        }
    }

    configured_target_dir()
}

pub fn target_dir(path: &Path) -> Result<Option<CargoTargetDir>, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("failed to inspect {}: {error}", path.display())),
    };

    if !metadata.is_dir() {
        return Err(format!(
            "configured Cargo target path is not a directory: {}",
            path.display()
        ));
    }

    let path = path
        .canonicalize()
        .map_err(|error| format!("failed to resolve {}: {error}", path.display()))?;
    let bytes = path_size(&path)?;

    Ok(Some(CargoTargetDir { path, bytes }))
}

pub fn render_projects(projects: &[RustProject]) -> String {
    if projects.is_empty() {
        return "No Rust projects found.".to_string();
    }

    let mut output = String::new();
    for project in projects {
        if project.workspace_members > 0 {
            output.push_str(&format!("{} (workspace)\n", project.name));
            output.push_str(&format!("  members: {}\n", project.workspace_members));
        } else {
            output.push_str(&project.name);
            output.push('\n');
        }
        output.push_str(&format!(
            "  target/ {:>9}\n",
            format_bytes(project.target_bytes)
        ));
        output.push_str(&format!(
            "  source  {:>9}\n\n",
            format_bytes(project.source_bytes)
        ));
    }

    let total: u64 = projects.iter().map(RustProject::reclaimable_bytes).sum();
    output.push_str("Total reclaimable build artifacts:\n");
    output.push_str(&format_bytes(total));
    output
}

pub fn discover_projects(root: &Path) -> Result<Vec<RustProject>, String> {
    let mut manifests = Vec::new();
    collect_manifests(root, &mut manifests)?;
    manifests.sort();

    let mut workspace_roots = Vec::new();
    for manifest in &manifests {
        if is_workspace_manifest(manifest)? {
            let root = manifest
                .parent()
                .ok_or_else(|| format!("manifest has no parent: {}", manifest.display()))?;
            workspace_roots.push(root.to_path_buf());
        }
    }

    manifests
        .iter()
        .filter(|manifest| should_report_manifest(manifest, &workspace_roots))
        .map(|manifest| project_from_manifest(manifest, &manifests, &workspace_roots))
        .collect()
}

pub fn path_size(path: &Path) -> Result<u64, String> {
    dir_size(path)
}

fn project_from_manifest(
    manifest: &Path,
    manifests: &[PathBuf],
    workspace_roots: &[PathBuf],
) -> Result<RustProject, String> {
    let path = manifest
        .parent()
        .ok_or_else(|| format!("manifest has no parent: {}", manifest.display()))?
        .to_path_buf();
    let name = package_name(manifest)?.unwrap_or_else(|| {
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("unknown")
            .to_string()
    });
    let target = path.join("target");
    let is_workspace = workspace_roots.iter().any(|root| root == &path);
    let member_manifests = if is_workspace {
        workspace_member_manifests(&path, manifests, workspace_roots)
    } else {
        Vec::new()
    };

    Ok(RustProject {
        name,
        source_bytes: source_size(&path, &member_manifests)?,
        target_bytes: dir_size(&target)?,
        workspace_members: member_manifests.len(),
        path,
    })
}

fn should_report_manifest(manifest: &Path, workspace_roots: &[PathBuf]) -> bool {
    let Some(path) = manifest.parent() else {
        return false;
    };

    workspace_roots.iter().any(|root| root == path)
        || !workspace_roots
            .iter()
            .any(|root| path != root && path.starts_with(root))
}

fn workspace_member_manifests(
    workspace_root: &Path,
    manifests: &[PathBuf],
    workspace_roots: &[PathBuf],
) -> Vec<PathBuf> {
    manifests
        .iter()
        .filter(|manifest| {
            let Some(path) = manifest.parent() else {
                return false;
            };
            path != workspace_root
                && path.starts_with(workspace_root)
                && !workspace_roots
                    .iter()
                    .any(|root| root != workspace_root && path.starts_with(root))
        })
        .cloned()
        .collect()
}

fn collect_manifests(dir: &Path, manifests: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("failed to read {}: {error}", dir.display())),
    };

    for entry in entries {
        let entry = entry.map_err(|error| format!("failed to read {}: {error}", dir.display()))?;
        let path = entry.path();
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();

        if path.is_file() && file_name == "Cargo.toml" {
            manifests.push(path);
            continue;
        }

        if path.is_dir() && !is_excluded_dir(&file_name) {
            collect_manifests(&path, manifests)?;
        }
    }

    Ok(())
}

fn dir_size(path: &Path) -> Result<u64, String> {
    dir_size_with_filter(path, &|_| true)
}

fn source_size(project_path: &Path, member_manifests: &[PathBuf]) -> Result<u64, String> {
    let mut total = source_size_for_manifest_dir(project_path)?;

    for manifest in member_manifests {
        let Some(member_path) = manifest.parent() else {
            continue;
        };
        total += source_size_for_manifest_dir(member_path)?;
    }

    Ok(total)
}

fn source_size_for_manifest_dir(path: &Path) -> Result<u64, String> {
    let mut total = 0;

    for file in ["Cargo.toml", "Cargo.lock", "build.rs"] {
        total += file_size(&path.join(file))?;
    }

    for dir in ["src", "tests", "benches", "examples"] {
        total += dir_size(&path.join(dir))?;
    }

    Ok(total)
}

fn file_size(path: &Path) -> Result<u64, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(format!("failed to inspect {}: {error}", path.display())),
    };

    if metadata.is_file() {
        Ok(metadata.len())
    } else {
        Ok(0)
    }
}

fn dir_size_with_filter(path: &Path, include_dir: &dyn Fn(&str) -> bool) -> Result<u64, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(format!("failed to inspect {}: {error}", path.display())),
    };

    if metadata.is_file() {
        return Ok(metadata.len());
    }

    if !metadata.is_dir() {
        return Ok(0);
    }

    let mut total = 0;
    let entries = fs::read_dir(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("failed to read {}: {error}", path.display()))?;
        let child = entry.path();
        let metadata = fs::symlink_metadata(&child)
            .map_err(|error| format!("failed to inspect {}: {error}", child.display()))?;

        if metadata.is_file() {
            total += metadata.len();
        } else if metadata.is_dir() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if include_dir(&name) && !is_excluded_dir(&name) {
                total += dir_size_with_filter(&child, include_dir)?;
            }
        }
    }

    Ok(total)
}

fn package_name(manifest: &Path) -> Result<Option<String>, String> {
    let contents = fs::read_to_string(manifest)
        .map_err(|error| format!("failed to read {}: {error}", manifest.display()))?;
    let mut in_package = false;

    for line in contents.lines() {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            in_package = line == "[package]";
            continue;
        }

        if in_package && let Some(value) = line.strip_prefix("name") {
            let Some((_, value)) = value.split_once('=') else {
                continue;
            };
            let value = value.trim().trim_matches('"');
            if !value.is_empty() {
                return Ok(Some(value.to_string()));
            }
        }
    }

    Ok(None)
}

fn cargo_metadata_target_dir(manifest: &Path) -> Result<Option<PathBuf>, String> {
    let output = Command::new("cargo")
        .arg("metadata")
        .arg("--format-version")
        .arg("1")
        .arg("--no-deps")
        .arg("--manifest-path")
        .arg(manifest)
        .output()
        .map_err(|error| {
            format!(
                "failed to run cargo metadata for {}: {error}",
                manifest.display()
            )
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "cargo metadata failed for {}: {}",
            manifest.display(),
            stderr.trim()
        ));
    }

    let stdout = String::from_utf8(output.stdout)
        .map_err(|error| format!("cargo metadata output was not UTF-8: {error}"))?;
    Ok(extract_json_string(&stdout, "target_directory").map(PathBuf::from))
}

fn configured_target_dir() -> Result<Option<PathBuf>, String> {
    let Some(value) = std::env::var_os("CARGO_TARGET_DIR") else {
        return Ok(None);
    };
    let path = PathBuf::from(value);
    if path.is_absolute() {
        Ok(Some(path))
    } else {
        std::env::current_dir()
            .map(|cwd| Some(cwd.join(path)))
            .map_err(|error| format!("failed to determine current directory: {error}"))
    }
}

fn extract_json_string(input: &str, key: &str) -> Option<String> {
    let marker = format!("\"{key}\"");
    let start = input.find(&marker)? + marker.len();
    let after_key = input[start..].trim_start();
    let after_colon = after_key.strip_prefix(':')?.trim_start();
    let raw = after_colon.strip_prefix('"')?;

    let mut value = String::new();
    let mut chars = raw.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '"' => return Some(value),
            '\\' => match chars.next()? {
                '"' => value.push('"'),
                '\\' => value.push('\\'),
                '/' => value.push('/'),
                'b' => value.push('\u{0008}'),
                'f' => value.push('\u{000c}'),
                'n' => value.push('\n'),
                'r' => value.push('\r'),
                't' => value.push('\t'),
                'u' => return None,
                escaped => value.push(escaped),
            },
            ch => value.push(ch),
        }
    }

    None
}

fn is_workspace_manifest(manifest: &Path) -> Result<bool, String> {
    let contents = fs::read_to_string(manifest)
        .map_err(|error| format!("failed to read {}: {error}", manifest.display()))?;

    Ok(contents.lines().any(|line| line.trim() == "[workspace]"))
}

fn is_excluded_dir(name: &str) -> bool {
    matches!(
        name,
        ".git"
            | "node_modules"
            | "dist"
            | "build"
            | "tmp"
            | "caches"
            | "logs"
            | "snap"
            | ".venv"
            | "venv"
            | "env"
            | ".env"
    )
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{discover_projects, render_projects};

    #[test]
    fn discovers_projects_and_separates_target_size() {
        let temp = test_dir("disk-maint-rust-discovery");
        let project = temp.join("example");
        fs::create_dir_all(project.join("src")).unwrap();
        fs::create_dir_all(project.join("target/debug")).unwrap();
        fs::write(
            project.join("Cargo.toml"),
            "[package]\nname = \"example\"\n",
        )
        .unwrap();
        fs::write(project.join("src/main.rs"), "fn main() {}\n").unwrap();
        fs::write(project.join("target/debug/app"), "artifact").unwrap();

        let projects = discover_projects(&temp).unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].name, "example");
        assert_eq!(projects[0].source_bytes, 40);
        assert_eq!(projects[0].target_bytes, 8);
        assert_eq!(projects[0].workspace_members, 0);

        let report = render_projects(&projects);
        assert!(report.contains("example"));
        assert!(report.contains("Total reclaimable build artifacts"));

        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn source_size_ignores_data_and_results() {
        let temp = test_dir("disk-maint-rust-source-size");
        let project = temp.join("example");
        fs::create_dir_all(project.join("src")).unwrap();
        fs::create_dir_all(project.join("data")).unwrap();
        fs::create_dir_all(project.join("results")).unwrap();
        fs::create_dir_all(project.join("target/debug")).unwrap();
        fs::write(
            project.join("Cargo.toml"),
            "[package]\nname = \"example\"\n",
        )
        .unwrap();
        fs::write(project.join("src/main.rs"), "fn main() {}\n").unwrap();
        fs::write(project.join("data/input.bin"), "downloaded dataset").unwrap();
        fs::write(project.join("results/output.bin"), "generated result").unwrap();
        fs::write(project.join("target/debug/app"), "artifact").unwrap();

        let projects = discover_projects(&temp).unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].source_bytes, 40);

        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn reports_cargo_workspace_as_single_project() {
        let temp = test_dir("disk-maint-rust-workspace");
        let workspace = temp.join("workspace");
        let crate_a = workspace.join("crates/a");
        let crate_b = workspace.join("crates/b");
        fs::create_dir_all(crate_a.join("src")).unwrap();
        fs::create_dir_all(crate_b.join("src")).unwrap();
        fs::create_dir_all(workspace.join("target/debug")).unwrap();
        fs::write(
            workspace.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/a\", \"crates/b\"]\n",
        )
        .unwrap();
        fs::write(
            crate_a.join("Cargo.toml"),
            "[package]\nname = \"crate-a\"\n",
        )
        .unwrap();
        fs::write(crate_a.join("src/lib.rs"), "pub fn a() {}\n").unwrap();
        fs::write(
            crate_b.join("Cargo.toml"),
            "[package]\nname = \"crate-b\"\n",
        )
        .unwrap();
        fs::write(crate_b.join("src/lib.rs"), "pub fn b() {}\n").unwrap();
        fs::write(workspace.join("target/debug/app"), "artifact").unwrap();

        let projects = discover_projects(&temp).unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].name, "workspace");
        assert_eq!(projects[0].workspace_members, 2);
        assert_eq!(projects[0].target_bytes, 8);

        let report = render_projects(&projects);
        assert!(report.contains("workspace (workspace)"));
        assert!(report.contains("  members: 2"));
        assert!(!report.contains("crate-a\n"));
        assert!(!report.contains("crate-b\n"));

        fs::remove_dir_all(temp).unwrap();
    }

    fn test_dir(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }
}
