//! Turns result pages into rows.

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
    let _ = (statement, columns, first_row);
    todo!()
}

/// Decodes the raw rows of one page.
pub(crate) fn decode_rows(
    columns: &Arc<[Column]>,
    rows: Vec<Vec<Option<String>>>,
) -> Result<Vec<Row>, DecodeError> {
    let _ = (columns, rows);
    todo!()
}

#[cfg(test)]
mod tests;
