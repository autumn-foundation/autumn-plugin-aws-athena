//! Counts `?` placeholders in SQL text.
//!
//! # Contract
//!
//! - A `?` counts only outside `'…'`, `"…"` and `` `…` `` and outside `--` and `/* */` comments.
//! - A `--` comment ends at `\n` or `\r`, as in Trino.
//! - A doubled quote mark stays inside its literal.
//! - An unterminated literal or comment continues to the end of the text.
//! - Athena does the full parse. This count only finds a mismatch early.

/// Counts the `?` placeholders outside literals, quoted identifiers and comments.
pub(crate) fn count(sql: &str) -> usize {
    let mut total = 0;
    let mut chars = sql.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '?' => total += 1,
            '\'' | '"' | '`' => skip_quoted(&mut chars, c),
            '-' if chars.peek() == Some(&'-') => {
                // A line comment continues to the end of the line.
                chars.find(|&c| c == '\n' || c == '\r');
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut last = '\0';
                chars.find(|&c| std::mem::replace(&mut last, c) == '*' && c == '/');
            }
            _ => {}
        }
    }
    total
}

/// Moves past the end of a quoted part. A doubled `mark` does not end it.
fn skip_quoted(chars: &mut std::iter::Peekable<std::str::Chars<'_>>, mark: char) {
    while let Some(c) = chars.next() {
        if c == mark {
            if chars.peek() == Some(&mark) {
                chars.next();
            } else {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests;
