use std::fs;
use std::path::{Path, PathBuf};

use crate::format_bytes;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustProject {
    pub name: String,
    pub path: PathBuf,
    pub source_bytes: u64,
    pub target_bytes: u64,
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

pub fn render_projects(projects: &[RustProject]) -> String {
    if projects.is_empty() {
        return "No Rust projects found.".to_string();
    }

    let mut output = String::new();
    for project in projects {
        output.push_str(&project.name);
        output.push('\n');
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

    manifests
        .into_iter()
        .map(|manifest| project_from_manifest(&manifest))
        .collect()
}

pub fn path_size(path: &Path) -> Result<u64, String> {
    dir_size(path)
}

fn project_from_manifest(manifest: &Path) -> Result<RustProject, String> {
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

    Ok(RustProject {
        name,
        source_bytes: dir_size_excluding(&path, &["target"])?,
        target_bytes: dir_size(&target)?,
        path,
    })
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

fn dir_size_excluding(path: &Path, excluded: &[&str]) -> Result<u64, String> {
    dir_size_with_filter(path, &|name| !excluded.contains(&name))
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

        let report = render_projects(&projects);
        assert!(report.contains("example"));
        assert!(report.contains("Total reclaimable build artifacts"));

        fs::remove_dir_all(temp).unwrap();
    }

    fn test_dir(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }
}
