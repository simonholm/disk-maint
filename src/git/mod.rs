use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryStatus {
    pub name: String,
    pub path: PathBuf,
    pub entries: Vec<String>,
}

pub fn report_status(root: &Path) -> Result<String, String> {
    let mut repositories = discover_repositories(root)?;
    repositories.sort();

    let mut dirty = Vec::new();
    for repository in repositories {
        let entries = status_entries(&repository)?;
        if !entries.is_empty() {
            dirty.push(RepositoryStatus {
                name: repository_name(&repository),
                path: repository,
                entries,
            });
        }
    }

    Ok(render_statuses(&dirty))
}

pub fn discover_repositories(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut repositories = Vec::new();
    collect_repositories(root, &mut repositories)?;
    repositories.sort();
    Ok(repositories)
}

pub fn render_statuses(repositories: &[RepositoryStatus]) -> String {
    if repositories.is_empty() {
        return "All repositories are clean.".to_string();
    }

    let mut output = String::new();
    for (index, repository) in repositories.iter().enumerate() {
        if index > 0 {
            output.push('\n');
        }
        output.push_str(&format!("== {} ==\n", repository.name));
        output.push_str(&repository.entries.join("\n"));
        output.push('\n');
    }
    output.trim_end().to_string()
}

fn collect_repositories(dir: &Path, repositories: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("failed to read {}: {error}", dir.display())),
    };

    for entry in entries {
        let entry =
            entry.map_err(|error| format!("failed to read entry in {}: {error}", dir.display()))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|error| format!("failed to inspect {}: {error}", path.display()))?;
        if !file_type.is_dir() {
            continue;
        }

        let name = entry.file_name().to_string_lossy().to_string();
        if name == ".git" {
            if let Some(repository) = path.parent()
                && is_git_work_tree(repository)?
            {
                repositories.push(repository.to_path_buf());
            }
            continue;
        }

        if !is_excluded_dir(&name) {
            collect_repositories(&path, repositories)?;
        }
    }

    Ok(())
}

fn status_entries(repository: &Path) -> Result<Vec<String>, String> {
    let mut tracked = BTreeMap::new();

    for (index, output) in [
        (
            0,
            run_git(
                repository,
                &["diff", "--name-status", "-z", "--cached", "--"],
            )?,
        ),
        (
            1,
            run_git(repository, &["diff", "--name-status", "-z", "--"])?,
        ),
    ] {
        for (status, path) in parse_name_status_records(&output) {
            let entry = tracked.entry(path).or_insert([' ', ' ']);
            entry[index] = status;
        }
    }

    let mut entries: Vec<String> = tracked
        .into_iter()
        .map(|(path, status)| format!("{}{} {path}", status[0], status[1]))
        .collect();

    let untracked = run_git(
        repository,
        &[
            "ls-files",
            "--others",
            "--exclude-standard",
            "--directory",
            "--no-empty-directory",
            "-z",
        ],
    )?;
    entries.extend(
        nul_records(&untracked)
            .into_iter()
            .filter(|path| !path.is_empty())
            .map(|path| format!("?? {path}")),
    );
    entries.sort();
    Ok(entries)
}

fn parse_name_status_records(output: &[u8]) -> Vec<(char, String)> {
    let records = nul_records(output);
    let mut entries = Vec::new();
    let mut index = 0;

    while index < records.len() {
        let status = records[index].chars().next();
        index += 1;

        let Some(status) = status else {
            continue;
        };

        if matches!(status, 'R' | 'C') {
            index += 1;
        }

        let Some(path) = records.get(index) else {
            break;
        };
        index += 1;

        if !path.is_empty() {
            entries.push((status, path.clone()));
        }
    }

    entries
}

fn nul_records(output: &[u8]) -> Vec<String> {
    output
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
        .map(|record| String::from_utf8_lossy(record).to_string())
        .collect()
}

fn run_git(repository: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(repository)
        .output()
        .map_err(|error| git_execution_error(error))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "git failed in {}: {}",
            repository.display(),
            stderr.trim()
        ));
    }

    Ok(output.stdout)
}

fn is_git_work_tree(repository: &Path) -> Result<bool, String> {
    let output = Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(repository)
        .output()
        .map_err(git_execution_error)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "repository cannot be inspected at {}: git rev-parse failed: {}",
            repository.display(),
            stderr.trim()
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim() == "true")
}

fn git_execution_error(error: std::io::Error) -> String {
    if error.kind() == std::io::ErrorKind::NotFound {
        return "git executable is unavailable: failed to run `git`".to_string();
    }

    format!("failed to run git: {error}")
}

fn repository_name(repository: &Path) -> String {
    repository
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| repository.display().to_string())
}

fn is_excluded_dir(name: &str) -> bool {
    matches!(
        name,
        "node_modules"
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
    use std::process::Command;

    use super::{RepositoryStatus, discover_repositories, render_statuses, status_entries};

    #[test]
    fn discovers_git_directories() {
        let temp = test_dir("disk-maint-git-discovery");
        let repo = temp.join("repo");
        let ignored = temp.join("node_modules/package");
        fs::create_dir_all(&repo).unwrap();
        git_init(&repo);
        fs::create_dir_all(ignored.join(".git")).unwrap();

        let repositories = discover_repositories(&temp).unwrap();
        assert_eq!(repositories, vec![repo]);

        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn reports_uninspectable_git_directories() {
        let temp = test_dir("disk-maint-git-invalid");
        let broken = temp.join("broken");
        fs::create_dir_all(broken.join(".git")).unwrap();

        let error = discover_repositories(&temp).unwrap_err();
        assert!(error.contains("repository cannot be inspected"));
        assert!(error.contains("git rev-parse failed"));

        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn renders_clean_message() {
        assert_eq!(render_statuses(&[]), "All repositories are clean.");
    }

    #[test]
    fn renders_dirty_repositories_with_blank_lines() {
        let output = render_statuses(&[
            RepositoryStatus {
                name: "alpha".to_string(),
                path: "alpha".into(),
                entries: vec![" M README.md".to_string()],
            },
            RepositoryStatus {
                name: "beta".to_string(),
                path: "beta".into(),
                entries: vec!["?? Cargo.lock".to_string()],
            },
        ]);

        assert_eq!(
            output,
            "== alpha ==\n M README.md\n\n== beta ==\n?? Cargo.lock"
        );
    }

    #[test]
    fn reports_modified_and_untracked_paths() {
        let temp = test_dir("disk-maint-git-status");
        git_init(&temp);
        fs::write(temp.join("README.md"), "before\n").unwrap();
        git(&temp, &["add", "README.md"]);
        git(
            &temp,
            &[
                "-c",
                "user.name=Disk Maint Test",
                "-c",
                "user.email=disk-maint@example.invalid",
                "commit",
                "--quiet",
                "-m",
                "initial",
            ],
        );

        fs::write(temp.join("README.md"), "after\n").unwrap();
        fs::write(temp.join("Cargo.lock"), "").unwrap();

        let entries = status_entries(&temp).unwrap();
        assert_eq!(
            entries,
            vec![" M README.md".to_string(), "?? Cargo.lock".to_string()]
        );

        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn parses_nul_delimited_paths() {
        let temp = test_dir("disk-maint-git-nul-paths");
        git_init(&temp);
        let tracked = "tracked\tfile.txt";
        let untracked = "untracked\nfile.txt";

        fs::write(temp.join(tracked), "before\n").unwrap();
        git(&temp, &["add", tracked]);
        git(
            &temp,
            &[
                "-c",
                "user.name=Disk Maint Test",
                "-c",
                "user.email=disk-maint@example.invalid",
                "commit",
                "--quiet",
                "-m",
                "initial",
            ],
        );

        fs::write(temp.join(tracked), "after\n").unwrap();
        fs::write(temp.join(untracked), "").unwrap();

        let entries = status_entries(&temp).unwrap();
        assert_eq!(
            entries,
            vec![format!(" M {tracked}"), format!("?? {untracked}"),]
        );

        fs::remove_dir_all(temp).unwrap();
    }

    fn test_dir(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn git_init(path: &std::path::Path) {
        git(path, &["init", "--quiet"]);
    }

    fn git(path: &std::path::Path, args: &[&str]) {
        let status = Command::new("git")
            .args(args)
            .current_dir(path)
            .status()
            .unwrap();
        assert!(status.success());
    }
}
