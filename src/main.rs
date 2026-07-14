use std::io::{self, Write};
use std::process::ExitCode;

use disk_maint::clean;
use disk_maint::cli::{self, CleanCommand, Command, GitCommand};

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
        Command::Clean(CleanCommand::Target) => run_clean_target(&cli.root),
    }
}

fn run_clean_target(root: &std::path::Path) -> Result<String, String> {
    let plan = clean::target::plan(root)?;
    let summary = clean::target::render_plan(&plan);

    if plan.items.is_empty() {
        return Ok(summary);
    }

    println!("{summary}");
    print!("Delete these target/ directories? Type 'yes' to continue: ");
    io::stdout()
        .flush()
        .map_err(|error| format!("failed to flush prompt: {error}"))?;

    let mut answer = String::new();
    io::stdin()
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
