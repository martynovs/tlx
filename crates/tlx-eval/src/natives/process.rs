//! The `IOUtils` process family (`tlc2/module/IOUtils.java`): `IOExec`,
//! `IOEnvExec` and their `*Template` forms. A `PreparedCommand` forces the
//! command and environment strings once, before the effect journal is locked.

use tlx_value::{Sym, Value};

use crate::error::{EResult, EvalError};

use super::csv::{java_format, unquoted};

/// The strings of a command (IOUtils.java `convert`: every element must be
/// a string).
fn command_strings(v: &Value, op: &str, which: &str) -> EResult<Vec<String>> {
    let v = crate::eval::force_seq(v.clone())?;
    let Value::Tuple(elems) = &v else {
        return Err(EvalError::of_kind(
            tlx_runtime::ErrorKind::ArgumentError,
            format!("The {which} argument of {op} should be a sequence, but instead it is:\n{v}"),
        ));
    };
    elems
        .iter()
        .map(|e| match e {
            Value::Str(s) => Ok(s.as_str().to_string()),
            e => Err(EvalError::of_kind(
                tlx_runtime::ErrorKind::ArgumentError,
                format!("The {which} argument of {op} should be a sequence of strings, but instead it contains:\n{e}"),
            )),
        })
        .collect()
}

/// The variables of an environment record (IOUtils.java `getEnv`: names to
/// the values' `toUnquotedString`).
fn env_vars(env: &Value, op: &str) -> EResult<Vec<(String, String)>> {
    match crate::eval::force_lazy_fcn(env.clone())? {
        Value::Record(fs) => Ok(fs.iter().map(|(k, v)| (k.as_str().to_string(), unquoted(v))).collect()),
        Value::Fcn(pairs, _) if pairs.iter().all(|(k, _)| matches!(k, Value::Str(_))) => {
            Ok(pairs.iter().map(|(k, v)| (unquoted(k), unquoted(v))).collect())
        }
        // The empty record.
        Value::Tuple(t) if t.is_empty() => Ok(Vec::new()),
        v => Err(EvalError::of_kind(
            tlx_runtime::ErrorKind::ArgumentError,
            format!("The first argument of {op} should be a record, but instead it is:\n{v}"),
        )),
    }
}

/// `runProcess`: runs `args[1]` with the arguments `args[2..]` (and the
/// extra environment variables `env`) and returns
/// `[exitValue |-> n, stdout |-> s, stderr |-> e]`.
fn run_process(args: &[String], env: &[(String, String)], op: &str) -> EResult<Value> {
    let Some((prog, rest)) = args.split_first() else {
        return Err(EvalError::new(format!("{op}: the command is empty")));
    };
    let out = std::process::Command::new(prog)
        .args(rest)
        .envs(env.iter().map(|(k, v)| (k, v)))
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| EvalError::new(format!("{op}: cannot run {prog}: {e}")))?;
    let text = |b: &[u8]| Value::str(&String::from_utf8_lossy(b));
    Ok(Value::record(vec![
        (Sym::new("exitValue"), Value::Int(out.status.code().unwrap_or(-1) as i64)),
        (Sym::new("stdout"), text(&out.stdout)),
        (Sym::new("stderr"), text(&out.stderr)),
    ]))
}

/// A fully evaluated IO invocation. The exact journal key and the process
/// consume the same strings, so lazy arguments and environment fields are
/// forced once, before the effect journal is locked.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct PreparedCommand {
    op: &'static str,
    args: Vec<String>,
    env: Vec<(String, String)>,
}

impl PreparedCommand {
    pub(crate) fn io_exec(command: &Value) -> EResult<Self> {
        Ok(Self { op: "IOExec", args: command_strings(command, "IOExec", "first")?, env: Vec::new() })
    }

    pub(crate) fn io_env_exec(env: &Value, command: &Value) -> EResult<Self> {
        let env = env_vars(env, "IOEnvExec")?;
        Ok(Self { op: "IOEnvExec", args: command_strings(command, "IOEnvExec", "second")?, env })
    }

    pub(crate) fn io_exec_template(template: &Value, params: &Value) -> EResult<Self> {
        let template = command_strings(template, "IOExecTemplate", "first")?;
        let params = command_strings(params, "IOExecTemplate", "second")?;
        let args = template.iter().map(|t| java_format(t, &params)).collect::<EResult<Vec<_>>>()?;
        Ok(Self { op: "IOExecTemplate", args, env: Vec::new() })
    }

    pub(crate) fn io_env_exec_template(env: &Value, template: &Value, params: &Value) -> EResult<Self> {
        let env = env_vars(env, "IOEnvExecTemplate")?;
        let template = command_strings(template, "IOEnvExecTemplate", "second")?;
        let params = command_strings(params, "IOEnvExecTemplate", "third")?;
        let args = template.iter().map(|t| java_format(t, &params)).collect::<EResult<Vec<_>>>()?;
        Ok(Self { op: "IOEnvExecTemplate", args, env })
    }

    pub(crate) fn run(&self) -> EResult<Value> {
        run_process(&self.args, &self.env, self.op)
    }
}

/// `IOExec(command)` (IOUtils): runs the program `command[1]` with the
/// arguments `command[2..]` (all strings).
pub fn io_exec(command: &Value) -> EResult<Value> {
    PreparedCommand::io_exec(command)?.run()
}

/// `IOEnvExec(env, command)`: `IOExec` with the variables of record `env`
/// added to the environment.
pub fn io_env_exec(env: &Value, command: &Value) -> EResult<Value> {
    PreparedCommand::io_env_exec(env, command)?.run()
}

/// `IOExecTemplate(template, parameters)`: each element of the template is
/// `String.format(element, parameters...)` (all strings).
pub fn io_exec_template(template: &Value, params: &Value) -> EResult<Value> {
    PreparedCommand::io_exec_template(template, params)?.run()
}

/// `IOEnvExecTemplate(env, template, parameters)`.
pub fn io_env_exec_template(env: &Value, template: &Value, params: &Value) -> EResult<Value> {
    PreparedCommand::io_env_exec_template(env, template, params)?.run()
}
