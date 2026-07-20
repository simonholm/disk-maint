use std::path::Path;

use crate::format_bytes;

const LABEL_WIDTH: usize = 22;
const VALUE_WIDTH: usize = 9;
const DESCRIPTION_WIDTH: usize = 72;
const DESCRIPTION_INDENT: &str = "    ";

pub fn report(root: &Path) -> Result<String, String> {
    let build_artifacts = crate::rust::discover_build_artifacts(root)?;
    let cargo_registry = crate::home_dir()
        .map(|home| home.join(".cargo").join("registry"))
        .map(|path| size_or_zero(&path))
        .transpose()?
        .unwrap_or(0);
    let cargo_git = crate::home_dir()
        .map(|home| home.join(".cargo").join("git"))
        .map(|path| size_or_zero(&path))
        .transpose()?
        .unwrap_or(0);
    let rustup_toolchains = crate::home_dir()
        .map(|home| home.join(".rustup").join("toolchains"))
        .map(|path| size_or_zero(&path))
        .transpose()?
        .unwrap_or(0);
    let project_count = build_artifacts.projects.len();

    let mut output = String::new();
    output.push_str("Rust maintenance report\n\n");
    push_cargo_build_artifacts(&mut output, &build_artifacts);
    push_described_metric(
        &mut output,
        "Cargo registry cache",
        &format_bytes(cargo_registry),
        "shared package cache; removing may require re-downloads",
        &[],
    );
    push_described_metric(
        &mut output,
        "Cargo git cache",
        &format_bytes(cargo_git),
        "shared git dependency cache; removing may require re-fetching",
        &[],
    );
    push_described_metric(
        &mut output,
        "Rustup toolchains",
        &format_bytes(rustup_toolchains),
        "remove old toolchains with",
        &["rustup toolchain uninstall <toolchain>"],
    );
    push_metric(
        &mut output,
        "Rust projects scanned",
        &project_count.to_string(),
    );
    output.push_str("\nNo changes made.");
    Ok(output)
}

fn push_cargo_build_artifacts(output: &mut String, artifacts: &crate::rust::CargoBuildArtifacts) {
    let project_local_bytes: u64 = artifacts
        .projects
        .iter()
        .map(|project| project.target_bytes)
        .sum();
    let project_local_count = artifacts
        .projects
        .iter()
        .filter(|project| project.target_bytes > 0)
        .count();
    let shared_target_bytes = artifacts
        .shared_target
        .as_ref()
        .map(|target| target.bytes)
        .unwrap_or(0);

    output.push_str("Cargo build artifacts\n\n");

    push_metric(output, "Shared target", &format_bytes(shared_target_bytes));
    if shared_target_bytes == 0 {
        push_wrapped_description(output, "no shared build artifacts found");
    } else {
        push_wrapped_description(output, "shared build artifacts");
        push_wrapped_description(output, "safe to remove with");
        output.push_str(DESCRIPTION_INDENT);
        output.push_str("`disk-maint clean shared`\n");
    }

    output.push('\n');

    push_metric(
        output,
        "Local targets",
        &format_local_targets_value(project_local_bytes, project_local_count),
    );
    if project_local_bytes == 0 {
        push_wrapped_description(output, "no local build artifacts found");
    } else {
        push_wrapped_description(output, "repository/workspace build artifacts");
        push_wrapped_description(output, "safe to remove with");
        output.push_str(DESCRIPTION_INDENT);
        output.push_str("`cargo clean`\n");
        output.push_str(DESCRIPTION_INDENT);
        output.push_str("`disk-maint clean target`\n");
    }

    output.push('\n');
}

fn format_local_targets_value(bytes: u64, repositories: usize) -> String {
    let noun = if repositories == 1 {
        "repository"
    } else {
        "repositories"
    };
    format!("{} ({repositories} {noun})", format_bytes(bytes))
}

fn push_described_metric(
    output: &mut String,
    label: &str,
    value: &str,
    description: &str,
    commands: &[&str],
) {
    push_metric(output, label, value);
    push_wrapped_description(output, description);
    for command in commands {
        output.push_str(DESCRIPTION_INDENT);
        output.push_str("`");
        output.push_str(command);
        output.push_str("`\n");
    }
    output.push('\n');
}

fn push_metric(output: &mut String, label: &str, value: &str) {
    output.push_str(&format!("{label:<LABEL_WIDTH$} {value:>VALUE_WIDTH$}\n"));
}

fn push_wrapped_description(output: &mut String, description: &str) {
    let mut line = String::new();
    for word in description.split_whitespace() {
        if line.is_empty() {
            line.push_str(word);
        } else if line.len() + 1 + word.len() <= DESCRIPTION_WIDTH {
            line.push(' ');
            line.push_str(word);
        } else {
            output.push_str(DESCRIPTION_INDENT);
            output.push_str(&line);
            output.push('\n');
            line.clear();
            line.push_str(word);
        }
    }

    if !line.is_empty() {
        output.push_str(DESCRIPTION_INDENT);
        output.push_str(&line);
        output.push('\n');
    }
}

fn size_or_zero(path: &Path) -> Result<u64, String> {
    crate::rust::path_size(path)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::rust::{CargoBuildArtifacts, CargoTargetDir, RustProject};

    use super::{push_cargo_build_artifacts, push_metric, push_wrapped_description};

    #[test]
    fn formats_scan_metrics_as_separated_blocks() {
        let mut output = String::new();
        push_cargo_build_artifacts(&mut output, &build_artifacts(1_234_567_890, Some(4096)));
        push_metric(&mut output, "Rust projects scanned", "12");

        assert_eq!(
            output,
            "Cargo build artifacts\n\nShared target               4.0K\n    shared build artifacts\n    safe to remove with\n    `disk-maint clean shared`\n\nLocal targets          1.1G (1 repository)\n    repository/workspace build artifacts\n    safe to remove with\n    `cargo clean`\n    `disk-maint clean target`\n\nRust projects scanned         12\n"
        );
    }

    #[test]
    fn formats_local_only_cargo_build_artifacts() {
        let mut output = String::new();
        push_cargo_build_artifacts(&mut output, &build_artifacts(77_000_000, None));
        assert_eq!(
            output,
            "Cargo build artifacts\n\nShared target                 0B\n    no shared build artifacts found\n\nLocal targets          73M (1 repository)\n    repository/workspace build artifacts\n    safe to remove with\n    `cargo clean`\n    `disk-maint clean target`\n\n"
        );
    }

    #[test]
    fn formats_shared_only_cargo_build_artifacts() {
        let mut output = String::new();
        push_cargo_build_artifacts(&mut output, &build_artifacts(0, Some(230_000_000)));
        assert_eq!(
            output,
            "Cargo build artifacts\n\nShared target               219M\n    shared build artifacts\n    safe to remove with\n    `disk-maint clean shared`\n\nLocal targets          0B (0 repositories)\n    no local build artifacts found\n\n"
        );
    }

    #[test]
    fn formats_zero_cargo_build_artifacts() {
        let mut output = String::new();
        push_cargo_build_artifacts(&mut output, &build_artifacts(0, None));
        assert_eq!(
            output,
            "Cargo build artifacts\n\nShared target                 0B\n    no shared build artifacts found\n\nLocal targets          0B (0 repositories)\n    no local build artifacts found\n\n"
        );
    }

    #[test]
    fn wraps_descriptions_on_word_boundaries() {
        let mut output = String::new();
        push_wrapped_description(
            &mut output,
            "this description is intentionally long enough to wrap without creating an awkward hanging paragraph",
        );

        assert_eq!(
            output,
            "    this description is intentionally long enough to wrap without creating\n    an awkward hanging paragraph\n"
        );
    }

    fn build_artifacts(project_bytes: u64, shared_bytes: Option<u64>) -> CargoBuildArtifacts {
        let projects = if project_bytes > 0 {
            vec![RustProject {
                name: "example".to_string(),
                path: PathBuf::from("/tmp/example"),
                source_bytes: 0,
                target_bytes: project_bytes,
                workspace_members: 0,
            }]
        } else {
            Vec::new()
        };
        let shared_target = shared_bytes.map(|bytes| CargoTargetDir {
            path: PathBuf::from("/tmp/shared-target"),
            bytes,
        });

        CargoBuildArtifacts {
            projects,
            shared_target,
        }
    }
}
