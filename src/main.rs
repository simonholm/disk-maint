use std::io::{self, BufRead, Write};
use std::process::ExitCode;

use disk_maint::clean;
use disk_maint::cli::{
    self, CleanCommand, CleanSharedOptions, CleanTargetOptions, Command, GitCommand,
};

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
        Command::Clean(CleanCommand::Shared(options)) => run_clean_shared(&cli.root, options),
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
    let planned = !plan.items.is_empty();

    if !planned {
        return Ok(summary);
    }

    if options.dry_run {
        return Ok(format!(
            "{summary}\n\nDry run: no files were deleted.\n\nRun 'disk-maint clean target' to review and confirm deletion,\nor 'disk-maint clean target --yes' to delete without prompting."
        ));
    }

    let deleted = format!(
        "Deleted {} target/ directories.\nReclaimed approximately {}.",
        plan.items.len(),
        disk_maint::format_bytes(plan.total_bytes)
    );
    run_confirmed_cleanup(
        &summary,
        "Delete these target/ directories? Type 'yes' to continue: ",
        options.yes,
        input,
        output,
        || clean::target::execute(&plan),
        &deleted,
    )
}

fn run_clean_shared(root: &std::path::Path, options: CleanSharedOptions) -> Result<String, String> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    run_clean_shared_with_io(root, options, &mut stdin.lock(), &mut stdout)
}

fn run_clean_shared_with_io<R: BufRead, W: Write>(
    root: &std::path::Path,
    options: CleanSharedOptions,
    input: &mut R,
    output: &mut W,
) -> Result<String, String> {
    let plan = clean::shared::plan(root)?;
    run_clean_shared_plan_with_io(&plan, options, input, output)
}

fn run_clean_shared_plan_with_io<R: BufRead, W: Write>(
    plan: &clean::shared::CleanPlan,
    options: CleanSharedOptions,
    input: &mut R,
    output: &mut W,
) -> Result<String, String> {
    let summary = clean::shared::render_plan(plan);
    let planned = plan.target_path.is_some();

    if !planned {
        return Ok(summary);
    }

    let deleted = format!(
        "Deleted shared Cargo target directory.\nReclaimed approximately {}.",
        disk_maint::format_bytes(plan.total_bytes)
    );
    run_confirmed_cleanup(
        &summary,
        "Delete the shared Cargo target directory? Type 'yes' to continue: ",
        options.yes,
        input,
        output,
        || clean::shared::execute(plan),
        &deleted,
    )
}

fn run_confirmed_cleanup<R: BufRead, W: Write>(
    summary: &str,
    prompt: &str,
    yes: bool,
    input: &mut R,
    output: &mut W,
    execute: impl FnOnce() -> Result<(), String>,
    deleted_message: &str,
) -> Result<String, String> {
    if yes {
        execute()?;
        return Ok(format!("{summary}\n\n{deleted_message}"));
    }

    writeln!(output, "{summary}").map_err(|error| format!("failed to write plan: {error}"))?;
    write!(output, "{prompt}").map_err(|error| format!("failed to write prompt: {error}"))?;
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

    execute()?;
    Ok(deleted_message.to_string())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::Cursor;

    use disk_maint::clean;
    use disk_maint::cli::{CleanSharedOptions, CleanTargetOptions};

    use super::{run_clean_shared_plan_with_io, run_clean_target_with_io};

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

    #[test]
    fn clean_shared_default_prompts_and_deletes_after_yes() {
        let (temp, plan) = shared_target_plan("clean-shared-default");
        let shared = plan.target_path.clone().unwrap();
        let mut input = Cursor::new(b"yes\n");
        let mut output = Vec::new();

        let result = run_clean_shared_plan_with_io(
            &plan,
            CleanSharedOptions::default(),
            &mut input,
            &mut output,
        )
        .unwrap();

        assert!(!shared.exists());
        assert!(
            String::from_utf8(output)
                .unwrap()
                .contains("Type 'yes' to continue")
        );
        assert!(result.contains("Deleted shared Cargo target directory."));
        assert!(result.contains("Reclaimed approximately 8B."));

        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn clean_shared_default_aborts_without_exact_yes() {
        let (temp, plan) = shared_target_plan("clean-shared-abort");
        let shared = plan.target_path.clone().unwrap();
        let mut input = Cursor::new(b"y\n");
        let mut output = Vec::new();

        let result = run_clean_shared_plan_with_io(
            &plan,
            CleanSharedOptions::default(),
            &mut input,
            &mut output,
        )
        .unwrap();

        assert!(shared.exists());
        assert!(
            String::from_utf8(output)
                .unwrap()
                .contains("Type 'yes' to continue")
        );
        assert_eq!(result, "Aborted. No files were deleted.");

        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn clean_shared_yes_deletes_without_prompting() {
        let (temp, plan) = shared_target_plan("clean-shared-yes");
        let shared = plan.target_path.clone().unwrap();
        let mut input = Cursor::new(Vec::new());
        let mut output = Vec::new();

        let result = run_clean_shared_plan_with_io(
            &plan,
            CleanSharedOptions { yes: true },
            &mut input,
            &mut output,
        )
        .unwrap();

        assert!(!shared.exists());
        assert!(output.is_empty());
        assert!(result.contains("Cargo's shared build cache"));
        assert!(result.contains("Deleted shared Cargo target directory."));

        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn clean_shared_absent_does_not_prompt() {
        let plan = clean::shared::CleanPlan {
            target_path: None,
            total_bytes: 0,
        };
        let mut input = Cursor::new(Vec::new());
        let mut output = Vec::new();

        let result = run_clean_shared_plan_with_io(
            &plan,
            CleanSharedOptions::default(),
            &mut input,
            &mut output,
        )
        .unwrap();

        assert!(output.is_empty());
        assert_eq!(
            result,
            "No shared Cargo target directory found. No files will be deleted."
        );
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

    fn shared_target_plan(
        name: &str,
    ) -> (std::path::PathBuf, disk_maint::clean::shared::CleanPlan) {
        let path = std::env::temp_dir().join(format!("{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        let shared = path.join("shared-target");
        fs::create_dir_all(&shared).unwrap();
        fs::write(shared.join("artifact"), "artifact").unwrap();
        let plan = clean::shared::plan_path(&shared).unwrap();
        (path, plan)
    }
}
