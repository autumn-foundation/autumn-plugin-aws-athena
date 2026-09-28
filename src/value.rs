//! Typed values and rows.

use std::sync::Arc;

use serde::de::DeserializeOwned;

use crate::api::Column;

/// One value of a result row.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Value {
    /// SQL `NULL`.
    Null,
    /// A `boolean`.
    Bool(bool),
    /// A `tinyint`, `smallint`, `integer` or `bigint`.
    Int(i64),
    /// A `real` or `double`.
    Double(f64),
    /// A `decimal`, as text.
    Decimal(String),
    /// A `date`, as text.
    Date(String),
    /// A `timestamp`, as text.
    Timestamp(String),
    /// A `varbinary`.
    Binary(Vec<u8>),
    /// All other types, as the Athena text.
    Text(String),
}

/// A value that the plugin can not decode.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("can not decode column `{column}`: {message}")]
pub struct DecodeError {
    column: String,
    message: String,
}

impl DecodeError {
    pub(crate) fn new(column: impl Into<String>, message: impl Into<String>) -> Self {
        Self { column: column.into(), message: message.into() }
    }

    /// The column label. It is empty for an error about the full row.
    #[must_use]
    pub fn column(&self) -> &str {
        &self.column
    }
}

/// Parses the Athena text of one value.
pub(crate) fn parse(column: &Column, raw: Option<&str>) -> Result<Value, DecodeError> {
    let _ = (column, raw);
    todo!()
}

/// One result row.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    columns: Arc<[Column]>,
    values: Vec<Value>,
}

impl Row {
    pub(crate) const fn new(columns: Arc<[Column]>, values: Vec<Value>) -> Self {
        Self { columns, values }
    }

    /// The columns of the row.
    #[must_use]
    pub fn columns(&self) -> &[Column] {
        &self.columns
    }

    /// The values, in column order.
    #[must_use]
    pub fn values(&self) -> &[Value] {
        &self.values
    }

    /// The value in the column with this label.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Value> {
        let _ = name;
        todo!()
    }

    /// Decodes the row into `T`. A struct uses the column labels. A tuple uses the column order.
    ///
    /// # Errors
    ///
    /// Returns [`DecodeError`] if a value does not fit `T`.
    pub fn deserialize<T: DeserializeOwned>(&self) -> Result<T, DecodeError> {
        todo!()
    }
}

#[cfg(test)]
mod tests;
