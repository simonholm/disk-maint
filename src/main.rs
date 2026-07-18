use std::io::{self, BufRead, Write};
use std::process::ExitCode;

use disk_maint::clean;
use disk_maint::cli::{self, CleanCommand, CleanTargetOptions, Command, GitCommand};

fn main() -> ExitCode {
    let cli = match cli::parse_args(std::env::args()) {
        Ok(cli) => cli,
        Err(cli::ParseError::Help(message)) => {
            println!("{message}");
            return ExitCode::SUCCESS;
        }
        Err(cli::ParseError::Error(message)) => {
            eprintln!("{message}");
            return ExitCode::from(2);
        }
    };

    match run(cli) {
        Ok(output) => {
            if !output.is_empty() {
                println!("{output}");
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("disk-maint: {error}");
            ExitCode::from(1)
        }
    }
}

fn run(cli: cli::Cli) -> Result<String, String> {
    match cli.command {
        Command::Scan => disk_maint::scan::report(&cli.root),
        Command::Rust => disk_maint::rust::report(&cli.root),
        Command::Git(GitCommand::Status) => disk_maint::git::report_status(&cli.root),
        Command::Clean(CleanCommand::Target(options)) => run_clean_target(&cli.root, options),
    }
}

fn run_clean_target(root: &std::path::Path, options: CleanTargetOptions) -> Result<String, String> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    run_clean_target_with_io(root, options, &mut stdin.lock(), &mut stdout)
}

fn run_clean_target_with_io<R: BufRead, W: Write>(
    root: &std::path::Path,
    options: CleanTargetOptions,
    input: &mut R,
    output: &mut W,
) -> Result<String, String> {
    let plan = clean::target::plan(root)?;
    let summary = clean::target::render_plan(&plan);

    if plan.items.is_empty() {
        return Ok(summary);
    }

    if options.dry_run {
        return Ok(format!(
            "{summary}\n\nDry run: no files were deleted.\n\nRun 'disk-maint clean target' to review and confirm deletion,\nor 'disk-maint clean target --yes' to delete without prompting."
        ));
    }

    if options.yes {
        clean::target::execute(&plan)?;
        return Ok(format!(
            "{summary}\n\nDeleted {} target/ directories.\nReclaimed approximately {}.",
            plan.items.len(),
            disk_maint::format_bytes(plan.total_bytes)
        ));
    }

    writeln!(output, "{summary}").map_err(|error| format!("failed to write plan: {error}"))?;
    write!(
        output,
        "Delete these target/ directories? Type 'yes' to continue: "
    )
    .map_err(|error| format!("failed to write prompt: {error}"))?;
    output
        .flush()
        .map_err(|error| format!("failed to flush prompt: {error}"))?;

    let mut answer = String::new();
    input
        .read_line(&mut answer)
        .map_err(|error| format!("failed to read confirmation: {error}"))?;

    if answer.trim() != "yes" {
        return Ok("Aborted. No files were deleted.".to_string());
    }

    clean::target::execute(&plan)?;
    Ok(format!(
        "Deleted {} target/ directories.\nReclaimed approximately {}.",
        plan.items.len(),
        disk_maint::format_bytes(plan.total_bytes)
    ))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::Cursor;

    use disk_maint::cli::CleanTargetOptions;

    use super::run_clean_target_with_io;

    #[test]
    fn clean_target_default_prompts_and_deletes_after_yes() {
        let temp = rust_project_with_target("clean-target-default");
        let target = temp.join("example/target");
        let mut input = Cursor::new(b"yes\n");
        let mut output = Vec::new();

        let result = run_clean_target_with_io(
            &temp,
            CleanTargetOptions::default(),
            &mut input,
            &mut output,
        )
        .unwrap();

        assert!(!target.exists());
        assert!(
            String::from_utf8(output)
                .unwrap()
                .contains("Type 'yes' to continue")
        );
        assert!(result.contains("Deleted 1 target/ directories."));

        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn clean_target_default_aborts_without_exact_yes() {
        let temp = rust_project_with_target("clean-target-abort");
        let target = temp.join("example/target");
        let mut input = Cursor::new(b"y\n");
        let mut output = Vec::new();

        let result = run_clean_target_with_io(
            &temp,
            CleanTargetOptions::default(),
            &mut input,
            &mut output,
        )
        .unwrap();

        assert!(target.exists());
        assert!(
            String::from_utf8(output)
                .unwrap()
                .contains("Type 'yes' to continue")
        );
        assert_eq!(result, "Aborted. No files were deleted.");

        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn clean_target_dry_run_prints_plan_without_prompting_or_deleting() {
        let temp = rust_project_with_target("clean-target-dry-run");
        let target = temp.join("example/target");
        let mut input = Cursor::new(Vec::new());
        let mut output = Vec::new();

        let result = run_clean_target_with_io(
            &temp,
            CleanTargetOptions {
                dry_run: true,
                yes: false,
            },
            &mut input,
            &mut output,
        )
        .unwrap();

        assert!(target.exists());
        assert!(output.is_empty());
        assert!(result.contains("The following Rust build artifact directories"));
        assert!(result.contains("Dry run: no files were deleted."));
        assert!(result.contains(
            "Run 'disk-maint clean target' to review and confirm deletion,\nor 'disk-maint clean target --yes' to delete without prompting."
        ));

        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn clean_target_yes_deletes_without_prompting() {
        let temp = rust_project_with_target("clean-target-yes");
        let target = temp.join("example/target");
        let mut input = Cursor::new(Vec::new());
        let mut output = Vec::new();

        let result = run_clean_target_with_io(
            &temp,
            CleanTargetOptions {
                dry_run: false,
                yes: true,
            },
            &mut input,
            &mut output,
        )
        .unwrap();

        assert!(!target.exists());
        assert!(output.is_empty());
        assert!(result.contains("The following Rust build artifact directories"));
        assert!(result.contains("Deleted 1 target/ directories."));

        fs::remove_dir_all(temp).unwrap();
    }

    fn rust_project_with_target(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);

        let project = path.join("example");
        fs::create_dir_all(project.join("src")).unwrap();
        fs::create_dir_all(project.join("target")).unwrap();
        fs::write(
            project.join("Cargo.toml"),
            "[package]\nname = \"example\"\n",
        )
        .unwrap();
        fs::write(project.join("target/app"), "artifact").unwrap();

        path
    }
}
