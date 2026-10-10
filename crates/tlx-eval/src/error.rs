use std::fmt;

use tlx_runtime::ErrorKind;
use tlx_sema::hir::Span;
use tlx_value::{ValueError, ValueErrorKind};

/// An evaluation error. Boxed: results of evaluation are moved through
/// every level of the evaluator, and the error is rare (`EResult<Value>`
/// is a third of the size with the box).
#[derive(Clone, Debug)]
pub struct EvalError(Box<EvalErrorData>);

#[derive(Clone, Debug)]
pub struct EvalErrorData {
    pub message: String,
    /// Innermost source location, when known.
    pub span: Option<Span>,
    /// The class of the error (how TLC reports it).
    pub kind: ErrorKind,
}

impl std::ops::Deref for EvalError {
    type Target = EvalErrorData;

    fn deref(&self) -> &EvalErrorData {
        &self.0
    }
}

impl std::ops::DerefMut for EvalError {
    fn deref_mut(&mut self) -> &mut EvalErrorData {
        &mut self.0
    }
}

impl EvalError {
    #[cold]
    fn make(message: String, span: Option<Span>, kind: ErrorKind) -> EvalError {
        EvalError(Box::new(EvalErrorData { message, span, kind }))
    }

    pub fn new(message: impl Into<String>) -> EvalError {
        EvalError::make(message.into(), None, ErrorKind::General)
    }

    pub fn at(message: impl Into<String>, span: Span) -> EvalError {
        EvalError::make(message.into(), Some(span), ErrorKind::General)
    }

    pub fn of_kind(kind: ErrorKind, message: impl Into<String>) -> EvalError {
        EvalError::make(message.into(), None, kind)
    }

    /// The message, consuming the error.
    pub fn into_message(self) -> String {
        self.0.message
    }

    /// Attach `span` if no location is known yet.
    #[cold]
    pub fn with_span(mut self, span: Span) -> EvalError {
        if self.span.is_none() {
            self.span = Some(span);
        }
        self
    }
}

impl From<ValueError> for EvalError {
    fn from(e: ValueError) -> EvalError {
        let kind = match e.kind {
            ValueErrorKind::General => ErrorKind::General,
            ValueErrorKind::FunctionDomain => ErrorKind::FunctionDomain,
            ValueErrorKind::DivisionByZero => ErrorKind::DivisionByZero,
            ValueErrorKind::ArgumentError => ErrorKind::ArgumentError,
            ValueErrorKind::Overflow => ErrorKind::Overflow,
            ValueErrorKind::NullPowerNull => ErrorKind::NullPowerNull,
            ValueErrorKind::ApplyEmptySeq => ErrorKind::ApplyEmptySeq,
        };
        EvalError::of_kind(kind, e.into_message())
    }
}

/// For value operations that evaluate expressions (lazy values): the kind
/// survives where values have it.
impl From<EvalError> for ValueError {
    fn from(e: EvalError) -> ValueError {
        let kind = match e.kind {
            ErrorKind::FunctionDomain => ValueErrorKind::FunctionDomain,
            ErrorKind::DivisionByZero => ValueErrorKind::DivisionByZero,
            ErrorKind::ArgumentError => ValueErrorKind::ArgumentError,
            ErrorKind::Overflow => ValueErrorKind::Overflow,
            ErrorKind::NullPowerNull => ValueErrorKind::NullPowerNull,
            ErrorKind::ApplyEmptySeq => ValueErrorKind::ApplyEmptySeq,
            _ => ValueErrorKind::General,
        };
        ValueError::of_kind(kind, e.into_message())
    }
}

impl fmt::Display for EvalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

pub type EResult<T> = Result<T, EvalError>;
