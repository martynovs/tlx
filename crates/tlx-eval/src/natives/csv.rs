//! The `CSV` family (`tlc2/module/CSV.java`: `CSVWrite`, `CSVRecords`,
//! `CSVWriteRecord`, `CSVRead`), whose grammar is Java's `String.format` and
//! `String.split`, plus the two Java string helpers the process family also
//! passes: `java_format` (`IOExecTemplate`) and `unquoted` (`IOEnvExec`).

use tlx_value::{ops, Sym, Value};

use crate::error::{EResult, EvalError};

use super::json;

/// The elements of sequence `v`, the second argument of `op` (TLC:
/// `toTuple`).
fn seq_elems(v: &Value, op: &str) -> EResult<Vec<Value>> {
    let n =
        ops::len(v).map_err(|_| EvalError::new(format!("The second argument of {op} should be a sequence, but instead it is:\n{v}")))?;
    (1..=n).map(|i| Ok(ops::apply(v, &Value::Int(i))?)).collect()
}

fn string_arg<'v>(v: &'v Value, op: &str) -> EResult<&'v str> {
    match v {
        Value::Str(s) => Ok(s.as_str()),
        _ => Err(EvalError::new(format!("{op} expects a string, but got {v}"))),
    }
}

/// Java's `String.format(template, args)` with `String` arguments (what
/// CSVWrite and IOExecTemplate pass): `%s` / `%S` with an optional argument
/// index (`%2$s`), `-` flag and width, and `%%`, `%n`. Like Java, `%d` (or
/// any numeric conversion) of a string is an error.
pub(super) fn java_format(template: &str, args: &[String]) -> EResult<String> {
    let mut out = String::new();
    let mut next = 0;
    let mut it = template.chars().peekable();
    while let Some(c) = it.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        let mut spec = String::new();
        while let Some(&d) = it.peek() {
            if d.is_ascii_digit() || d == '$' || d == '-' {
                spec.push(d);
                it.next();
            } else {
                break;
            }
        }
        let (index, rest) = match spec.find('$') {
            Some(k) => (Some(&spec[..k]), &spec[k + 1..]),
            None => (None, spec.as_str()),
        };
        let left = rest.starts_with('-');
        let width: usize = rest.trim_start_matches('-').parse().unwrap_or(0);
        match it.next() {
            Some('%') => out.push('%'),
            Some('n') => out.push('\n'),
            Some(conv @ ('s' | 'S')) => {
                let i = match index {
                    Some(n) => n.parse::<usize>().ok().filter(|&n| n > 0).map(|n| n - 1),
                    None => {
                        next += 1;
                        Some(next - 1)
                    }
                };
                let Some(a) = i.and_then(|i| args.get(i)) else {
                    return Err(EvalError::new(format!("Format specifier '%{spec}{conv}' in \"{template}\" has no argument")));
                };
                let a = if conv == 'S' { a.to_uppercase() } else { a.clone() };
                let pad = " ".repeat(width.saturating_sub(a.chars().count()));
                if left {
                    out.push_str(&a);
                    out.push_str(&pad);
                } else {
                    out.push_str(&pad);
                    out.push_str(&a);
                }
            }
            Some(conv) => {
                return Err(EvalError::new(format!("Format specifier '%{spec}{conv}' in \"{template}\" cannot format a java.lang.String")))
            }
            None => return Err(EvalError::new(format!("Unsupported format specifier in \"{template}\""))),
        }
    }
    Ok(out)
}

/// `CSVWrite(template, val, file)` (CSV): appends the line
/// `String.format(template, val[1], val[2], ...)` to `file`. The writer
/// receives serialized bytes after value evaluation and validation.
pub fn csv_write(template: &Value, val: &Value, file: &Value, write: impl FnOnce(&str, &[u8]) -> EResult<()>) -> EResult<Value> {
    let template = string_arg(template, "CSVWrite")?;
    let args: Vec<String> = seq_elems(val, "CSVWrite")?.iter().map(|v| v.to_string()).collect();
    let line = java_format(template, &args)? + "\n";
    let path = string_arg(file, "CSVWrite")?;
    write(path, line.as_bytes())?;
    Ok(Value::Bool(true))
}

/// `CSVRecords(file)` (CSV): the number of lines of `file` (0 if it does
/// not exist).
pub fn csv_records(file: &Value) -> EResult<Value> {
    let path = string_arg(file, "CSVRecords")?;
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Value::Int(json::java_lines(&text).len() as i64)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Value::Int(0)),
        Err(e) => Err(EvalError::new(format!("CSVRecords: cannot read {path}: {e}"))),
    }
}

/// `toUnquotedString`: a string without its quotes, any other value as TLC
/// prints it.
pub(super) fn unquoted(v: &Value) -> String {
    match v {
        Value::Str(s) => s.as_str().to_string(),
        v => v.to_string(),
    }
}

/// `CSVWriteRecord(record, delim, headers, file)` (CSV.java `writeRecord`):
/// appends the field names (if `headers`) and then the field values (as
/// TLC prints them), each line joined by `delim`, in the record's
/// (normalized) field order.
pub fn csv_write_record(
    rec: &Value,
    delim: &Value,
    headers: &Value,
    file: &Value,
    write: impl FnOnce(&str, &[u8]) -> EResult<()>,
) -> EResult<Value> {
    let fields: Vec<(String, String)> = match crate::eval::force_lazy_fcn(rec.clone())? {
        Value::Record(fs) => fs.iter().map(|(k, v)| (k.as_str().to_string(), v.to_string())).collect(),
        // FcnRcdValue.toRcd: a function over strings is a record.
        Value::Fcn(pairs, _) if !pairs.is_empty() && pairs.iter().all(|(k, _)| matches!(k, Value::Str(_))) => {
            let rec = Value::record(pairs.iter().map(|(k, v)| (Sym::new(&unquoted(k)), v.clone())).collect());
            let Value::Record(fs) = rec else { unreachable!() };
            fs.iter().map(|(k, v)| (k.as_str().to_string(), v.to_string())).collect()
        }
        v => {
            return Err(EvalError::of_kind(
                tlx_runtime::ErrorKind::ArgumentError,
                format!("The first argument of CSVWriteRecord should be a record, but instead it is:\n{v}"),
            ))
        }
    };
    let delim = string_arg(delim, "CSVWriteRecord")?;
    let Value::Bool(headers) = headers else {
        return Err(EvalError::new(format!("CSVWriteRecord expects a boolean, but got {headers}")));
    };
    let path = string_arg(file, "CSVWriteRecord")?;
    let mut text = String::new();
    if *headers {
        text += &fields.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>().join(delim);
        text.push('\n');
    }
    text += &fields.iter().map(|(_, v)| v.as_str()).collect::<Vec<_>>().join(delim);
    text.push('\n');
    write(path, text.as_bytes())?;
    Ok(Value::TRUE)
}

/// Java's `String.split(regex)` for a literal delimiter (a regex without
/// metacharacters, or one escaped character `\x`): trailing empty strings
/// are removed, and a string without the delimiter is its only part.
fn java_split<'s>(s: &'s str, regex: &str) -> EResult<Vec<&'s str>> {
    const META: &str = ".$|()[]{}^?*+\\";
    let lit = match regex.strip_prefix('\\') {
        Some(rest) if rest.chars().count() == 1 && !rest.chars().all(|c| c.is_ascii_alphanumeric()) => rest,
        _ if !regex.is_empty() && !regex.chars().any(|c| META.contains(c)) => regex,
        _ => {
            return Err(EvalError::new(format!(
                "CSVRead: the delimiter regex \"{regex}\" is not supported by tlx (only literal delimiters are)"
            )))
        }
    };
    if !s.contains(lit) {
        return Ok(vec![s]);
    }
    let mut parts: Vec<&str> = s.split(lit).collect();
    while parts.last() == Some(&"") {
        parts.pop();
    }
    Ok(parts)
}

/// `CSVRead(columns, delimiter, file)` (CSV.java `read`): a sequence with a
/// record `[columns[i] |-> part i]` of strings per line of `file` (each
/// line split at `delimiter`, a Java regex); `<<>>` if the file does not
/// exist.
pub fn csv_read(columns: &Value, delim: &Value, file: &Value) -> EResult<Value> {
    let columns = crate::eval::force_seq(columns.clone())?;
    let Value::Tuple(cols) = &columns else {
        return Err(EvalError::of_kind(
            tlx_runtime::ErrorKind::ArgumentError,
            format!("The first argument of CSVRead should be a sequence, but instead it is:\n{columns}"),
        ));
    };
    let names: Vec<Sym> = cols
        .iter()
        .map(|c| match c {
            Value::Str(s) => Ok(*s),
            c => Err(EvalError::new(format!("CSVRead: the column name {c} is not a string"))),
        })
        .collect::<EResult<_>>()?;
    let delim = string_arg(delim, "CSVRead")?;
    let path = string_arg(file, "CSVRead")?;
    if !std::path::Path::new(path).exists() {
        return Ok(Value::tuple(Vec::new()));
    }
    let text = std::fs::read_to_string(path).map_err(|e| EvalError::new(format!("CSVRead: cannot read {path}: {e}")))?;
    let mut rows = Vec::new();
    for line in json::java_lines(&text) {
        let parts = java_split(line, delim)?;
        if parts.len() < names.len() {
            // (TLC's RecordValue then indexes past its values.)
            return Err(EvalError::new(format!(
                "CSVRead: the line \"{line}\" has {} fields, but there are {} columns",
                parts.len(),
                names.len()
            )));
        }
        rows.push(Value::record(names.iter().zip(parts).map(|(n, p)| (*n, Value::str(p))).collect()));
    }
    Ok(Value::tuple(rows))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn java_formats() {
        let args = ["\"a\"".to_string(), "8".to_string()];
        assert_eq!(java_format("%1$s,%2$s,%1$s", &args).unwrap(), "\"a\",8,\"a\"");
        assert_eq!(java_format("%s#%s%%", &args).unwrap(), "\"a\"#8%");
        assert_eq!(java_format("[%4s|%-4s|%S]", &["ab".into(), "c".into(), "x".into()]).unwrap(), "[  ab|c   |X]");
        assert!(java_format("%3$s", &args).is_err());
        assert!(java_format("%d", &args).is_err());
    }

    #[test]
    fn java_splits() {
        assert_eq!(java_split("a,b,,", ",").unwrap(), vec!["a", "b"]);
        assert_eq!(java_split(",a", ",").unwrap(), vec!["", "a"]);
        assert_eq!(java_split("", ",").unwrap(), vec![""]);
        assert_eq!(java_split("a|b", "\\|").unwrap(), vec!["a", "b"]);
        assert_eq!(java_split("a#!b", "#!").unwrap(), vec!["a", "b"]);
        assert!(java_split("a b", "\\s").is_err());
    }
}
