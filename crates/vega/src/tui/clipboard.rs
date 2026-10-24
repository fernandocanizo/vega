//! Copying to the clipboard with an OSC 52 escape sequence: the terminal, not this
//! process, owns the clipboard, so it works over SSH and needs no display server.

use std::io::Write;

use base64::{engine::general_purpose::STANDARD, Engine};

/// The sequence that asks the terminal to put `text` on the system clipboard.
pub fn osc52(text: &str) -> String {
    format!("\x1b]52;c;{}\x07", STANDARD.encode(text))
}

/// Sends `text` to the terminal. A terminal that ignores OSC 52 drops it silently, so
/// there is nothing to report either way.
pub fn copy(text: &str) {
    let mut out = std::io::stdout();
    let _ = out.write_all(osc52(text).as_bytes());
    let _ = out.flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_text_is_base64_encoded_in_a_clipboard_sequence() {
        assert_eq!(osc52("hi"), "\x1b]52;c;aGk=\x07");
    }

    #[test]
    fn non_ascii_text_is_encoded_as_utf8() {
        assert_eq!(osc52("é"), "\x1b]52;c;w6k=\x07");
    }
}
