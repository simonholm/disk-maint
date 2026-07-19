use std::fs;
use std::path::{Path, PathBuf};

use crate::format_bytes;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanPlan {
    pub target_path: Option<PathBuf>,
    pub total_bytes: u64,
}

pub fn plan(root: &Path) -> Result<CleanPlan, String> {
    let Some(path) = crate::rust::discover_shared_target_dir(root)? else {
        return Ok(CleanPlan {
            target_path: None,
            total_bytes: 0,
        });
    };

    plan_path(&path)
}

pub fn plan_path(path: &Path) -> Result<CleanPlan, String> {
    let Some(target) = crate::rust::target_dir(path)? else {
        return Ok(CleanPlan {
            target_path: None,
            total_bytes: 0,
        });
    };

    Ok(CleanPlan {
        target_path: Some(target.path),
        total_bytes: target.bytes,
    })
}

pub fn render_plan(plan: &CleanPlan) -> String {
    let Some(target_path) = &plan.target_path else {
        return "No shared Cargo target directory found. No files will be deleted.".to_string();
    };

    format!(
        "The shared Cargo target directory will be removed:\n\n  shared target {:>9}  {}\n\nEstimated reclaimable space: {}\n\nSafety: this is Cargo's shared build cache. Removing it is normally safe because Cargo rebuilds it, but all projects that use this shared target directory will rebuild afterwards.",
        format_bytes(plan.total_bytes),
        target_path.display(),
        format_bytes(plan.total_bytes)
    )
}

pub fn execute(plan: &CleanPlan) -> Result<(), String> {
    let Some(target_path) = &plan.target_path else {
        return Ok(());
    };

    if target_path.parent().is_none() {
        return Err(format!(
            "refusing to delete filesystem root: {}",
            target_path.display()
        ));
    }

    fs::remove_dir_all(target_path)
        .map_err(|error| format!("failed to delete {}: {error}", target_path.display()))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{execute, plan_path, render_plan};

    #[test]
    fn plans_existing_shared_target_directory() {
        let temp = test_dir("disk-maint-clean-shared");
        let shared = temp.join("shared-target");
        fs::create_dir_all(shared.join("debug")).unwrap();
        fs::write(shared.join("debug/app"), "artifact").unwrap();

        let plan = plan_path(&shared).unwrap();
        assert_eq!(plan.target_path, Some(shared.canonicalize().unwrap()));
        assert_eq!(plan.total_bytes, 8);

        let rendered = render_plan(&plan);
        assert!(rendered.contains("shared target"));
        assert!(rendered.contains("Cargo's shared build cache"));
        assert!(rendered.contains(&shared.canonicalize().unwrap().display().to_string()));

        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn plans_absent_shared_target_directory() {
        let temp = test_dir("disk-maint-clean-shared-absent");
        let plan = plan_path(&temp.join("missing")).unwrap();

        assert_eq!(plan.target_path, None);
        assert_eq!(
            render_plan(&plan),
            "No shared Cargo target directory found. No files will be deleted."
        );

        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn deletes_existing_shared_target_directory() {
        let temp = test_dir("disk-maint-clean-shared-delete");
        let shared = temp.join("shared-target");
        fs::create_dir_all(&shared).unwrap();
        fs::write(shared.join("artifact"), "artifact").unwrap();

        let plan = plan_path(&shared).unwrap();
        execute(&plan).unwrap();

        assert!(!shared.exists());

        fs::remove_dir_all(temp).unwrap();
    }

    fn test_dir(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }
}
