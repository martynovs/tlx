//! Formatting shared by the report writers: numbers (TLC's progress lines in
//! the CLI, the `-stats` report in the runtime) and JSON string literals
//! (the `-stats` JSON and the details the CLI records in it).

use std::fmt::Write as _;

/// `n` with thousands separators: `39,928,269` (TLC's progress reports).
pub fn commas(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// How [`push_json_string`] spells a line feed (U+000A). Both spellings
/// decode to the same string; each writer keeps the one its output has
/// always had, so a byte comparison of earlier reports stays meaningful.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JsonNewline {
    /// `\n` (the runtime's `-stats` report).
    Short,
    /// `\u000a`, like every other control character (the CLI's
    /// optimization details).
    Unicode,
}

/// Append `s` to `out` as a JSON string literal (RFC 8259): `"` and `\`
/// are escaped with a backslash, a line feed is spelled as `newline` says,
/// every other character below U+0020 as `\u00xx` with lower-case hex
/// digits, and every other character (DEL, non-ASCII, U+2028/U+2029)
/// is copied unchanged. The result is valid JSON, not a JavaScript-safe
/// literal; writers that must match Gson's spelling (`ToJson`) use their
/// own escaper.
pub fn push_json_string(out: &mut String, s: &str, newline: JsonNewline) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' if newline == JsonNewline::Short => out.push_str("\\n"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// `s` as a JSON string literal ([`push_json_string`]).
pub fn json_string(s: &str, newline: JsonNewline) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    push_json_string(&mut out, s, newline);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commas_groups_by_three() {
        assert_eq!(commas(0), "0");
        assert_eq!(commas(999), "999");
        assert_eq!(commas(1000), "1,000");
        assert_eq!(commas(100_000), "100,000");
        assert_eq!(commas(39928269), "39,928,269");
        assert_eq!(commas(u64::MAX), "18,446,744,073,709,551,615");
    }

    /// The escape every control character gets, as written by hand.
    const CONTROLS: [&str; 32] = [
        "\\u0000", "\\u0001", "\\u0002", "\\u0003", "\\u0004", "\\u0005", "\\u0006", "\\u0007", "\\u0008", "\\u0009", "\\u000a", "\\u000b",
        "\\u000c", "\\u000d", "\\u000e", "\\u000f", "\\u0010", "\\u0011", "\\u0012", "\\u0013", "\\u0014", "\\u0015", "\\u0016", "\\u0017",
        "\\u0018", "\\u0019", "\\u001a", "\\u001b", "\\u001c", "\\u001d", "\\u001e", "\\u001f",
    ];

    #[test]
    fn json_string_escapes_every_control_character() {
        for (code, want) in CONTROLS.iter().enumerate() {
            let c = char::from_u32(code as u32).unwrap();
            assert_eq!(json_string(&c.to_string(), JsonNewline::Unicode), format!("\"{want}\""), "U+{code:04X}");
            let short = if c == '\n' { "\\n" } else { want };
            assert_eq!(json_string(&c.to_string(), JsonNewline::Short), format!("\"{short}\""), "U+{code:04X}");
        }
    }

    #[test]
    fn json_string_exact_bytes() {
        let s = "a\"b\\c\nd\te\r\u{1}f\u{1f} \u{7f}é中🦀\u{2028}\u{2029}/";
        assert_eq!(json_string(s, JsonNewline::Short), "\"a\\\"b\\\\c\\nd\\u0009e\\u000d\\u0001f\\u001f \u{7f}é中🦀\u{2028}\u{2029}/\"");
        assert_eq!(
            json_string(s, JsonNewline::Unicode),
            "\"a\\\"b\\\\c\\u000ad\\u0009e\\u000d\\u0001f\\u001f \u{7f}é中🦀\u{2028}\u{2029}/\""
        );
        assert_eq!(json_string("", JsonNewline::Short), "\"\"");
        let mut out = String::from("x:");
        push_json_string(&mut out, "\\\n", JsonNewline::Short);
        assert_eq!(out, "x:\"\\\\\\n\"");
    }

    /// An independent strict decoder of the escapes RFC 8259 allows (no
    /// surrogate pairs: the writer never emits `\u` above U+001F).
    fn decode(lit: &str) -> String {
        let inner = lit.strip_prefix('"').and_then(|l| l.strip_suffix('"')).expect("quoted");
        let mut out = String::new();
        let mut it = inner.chars();
        while let Some(c) = it.next() {
            assert!(c != '"' && (c as u32) >= 0x20, "unescaped {c:?} in {lit:?}");
            if c != '\\' {
                out.push(c);
                continue;
            }
            match it.next().expect("escape") {
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                '/' => out.push('/'),
                'b' => out.push('\u{8}'),
                'f' => out.push('\u{c}'),
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                'u' => {
                    let hex: String = it.by_ref().take(4).collect();
                    out.push(char::from_u32(u32::from_str_radix(&hex, 16).unwrap()).unwrap());
                }
                e => panic!("bad escape \\{e} in {lit:?}"),
            }
        }
        out
    }

    #[test]
    fn json_string_round_trips() {
        let mut every: String = (0u32..0x80).filter_map(char::from_u32).collect();
        every.push_str("é中🦀\u{2028}\u{2029}\u{fffd}\u{10ffff}");
        for s in [every.as_str(), "", "\"\\", "\n\n", "plain"] {
            for nl in [JsonNewline::Short, JsonNewline::Unicode] {
                assert_eq!(decode(&json_string(s, nl)), s, "{nl:?}");
            }
        }
    }
}
