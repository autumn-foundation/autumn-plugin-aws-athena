//! Athena SQL literals for execution parameters.

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
    /// A `decimal`.
    Decimal(String),
    /// A `date`.
    Date(String),
    /// A `timestamp`.
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

impl Param {
    /// Makes a `decimal` parameter.
    ///
    /// # Errors
    ///
    /// Returns [`ParamError`] if `text` is not a decimal number.
    pub fn decimal(text: impl Into<String>) -> Result<Self, ParamError> {
        let _ = text.into();
        todo!()
    }

    /// Makes a `date` parameter.
    ///
    /// # Errors
    ///
    /// Returns [`ParamError`] if `text` is not a date.
    pub fn date(text: impl Into<String>) -> Result<Self, ParamError> {
        let _ = text.into();
        todo!()
    }

    /// Makes a `timestamp` parameter.
    ///
    /// # Errors
    ///
    /// Returns [`ParamError`] if `text` is not a timestamp.
    pub fn timestamp(text: impl Into<String>) -> Result<Self, ParamError> {
        let _ = text.into();
        todo!()
    }

    /// Encodes the value as an Athena SQL literal.
    #[must_use]
    pub fn to_sql(&self) -> String {
        todo!()
    }
}

/// Quotes an identifier for a DML statement.
///
/// # Errors
///
/// Returns [`ParamError`] if `name` is empty.
pub fn quote_identifier(name: &str) -> Result<String, ParamError> {
    let _ = name;
    todo!()
}

macro_rules! stub_from {
    ($($t:ty),*) => {$(
        impl From<$t> for Param {
            fn from(_: $t) -> Self {
                todo!()
            }
        }
    )*};
}
stub_from!(bool, i8, i16, i32, i64, u8, u16, u32, u64, f32, f64, &str, String, Vec<u8>);

impl<T: Into<Self>> From<Option<T>> for Param {
    fn from(_: Option<T>) -> Self {
        todo!()
    }
}

#[cfg(test)]
mod tests;
