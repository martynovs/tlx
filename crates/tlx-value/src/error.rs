use std::borrow::Cow;
use std::fmt;
use std::sync::Arc;

/// A TLC-style evaluation error raised by a value operation
/// (e.g. "Attempted to compare integer 1 with non-integer "a"").
#[derive(Clone)]
pub struct ValueError {
    /// The class of the error: TLC reports some of them with their own
    /// message codes (tlc2.output.EC).
    pub kind: ValueErrorKind,
    msg: Msg,
}

/// The text of a `ValueError`. Most errors carry an already-formatted string;
/// `Lazy` defers the formatting until the message is shown. Messages often
/// embed a whole value (an overflow names the set whose size overflowed, an
/// enumeration error names the set it could not enumerate), which can be
/// expensive to print; errors that the caller discards (e.g. an ALIAS that
/// fails to evaluate) then cost nothing.
#[derive(Clone)]
enum Msg {
    Text(String),
    /// Built by the closure when the message is read, on every read.
    Lazy(Arc<dyn Fn() -> String + Send + Sync>),
}

/// Classes of value errors that TLC reports with their own message codes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ValueErrorKind {
    /// Any other error (TLC: a `RuntimeException`, EC.GENERAL).
    #[default]
    General,
    /// Applying an explicit function outside its domain. TLC wraps this
    /// value-operation failure in FingerprintException when it has source.
    FunctionDomain,
    /// `\div` or `%` by zero (TLC_MODULE_DIVISION_BY_ZERO).
    DivisionByZero,
    /// An argument outside an operator's domain, e.g. `x % 0`
    /// (TLC_MODULE_ARGUMENT_ERROR).
    ArgumentError,
    /// An integer result outside TLC's range (TLC_MODULE_OVERFLOW).
    Overflow,
    /// `0^0` (TLC_MODULE_NULL_POWER_NULL).
    NullPowerNull,
    /// `Head` / `Tail` of the empty sequence (TLC_MODULE_APPLY_EMPTY_SEQ).
    ApplyEmptySeq,
}

impl ValueError {
    pub fn new(message: impl Into<String>) -> ValueError {
        ValueError { msg: Msg::Text(message.into()), kind: ValueErrorKind::General }
    }

    pub fn of_kind(kind: ValueErrorKind, message: impl Into<String>) -> ValueError {
        ValueError { msg: Msg::Text(message.into()), kind }
    }

    /// The message is built by `f` each time it is read (see `Msg::Lazy`);
    /// nothing is cached.
    pub fn lazy(kind: ValueErrorKind, f: impl Fn() -> String + Send + Sync + 'static) -> ValueError {
        ValueError { msg: Msg::Lazy(Arc::new(f)), kind }
    }

    /// The message text (a lazy error builds it now, by its closure).
    pub fn message(&self) -> Cow<'_, str> {
        match &self.msg {
            Msg::Text(s) => Cow::Borrowed(s),
            Msg::Lazy(f) => Cow::Owned(f()),
        }
    }

    /// The message text, consuming the error.
    pub fn into_message(self) -> String {
        match self.msg {
            Msg::Text(s) => s,
            Msg::Lazy(f) => f(),
        }
    }
}

impl PartialEq for ValueError {
    fn eq(&self, other: &ValueError) -> bool {
        self.kind == other.kind && self.message() == other.message()
    }
}

impl Eq for ValueError {}

impl fmt::Debug for ValueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ValueError").field("message", &self.message()).field("kind", &self.kind).finish()
    }
}

impl fmt::Display for ValueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for ValueError {}

/// Shorthand for returning a `ValueError`.
pub(crate) fn err<T>(message: impl Into<String>) -> crate::Result<T> {
    Err(ValueError::new(message))
}

/// Shorthand for returning a `ValueError` of a given kind.
pub(crate) fn err_of<T>(kind: ValueErrorKind, message: impl Into<String>) -> crate::Result<T> {
    Err(ValueError::of_kind(kind, message))
}
