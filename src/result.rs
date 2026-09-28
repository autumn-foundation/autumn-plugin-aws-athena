//! Turns result pages into rows.
//!
//! # Contract
//!
//! - Row one of page one is a header only for DML or unknown statements with the exact labels.
//! - Each row has one value for each column. A different width is an error.

use std::sync::Arc;

use crate::api::{Column, StatementType};
use crate::value::{DecodeError, Row};

/// Returns `true` if `first_row` is the header row that Athena adds.
///
/// Athena adds a header row on the first page of a DML result.
pub(crate) fn is_header(
    statement: StatementType,
    columns: &[Column],
    first_row: &[Option<String>],
) -> bool {
    let may_have_header = matches!(statement, StatementType::Dml | StatementType::Unknown);
    may_have_header
        && columns.len() == first_row.len()
        && columns
            .iter()
            .zip(first_row)
            .all(|(column, value)| value.as_deref() == Some(column.name.as_str()))
}

/// Decodes the raw rows of one page.
pub(crate) fn decode_rows(
    columns: &Arc<[Column]>,
    rows: Vec<Vec<Option<String>>>,
) -> Result<Vec<Row>, DecodeError> {
    rows.into_iter()
        .map(|raw| {
            if raw.len() != columns.len() {
                return Err(DecodeError::new(
                    "",
                    format!(
                        "the row has {} values for {} columns",
                        raw.len(),
                        columns.len()
                    ),
                ));
            }
            let values = columns
                .iter()
                .zip(&raw)
                .map(|(column, value)| crate::value::parse(column, value.as_deref()))
                .collect::<Result<_, _>>()?;
            Ok(Row::new(Arc::clone(columns), values))
        })
        .collect()
}

#[cfg(test)]
mod tests;
