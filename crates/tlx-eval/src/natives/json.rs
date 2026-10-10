//! `Json!ToJson` / `ToJsonArray` / `ToJsonObject` (tlc2/module/Json.java).
//!
//! TLC converts a value to a Gson `JsonElement` and returns its `toString`:
//! no whitespace, record fields / function domains in their stored order,
//! sets as arrays in normalized order, and Gson's string escaping (control
//! characters and `"`/`\` escaped, **no** HTML escaping — `JsonWriter`'s
//! `htmlSafe` is off by default).

use tlx_value::{ops, FcnKind, Sym, Value};

use crate::error::{EResult, EvalError};

/// A JSON tree, rendered exactly like Gson's `JsonElement.toString()`.
enum J {
    Arr(Vec<J>),
    Obj(Vec<(String, J)>),
    Str(String),
    Num(i64),
    Bool(bool),
}

impl J {
    fn render(&self, out: &mut String) {
        match self {
            J::Arr(items) => {
                out.push('[');
                for (i, x) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    x.render(out);
                }
                out.push(']');
            }
            J::Obj(fields) => {
                out.push('{');
                for (i, (k, v)) in fields.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    escape(k, out);
                    out.push(':');
                    v.render(out);
                }
                out.push('}');
            }
            J::Str(s) => escape(s, out),
            J::Num(n) => out.push_str(&n.to_string()),
            J::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        }
    }
}

/// Gson's `JsonWriter.string`: a quoted, escaped JSON string.
fn escape(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\u{0008}' => out.push_str("\\b"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\u{000c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 || c == '\u{2028}' || c == '\u{2029}' => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// `getNode(value)`: the JSON value of `ToJson`.
fn node(v: &Value) -> EResult<J> {
    Ok(match v {
        // RecordValue / FcnRcdValue with string keys: an object.
        Value::Record(fields) => J::Obj(fields.iter().map(|(k, v)| Ok((k.as_str().to_string(), node(v)?))).collect::<EResult<_>>()?),
        // TupleValue: a JSON array.
        Value::Tuple(elems) => J::Arr(elems.iter().map(node).collect::<EResult<_>>()?),
        Value::Str(s) => J::Str(s.as_str().to_string()),
        Value::ModelValue(s) => J::Str(s.as_str().to_string()),
        Value::Int(n) => J::Num(*n),
        Value::Bool(b) => J::Bool(*b),
        // FcnRcdValue: an array if it is a sequence (also the empty one),
        // otherwise an object keyed by the domain's rendering (a string key
        // is used as-is, anything else via `toString`).
        Value::Fcn(pairs, _) => match FcnKind::of(pairs) {
            FcnKind::Tuple => J::Arr(pairs.iter().map(|(_, v)| node(v)).collect::<EResult<_>>()?),
            _ => J::Obj(pairs.iter().map(|(k, v)| Ok((key_string(k), node(v)?))).collect::<EResult<_>>()?),
        },
        // SetEnumValue / EnumerableValue: an array in normalized order.
        Value::Set(_) | Value::Interval(..) | Value::LazySet(_) => {
            J::Arr(ops::enumerate_normalized(v)?.iter().map(node).collect::<EResult<_>>()?)
        }
        // FcnLambdaValue: materialize, then convert.
        Value::LazyFcn(_) => node(&crate::eval::force_lazy_fcn(v.clone())?)?,
    })
}

/// The key TLC uses for a non-string function domain element (`toString`).
fn key_string(k: &Value) -> String {
    match k {
        Value::Str(s) => s.as_str().to_string(),
        v => v.to_string(),
    }
}

fn unsupported(what: &str, v: &Value) -> EvalError {
    EvalError::new(format!("Cannot convert value to a JSON {what}: unsupported value type {v}"))
}

fn finish(j: J) -> Value {
    let mut s = String::new();
    j.render(&mut s);
    Value::str(&s)
}

/// `ToJson(value)`.
pub fn to_json(v: &Value) -> EResult<Value> {
    Ok(finish(node(v)?))
}

/// `ToJsonArray(value)`: a tuple or set becomes an array, a function that is
/// not a sequence becomes an object (Json.java `getArrayNode`).
pub fn to_json_array(v: &Value) -> EResult<Value> {
    let v = match v {
        Value::LazyFcn(_) => crate::eval::force_lazy_fcn(v.clone())?,
        _ => v.clone(),
    };
    let j = match &v {
        Value::Tuple(elems) => J::Arr(elems.iter().map(node).collect::<EResult<_>>()?),
        // A FcnRcdValue that is not a valid sequence falls back to an object.
        Value::Fcn(..) => node(&v)?,
        Value::Set(_) | Value::Interval(..) | Value::LazySet(_) => {
            J::Arr(ops::enumerate_normalized(&v)?.iter().map(node).collect::<EResult<_>>()?)
        }
        _ => return Err(unsupported("array", &v)),
    };
    Ok(finish(j))
}

/// `ToJsonObject(value)`: a record or a non-sequence function becomes an
/// object, a tuple an object keyed by `"0"`, `"1"`, ... (Json.java
/// `getObjectNode`; a *function* with domain `1..n` is an array there, but
/// tlx represents it as a tuple).
pub fn to_json_object(v: &Value) -> EResult<Value> {
    let v = match v {
        Value::LazyFcn(_) => crate::eval::force_lazy_fcn(v.clone())?,
        _ => v.clone(),
    };
    let j = match &v {
        Value::Record(fields) => J::Obj(fields.iter().map(|(k, val)| Ok((k.as_str().to_string(), node(val)?))).collect::<EResult<_>>()?),
        Value::Tuple(elems) => J::Obj(elems.iter().enumerate().map(|(i, val)| Ok((i.to_string(), node(val)?))).collect::<EResult<_>>()?),
        Value::Fcn(..) => match node(&v)? {
            J::Arr(items) => J::Arr(items),
            J::Obj(fields) => J::Obj(fields),
            other => other,
        },
        _ => return Err(unsupported("object", &v)),
    };
    Ok(finish(j))
}

/// The string argument `which` of `op` (a file name).
fn path_arg<'v>(v: &'v Value, op: &str, which: &str) -> EResult<&'v str> {
    match v {
        Value::Str(s) => Ok(s.as_str()),
        _ => Err(EvalError::of_kind(
            tlx_runtime::ErrorKind::ArgumentError,
            format!("The {which} argument of {op} should be a string, but instead it is:\n{v}"),
        )),
    }
}

fn read_file(path: &str, op: &str) -> EResult<String> {
    std::fs::read_to_string(path).map_err(|e| EvalError::new(format!("{op}: cannot read {path}: {e}")))
}

/// `JsonSerialize(file, value)` (Json.java `serialize`): `value` must be a
/// sequence or a record (`toTuple` / `toRcd`); the file gets
/// `ToJson(value)`, without a trailing newline. The writer receives the
/// serialized bytes after all value evaluation and validation.
pub fn json_serialize(file: &Value, v: &Value, write: impl FnOnce(&str, &[u8]) -> EResult<()>) -> EResult<Value> {
    let path = path_arg(file, "JsonSerialize", "first")?;
    let v = crate::eval::force_seq(v.clone())?;
    let ok = match &v {
        Value::Tuple(_) | Value::Record(_) => true,
        // FcnRcdValue.toRcd: a function whose domain is a set of strings.
        Value::Fcn(pairs, _) => pairs.iter().all(|(k, _)| matches!(k, Value::Str(_))),
        _ => false,
    };
    if !ok {
        return Err(EvalError::of_kind(
            tlx_runtime::ErrorKind::ArgumentError,
            format!("The second argument of JsonSerialize should be a sequence or record, but instead it is:\n{v}"),
        ));
    }
    let mut text = String::new();
    node(&v)?.render(&mut text);
    write(path, text.as_bytes())?;
    Ok(Value::TRUE)
}

/// `ndJsonSerialize(file, value)` (Json.java `ndSerialize`): one line
/// `ToJson(value[i])` per element of the sequence `value`.
pub fn nd_json_serialize(file: &Value, v: &Value, write: impl FnOnce(&str, &[u8]) -> EResult<()>) -> EResult<Value> {
    let path = path_arg(file, "ndJsonSerialize", "first")?;
    let v = crate::eval::force_seq(v.clone())?;
    let Value::Tuple(elems) = &v else {
        return Err(EvalError::of_kind(
            tlx_runtime::ErrorKind::ArgumentError,
            format!("The second argument of ndJsonSerialize should be a sequence, but instead it is:\n{v}"),
        ));
    };
    let mut text = String::new();
    for e in elems.iter() {
        node(e)?.render(&mut text);
        text.push('\n');
    }
    write(path, text.as_bytes())?;
    Ok(Value::TRUE)
}

/// `JsonDeserialize(file)` (Json.java `deserialize`): the file holds one
/// JSON value; arrays become sequences, objects records, numbers integers
/// (Gson's `getAsInt`), strings and booleans themselves; `null` is an error.
pub fn json_deserialize(file: &Value) -> EResult<Value> {
    let path = path_arg(file, "JsonDeserialize", "first")?;
    let text = read_file(path, "JsonDeserialize")?;
    let mut p = Parser { s: text.as_bytes(), i: 0 };
    p.ws();
    if p.i == p.s.len() {
        // (Gson parses an empty document as JsonNull.)
        return Err(EvalError::new("Cannot convert value: unsupported JSON value null"));
    }
    let v = p.value()?;
    p.ws();
    if p.i != p.s.len() {
        return Err(p.error("Did not consume the entire document"));
    }
    Ok(v)
}

/// `ndJsonDeserialize(file)` (Json.java `ndDeserialize`): the sequence of
/// the JSON values on the file's lines (blank lines are skipped).
pub fn nd_json_deserialize(file: &Value) -> EResult<Value> {
    let path = path_arg(file, "ndJsonDeserialize", "first")?;
    let text = read_file(path, "ndJsonDeserialize")?;
    let mut out = Vec::new();
    for line in java_lines(&text) {
        // String.trim: strips the characters <= ' '.
        let line = line.trim_matches(|c: char| c <= ' ');
        if line.is_empty() {
            continue;
        }
        let mut p = Parser { s: line.as_bytes(), i: 0 };
        let v = p.value()?;
        p.ws();
        if p.i != p.s.len() {
            return Err(p.error("Did not consume the entire document"));
        }
        out.push(v);
    }
    Ok(Value::tuple(out))
}

/// The lines of `text` as `BufferedReader.readLine` returns them (ended by
/// `\n`, `\r` or `\r\n`; no empty last line after a final terminator).
pub(crate) fn java_lines(text: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let b = text.as_bytes();
    let (mut start, mut i) = (0, 0);
    while i < b.len() {
        match b[i] {
            b'\n' => {
                lines.push(&text[start..i]);
                start = i + 1;
            }
            b'\r' => {
                lines.push(&text[start..i]);
                if b.get(i + 1) == Some(&b'\n') {
                    i += 1;
                }
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    if start < b.len() {
        lines.push(&text[start..]);
    }
    lines
}

/// A JSON reader producing TLA+ values as Json.java's `getValue` does.
struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn error(&self, msg: &str) -> EvalError {
        EvalError::new(format!("Malformed JSON at offset {}: {msg}", self.i))
    }

    fn ws(&mut self) {
        while self.i < self.s.len() && matches!(self.s[self.i], b' ' | b'\t' | b'\n' | b'\r') {
            self.i += 1;
        }
    }

    fn eat(&mut self, lit: &[u8]) -> bool {
        if self.s[self.i..].starts_with(lit) {
            self.i += lit.len();
            true
        } else {
            false
        }
    }

    fn value(&mut self) -> EResult<Value> {
        self.ws();
        match self.s.get(self.i) {
            None => Err(self.error("unexpected end of input")),
            Some(b'[') => {
                self.i += 1;
                let mut elems = Vec::new();
                self.ws();
                if self.eat(b"]") {
                    return Ok(Value::tuple(elems));
                }
                loop {
                    elems.push(self.value()?);
                    self.ws();
                    if self.eat(b",") {
                        continue;
                    }
                    if self.eat(b"]") {
                        return Ok(Value::tuple(elems));
                    }
                    return Err(self.error("expected ',' or ']'"));
                }
            }
            Some(b'{') => {
                self.i += 1;
                // (Gson's JsonObject: a repeated name keeps its first
                // position and takes the last value.)
                let mut fields: Vec<(String, Value)> = Vec::new();
                self.ws();
                if self.eat(b"}") {
                    return Ok(Value::record(Vec::new()));
                }
                loop {
                    self.ws();
                    if self.s.get(self.i) != Some(&b'"') {
                        return Err(self.error("expected a name"));
                    }
                    let k = self.string()?;
                    self.ws();
                    if !self.eat(b":") {
                        return Err(self.error("expected ':'"));
                    }
                    let v = self.value()?;
                    match fields.iter_mut().find(|(n, _)| *n == k) {
                        Some(f) => f.1 = v,
                        None => fields.push((k, v)),
                    }
                    self.ws();
                    if self.eat(b",") {
                        continue;
                    }
                    if self.eat(b"}") {
                        // (The field names are created in the order Gson
                        // returns them, as TLC's UniqueStrings are.)
                        let fields = fields.into_iter().map(|(k, v)| (Sym::new(&k), v)).collect();
                        return Ok(Value::record(fields));
                    }
                    return Err(self.error("expected ',' or '}'"));
                }
            }
            Some(b'"') => Ok(Value::str(&self.string()?)),
            Some(b't') if self.eat(b"true") => Ok(Value::TRUE),
            Some(b'f') if self.eat(b"false") => Ok(Value::FALSE),
            Some(b'n') if self.eat(b"null") => Err(EvalError::new("Cannot convert value: unsupported JSON value null")),
            Some(b'-' | b'0'..=b'9') => {
                let start = self.i;
                while self.i < self.s.len() && matches!(self.s[self.i], b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9') {
                    self.i += 1;
                }
                let text = std::str::from_utf8(&self.s[start..self.i]).unwrap_or("");
                json_int(text).map(Value::Int).ok_or_else(|| self.error(&format!("malformed number {text}")))
            }
            Some(_) => Err(self.error("unexpected character")),
        }
    }

    /// A JSON string (the reader is at its opening quote).
    fn string(&mut self) -> EResult<String> {
        self.i += 1;
        let mut out: Vec<u16> = Vec::new();
        let mut buf = [0u16; 2];
        loop {
            let Some(&c) = self.s.get(self.i) else {
                return Err(self.error("unterminated string"));
            };
            match c {
                b'"' => {
                    self.i += 1;
                    return Ok(String::from_utf16_lossy(&out));
                }
                b'\\' => {
                    let Some(&e) = self.s.get(self.i + 1) else {
                        return Err(self.error("unterminated escape"));
                    };
                    self.i += 2;
                    match e {
                        b'"' | b'\\' | b'/' | b'\'' => out.push(e as u16),
                        b'b' => out.push(8),
                        b'f' => out.push(12),
                        b'n' => out.push(10),
                        b'r' => out.push(13),
                        b't' => out.push(9),
                        b'u' => {
                            let hex = self.s.get(self.i..self.i + 4).and_then(|h| std::str::from_utf8(h).ok());
                            let Some(u) = hex.and_then(|h| u16::from_str_radix(h, 16).ok()) else {
                                return Err(self.error("malformed \\u escape"));
                            };
                            self.i += 4;
                            out.push(u);
                        }
                        _ => return Err(self.error("invalid escape sequence")),
                    }
                }
                _ => {
                    // One UTF-8 character, as UTF-16 (escapes may form
                    // surrogate pairs).
                    let rest = std::str::from_utf8(&self.s[self.i..]).map_err(|_| self.error("invalid UTF-8"))?;
                    let ch = rest.chars().next().unwrap();
                    self.i += ch.len_utf8();
                    out.extend_from_slice(ch.encode_utf16(&mut buf));
                }
            }
        }
    }
}

/// Gson's `JsonPrimitive.getAsInt` of a JSON number: `Integer.parseInt`,
/// else `(int) Long.parseLong`, else `new BigDecimal(text).intValue()` (the
/// integer part, truncated, wrapped to 32 bits).
fn json_int(text: &str) -> Option<i64> {
    if let Ok(i) = text.parse::<i32>() {
        return Some(i as i64);
    }
    if let Ok(l) = text.parse::<i64>() {
        return Some(l as i32 as i64);
    }
    let (neg, body) = match text.strip_prefix('-') {
        Some(b) => (true, b),
        None => (false, text),
    };
    let (mantissa, exp) = match body.find(['e', 'E']) {
        Some(k) => (&body[..k], body[k + 1..].parse::<i64>().ok()?),
        None => (body, 0),
    };
    let (int_part, frac) = match mantissa.find('.') {
        Some(k) => (&mantissa[..k], &mantissa[k + 1..]),
        None => (mantissa, ""),
    };
    if int_part.is_empty() && frac.is_empty() || !int_part.bytes().chain(frac.bytes()).all(|b| b.is_ascii_digit()) {
        return None;
    }
    let digits: Vec<u8> = int_part.bytes().chain(frac.bytes()).map(|b| b - b'0').collect();
    let scale = exp - frac.len() as i64;
    let keep = if scale < 0 { (digits.len() as i64 + scale).max(0) as usize } else { digits.len() };
    let mut v: u64 = 0;
    for &d in &digits[..keep] {
        v = (v * 10 + d as u64) & 0xffff_ffff;
    }
    for _ in 0..scale.clamp(0, 64) {
        v = (v * 10) & 0xffff_ffff;
    }
    let v = v as u32 as i32;
    Some(if neg { v.wrapping_neg() } else { v } as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gson_get_as_int() {
        assert_eq!(json_int("42"), Some(42));
        assert_eq!(json_int("-7"), Some(-7));
        assert_eq!(json_int("3000000000"), Some(3000000000u32 as i32 as i64));
        assert_eq!(json_int("3.9"), Some(3));
        assert_eq!(json_int("-3.9"), Some(-3));
        assert_eq!(json_int("1e3"), Some(1000));
        assert_eq!(json_int("1.5E1"), Some(15));
        assert_eq!(json_int("1e40"), Some(0));
        assert_eq!(json_int("12e-1"), Some(1));
        assert_eq!(json_int("-"), None);
    }

    #[test]
    fn reader_lines() {
        assert_eq!(java_lines("a\nb\r\nc\rd\n"), vec!["a", "b", "c", "d"]);
        assert_eq!(java_lines("a\n\nb"), vec!["a", "", "b"]);
        assert!(java_lines("").is_empty());
    }
}
