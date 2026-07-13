pub mod clean;
pub mod cli;
pub mod rust;
pub mod scan;

pub fn default_repo_root() -> std::path::PathBuf {
    home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("labs")
        .join("repos")
}

pub fn home_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME").map(std::path::PathBuf::from)
}

pub fn expand_home(path: &str) -> std::path::PathBuf {
    if path == "~" {
        return home_dir().unwrap_or_else(|| std::path::PathBuf::from(path));
    }

    if let Some(rest) = path.strip_prefix("~/")
        && let Some(home) = home_dir()
    {
        return home.join(rest);
    }

    std::path::PathBuf::from(path)
}

pub fn format_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "K", "M", "G", "T"];
    if bytes < 1024 {
        return format!("{bytes}B");
    }

    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }

    if value >= 10.0 {
        format!("{value:.0}{}", UNITS[unit])
    } else {
        format!("{value:.1}{}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::format_bytes;

    #[test]
    fn formats_human_readable_sizes() {
        assert_eq!(format_bytes(0), "0B");
        assert_eq!(format_bytes(400), "400B");
        assert_eq!(format_bytes(4096), "4.0K");
        assert_eq!(format_bytes(1_572_864), "1.5M");
        assert_eq!(format_bytes(1_234_567_890), "1.1G");
    }
}
