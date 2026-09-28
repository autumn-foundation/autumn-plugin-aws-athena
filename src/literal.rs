//! Athena SQL literals for execution parameters.
//!
//! Athena puts each execution parameter into the query as SQL text. A string
//! parameter must therefore be a quoted literal.
//!
//! # Contract
//!
//! - [`Param::to_sql`] gives exactly one SQL literal, keyword or parenthesized value.
//! - Each text part is quoted, and each `'` in it is doubled. This is also true for unchecked variants.
//! - A negative number is in parentheses, so a `-` before the placeholder cannot make a `--` comment.
//! - Validated kinds (`decimal`, `date`, `timestamp`) contain only digits and separators.
//! - [`quote_identifier`] gives one quoted identifier. Each `"` is doubled.

use std::fmt::Write as _;

/// The most digits in an Athena `decimal`.
const MAX_DECIMAL_DIGITS: usize = 38;

/// The most fraction digits in an Athena `timestamp`.
const MAX_FRACTION_DIGITS: usize = 12;

/// A value to bind to one `?` placeholder.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Param {
    /// SQL `NULL`.
    Null,
    /// A `boolean`.
    Bool(bool),
    /// A `bigint`.
    Int(i64),
    /// A `double`.
    Double(f64),
    /// A `varchar`.
    Text(String),
    /// A `decimal`. Use [`Param::decimal`] to make one.
    Decimal(String),
    /// A `date`. Use [`Param::date`] to make one.
    Date(String),
    /// A `timestamp`. Use [`Param::timestamp`] to make one.
    Timestamp(String),
    /// A `varbinary`.
    Binary(Vec<u8>),
}

/// A value that is not a valid literal.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid {kind} parameter: {reason}")]
pub struct ParamError {
    kind: &'static str,
    reason: &'static str,
}

impl ParamError {
    pub(crate) const fn new(kind: &'static str, reason: &'static str) -> Self {
        Self { kind, reason }
    }
}

impl Param {
    /// Makes a `decimal` parameter, for example `"-12.50"`.
    ///
    /// # Errors
    ///
    /// Returns [`ParamError`] if `text` is not a decimal number with 38 digits or fewer.
    pub fn decimal(text: impl Into<String>) -> Result<Self, ParamError> {
        let text = text.into();
        if !is_decimal(&text) {
            return Err(ParamError::new(
                "decimal",
                "expected digits with an optional sign and fraction",
            ));
        }
        if text.bytes().filter(u8::is_ascii_digit).count() > MAX_DECIMAL_DIGITS {
            return Err(ParamError::new("decimal", "more than 38 digits"));
        }
        Ok(Self::Decimal(text))
    }

    /// Makes a `date` parameter in the format `YYYY-MM-DD`.
    ///
    /// # Errors
    ///
    /// Returns [`ParamError`] if `text` is not a date in this format.
    pub fn date(text: impl Into<String>) -> Result<Self, ParamError> {
        let text = text.into();
        if !is_date(&text) {
            return Err(ParamError::new("date", "expected YYYY-MM-DD"));
        }
        Ok(Self::Date(text))
    }

    /// Makes a `timestamp` parameter in the format `YYYY-MM-DD HH:MM:SS[.fff]`.
    ///
    /// # Errors
    ///
    /// Returns [`ParamError`] if `text` is not a timestamp in this format.
    pub fn timestamp(text: impl Into<String>) -> Result<Self, ParamError> {
        let text = text.into();
        let valid = text
            .split_once(' ')
            .is_some_and(|(date, time)| is_date(date) && is_time(time));
        if !valid {
            return Err(ParamError::new(
                "timestamp",
                "expected YYYY-MM-DD HH:MM:SS[.fff]",
            ));
        }
        Ok(Self::Timestamp(text))
    }

    /// Encodes the value as an Athena SQL literal.
    #[must_use]
    pub fn to_sql(&self) -> String {
        match self {
            Self::Null => "NULL".to_owned(),
            Self::Bool(value) => value.to_string(),
            // The unsigned part of `i64::MIN` is out of `bigint` range.
            Self::Int(i64::MIN) => format!("BIGINT '{}'", i64::MIN),
            Self::Int(value) if *value < 0 => format!("({value})"),
            Self::Int(value) => value.to_string(),
            Self::Double(value) => double_literal(*value),
            Self::Text(text) => quote(text, '\''),
            // The constructors validate these kinds. Quoting also covers direct variant use.
            Self::Decimal(text) => format!("DECIMAL {}", quote(text, '\'')),
            Self::Date(text) => format!("DATE {}", quote(text, '\'')),
            Self::Timestamp(text) => format!("TIMESTAMP {}", quote(text, '\'')),
            Self::Binary(bytes) => {
                let mut sql = String::with_capacity(bytes.len() * 2 + 3);
                sql.push_str("X'");
                for byte in bytes {
                    let _ = write!(sql, "{byte:02X}");
                }
                sql.push('\'');
                sql
            }
        }
    }
}

/// Quotes an identifier for a DML statement, for example a table name.
///
/// The result goes into the SQL text. Use it for DML only. Athena DDL uses backticks.
///
/// # Errors
///
/// Returns [`ParamError`] if `name` is empty or contains NUL.
pub fn quote_identifier(name: &str) -> Result<String, ParamError> {
    if name.is_empty() {
        return Err(ParamError::new("identifier", "empty name"));
    }
    if name.contains('\0') {
        return Err(ParamError::new("identifier", "the name contains NUL"));
    }
    Ok(quote(name, '"'))
}

/// Puts `text` between two `mark` characters. Doubles each `mark` in `text`.
fn quote(text: &str, mark: char) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push(mark);
    for c in text.chars() {
        if c == mark {
            out.push(mark);
        }
        out.push(c);
    }
    out.push(mark);
    out
}

/// A `double` literal. The exponent makes Athena read a `double`, not a `decimal`.
fn double_literal(value: f64) -> String {
    if value.is_nan() {
        "nan()".to_owned()
    } else if value.is_infinite() {
        if value > 0.0 {
            "infinity()"
        } else {
            "(-infinity())"
        }
        .to_owned()
    } else if value.is_sign_negative() {
        format!("({value:e})")
    } else {
        format!("{value:e}")
    }
}

fn is_digits(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit())
}

fn is_decimal(text: &str) -> bool {
    let unsigned = text.strip_prefix('-').unwrap_or(text);
    match unsigned.split_once('.') {
        Some((int, frac)) => is_digits(int) && is_digits(frac),
        None => is_digits(unsigned),
    }
}

/// Parses a field of exactly `width` digits that is in `min..=max`.
fn field(text: &str, width: usize, min: u32, max: u32) -> bool {
    text.len() == width
        && is_digits(text)
        && text.parse::<u32>().is_ok_and(|v| (min..=max).contains(&v))
}

fn is_date(text: &str) -> bool {
    let mut parts = text.split('-');
    matches!(
        (parts.next(), parts.next(), parts.next(), parts.next()),
        (Some(y), Some(m), Some(d), None)
            if field(y, 4, 0, 9999) && field(m, 2, 1, 12) && field(d, 2, 1, 31)
    )
}

fn is_time(text: &str) -> bool {
    let (clock, fraction) = match text.split_once('.') {
        Some((clock, fraction)) => (clock, Some(fraction)),
        None => (text, None),
    };
    let fraction_ok = fraction.is_none_or(|f| is_digits(f) && f.len() <= MAX_FRACTION_DIGITS);
    let mut parts = clock.split(':');
    fraction_ok
        && matches!(
            (parts.next(), parts.next(), parts.next(), parts.next()),
            (Some(h), Some(m), Some(s), None)
                if field(h, 2, 0, 23) && field(m, 2, 0, 59) && field(s, 2, 0, 59)
        )
}

macro_rules! from_int {
    ($($t:ty),*) => {$(
        impl From<$t> for Param {
            fn from(value: $t) -> Self {
                Self::Int(i64::from(value))
            }
        }
    )*};
}
from_int!(i8, i16, i32, i64, u8, u16, u32);

impl From<u64> for Param {
    fn from(value: u64) -> Self {
        i64::try_from(value).map_or_else(|_| Self::Decimal(value.to_string()), Self::Int)
    }
}

impl From<bool> for Param {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

impl From<f32> for Param {
    fn from(value: f32) -> Self {
        Self::Double(f64::from(value))
    }
}

impl From<f64> for Param {
    fn from(value: f64) -> Self {
        Self::Double(value)
    }
}

impl From<&str> for Param {
    fn from(value: &str) -> Self {
        Self::Text(value.to_owned())
    }
}

impl From<String> for Param {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<Vec<u8>> for Param {
    fn from(value: Vec<u8>) -> Self {
        Self::Binary(value)
    }
}

impl<T: Into<Self>> From<Option<T>> for Param {
    fn from(value: Option<T>) -> Self {
        value.map_or(Self::Null, Into::into)
    }
}

#[cfg(test)]
mod tests;
