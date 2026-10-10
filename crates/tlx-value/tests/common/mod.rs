#![allow(dead_code)]

use std::sync::Arc;

use tlx_value::{ops, LazyFcn, Sym, Value};

pub fn int(n: i64) -> Value {
    Value::Int(n)
}

pub fn s(x: &str) -> Value {
    Value::str(x)
}

pub fn mv(x: &str) -> Value {
    Value::model_value(x)
}

pub fn set(elems: Vec<Value>) -> Value {
    Value::set_from(elems).unwrap()
}

pub fn ints(xs: &[i64]) -> Value {
    set(xs.iter().map(|&n| int(n)).collect())
}

pub fn tup(elems: Vec<Value>) -> Value {
    Value::tuple(elems)
}

pub fn rec(fields: &[(&str, Value)]) -> Value {
    Value::record(fields.iter().map(|(k, v)| (Sym::new(k), v.clone())).collect())
}

pub fn fcn(pairs: Vec<(Value, Value)>) -> Value {
    Value::fcn_from_pairs(pairs).unwrap()
}

pub fn show(vs: &[Value]) -> String {
    vs.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(" ")
}

pub fn enum_str(v: &Value) -> String {
    show(&ops::enumerate(v).unwrap())
}

pub fn err_msg<T: std::fmt::Debug>(r: tlx_value::Result<T>) -> String {
    r.expect_err("expected an error").into_message()
}

/// `[x \in dom |-> x * mult]`
#[derive(Debug)]
pub struct Scale {
    pub dom: Value,
    pub mult: i64,
}

impl LazyFcn for Scale {
    fn apply(&self, arg: &Value) -> tlx_value::Result<Value> {
        if !ops::member(arg, &self.dom)? {
            return Err(tlx_value::ValueError::new("not in domain"));
        }
        Ok(Value::Int(arg.as_int()? * self.mult))
    }
    fn domain(&self) -> tlx_value::Result<Value> {
        Ok(self.dom.clone())
    }
}

pub fn scale(dom: Value, mult: i64) -> Value {
    Value::LazyFcn(Arc::new(Scale { dom, mult }))
}
