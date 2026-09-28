//! Typed values and rows.
//!
//! # Contract
//!
//! - A missing Athena value is [`Value::Null`]. An empty value is an empty string.
//! - The column type selects the variant. Unknown and complex types stay [`Value::Text`].
//! - Text that does not match its numeric or boolean type is a [`DecodeError`].
//! - [`Row::deserialize`] reads a struct by label and a tuple or sequence by position.

use std::sync::Arc;

use serde::de::value::{Error as DeError, MapDeserializer, SeqDeserializer};
use serde::de::{DeserializeOwned, Error as _, IntoDeserializer, Visitor};
use serde::{Deserializer as _, forward_to_deserialize_any};

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
        Self {
            column: column.into(),
            message: message.into(),
        }
    }

    /// The column label. It is empty for an error about the full row.
    #[must_use]
    pub fn column(&self) -> &str {
        &self.column
    }
}

/// Parses the Athena text of one value.
pub(crate) fn parse(column: &Column, raw: Option<&str>) -> Result<Value, DecodeError> {
    let Some(raw) = raw else {
        return Ok(Value::Null);
    };
    let fail = |message: &str| DecodeError::new(&column.name, format!("{message}: {raw:?}"));
    let type_name = column.type_name.trim().to_ascii_lowercase();
    let base = type_name.split('(').next().unwrap_or_default().trim();
    let value = match base {
        "tinyint" | "smallint" | "integer" | "int" | "bigint" => {
            Value::Int(raw.parse().map_err(|_| fail("expected an integer"))?)
        }
        "real" | "float" | "double" => {
            Value::Double(raw.parse().map_err(|_| fail("expected a number"))?)
        }
        "boolean" => match raw {
            "true" => Value::Bool(true),
            "false" => Value::Bool(false),
            _ => return Err(fail("expected true or false")),
        },
        "decimal" => Value::Decimal(raw.to_owned()),
        "date" => Value::Date(raw.to_owned()),
        "varbinary" => {
            Value::Binary(parse_hex_pairs(raw).ok_or_else(|| fail("expected hex byte pairs"))?)
        }
        _ if base.starts_with("timestamp") => Value::Timestamp(raw.to_owned()),
        _ => Value::Text(raw.to_owned()),
    };
    Ok(value)
}

/// Parses the Athena text of a `varbinary`, for example `68 65 0a`.
fn parse_hex_pairs(raw: &str) -> Option<Vec<u8>> {
    raw.split_whitespace()
        .map(|pair| {
            if pair.len() == 2 {
                u8::from_str_radix(pair, 16).ok()
            } else {
                None
            }
        })
        .collect()
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
        let index = self.columns.iter().position(|column| column.name == name)?;
        self.values.get(index)
    }

    /// Decodes the row into `T`. A struct uses the column labels. A tuple uses the column order.
    ///
    /// # Errors
    ///
    /// Returns [`DecodeError`] if a value does not fit `T`.
    pub fn deserialize<T: DeserializeOwned>(&self) -> Result<T, DecodeError> {
        T::deserialize(RowDeserializer(self)).map_err(|err| DecodeError::new("", err.to_string()))
    }
}

/// Reads a row as a map of labels, or as a sequence for tuples.
struct RowDeserializer<'a>(&'a Row);

impl<'a> RowDeserializer<'a> {
    fn entries(&self) -> impl Iterator<Item = (&'a str, ValueDeserializer<'a>)> {
        let row = self.0;
        row.columns
            .iter()
            .zip(&row.values)
            .map(|(column, value)| (column.name.as_str(), ValueDeserializer(value)))
    }

    fn items(&self) -> impl Iterator<Item = ValueDeserializer<'a>> {
        self.0.values.iter().map(ValueDeserializer)
    }
}

impl<'de> serde::Deserializer<'de> for RowDeserializer<'de> {
    type Error = DeError;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
        MapDeserializer::new(self.entries()).deserialize_any(visitor)
    }

    fn deserialize_seq<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
        SeqDeserializer::new(self.items()).deserialize_any(visitor)
    }

    fn deserialize_tuple<V: Visitor<'de>>(
        self,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value, DeError> {
        self.deserialize_seq(visitor)
    }

    fn deserialize_tuple_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value, DeError> {
        self.deserialize_seq(visitor)
    }

    forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf option unit unit_struct newtype_struct map struct enum identifier ignored_any
    }
}

/// Reads one value. Numeric and text hints convert where the conversion is exact.
#[derive(Clone, Copy)]
struct ValueDeserializer<'a>(&'a Value);

impl<'de> IntoDeserializer<'de, DeError> for ValueDeserializer<'de> {
    type Deserializer = Self;

    fn into_deserializer(self) -> Self {
        self
    }
}

impl<'de> ValueDeserializer<'de> {
    /// The text of a value that is stored as text.
    const fn text(self) -> Option<&'de str> {
        match self.0 {
            Value::Decimal(t) | Value::Date(t) | Value::Timestamp(t) | Value::Text(t) => {
                Some(t.as_str())
            }
            _ => None,
        }
    }

    fn integer<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
        if let Some(text) = self.text() {
            if let Ok(v) = text.parse::<i64>() {
                return visitor.visit_i64(v);
            }
            if let Ok(v) = text.parse::<u64>() {
                return visitor.visit_u64(v);
            }
        }
        self.deserialize_any(visitor)
    }

    fn float<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
        match (self.0, self.text().map(str::parse::<f64>)) {
            (_, Some(Ok(v))) => visitor.visit_f64(v),
            #[allow(clippy::cast_precision_loss, reason = "a bigint read as a float")]
            (Value::Int(v), _) => visitor.visit_f64(*v as f64),
            _ => self.deserialize_any(visitor),
        }
    }
}

impl<'de> serde::Deserializer<'de> for ValueDeserializer<'de> {
    type Error = DeError;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
        match self.0 {
            Value::Null => visitor.visit_unit(),
            Value::Bool(v) => visitor.visit_bool(*v),
            Value::Int(v) => visitor.visit_i64(*v),
            Value::Double(v) => visitor.visit_f64(*v),
            Value::Binary(v) => visitor.visit_borrowed_bytes(v),
            Value::Decimal(t) | Value::Date(t) | Value::Timestamp(t) | Value::Text(t) => {
                visitor.visit_borrowed_str(t)
            }
        }
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
        match self.0 {
            Value::Null => visitor.visit_none(),
            _ => visitor.visit_some(self),
        }
    }

    fn deserialize_str<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
        match self.0 {
            Value::Bool(v) => visitor.visit_string(v.to_string()),
            Value::Int(v) => visitor.visit_string(v.to_string()),
            Value::Double(v) => visitor.visit_string(v.to_string()),
            _ => self.deserialize_any(visitor),
        }
    }

    fn deserialize_string<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
        self.deserialize_str(visitor)
    }

    fn deserialize_seq<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
        match self.0 {
            Value::Binary(bytes) => {
                SeqDeserializer::new(bytes.iter().copied()).deserialize_any(visitor)
            }
            _ => self.deserialize_any(visitor),
        }
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, DeError> {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, DeError> {
        self.text().map_or_else(
            || Err(DeError::custom("expected a text value for an enum")),
            |text| visitor.visit_enum(text.into_deserializer()),
        )
    }

    fn deserialize_i8<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
        self.integer(visitor)
    }
    fn deserialize_i16<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
        self.integer(visitor)
    }
    fn deserialize_i32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
        self.integer(visitor)
    }
    fn deserialize_i64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
        self.integer(visitor)
    }
    fn deserialize_u8<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
        self.integer(visitor)
    }
    fn deserialize_u16<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
        self.integer(visitor)
    }
    fn deserialize_u32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
        self.integer(visitor)
    }
    fn deserialize_u64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
        self.integer(visitor)
    }
    fn deserialize_f32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
        self.float(visitor)
    }
    fn deserialize_f64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
        self.float(visitor)
    }

    forward_to_deserialize_any! {
        bool i128 u128 char bytes byte_buf unit unit_struct
        tuple tuple_struct map struct identifier ignored_any
    }
}

#[cfg(test)]
mod tests;
