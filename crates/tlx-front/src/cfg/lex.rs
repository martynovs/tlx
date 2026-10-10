//! Tokenizer for `.cfg` files.

use super::Keyword;
use crate::ast::Span;
use crate::escape;
use crate::Diagnostic;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Tok {
    Ident(String),
    /// Decimal digits, not yet range-checked.
    Num(String),
    /// String literal with escapes decoded.
    Str(String),
    /// Punctuation: `=`, `<-`, `{`, `}`, `[`, `]`, `(`, `)`, `,`, `<<`, `>>`,
    /// `-`, `!`.
    Sym(&'static str),
    /// A TLA+ operator symbol other than the punctuation (`++`, `+`, `\o`):
    /// a name in a substitution `++ <- PlusPlus`, `Plus <- +`.
    Op(String),
    /// An invalid character, already reported by the lexer.
    Error,
    Eof,
}

#[derive(Clone, Debug)]
pub(super) struct Token {
    pub(super) tok: Tok,
    pub(super) span: Span,
}

impl Token {
    pub(super) fn describe(&self) -> String {
        match &self.tok {
            Tok::Ident(s) if Keyword::from_str(s).is_some() => format!("keyword `{s}`"),
            Tok::Ident(s) => format!("identifier `{s}`"),
            Tok::Num(s) => format!("number `{s}`"),
            Tok::Str(_) => "string literal".to_string(),
            Tok::Sym(s) => format!("`{s}`"),
            Tok::Op(s) => format!("operator `{s}`"),
            Tok::Error => "invalid character".to_string(),
            Tok::Eof => "end of file".to_string(),
        }
    }
}

struct Lexer<'a> {
    src: &'a str,
    pos: usize,
    line: u32,
    col: u32,
    diags: Vec<Diagnostic>,
}

impl<'a> Lexer<'a> {
    fn peek(&self) -> Option<char> {
        self.src[self.pos..].chars().next()
    }

    fn peek2(&self) -> Option<char> {
        let mut it = self.src[self.pos..].chars();
        it.next();
        it.next()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        if c == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(c)
    }

    fn here(&self) -> Span {
        Span { start: self.pos as u32, end: self.pos as u32, line: self.line, col: self.col, ..Span::default() }
    }

    fn finish(&self, mut span: Span) -> Span {
        span.end = self.pos as u32;
        span
    }

    fn error(&mut self, span: Span, message: impl Into<String>) {
        self.diags.push(Diagnostic { span, message: message.into() });
    }

    /// Skips whitespace and comments.
    fn skip_trivia(&mut self) {
        loop {
            match (self.peek(), self.peek2()) {
                (Some(c), _) if c.is_whitespace() => {
                    self.bump();
                }
                (Some('\\'), Some('*')) => {
                    while let Some(c) = self.peek() {
                        if c == '\n' {
                            break;
                        }
                        self.bump();
                    }
                }
                (Some('('), Some('*')) => self.skip_block_comment(),
                _ => return,
            }
        }
    }

    /// Skips a (possibly nested) `(* ... *)` comment starting at the cursor.
    fn skip_block_comment(&mut self) {
        let start = self.here();
        let mut depth = 0u32;
        loop {
            match (self.peek(), self.peek2()) {
                (Some('('), Some('*')) => {
                    self.bump();
                    self.bump();
                    depth += 1;
                }
                (Some('*'), Some(')')) => {
                    self.bump();
                    self.bump();
                    depth -= 1;
                    if depth == 0 {
                        return;
                    }
                }
                (Some(_), _) => {
                    self.bump();
                }
                (None, _) => {
                    let span = self.finish(start);
                    self.error(span, "unterminated block comment `(*`");
                    return;
                }
            }
        }
    }

    fn string(&mut self, start: Span) -> Tok {
        self.bump(); // opening quote
        let mut out = String::new();
        loop {
            match self.peek() {
                Some('"') => {
                    self.bump();
                    return Tok::Str(out);
                }
                None | Some('\n') | Some('\r') => {
                    let span = self.finish(start);
                    self.error(span, "unterminated string literal");
                    return Tok::Str(out);
                }
                Some('\\') => {
                    let esc_start = self.here();
                    self.bump();
                    match self.peek().and_then(escape::unescape) {
                        Some(c) => {
                            self.bump();
                            out.push(c);
                        }
                        None => {
                            let span = self.finish(esc_start);
                            self.error(span, "invalid escape sequence in string literal (expected one of \\\" \\\\ \\n \\t \\r \\f)");
                        }
                    }
                }
                Some(c) => {
                    self.bump();
                    out.push(c);
                }
            }
        }
    }

    fn next_token(&mut self) -> Token {
        self.skip_trivia();
        let start = self.here();
        let Some(c) = self.peek() else {
            return Token { tok: Tok::Eof, span: start };
        };
        let tok = if c.is_ascii_alphanumeric() || c == '_' {
            let begin = self.pos;
            while matches!(self.peek(), Some(c) if c.is_ascii_alphanumeric() || c == '_') {
                self.bump();
            }
            let word = &self.src[begin..self.pos];
            if word.bytes().all(|b| b.is_ascii_digit()) {
                Tok::Num(word.to_string())
            } else {
                Tok::Ident(word.to_string())
            }
        } else if c == '"' {
            self.string(start)
        } else if let Some(op) = self.operator() {
            for _ in 0..op.chars().count() {
                self.bump();
            }
            Tok::Op(op)
        } else {
            let two: &'static str = match (c, self.peek2()) {
                ('<', Some('-')) => "<-",
                ('<', Some('<')) => "<<",
                ('>', Some('>')) => ">>",
                _ => "",
            };
            if !two.is_empty() {
                self.bump();
                self.bump();
                Tok::Sym(two)
            } else {
                let one: &'static str = match c {
                    '=' => "=",
                    '{' => "{",
                    '}' => "}",
                    '[' => "[",
                    ']' => "]",
                    '(' => "(",
                    ')' => ")",
                    ',' => ",",
                    '-' => "-",
                    '!' => "!",
                    _ => "",
                };
                self.bump();
                if one.is_empty() {
                    let span = self.finish(start);
                    self.error(span, format!("unexpected character `{}`", c.escape_debug()));
                    Tok::Error
                } else {
                    Tok::Sym(one)
                }
            }
        };
        let span = self.finish(start);
        Token { tok, span }
    }
}

impl Lexer<'_> {
    /// The TLA+ operator symbol at the cursor (the longest spelling), unless
    /// it is punctuation (`=`, `<-`, `<<`, `>>`, `-`, `!`) or no longer than
    /// the punctuation there (`<-`, not `<`).
    fn operator(&self) -> Option<String> {
        let rest = &self.src[self.pos..];
        let b = rest.as_bytes();
        let op = if b.first() == Some(&b'\\') && b.get(1).is_some_and(|c| c.is_ascii_alphabetic()) {
            // `\o`, `\prec`, ...
            let n = 1 + b[1..].iter().take_while(|c| c.is_ascii_alphabetic()).count();
            &rest[..n]
        } else {
            let extra: &[&str] = &["..", "...", "@@", "!!", "(+)", "(-)", "(.)", "(/)", "(\\X)"];
            crate::parse::PATH_SYMBOLS.iter().chain(extra).copied().filter(|s| rest.starts_with(s)).max_by_key(|s| s.len())?
        };
        let punct = ["<-", "<<", ">>", "=", "-", "!"].into_iter().filter(|s| rest.starts_with(s)).map(str::len).max().unwrap_or(0);
        (op.len() > punct).then(|| op.to_string())
    }
}

pub(super) fn lex(src: &str) -> (Vec<Token>, Vec<Diagnostic>) {
    // Skip a leading byte-order mark; byte offsets stay relative to `src`.
    let pos = if src.starts_with('\u{feff}') { '\u{feff}'.len_utf8() } else { 0 };
    let mut lexer = Lexer { src, pos, line: 1, col: 1, diags: Vec::new() };
    let mut tokens = Vec::new();
    loop {
        let t = lexer.next_token();
        let eof = t.tok == Tok::Eof;
        tokens.push(t);
        if eof {
            break;
        }
    }
    (tokens, lexer.diags)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The decoded string token of `src` and every diagnostic, as rendered.
    fn string_of(src: &str) -> (String, Vec<String>) {
        let (tokens, diags) = lex(src);
        let s = tokens.iter().find_map(|t| match &t.tok {
            Tok::Str(s) => Some(s.clone()),
            _ => None,
        });
        (s.expect("no string token"), diags.iter().map(Diagnostic::to_string).collect())
    }

    const INVALID_ESCAPE: &str = r#"invalid escape sequence in string literal (expected one of \" \\ \n \t \r \f)"#;

    #[test]
    fn decodes_the_six_escapes() {
        let (s, diags) = string_of(r#"CONSTANT Name = "a\"b\\c\nd\te\rf\fg""#);
        assert_eq!(s, "a\"b\\c\nd\te\rf\u{c}g");
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn unknown_escape_keeps_the_character_and_reports_once() {
        let (s, diags) = string_of(r#"CONSTANT Name = "a\qb""#);
        assert_eq!(s, "aqb");
        assert_eq!(diags, [format!("1:19: {INVALID_ESCAPE}")]);
    }

    #[test]
    fn trailing_backslash_before_eof_reports_the_escape_then_the_open_string() {
        let (s, diags) = string_of("CONSTANT Name = \"abc\\");
        assert_eq!(s, "abc");
        assert_eq!(diags, [format!("1:21: {INVALID_ESCAPE}"), "1:17: unterminated string literal".to_string()]);
    }
}
