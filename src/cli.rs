use std::path::PathBuf;

use crate::{default_repo_root, expand_home};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cli {
    pub root: PathBuf,
    pub command: Command,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    Help(String),
    Error(String),
}

impl ParseError {
    pub fn message(&self) -> &str {
        match self {
            ParseError::Help(message) | ParseError::Error(message) => message,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Scan,
    Rust,
    Git(GitCommand),
    Clean(CleanCommand),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitCommand {
    Status,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CleanCommand {
    Target(CleanTargetOptions),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CleanTargetOptions {
    pub dry_run: bool,
    pub yes: bool,
}

pub fn parse_args<I, S>(args: I) -> Result<Cli, ParseError>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut args = args.into_iter().map(Into::into);
    let _program = args.next();

    let mut root = default_repo_root();
    let mut clean_target_options = CleanTargetOptions::default();
    let mut positional = Vec::new();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => return Err(ParseError::Help(help_text())),
            "--dry-run" => clean_target_options.dry_run = true,
            "--yes" => clean_target_options.yes = true,
            "-r" | "--root" => {
                let Some(value) = args.next() else {
                    return Err(ParseError::Error("missing value for --root".to_string()));
                };
                root = expand_home(&value);
            }
            _ if arg.starts_with("--root=") => {
                let value = arg.trim_start_matches("--root=");
                root = expand_home(value);
            }
            _ if arg.starts_with('-') => {
                return Err(ParseError::Error(format!(
                    "unknown option: {arg}\n\n{}",
                    help_text()
                )));
            }
            _ => positional.push(arg),
        }
    }

    let command = match positional.as_slice() {
        [command] if command == "scan" => Command::Scan,
        [command] if command == "rust" => Command::Rust,
        [command, target] if command == "git" && target == "status" => {
            Command::Git(GitCommand::Status)
        }
        [command, target] if command == "clean" && target == "target" => {
            if clean_target_options.dry_run && clean_target_options.yes {
                return Err(ParseError::Error(
                    "--dry-run cannot be used with --yes".to_string(),
                ));
            }
            Command::Clean(CleanCommand::Target(clean_target_options))
        }
        [] => return Err(ParseError::Help(help_text())),
        _ => {
            return Err(ParseError::Error(format!(
                "unknown command: {}\n\n{}",
                positional.join(" "),
                help_text()
            )));
        }
    };

    if !matches!(command, Command::Clean(CleanCommand::Target(_)))
        && clean_target_options != CleanTargetOptions::default()
    {
        return Err(ParseError::Error(
            "--dry-run and --yes are only valid with `clean target`".to_string(),
        ));
    }

    Ok(Cli { root, command })
}

pub fn help_text() -> String {
    "Usage:
  disk-maint [--root PATH] scan
  disk-maint [--root PATH] rust
  disk-maint [--root PATH] git status
  disk-maint [--root PATH] clean target [--dry-run | --yes]

Options:
  -r, --root PATH   Repository root to scan (default: ~/labs/repos)
      --dry-run     Show the clean target plan without prompting or deleting
      --yes         Delete planned target/ directories without prompting
  -h, --help        Show this help
"
    .trim_end()
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::{CleanCommand, Command, GitCommand, parse_args};

    #[test]
    fn parses_clean_target() {
        let cli = parse_args(["disk-maint", "--root", "/tmp/repos", "clean", "target"]).unwrap();
        assert_eq!(
            cli.command,
            Command::Clean(CleanCommand::Target(Default::default()))
        );
        assert_eq!(cli.root.to_string_lossy(), "/tmp/repos");
    }

    #[test]
    fn parses_clean_target_dry_run() {
        let cli = parse_args(["disk-maint", "clean", "target", "--dry-run"]).unwrap();
        assert_eq!(
            cli.command,
            Command::Clean(CleanCommand::Target(super::CleanTargetOptions {
                dry_run: true,
                yes: false,
            }))
        );
    }

    #[test]
    fn parses_clean_target_yes() {
        let cli = parse_args(["disk-maint", "--yes", "clean", "target"]).unwrap();
        assert_eq!(
            cli.command,
            Command::Clean(CleanCommand::Target(super::CleanTargetOptions {
                dry_run: false,
                yes: true,
            }))
        );
    }

    #[test]
    fn parses_git_status() {
        let cli = parse_args(["disk-maint", "--root=/tmp/repos", "git", "status"]).unwrap();
        assert_eq!(cli.command, Command::Git(GitCommand::Status));
        assert_eq!(cli.root.to_string_lossy(), "/tmp/repos");
    }

    #[test]
    fn rejects_unknown_commands() {
        let error = parse_args(["disk-maint", "clean", "registry"]).unwrap_err();
        assert!(error.message().contains("unknown command"));
    }

    #[test]
    fn rejects_clean_target_dry_run_with_yes() {
        let error =
            parse_args(["disk-maint", "clean", "target", "--dry-run", "--yes"]).unwrap_err();
        assert!(
            error
                .message()
                .contains("--dry-run cannot be used with --yes")
        );
    }

    #[test]
    fn rejects_clean_target_options_on_other_commands() {
        let error = parse_args(["disk-maint", "scan", "--dry-run"]).unwrap_err();
        assert!(
            error
                .message()
                .contains("--dry-run and --yes are only valid with `clean target`")
        );
    }
}
