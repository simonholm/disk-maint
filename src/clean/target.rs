use std::fs;
use std::path::{Path, PathBuf};

use crate::format_bytes;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanPlan {
    pub items: Vec<TargetCleanup>,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetCleanup {
    pub project_name: String,
    pub project_path: PathBuf,
    pub target_path: PathBuf,
    pub bytes: u64,
}

pub fn plan(root: &Path) -> Result<CleanPlan, String> {
    let projects = crate::rust::discover_projects(root)?;
    let mut items = Vec::new();

    for project in projects {
        if project.target_bytes == 0 {
            continue;
        }
        items.push(TargetCleanup {
            project_name: project.name,
            target_path: project.path.join("target"),
            project_path: project.path,
            bytes: project.target_bytes,
        });
    }

    let total_bytes = items.iter().map(|item| item.bytes).sum();
    Ok(CleanPlan { items, total_bytes })
}

pub fn render_plan(plan: &CleanPlan) -> String {
    if plan.items.is_empty() {
        return "No target/ directories found. No files will be deleted.".to_string();
    }

    let mut output = String::new();
    output.push_str("The following Rust build artifact directories will be removed:\n\n");

    for item in &plan.items {
        output.push_str(&format!(
            "{}\n  target/ {:>9}  {}\n",
            item.project_name,
            format_bytes(item.bytes),
            item.target_path.display()
        ));
    }

    output.push_str(&format!(
        "\nEstimated reclaimable space: {}\n\n",
        format_bytes(plan.total_bytes)
    ));
    output.push_str(
        "Safety: target/ contains Cargo build artifacts. Removing it is normally safe because Cargo rebuilds it, but the next build may take longer and any manually placed files under target/ will be lost.",
    );
    output
}

pub fn execute(plan: &CleanPlan) -> Result<(), String> {
    for item in &plan.items {
        if item.target_path.file_name().and_then(|name| name.to_str()) != Some("target") {
            return Err(format!(
                "refusing to delete non-target path: {}",
                item.target_path.display()
            ));
        }

        fs::remove_dir_all(&item.target_path)
            .map_err(|error| format!("failed to delete {}: {error}", item.target_path.display()))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{plan, render_plan};

    #[test]
    fn plans_only_existing_target_directories() {
        let temp = test_dir("disk-maint-clean-target");
        let project = temp.join("example");
        fs::create_dir_all(project.join("src")).unwrap();
        fs::create_dir_all(project.join("target")).unwrap();
        fs::write(
            project.join("Cargo.toml"),
            "[package]\nname = \"example\"\n",
        )
        .unwrap();
        fs::write(project.join("target/app"), "artifact").unwrap();

        let plan = plan(&temp).unwrap();
        assert_eq!(plan.items.len(), 1);
        assert_eq!(plan.items[0].project_name, "example");
        assert_eq!(plan.total_bytes, 8);

        let rendered = render_plan(&plan);
        assert!(rendered.contains("Estimated reclaimable space"));
        assert!(rendered.contains("Safety: target/ contains Cargo build artifacts"));

        fs::remove_dir_all(temp).unwrap();
    }

    fn test_dir(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }
}
