//! The string escape sequences shared by the `.tla` and `.cfg` readers.
//!
//! TLA+ and TLC's `.cfg` reader recognize the same six escapes inside a
//! string literal: `\"`, `\\`, `\n`, `\t`, `\r` and `\f`. Each scanner keeps
//! its own loop, positions and error recovery; only the character mapping is
//! shared here.

/// The character the escape `\c` decodes to, or `None` when `c` is not one of
/// the six supported escapes.
pub(crate) fn unescape(c: char) -> Option<char> {
    Some(match c {
        '"' => '"',
        '\\' => '\\',
        'n' => '\n',
        't' => '\t',
        'r' => '\r',
        'f' => '\u{c}',
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::unescape;

    #[test]
    fn decodes_the_six_supported_escapes() {
        let cases = [('"', '"'), ('\\', '\\'), ('n', '\n'), ('t', '\t'), ('r', '\r'), ('f', '\u{c}')];
        for (c, want) in cases {
            assert_eq!(unescape(c), Some(want), "escape for {c:?}");
        }
    }

    #[test]
    fn rejects_every_other_character() {
        for c in ['q', 's', 'b', '0', '/', '\'', ' ', '\n', '\t', 'F', 'N'] {
            assert_eq!(unescape(c), None, "character {c:?} must not decode");
        }
    }
}
