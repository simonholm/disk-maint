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
    Clean(CleanCommand),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CleanCommand {
    Target,
}

pub fn parse_args<I, S>(args: I) -> Result<Cli, ParseError>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut args = args.into_iter().map(Into::into);
    let _program = args.next();

    let mut root = default_repo_root();
    let mut positional = Vec::new();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => return Err(ParseError::Help(help_text())),
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
        [command, target] if command == "clean" && target == "target" => {
            Command::Clean(CleanCommand::Target)
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

    Ok(Cli { root, command })
}

pub fn help_text() -> String {
    "Usage:
  disk-maint [--root PATH] scan
  disk-maint [--root PATH] rust
  disk-maint [--root PATH] clean target

Options:
  -r, --root PATH   Repository root to scan (default: ~/labs/repos)
  -h, --help        Show this help
"
    .trim_end()
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::{CleanCommand, Command, parse_args};

    #[test]
    fn parses_clean_target() {
        let cli = parse_args(["disk-maint", "--root", "/tmp/repos", "clean", "target"]).unwrap();
        assert_eq!(cli.command, Command::Clean(CleanCommand::Target));
        assert_eq!(cli.root.to_string_lossy(), "/tmp/repos");
    }

    #[test]
    fn rejects_unknown_commands() {
        let error = parse_args(["disk-maint", "clean", "registry"]).unwrap_err();
        assert!(error.message().contains("unknown command"));
    }
}
