//! The `.tla` and `.cfg` string readers share one six-escape mapping while
//! keeping their own loops, recovery and diagnostics. These vectors pin the
//! decoded values and the exact diagnostic wording of each path.

use tlx_front::ast::{ExprKind, FileId, Unit};
use tlx_front::cfg::{parse_cfg, CfgValue, ConstBinding};
use tlx_front::parse::parse_module;
use tlx_front::Diagnostic;

/// A literal with all six escapes and one ordinary character after each.
const SIX: &str = r#""a\"b\\c\nd\te\rf\fg""#;
const SIX_DECODED: &str = "a\"b\\c\nd\te\rf\u{c}g";

/// A literal whose only escape is the unsupported `q`.
const UNKNOWN: &str = r#""a\qb""#;

/// Recovered lexically by `.cfg`: the backslash is dropped, `q` is kept.
const CFG_INVALID_ESCAPE: &str = r#"1:19: invalid escape sequence in string literal (expected one of \" \\ \n \t \r \f)"#;

/// Reported by `.tla` lowering, which names the offending escape.
const TLA_UNKNOWN_ESCAPE: &str = r#"2:6: unknown string escape `\q`"#;

/// Decoded contents of the single string constant in a `.cfg` source.
fn cfg_string(src: &str) -> String {
    match parse_cfg(src) {
        Ok(cfg) => match &cfg.constants[..] {
            [ConstBinding::Value { value: CfgValue::Str(s), .. }] => s.clone(),
            other => panic!("unexpected cfg constants: {other:?}"),
        },
        Err(d) => panic!("cfg parse failed: {d:?}"),
    }
}

fn cfg_errors(src: &str) -> Vec<String> {
    match parse_cfg(src) {
        Ok(cfg) => panic!("expected cfg errors, got {cfg:?}"),
        Err(d) => d.iter().map(Diagnostic::to_string).collect(),
    }
}

/// Decoded contents of the `X == <literal>` definition of a `.tla` module.
fn tla_string(literal: &str) -> String {
    let src = format!("---- MODULE T ----\nX == {literal}\n====\n");
    match parse_module(&src, FileId(0)) {
        Ok(m) => match &m.units[..] {
            [Unit::OpDef(d)] => match &*d.body.kind {
                ExprKind::Str(s) => s.to_string(),
                k => panic!("unexpected body: {k:?}"),
            },
            other => panic!("unexpected units: {other:?}"),
        },
        Err(d) => panic!("tla parse failed: {d:?}"),
    }
}

fn tla_errors(literal: &str) -> Vec<String> {
    let src = format!("---- MODULE T ----\nX == {literal}\n====\n");
    match parse_module(&src, FileId(0)) {
        Ok(m) => panic!("expected tla errors, got {m:?}"),
        Err(d) => d.iter().map(Diagnostic::to_string).collect(),
    }
}

#[test]
fn both_readers_decode_the_six_escapes_identically() {
    assert_eq!(cfg_string(&format!("CONSTANT Name = {SIX}")), SIX_DECODED);
    assert_eq!(tla_string(SIX), SIX_DECODED);
}

#[test]
fn each_reader_reports_the_unknown_escape_its_own_way() {
    assert_eq!(cfg_errors(&format!("CONSTANT Name = {UNKNOWN}")), [CFG_INVALID_ESCAPE.to_string()]);
    assert_eq!(tla_errors(UNKNOWN), [TLA_UNKNOWN_ESCAPE.to_string()]);
}
