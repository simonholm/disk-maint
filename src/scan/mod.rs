use std::path::Path;

use crate::format_bytes;

const LABEL_WIDTH: usize = 22;
const VALUE_WIDTH: usize = 9;
const DESCRIPTION_WIDTH: usize = 72;
const DESCRIPTION_INDENT: &str = "    ";

pub fn report(root: &Path) -> Result<String, String> {
    let projects = crate::rust::discover_projects(root)?;
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
    let build_artifacts: u64 = projects.iter().map(|project| project.target_bytes).sum();

    let mut output = String::new();
    output.push_str("Rust maintenance report\n\n");
    push_described_metric(
        &mut output,
        "Cargo build artifacts",
        &format_bytes(build_artifacts),
        "safe to remove per-project with",
        &["cargo clean", "disk-maint clean target"],
    );
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
        &projects.len().to_string(),
    );
    output.push_str("\nNo changes made.");
    Ok(output)
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
    use super::{push_described_metric, push_metric, push_wrapped_description};

    #[test]
    fn formats_scan_metrics_as_separated_blocks() {
        let mut output = String::new();
        push_described_metric(
            &mut output,
            "Cargo build artifacts",
            "1.2G",
            "safe to remove per-project with",
            &["cargo clean", "disk-maint clean target"],
        );
        push_metric(&mut output, "Rust projects scanned", "12");

        assert_eq!(
            output,
            "Cargo build artifacts       1.2G\n    safe to remove per-project with\n    `cargo clean`\n    `disk-maint clean target`\n\nRust projects scanned         12\n"
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
}
