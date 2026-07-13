use std::path::Path;

use crate::format_bytes;

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
    output.push_str(&format!(
        "Cargo build artifacts   {:>9}  safe to remove per-project with `cargo clean` or `disk-maint clean target`\n",
        format_bytes(build_artifacts)
    ));
    output.push_str(&format!(
        "Cargo registry cache    {:>9}  shared package cache; removing may require re-downloads\n",
        format_bytes(cargo_registry)
    ));
    output.push_str(&format!(
        "Cargo git cache         {:>9}  shared git dependency cache; removing may require re-fetching\n",
        format_bytes(cargo_git)
    ));
    output.push_str(&format!(
        "Rustup toolchains       {:>9}  remove old toolchains with `rustup toolchain uninstall <toolchain>`\n",
        format_bytes(rustup_toolchains)
    ));
    output.push_str(&format!("Rust projects scanned   {:>9}\n", projects.len()));
    output.push_str("\nNo changes made.");
    Ok(output)
}

fn size_or_zero(path: &Path) -> Result<u64, String> {
    crate::rust::path_size(path)
}
