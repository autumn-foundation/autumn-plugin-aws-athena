//! Counts `?` placeholders in SQL text.

/// Counts the `?` placeholders outside literals, quoted identifiers and comments.
pub(crate) fn count(sql: &str) -> usize {
    let _ = sql;
    todo!()
}

#[cfg(test)]
mod tests;
