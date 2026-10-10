//! CST (tree-sitter-tlaplus) -> surface AST lowering.
//!
//! The work is split into:
//! - `ops`: operator-symbol node kind -> canonical operator name;
//! - `lower`: the actual CST walk producing `ast` values;
//! - `sexp`: a compact s-expression printer for the AST (tests / debugging);
//! - `syntax`: diagnostics for tree-sitter ERROR / MISSING nodes.

mod lower;
mod ops;
pub mod sexp;
mod syntax;

use crate::ast::{FileId, Module, Span};
use crate::Diagnostic;
use tree_sitter::{Node, Parser, Tree};

/// The allocation functions of the parser's C library (tree-sitter).
pub use tree_sitter::Allocator;

/// Have the parser's C library allocate with `a` (by default it uses the C
/// library's `malloc`): with the program's allocator, the memory of the
/// syntax trees, freed after lowering, serves the rest of the run instead
/// of staying with a separate heap (on Linux, glibc keeps it in the arena
/// of the parsing thread: about 3 MiB).
///
/// # Safety
/// The four functions must be one allocator's (as `malloc`, `calloc`,
/// `realloc` and `free`), never return null for a non-zero size, and
/// align as `malloc` does; the call must come before any parse, while no
/// other thread parses.
pub unsafe fn set_allocator(a: Allocator) {
    // SAFETY: the caller's contract is tree-sitter's.
    unsafe { tree_sitter::set_allocator(Some(a)) }
}

/// Run tree-sitter-tlaplus on `src` and return the raw CST.
pub fn parse_cst(src: &str) -> Option<Tree> {
    let mut parser = Parser::new();
    parser.set_language(&tree_sitter_tlaplus::LANGUAGE.into()).expect("tree-sitter-tlaplus language version mismatch");
    parser.parse(src, None)
}

/// Parse the text of a `.tla` file and lower its first top-level module.
/// Text outside the module (and PlusCal in comments) is ignored.
pub fn parse_module(src: &str, file: FileId) -> Result<Module, Vec<Diagnostic>> {
    parse_modules(src, file).map(|mut ms| ms.swap_remove(0))
}

/// Parse the text of a `.tla` file and lower all of its top-level modules
/// (a file may contain several, e.g. a spec followed by a helper module).
pub fn parse_modules(src: &str, file: FileId) -> Result<Vec<Module>, Vec<Diagnostic>> {
    // Parse a preprocessed copy (tabs expanded, operator symbols ending
    // subexpression paths replaced); spans are mapped back to `src`.
    let (expanded, map) = preprocess(src);
    let src: &str = &expanded;
    // PlusCal is irrelevant to us, but the grammar parses `--algorithm` blocks
    // inside comments, and a malformed algorithm can derail the parse of the
    // whole module. So first parse with the markers neutralized (same byte
    // length, so all offsets stay valid; lowering reads text from the original
    // `src`). If that fails, retry on the original text, which differs only
    // when a comment relies on PlusCal-aware lexing (e.g. a `"*)"` string).
    let fields = neutralize_field_names(src);
    let neutral = neutralize_pluscal(&fields);
    let mut attempt = check_syntax(&neutral, file, &map);
    if attempt.is_err() && neutral != fields {
        if let ok @ Ok(_) = check_syntax(&fields, file, &map) {
            attempt = ok;
        }
    }
    let tree = attempt?;
    let root = tree.root_node();
    let mut c = root.walk();
    let modules: Vec<_> = root.named_children(&mut c).filter(|n| n.kind() == "module").collect();
    if modules.is_empty() {
        return Err(vec![file_start_diag(file, "no `---- MODULE Name ----` found")]);
    }

    let mut cx = lower::Lower::new(src, file, &map);
    let ms: Vec<Module> = modules.into_iter().map(|m| cx.module(m)).collect();
    if cx.diags.is_empty() {
        Ok(ms)
    } else {
        Err(cx.diags)
    }
}

/// Parse `text` and return the tree if its modules (or the whole file,
/// when no module is recognized) have no ERROR / MISSING nodes.
fn check_syntax(text: &str, file: FileId, map: &SrcMap) -> Result<Tree, Vec<Diagnostic>> {
    let Some(tree) = parse_cst(text) else {
        return Err(vec![file_start_diag(file, "tree-sitter failed to parse the file")]);
    };
    let mut diags = Vec::new();
    {
        let root = tree.root_node();
        let mut c = root.walk();
        let last = root.named_children(&mut c).filter(|n| n.kind() == "module").last();
        let limit = last.map_or(usize::MAX, |m| m.end_byte());
        syntax::collect_syntax_errors(root, text, file, map, limit, &mut diags);
    }
    if diags.is_empty() {
        Ok(tree)
    } else {
        Err(diags)
    }
}

/// Replace `--algorithm` / `--fair` by `--xlgorithm` / `--xair`. Outside
/// comments these spellings can only occur as `--` followed by an identifier,
/// which tokenizes identically either way.
fn neutralize_pluscal(src: &str) -> std::borrow::Cow<'_, str> {
    if !src.contains("--algorithm") && !src.contains("--fair") {
        return std::borrow::Cow::Borrowed(src);
    }
    let mut b = src.as_bytes().to_vec();
    let mut i = 0;
    while i + 2 < b.len() {
        if b[i] == b'-' && b[i + 1] == b'-' && (b[i + 2..].starts_with(b"algorithm") || b[i + 2..].starts_with(b"fair")) {
            b[i + 2] = b'x';
            i += 3;
        } else {
            i += 1;
        }
    }
    std::borrow::Cow::Owned(String::from_utf8(b).expect("ASCII-only substitution keeps UTF-8 valid"))
}

/// The grammar lexes `Nat`, `Int` and `Real` as keywords, but in TLA+ they
/// are ordinary names (defined in Naturals, Integers and Reals), so they can
/// also be record fields: `[Nat |-> 1]`, `[Nat : S]`, `r.Nat` (SANY accepts
/// them, with a lint warning). Respell them in those positions as an
/// identifier of the same length; lowering reads names from the original
/// text, so the spelling never reaches the AST.
fn neutralize_field_names(src: &str) -> std::borrow::Cow<'_, str> {
    const WORDS: [&str; 3] = ["Nat", "Int", "Real"];
    let b = src.as_bytes();
    let is_word = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    let skip_ws_back = |mut j: usize| {
        while j > 0 && b[j - 1].is_ascii_whitespace() {
            j -= 1;
        }
        j
    };
    let skip_ws = |mut j: usize| {
        while j < b.len() && b[j].is_ascii_whitespace() {
            j += 1;
        }
        j
    };
    let mut hits = Vec::new();
    for w in WORDS {
        for (i, _) in src.match_indices(w) {
            let end = i + w.len();
            if (i > 0 && is_word(b[i - 1])) || b.get(end).is_some_and(|&c| is_word(c)) {
                continue;
            }
            let j = skip_ws_back(i);
            let field = match j.checked_sub(1).map(|p| b[p]) {
                // `r.Nat` (not `1..Nat`)
                Some(b'.') => j < 2 || b[j - 2] != b'.',
                // `[Nat |-> e, ...]` / `[a : S, Nat : T]`
                Some(b'[' | b',') => {
                    let rest = &b[skip_ws(end)..];
                    rest.starts_with(b"|->") || (rest.starts_with(b":") && !matches!(rest.get(1), Some(b':' | b'=' | b'>')))
                }
                _ => false,
            };
            if field {
                hits.push(end - 1);
            }
        }
    }
    if hits.is_empty() {
        return std::borrow::Cow::Borrowed(src);
    }
    let mut out = b.to_vec();
    for h in hits {
        out[h] = b'_';
    }
    std::borrow::Cow::Owned(String::from_utf8(out).expect("ASCII-only substitution keeps UTF-8 valid"))
}

/// A diagnostic located at the start of the file.
fn file_start_diag(file: FileId, message: &str) -> Diagnostic {
    Diagnostic { span: Span { file, start: 0, end: 0, line: 1, col: 1 }, message: message.into() }
}

/// Maps byte offsets of the preprocessed text (see `preprocess`) back to the
/// original text, and records the operator symbols that were replaced by
/// identifiers.
#[derive(Default)]
pub(crate) struct SrcMap {
    /// For each expanded tab, sorted: (offset in the expanded text, number
    /// of spaces it became, offset of the tab in the original text).
    tabs: Vec<(usize, usize, usize)>,
    /// (offset in the preprocessed text, canonical operator name) of each
    /// operator symbol replaced by an identifier, sorted.
    ops: Vec<(usize, &'static str)>,
}

impl SrcMap {
    fn orig(&self, p: usize) -> usize {
        let i = self.tabs.partition_point(|t| t.0 < p);
        if i == 0 {
            return p;
        }
        let (e, w, o) = self.tabs[i - 1];
        if p < e + w {
            o + 1
        } else {
            p - (e + w) + o + 1
        }
    }

    /// The operator name of an identifier at `p` that stands for a replaced
    /// operator symbol.
    pub(crate) fn op_at(&self, p: usize) -> Option<&'static str> {
        self.ops.binary_search_by_key(&p, |o| o.0).ok().map(|i| self.ops[i].1)
    }
}

/// ASCII spellings of the operator symbols that can end a subexpression
/// path (`Def!++`). Not included: those clashing with selectors or other
/// uses of `!` (`<<`, `>>`, `@@`, `..`, `...`, `!!`, `(+)`-style forms).
pub(crate) const PATH_SYMBOLS: &[&str] = &[
    "=>",
    "-+->",
    "\\equiv",
    "<=>",
    "~>",
    "/\\",
    "\\land",
    "\\/",
    "\\lor",
    ":=",
    "::=",
    "=",
    "/=",
    "#",
    "<",
    ">",
    "<=",
    "=<",
    "\\leq",
    ">=",
    "\\geq",
    "\\approx",
    "|-",
    "|=",
    "-|",
    "=|",
    "\\asymp",
    "\\cong",
    "\\doteq",
    "\\gg",
    "\\ll",
    "\\in",
    "\\notin",
    "\\prec",
    "\\succ",
    "\\preceq",
    "\\succeq",
    "\\propto",
    "\\sim",
    "\\simeq",
    "\\sqsubset",
    "\\sqsupset",
    "\\sqsubseteq",
    "\\sqsupseteq",
    "\\subset",
    "\\supset",
    "\\subseteq",
    "\\supseteq",
    ":>",
    "<:",
    "\\",
    "\\cap",
    "\\intersect",
    "\\cup",
    "\\union",
    "+",
    "++",
    "\\oplus",
    "\\ominus",
    "%",
    "%%",
    "|",
    "||",
    "-",
    "--",
    "&",
    "&&",
    "\\odot",
    "\\oslash",
    "\\otimes",
    "*",
    "**",
    "/",
    "//",
    "\\bigcirc",
    "\\bullet",
    "\\div",
    "\\o",
    "\\circ",
    "\\star",
    "##",
    "$",
    "$$",
    "??",
    "\\sqcap",
    "\\sqcup",
    "\\uplus",
    "\\X",
    "\\times",
    "\\wr",
    "\\cdot",
    "^",
    "^^",
    "~",
    "\\lnot",
    "\\neg",
    "-.",
    "[]",
    "<>",
    "^+",
    "^*",
    "^#",
];

/// The operator symbol (one of `PATH_SYMBOLS`) that ends a subexpression
/// path at byte `i` of `b` (just after a `!`), if any: the longest match,
/// not followed by `(` or `!` (those forms are handled by the grammar).
fn path_symbol_at(b: &[u8], i: usize) -> Option<&'static str> {
    if b[i..].starts_with(b"<<") || b[i..].starts_with(b">>") {
        return None;
    }
    let sym = PATH_SYMBOLS
        .iter()
        .filter(|s| b[i..].starts_with(s.as_bytes()))
        .filter(|s| {
            // `\in` must not match a prefix of `\inter`.
            let word = s.len() > 1 && s.as_bytes()[1].is_ascii_alphabetic();
            !word || !b.get(i + s.len()).is_some_and(|c| c.is_ascii_alphanumeric())
        })
        .max_by_key(|s| s.len())?;
    let next = b[i + sym.len()..].iter().find(|c| !c.is_ascii_whitespace());
    if matches!(next, Some(b'(' | b'!')) {
        return None;
    }
    Some(sym)
}

/// The canonical operator name of an operator symbol, as the grammar
/// classifies it (by parsing it as an operator argument; with spaces, as
/// `(+)` would be `\oplus`).
pub(crate) fn canonical_symbol(sym: &str) -> Option<&'static str> {
    let text = format!("---- MODULE M ----\nX == F( {sym} )\n====\n");
    let tree = parse_cst(&text)?;
    let at = text.find(&format!("( {sym} )"))? + 2;
    let mut n = tree.root_node().descendant_for_byte_range(at, at + sym.len())?;
    loop {
        if let Some(name) = ops::any_name(n.kind()) {
            return Some(name);
        }
        n = n.parent()?;
    }
}

/// Rewrite `src` into what tree-sitter-tlaplus parses like SANY:
/// - every tab outside string literals becomes spaces up to the next
///   multiple-of-8 column (junction lists are aligned by column; SANY counts
///   a tab that way, tree-sitter as one column; columns count characters);
/// - an operator symbol ending a subexpression path (`Def!++`, which the
///   grammar rejects) becomes an identifier of the same length, recorded in
///   the map.
fn preprocess(src: &str) -> (std::borrow::Cow<'_, str>, SrcMap) {
    let b = src.as_bytes();
    let path_syms: Vec<usize> = (2..b.len())
        .filter(|&i| {
            b[i - 1] == b'!'
                && !matches!(b[i - 2], b' ' | b'\t' | b'\n' | b'\r' | b',' | b'[' | b'(' | b'!')
                && path_symbol_at(b, i).is_some()
        })
        .collect();
    if !src.contains('\t') && path_syms.is_empty() {
        return (std::borrow::Cow::Borrowed(src), SrcMap::default());
    }
    // String literals (and, for symbols, comments) stay as they are.
    let mut keep = Vec::new();
    if let Some(tree) = parse_cst(src) {
        let mut stack = vec![tree.root_node()];
        while let Some(n) = stack.pop() {
            if matches!(n.kind(), "string" | "comment" | "block_comment") {
                keep.push((n.byte_range(), n.kind() == "string"));
                continue;
            }
            let mut c = n.walk();
            stack.extend(n.children(&mut c));
        }
    }
    let kept = |o: usize, strings_only: bool| keep.iter().any(|(r, is_str)| (*is_str || !strings_only) && r.contains(&o));
    let mut out = String::with_capacity(src.len() + 64);
    let mut map = SrcMap::default();
    let mut col = 0usize;
    let mut syms = path_syms.into_iter().peekable();
    let mut skip_to = 0;
    for (o, ch) in src.char_indices() {
        if o < skip_to {
            continue;
        }
        if syms.peek() == Some(&o) {
            syms.next();
            let sym = path_symbol_at(b, o).expect("checked above");
            if let (false, Some(name)) = (kept(o, false), canonical_symbol(sym)) {
                map.ops.push((out.len(), name));
                out.push('X');
                out.extend(std::iter::repeat_n('_', sym.len() - 1));
                col += sym.len();
                skip_to = o + sym.len();
                continue;
            }
        }
        match ch {
            '\t' if !kept(o, true) => {
                let w = 8 - col % 8;
                map.tabs.push((out.len(), w, o));
                out.extend(std::iter::repeat_n(' ', w));
                col += w;
            }
            '\n' => {
                out.push(ch);
                col = 0;
            }
            _ => {
                out.push(ch);
                col += 1;
            }
        }
    }
    (std::borrow::Cow::Owned(out), map)
}

pub(crate) fn node_span(n: Node, file: FileId, map: &SrcMap) -> Span {
    let p = n.start_position();
    let start = map.orig(n.start_byte());
    let line_start = map.orig(n.start_byte() - p.column);
    Span { file, start: start as u32, end: map.orig(n.end_byte()) as u32, line: p.row as u32 + 1, col: (start - line_start) as u32 + 1 }
}

/// Token texts of a `.tla` file in source order, for reproducing TLC's string
/// intern order (tlc-semantics.md §1.3): `tokens` are all non-string,
/// non-comment tokens (identifiers, keywords, operators); `strings` are the
/// decoded contents of string literals.
pub fn token_texts(src: &str) -> (Vec<String>, Vec<String>) {
    let mut tokens = Vec::new();
    let mut strings = Vec::new();
    let fields = neutralize_field_names(src);
    let neutral = neutralize_pluscal(&fields);
    let Some(tree) = parse_cst(&neutral) else { return (tokens, strings) };
    let mut stack = vec![tree.root_node()];
    while let Some(n) = stack.pop() {
        match n.kind() {
            "comment" | "block_comment" | "extramodular_text" => continue,
            "string" => {
                if let Some(t) = src.get(n.start_byte()..n.end_byte()) {
                    strings.push(lower::decode_string(t).0);
                }
                continue;
            }
            _ => {}
        }
        if n.child_count() == 0 {
            if let Some(t) = src.get(n.start_byte()..n.end_byte()) {
                if !t.is_empty() {
                    tokens.push(t.to_string());
                }
            }
            continue;
        }
        let mut c = n.walk();
        let kids: Vec<_> = n.children(&mut c).collect();
        stack.extend(kids.into_iter().rev());
    }
    (tokens, strings)
}
