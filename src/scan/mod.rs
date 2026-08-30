use std::path::Path;

use crate::format_bytes;

const LABEL_WIDTH: usize = 22;
const VALUE_WIDTH: usize = 9;
const DESCRIPTION_WIDTH: usize = 72;
const DESCRIPTION_INDENT: &str = "    ";

#[derive(Debug, Clone, PartialEq, Eq)]
struct CargoRegistryData {
    cache_bytes: u64,
    src_bytes: u64,
    index_bytes: u64,
}

pub fn report(root: &Path) -> Result<String, String> {
    let build_artifacts = crate::rust::discover_build_artifacts(root)?;
    let cargo_registry = crate::home_dir()
        .map(|home| home.join(".cargo").join("registry"))
        .map(|path| cargo_registry_data(&path))
        .transpose()?
        .unwrap_or_else(empty_cargo_registry_data);
    let cargo_git = crate::home_dir()
        .map(|home| home.join(".cargo").join("git"))
        .map(|path| size_or_zero(&path))
        .transpose()?
        .unwrap_or(0);
    let rustup_toolchains_dir =
        crate::home_dir().map(|home| home.join(".rustup").join("toolchains"));
    let rustup_toolchain_details = crate::rust::discover_rustup_toolchains()?;
    let rustup_toolchains = match (&rustup_toolchains_dir, rustup_toolchain_details.as_ref()) {
        (Some(path), Some(toolchains)) => rustup_toolchain_reclaimable_bytes(path, toolchains)?,
        (Some(path), None) => size_or_zero(path)?,
        (None, _) => 0,
    };
    let project_count = build_artifacts.projects.len();

    let mut output = String::new();
    output.push_str("Rust maintenance report\n\n");
    push_cargo_build_artifacts(&mut output, &build_artifacts);
    push_cargo_registry_data(&mut output, &cargo_registry);
    push_described_metric_if_nonzero(
        &mut output,
        "Cargo git cache",
        cargo_git,
        "shared git dependency cache; removing may require re-fetching",
        &[],
    );
    push_rustup_toolchains(
        &mut output,
        rustup_toolchains,
        rustup_toolchain_details.as_ref(),
    );
    push_metric(
        &mut output,
        "Rust projects scanned",
        &project_count.to_string(),
    );
    output.push_str("\nNo changes made.");
    Ok(output)
}

fn cargo_registry_data(path: &Path) -> Result<CargoRegistryData, String> {
    Ok(CargoRegistryData {
        cache_bytes: size_or_zero(&path.join("cache"))?,
        src_bytes: size_or_zero(&path.join("src"))?,
        index_bytes: size_or_zero(&path.join("index"))?,
    })
}

fn empty_cargo_registry_data() -> CargoRegistryData {
    CargoRegistryData {
        cache_bytes: 0,
        src_bytes: 0,
        index_bytes: 0,
    }
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

    if shared_target_bytes > 0 {
        push_metric(output, "Shared target", &format_bytes(shared_target_bytes));
        push_wrapped_description(output, "shared build artifacts");
        push_wrapped_description(output, "safe to remove with");
        output.push_str(DESCRIPTION_INDENT);
        output.push_str("`disk-maint clean shared`\n");

        output.push('\n');
    }

    if project_local_count > 0 {
        push_metric(
            output,
            "Local targets",
            &format_local_targets_value(project_local_bytes, project_local_count),
        );
        push_wrapped_description(output, "repository/workspace build artifacts");
        push_wrapped_description(output, "safe to remove with");
        output.push_str(DESCRIPTION_INDENT);
        output.push_str("`cargo clean`\n");
        output.push_str(DESCRIPTION_INDENT);
        output.push_str("`disk-maint clean target`\n");

        output.push('\n');
    }
}

fn push_cargo_registry_data(output: &mut String, registry: &CargoRegistryData) {
    push_described_metric_if_nonzero(
        output,
        "Cargo crate archives",
        registry.cache_bytes,
        "downloaded .crate archives used as Cargo's package cache",
        &[],
    );
    push_described_metric_if_nonzero(
        output,
        "Cargo crate sources",
        registry.src_bytes,
        "unpacked crate source trees from downloaded registry packages",
        &[],
    );
    push_described_metric_if_nonzero(
        output,
        "Cargo registry index",
        registry.index_bytes,
        "registry index and package metadata used for dependency resolution",
        &[],
    );
}

fn format_local_targets_value(bytes: u64, repositories: usize) -> String {
    let noun = if repositories == 1 {
        "repository"
    } else {
        "repositories"
    };
    format!("{} ({repositories} {noun})", format_bytes(bytes))
}

fn push_rustup_toolchains(
    output: &mut String,
    bytes: u64,
    toolchains: Option<&crate::rust::RustupToolchains>,
) {
    let Some(toolchains) = toolchains else {
        push_metric(output, "Rustup toolchains", &format_bytes(bytes));
        push_wrapped_description(output, "remove old toolchains with");
        output.push_str(DESCRIPTION_INDENT);
        output.push_str("`rustup toolchain uninstall <toolchain>`\n");
        output.push('\n');
        return;
    };

    let removable = toolchains.removable();
    if removable.is_empty() {
        return;
    }

    push_metric(output, "Rustup toolchains", &format_bytes(bytes));

    if toolchains.active == toolchains.default {
        push_wrapped_description(output, &format!("active/default: {}", toolchains.default));
    } else {
        push_wrapped_description(output, &format!("active: {}", toolchains.active));
        push_wrapped_description(output, &format!("default: {}", toolchains.default));
    }

    let additional = toolchains.additional();
    if !additional.is_empty() {
        push_wrapped_description(output, &format!("additional: {}", additional.join(", ")));
    }

    push_wrapped_description(output, "removable:");
    for toolchain in removable {
        output.push_str(DESCRIPTION_INDENT);
        output.push_str("  rustup toolchain uninstall ");
        output.push_str(toolchain);
        output.push('\n');
    }
    output.push('\n');
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

fn push_described_metric_if_nonzero(
    output: &mut String,
    label: &str,
    bytes: u64,
    description: &str,
    commands: &[&str],
) {
    if bytes > 0 {
        push_described_metric(output, label, &format_bytes(bytes), description, commands);
    }
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

fn rustup_toolchain_reclaimable_bytes(
    toolchains_dir: &Path,
    toolchains: &crate::rust::RustupToolchains,
) -> Result<u64, String> {
    toolchains
        .removable()
        .into_iter()
        .map(|toolchain| size_or_zero(&toolchains_dir.join(toolchain)))
        .sum()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use crate::rust::{CargoBuildArtifacts, CargoTargetDir, RustProject, RustupToolchains};

    use super::{
        CargoRegistryData, cargo_registry_data, push_cargo_build_artifacts,
        push_cargo_registry_data, push_described_metric_if_nonzero, push_metric,
        push_rustup_toolchains, push_wrapped_description,
    };

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
            "Cargo build artifacts\n\nLocal targets          73M (1 repository)\n    repository/workspace build artifacts\n    safe to remove with\n    `cargo clean`\n    `disk-maint clean target`\n\n"
        );
    }

    #[test]
    fn formats_shared_only_cargo_build_artifacts() {
        let mut output = String::new();
        push_cargo_build_artifacts(&mut output, &build_artifacts(0, Some(230_000_000)));
        assert_eq!(
            output,
            "Cargo build artifacts\n\nShared target               219M\n    shared build artifacts\n    safe to remove with\n    `disk-maint clean shared`\n\n"
        );
    }

    #[test]
    fn omits_zero_cargo_build_artifacts() {
        let mut output = String::new();
        push_cargo_build_artifacts(&mut output, &build_artifacts(0, None));
        assert_eq!(output, "Cargo build artifacts\n\n");
    }

    #[test]
    fn omits_zero_size_cache_metrics() {
        let mut output = String::new();
        push_described_metric_if_nonzero(
            &mut output,
            "Cargo git cache",
            0,
            "shared git dependency cache; removing may require re-fetching",
            &[],
        );

        assert_eq!(output, "");
    }

    #[test]
    fn formats_nonzero_cache_metrics() {
        let mut output = String::new();
        push_described_metric_if_nonzero(
            &mut output,
            "Cargo git cache",
            123_456,
            "shared git dependency cache; removing may require re-fetching",
            &[],
        );

        assert_eq!(
            output,
            "Cargo git cache             121K\n    shared git dependency cache; removing may require re-fetching\n\n"
        );
    }

    #[test]
    fn formats_cargo_registry_data_as_separate_rows() {
        let mut output = String::new();
        push_cargo_registry_data(
            &mut output,
            &CargoRegistryData {
                cache_bytes: 255_852_544,
                src_bytes: 1_610_612_736,
                index_bytes: 83_886_080,
            },
        );

        assert_eq!(
            output,
            "Cargo crate archives        244M\n    downloaded .crate archives used as Cargo's package cache\n\nCargo crate sources         1.5G\n    unpacked crate source trees from downloaded registry packages\n\nCargo registry index         80M\n    registry index and package metadata used for dependency resolution\n\n"
        );
    }

    #[test]
    fn omits_zero_cargo_registry_data_rows() {
        let mut output = String::new();
        push_cargo_registry_data(
            &mut output,
            &CargoRegistryData {
                cache_bytes: 123_456,
                src_bytes: 0,
                index_bytes: 0,
            },
        );

        assert_eq!(
            output,
            "Cargo crate archives        121K\n    downloaded .crate archives used as Cargo's package cache\n\n"
        );
    }

    #[test]
    fn cargo_registry_data_counts_standard_registry_subdirectories_only() {
        let temp = test_dir("disk-maint-cargo-registry-data");
        let registry = temp.join("registry");
        fs::create_dir_all(registry.join("cache")).unwrap();
        fs::create_dir_all(registry.join("src")).unwrap();
        fs::create_dir_all(registry.join("index")).unwrap();
        fs::create_dir_all(registry.join("other")).unwrap();
        fs::write(registry.join("cache/crate.crate"), [0; 10]).unwrap();
        fs::write(registry.join("src/lib.rs"), [0; 20]).unwrap();
        fs::write(registry.join("index/config.json"), [0; 30]).unwrap();
        fs::write(registry.join("other/ignored"), [0; 40]).unwrap();

        let data = cargo_registry_data(&registry).unwrap();

        assert_eq!(
            data,
            CargoRegistryData {
                cache_bytes: 10,
                src_bytes: 20,
                index_bytes: 30,
            }
        );
        fs::remove_dir_all(temp).unwrap();
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

    #[test]
    fn omits_one_installed_rustup_toolchain() {
        let mut output = String::new();
        push_rustup_toolchains(
            &mut output,
            653_262_848,
            Some(&RustupToolchains {
                active: "stable".to_string(),
                default: "stable".to_string(),
                installed: vec!["stable".to_string()],
            }),
        );

        assert_eq!(output, "");
    }

    #[test]
    fn formats_actionable_rustup_toolchains() {
        let mut output = String::new();
        push_rustup_toolchains(
            &mut output,
            1_503_238_554,
            Some(&RustupToolchains {
                active: "stable".to_string(),
                default: "stable".to_string(),
                installed: vec![
                    "stable".to_string(),
                    "beta".to_string(),
                    "nightly".to_string(),
                ],
            }),
        );

        assert_eq!(
            output,
            "Rustup toolchains           1.4G\n    active/default: stable\n    additional: beta, nightly\n    removable:\n      rustup toolchain uninstall beta\n      rustup toolchain uninstall nightly\n\n"
        );
    }

    #[test]
    fn omits_rustup_toolchains_when_only_active_and_default_are_installed() {
        let mut output = String::new();
        push_rustup_toolchains(
            &mut output,
            1_503_238_554,
            Some(&RustupToolchains {
                active: "nightly".to_string(),
                default: "stable".to_string(),
                installed: vec!["stable".to_string(), "nightly".to_string()],
            }),
        );

        assert_eq!(output, "");
    }

    #[test]
    fn formats_rustup_unavailable_with_existing_guidance() {
        let mut output = String::new();
        push_rustup_toolchains(&mut output, 0, None);

        assert_eq!(
            output,
            "Rustup toolchains             0B\n    remove old toolchains with\n    `rustup toolchain uninstall <toolchain>`\n\n"
        );
    }

    #[test]
    fn formats_active_override_without_uninstalling_active_toolchain() {
        let mut output = String::new();
        push_rustup_toolchains(
            &mut output,
            1_503_238_554,
            Some(&RustupToolchains {
                active: "nightly".to_string(),
                default: "stable".to_string(),
                installed: vec![
                    "stable".to_string(),
                    "nightly".to_string(),
                    "beta".to_string(),
                ],
            }),
        );

        assert_eq!(
            output,
            "Rustup toolchains           1.4G\n    active: nightly\n    default: stable\n    additional: nightly, beta\n    removable:\n      rustup toolchain uninstall beta\n\n"
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

    fn test_dir(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }
}
