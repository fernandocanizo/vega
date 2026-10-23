//! Turns a raw HTTP response into what the Response pane and `--run` show. Pure data:
//! no terminal code.

use serde_json::Value;

use crate::client::Response;

/// The terminal pane keeps at most this many lines of output; the command line prints
/// everything.
pub const MAX_LINES: usize = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    /// A GraphQL error message.
    Error,
    /// A line of the pretty-printed data (or of a body that is not JSON).
    Data,
    /// A remark about the output itself, such as it being capped.
    Note,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponseView {
    pub status: u16,
    pub elapsed_ms: u64,
    /// Messages of the GraphQL `errors`, if any.
    pub errors: Vec<String>,
    /// The body was not JSON and is shown as received.
    pub not_json: bool,
    /// The output lines, at most `MAX_LINES`.
    pub lines: Vec<String>,
    /// How many lines the full output has.
    pub total_lines: usize,
}

impl ResponseView {
    /// A 2xx answer with a JSON body that holds no GraphQL errors.
    pub fn ok(&self) -> bool {
        (200..300).contains(&self.status) && !self.not_json && self.errors.is_empty()
    }

    /// Everything the pane lists, in order: errors, a blank line when data follows, the
    /// data lines, and a note when the output was capped.
    pub fn body_lines(&self) -> Vec<(LineKind, String)> {
        let mut out: Vec<(LineKind, String)> = self
            .errors
            .iter()
            .map(|message| (LineKind::Error, format!("error: {message}")))
            .collect();
        if !self.errors.is_empty() && !self.lines.is_empty() {
            out.push((LineKind::Data, String::new()));
        }
        out.extend(self.lines.iter().map(|line| (LineKind::Data, line.clone())));
        if self.total_lines > self.lines.len() {
            out.push((
                LineKind::Note,
                format!(
                    "… output capped: showing {} of {} lines (--run prints everything)",
                    self.lines.len(),
                    self.total_lines
                ),
            ));
        }
        out
    }
}

fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}

/// The messages of a GraphQL `errors` array (a whole item when it has no `message`).
fn error_messages(value: &Value) -> Vec<String> {
    value
        .get("errors")
        .and_then(Value::as_array)
        .map(|errors| {
            errors
                .iter()
                .map(|error| match error.get("message").and_then(Value::as_str) {
                    Some(message) => message.to_string(),
                    None => error.to_string(),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The body pretty-printed when it is JSON, as received otherwise.
pub fn pretty_body(body: &str) -> String {
    match serde_json::from_str::<Value>(body) {
        Ok(value) => pretty(&value),
        Err(_) => body.to_string(),
    }
}

pub fn view(response: &Response) -> ResponseView {
    let elapsed_ms = u64::try_from(response.elapsed.as_millis()).unwrap_or(u64::MAX);
    let (errors, not_json, text) = match serde_json::from_str::<Value>(&response.body) {
        Ok(value) => {
            let errors = error_messages(&value);
            let text = match value.get("data") {
                Some(data) if !data.is_null() => pretty(data),
                _ if errors.is_empty() => pretty(&value),
                _ => String::new(),
            };
            (errors, false, text)
        }
        Err(_) => (Vec::new(), true, response.body.clone()),
    };
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let total_lines = lines.len();
    lines.truncate(MAX_LINES);
    ResponseView {
        status: response.status,
        elapsed_ms,
        errors,
        not_json,
        lines,
        total_lines,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn response(status: u16, body: &str) -> Response {
        Response {
            status,
            elapsed: Duration::from_millis(42),
            body: body.to_string(),
        }
    }

    fn texts(view: &ResponseView) -> Vec<String> {
        view.body_lines()
            .into_iter()
            .map(|(_, text)| text)
            .collect()
    }

    #[test]
    fn data_is_pretty_printed_with_status_and_time() {
        let view = view(&response(200, r#"{"data":{"me":{"id":"1"}}}"#));
        assert_eq!(view.status, 200);
        assert_eq!(view.elapsed_ms, 42);
        assert!(view.ok());
        assert_eq!(
            texts(&view),
            ["{", r#"  "me": {"#, r#"    "id": "1""#, "  }", "}"]
        );
    }

    #[test]
    fn data_and_errors_show_both_errors_first() {
        let body = r#"{"data":{"me":null},"errors":[{"message":"boom"},{"code":7}]}"#;
        let view = view(&response(200, body));
        assert!(!view.ok());
        assert_eq!(view.errors, ["boom", r#"{"code":7}"#]);
        let lines = view.body_lines();
        assert_eq!(lines[0], (LineKind::Error, "error: boom".to_string()));
        assert_eq!(
            lines[1],
            (LineKind::Error, r#"error: {"code":7}"#.to_string())
        );
        assert_eq!(lines[2], (LineKind::Data, String::new()));
        assert_eq!(lines[3].1, "{");
    }

    #[test]
    fn errors_without_data_show_only_the_errors() {
        let view = view(&response(200, r#"{"errors":[{"message":"nope"}]}"#));
        assert_eq!(texts(&view), ["error: nope"]);
        let view = self::view(&response(
            200,
            r#"{"data":null,"errors":[{"message":"nope"}]}"#,
        ));
        assert_eq!(texts(&view), ["error: nope"]);
    }

    #[test]
    fn a_body_without_data_or_errors_is_shown_whole() {
        let view = view(&response(200, r#"{"hello":1}"#));
        assert!(view.ok());
        assert_eq!(texts(&view), ["{", r#"  "hello": 1"#, "}"]);
    }

    #[test]
    fn a_body_that_is_not_json_is_shown_as_received_and_flagged() {
        let view = view(&response(502, "<html>\nbad gateway\n</html>"));
        assert!(view.not_json);
        assert!(!view.ok());
        assert_eq!(texts(&view), ["<html>", "bad gateway", "</html>"]);
    }

    #[test]
    fn a_non_2xx_status_is_not_ok_even_with_json() {
        let view = view(&response(500, r#"{"data":{"a":1}}"#));
        assert!(!view.ok());
        assert_eq!(view.status, 500);
    }

    #[test]
    fn long_output_is_capped_with_a_note() {
        let numbers: Vec<String> = (0..12_000).map(|n| n.to_string()).collect();
        let body = format!(r#"{{"data":[{}]}}"#, numbers.join(","));
        let view = view(&response(200, &body));
        assert_eq!(view.lines.len(), MAX_LINES);
        assert_eq!(view.total_lines, 12_002);
        let lines = view.body_lines();
        let (kind, note) = lines.last().unwrap();
        assert_eq!(*kind, LineKind::Note);
        assert!(note.contains("10000 of 12002"), "{note}");
    }

    #[test]
    fn pretty_body_formats_json_and_leaves_other_text_alone() {
        assert_eq!(pretty_body(r#"{"a":1}"#), "{\n  \"a\": 1\n}");
        assert_eq!(pretty_body("plain"), "plain");
    }
}
