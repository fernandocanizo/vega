//! XDG base directory resolution for vega's own subdirectories.

use std::path::PathBuf;

/// An absolute `$XDG_*` value wins; relative or empty values are ignored per the
/// XDG spec; otherwise `$HOME/<fallback>`.
fn resolve(xdg_value: Option<&str>, home: Option<&str>, fallback: &str) -> Option<PathBuf> {
    if let Some(dir) = xdg_value {
        let path = PathBuf::from(dir);
        if path.is_absolute() {
            return Some(path);
        }
    }
    Some(PathBuf::from(home?).join(fallback))
}

fn app_dir(var: &str, fallback: &str) -> Option<PathBuf> {
    let xdg_value = std::env::var(var).ok();
    let home = std::env::var("HOME").ok();
    resolve(xdg_value.as_deref(), home.as_deref(), fallback).map(|dir| dir.join("vega"))
}

/// `$XDG_CACHE_HOME/vega`, or `~/.cache/vega`.
pub fn cache_dir() -> Option<PathBuf> {
    app_dir("XDG_CACHE_HOME", ".cache")
}

/// `$XDG_CONFIG_HOME/vega`, or `~/.config/vega`.
pub fn config_dir() -> Option<PathBuf> {
    app_dir("XDG_CONFIG_HOME", ".config")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn absolute_xdg_value_wins() {
        assert_eq!(
            resolve(Some("/x/cache"), Some("/home/u"), ".cache"),
            Some(PathBuf::from("/x/cache"))
        );
    }

    #[test]
    fn relative_or_empty_xdg_value_is_ignored() {
        let expected = Some(PathBuf::from("/home/u/.cache"));
        assert_eq!(
            resolve(Some("relative/dir"), Some("/home/u"), ".cache"),
            expected
        );
        assert_eq!(resolve(Some(""), Some("/home/u"), ".cache"), expected);
    }

    #[test]
    fn falls_back_to_home() {
        assert_eq!(
            resolve(None, Some("/home/u"), ".config"),
            Some(PathBuf::from("/home/u/.config"))
        );
    }

    #[test]
    fn nothing_to_go_on_returns_none() {
        assert_eq!(resolve(None, None, ".cache"), None);
    }
}
