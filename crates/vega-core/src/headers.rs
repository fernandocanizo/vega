//! HTTP headers sent with every request. Every value is a secret: it is hidden in
//! `Debug` output and masked in anything shown to the user.

use std::{cmp::Reverse, fmt};

/// Name of the header that carries `VEGA_GRAPHQL_KEY` unless another one is configured.
pub const DEFAULT_KEY_HEADER: &str = "X-API-Key";

/// The text is not a valid HTTP header name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("not a valid header name")]
pub struct InvalidName;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum HeaderError {
    /// The message names the line only: the line may hold a secret.
    #[error("VEGA_GRAPHQL_HEADERS line {line}: expected `Name: value`")]
    Malformed { line: usize },
}

#[derive(Clone, PartialEq, Eq)]
pub struct Header {
    name: String,
    value: String,
}

impl Header {
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Result<Self, InvalidName> {
        let name = name.into();
        if !is_valid_name(&name) {
            return Err(InvalidName);
        }
        Ok(Self {
            name,
            value: value.into(),
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn value(&self) -> &str {
        &self.value
    }
}

impl fmt::Debug for Header {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Header")
            .field("name", &self.name)
            .field("value", &"***")
            .finish()
    }
}

/// An RFC 9110 token: ASCII letters, digits and `!#$%&'*+-.^_`|~`.
fn is_valid_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "!#$%&'*+-.^_`|~".contains(c))
}

/// Parses `Name: value` lines (the format of `VEGA_GRAPHQL_HEADERS`). Blank lines are
/// ignored; a line without a name, a colon or a value is an error.
pub fn parse_lines(text: &str) -> Result<Vec<Header>, HeaderError> {
    let mut headers = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let malformed = HeaderError::Malformed { line: index + 1 };
        let (name, value) = line.split_once(':').ok_or(malformed)?;
        let value = value.trim();
        if value.is_empty() {
            return Err(malformed);
        }
        headers.push(Header::new(name.trim(), value).map_err(|_| malformed)?);
    }
    Ok(headers)
}

/// Adds `overrides` after `base`, dropping any header of `base` (or an earlier override)
/// that has the same name, compared case-insensitively.
pub fn merge(mut base: Vec<Header>, overrides: Vec<Header>) -> Vec<Header> {
    for header in overrides {
        base.retain(|existing| !existing.name.eq_ignore_ascii_case(&header.name));
        base.push(header);
    }
    base
}

/// Replaces every secret in `text` with `***`. For a value such as `Bearer abc` both the
/// whole value and its last word count as secrets, because a server may echo either.
pub fn mask(text: &str, headers: &[Header]) -> String {
    let mut secrets: Vec<&str> = headers
        .iter()
        .flat_map(|header| {
            let value = header.value();
            [Some(value), value.rsplit_once(' ').map(|(_, last)| last)]
        })
        .flatten()
        .filter(|secret| !secret.is_empty())
        .collect();
    secrets.sort_by_key(|secret| Reverse(secret.len()));
    secrets
        .into_iter()
        .fold(text.to_string(), |text, secret| text.replace(secret, "***"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(name: &str, value: &str) -> Header {
        Header::new(name, value).unwrap()
    }

    #[test]
    fn debug_output_hides_the_value() {
        let text = format!("{:?}", h("X-API-Key", "super-secret"));
        assert!(!text.contains("super-secret"), "{text}");
        assert!(text.contains("X-API-Key"), "{text}");
        assert!(text.contains("***"), "{text}");
    }

    #[test]
    fn invalid_names_are_rejected() {
        for name in ["", "Has Space", "Colon:Name", "Accént"] {
            assert_eq!(Header::new(name, "v"), Err(InvalidName), "{name:?}");
        }
        assert!(Header::new("X-Api_Key.1", "v").is_ok());
    }

    #[test]
    fn parse_lines_reads_name_value_pairs() {
        let parsed =
            parse_lines("Authorization: Bearer abc\r\n\n  X-Tenant :  team-9  \n").unwrap();
        assert_eq!(
            parsed,
            vec![h("Authorization", "Bearer abc"), h("X-Tenant", "team-9")]
        );
    }

    #[test]
    fn parse_lines_keeps_colons_inside_the_value() {
        let parsed = parse_lines("X-Url: https://example.com:8080/x").unwrap();
        assert_eq!(parsed, vec![h("X-Url", "https://example.com:8080/x")]);
    }

    #[test]
    fn parse_lines_of_empty_text_is_empty() {
        assert_eq!(parse_lines("").unwrap(), vec![]);
        assert_eq!(parse_lines("\n  \n").unwrap(), vec![]);
    }

    #[test]
    fn malformed_lines_name_the_line_and_never_the_value() {
        for text in [
            "Authorization Bearer tok-123",
            "X-Ok: fine\nBad Name: tok-123",
            "X-Ok: fine\nX-Empty:   ",
        ] {
            let err = parse_lines(text).unwrap_err();
            let message = err.to_string();
            assert!(!message.contains("tok-123"), "{message}");
            assert!(message.contains("line"), "{message}");
        }
        assert_eq!(
            parse_lines("X-Ok: fine\nBad Name: v").unwrap_err(),
            HeaderError::Malformed { line: 2 }
        );
    }

    #[test]
    fn merge_replaces_by_name_ignoring_case_and_the_override_wins() {
        let base = vec![h("X-API-Key", "k"), h("X-Keep", "1")];
        let overrides = vec![h("x-api-key", "other"), h("X-New", "2")];
        assert_eq!(
            merge(base, overrides),
            vec![h("X-Keep", "1"), h("x-api-key", "other"), h("X-New", "2")]
        );
    }

    #[test]
    fn merge_lets_a_later_override_win_over_an_earlier_one() {
        let merged = merge(vec![], vec![h("X-A", "1"), h("x-a", "2")]);
        assert_eq!(merged, vec![h("x-a", "2")]);
    }

    #[test]
    fn mask_hides_whole_values_and_the_last_word_of_a_value() {
        let headers = vec![
            h("Authorization", "Bearer tok-123"),
            h("X-Tenant", "team-9"),
        ];
        let text = "echo Bearer tok-123 / tok-123 / team-9";
        assert_eq!(mask(text, &headers), "echo *** / *** / ***");
    }

    #[test]
    fn mask_replaces_the_longest_secret_first() {
        let headers = vec![h("X-A", "abc"), h("X-B", "abcdef")];
        assert_eq!(mask("abcdef abc", &headers), "*** ***");
    }

    #[test]
    fn mask_ignores_empty_values_and_leaves_other_text_alone() {
        let headers = vec![h("X-Empty", "")];
        assert_eq!(mask("nothing to hide", &headers), "nothing to hide");
        assert_eq!(mask("no headers", &[]), "no headers");
    }
}
