//! Native implementations of CommunityModules operators whose TLA+
//! definitions cannot be evaluated directly (they quantify over infinite
//! sets such as `Seq(S)` or `[STRING -> STRING]`), like TLC's Java
//! overrides. Each follows the module's TLA+ definition.
//!
//! The operator families with their own helpers live in child modules
//! (CODE-03): `csv` (`CSVRead`/`CSVWrite`/`CSVWriteRecord`), `graph`
//! (`SimplePath`, `AreConnectedIn`, `IsStronglyConnected`,
//! `ConnectedComponents`) and `process` (`IOExec`/`IOEnvExec` and their
//! templates), next to `json` and `stats`. Their public paths are re-exported
//! here, so the extraction moves no API.

use tlx_value::{ops, Sym, Value};

use crate::error::{EResult, EvalError};

mod csv;
mod graph;
pub mod json;
mod process;
pub mod stats;

pub use csv::{csv_read, csv_records, csv_write, csv_write_record};
pub use graph::{are_connected_in, connected_components, is_strongly_connected, simple_path};
pub(crate) use process::PreparedCommand;
pub use process::{io_env_exec, io_env_exec_template, io_exec, io_exec_template};

/// `IOEnv` (IOUtils): the process environment as a record.
pub fn io_env() -> Value {
    let mut fields: Vec<(Sym, Value)> = std::env::vars().map(|(k, v)| (Sym::new(&k), Value::str(&v))).collect();
    fields.sort_by(|a, b| a.0.cmp(&b.0));
    fields.dedup_by(|a, b| a.0 == b.0);
    Value::record(fields)
}

/// `atoi(str)` (IOUtils; the module defines it as `CHOOSE i \in Int :
/// ToString(i) = str`).
pub fn atoi(s: &Value) -> EResult<Value> {
    // IOUtils.java `atoi`: Java's Integer.parseInt (an optional sign, then
    // decimal digits, within 32 bits), else an argument error.
    if let Value::Str(t) = s {
        let t = t.as_str();
        let digits = t.strip_prefix(['+', '-']).unwrap_or(t);
        if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
            if let Ok(i) = t.parse::<i32>() {
                return Ok(Value::Int(i64::from(i)));
            }
        }
    }
    Err(EvalError::of_kind(
        tlx_runtime::ErrorKind::ArgumentError,
        format!("The argument of atoi should be a string, but instead it is:\n{s}"),
    ))
}

/// `IsInjective(f)` (Functions.java): no two arguments of `f` have equal
/// values. As TLC, a set of records `[a : S, b : T]` (an easy slip for
/// `[a |-> S, b |-> T]`) counts as the function from its fields to their
/// sets (TLC's `SetOfRcdsValue.values`).
pub fn is_injective(f: &Value) -> EResult<Value> {
    let values: Vec<Value> = match f {
        Value::Tuple(_) | Value::Record(_) | Value::Fcn(..) | Value::LazyFcn(_) => {
            let f = crate::eval::force_lazy_fcn(f.clone())?;
            ops::enumerate_normalized(&ops::domain(&f)?)?.iter().map(|x| ops::apply(&f, x)).collect::<tlx_value::Result<_>>()?
        }
        Value::LazySet(s) if matches!(&**s, tlx_value::LazySet::RecordSet(_)) => {
            let tlx_value::LazySet::RecordSet(fields) = &**s else { unreachable!() };
            fields.iter().map(|(_, v)| v.clone()).collect()
        }
        _ => {
            return Err(EvalError::of_kind(
                tlx_runtime::ErrorKind::ArgumentError,
                format!("The argument of IsInjective should be a function, but instead it is:\n{f}"),
            ))
        }
    };
    for (i, x) in values.iter().enumerate() {
        for y in &values[i + 1..] {
            if tlx_value::order::eq_tlc(x, y)? {
                return Ok(Value::FALSE);
            }
        }
    }
    Ok(Value::TRUE)
}

/// `AllSubSeqs(s)` (SequencesExt.java): every subsequence of `s`, each
/// keeping the elements in index order (the TLA+ definition folds a
/// function over `SUBSET DOMAIN s`, whose CHOOSE order can reverse them).
pub fn all_sub_seqs(s: &Value) -> EResult<Value> {
    let s = crate::eval::force_seq(s.clone())?;
    let Value::Tuple(elems) = &s else {
        return Err(EvalError::new(format!("The argument of AllSubSeqs should be a sequence, but instead it is:\n{s}")));
    };
    if elems.len() >= 31 {
        // TLC allocates an int-sized array of 2^n subsequences and fails too.
        return Err(EvalError::new(format!("AllSubSeqs cannot enumerate 2^{} subsequences", elems.len())));
    }
    let mut out = Vec::with_capacity(1usize << elems.len());
    for mask in 0u32..(1u32 << elems.len()) {
        let mut sub = Vec::new();
        for (i, e) in elems.iter().enumerate() {
            if mask & (1 << i) != 0 {
                sub.push(e.clone());
            }
        }
        out.push(Value::tuple(sub));
    }
    Ok(Value::set_from(out)?)
}
