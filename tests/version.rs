use std::process::Command;

fn assert_version(flag: &str) {
    let output = Command::new(env!("CARGO_BIN_EXE_disk-maint"))
        .arg(flag)
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("{} {}\n", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn long_version_flag_reports_package_version() {
    assert_version("--version");
}

#[test]
fn short_version_flag_reports_package_version() {
    assert_version("-V");
}

#[test]
fn help_documents_version_flags() {
    let output = Command::new(env!("CARGO_BIN_EXE_disk-maint"))
        .arg("--help")
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("-V, --version")
    );
}
