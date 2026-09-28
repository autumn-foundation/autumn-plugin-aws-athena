use proptest::prelude::*;

use super::count;

#[test]
fn counts_bare_placeholders() {
    assert_eq!(count("SELECT 1"), 0);
    assert_eq!(count("SELECT * FROM t WHERE a = ? AND b IN (?, ?)"), 3);
}

#[test]
fn skips_string_literals() {
    assert_eq!(count("SELECT '?' , ?"), 1);
    assert_eq!(count("SELECT 'it''s ?' , ?"), 1);
}

#[test]
fn skips_quoted_identifiers() {
    assert_eq!(count(r#"SELECT "a?b", "c""?" FROM t WHERE x = ?"#), 1);
    assert_eq!(count("SELECT `a?` FROM t WHERE x = ?"), 1);
}

#[test]
fn skips_comments() {
    assert_eq!(count("SELECT ? -- why?\n, ?"), 2);
    assert_eq!(count("SELECT /* ? */ ?"), 1);
    assert_eq!(count("SELECT ? -- ? at the end"), 1);
}

#[test]
fn unterminated_parts_hide_the_rest() {
    assert_eq!(count("SELECT ?, '?"), 1);
    assert_eq!(count("SELECT ? /* ?"), 1);
}

#[test]
fn a_single_dash_is_not_a_comment() {
    assert_eq!(count("SELECT 5 - ?"), 1);
    assert_eq!(count("SELECT ? / 2, ?"), 2);
}

proptest! {
    #[test]
    fn placeholders_in_encoded_text_never_count(s in any::<String>(), n in 0_usize..5) {
        let literal = crate::literal::Param::Text(s).to_sql();
        let sql = format!("SELECT {literal}{}", ", ?".repeat(n));
        prop_assert_eq!(count(&sql), n);
    }

    #[test]
    fn plain_placeholders_all_count(n in 0_usize..50) {
        let sql = vec!["?"; n].join(", ");
        prop_assert_eq!(count(&sql), n);
    }
}

#[test]
fn a_carriage_return_ends_a_line_comment() {
    // Trino ends a `--` comment at `\r` or `\n`.
    assert_eq!(count("SELECT 1 -- c\r AND x = ?"), 1);
}

#[test]
fn a_backslash_does_not_escape_a_quote() {
    assert_eq!(count("SELECT 'a\\', ?"), 1);
}

#[test]
fn quotes_inside_comments_do_not_open_literals() {
    assert_eq!(count("SELECT 1 -- it's\n, ?"), 1);
    assert_eq!(count("SELECT /* it's */ ?"), 1);
    assert_eq!(count("SELECT /**/ ?"), 1);
    assert_eq!(count("SELECT `a``?` , ?"), 1);
}

fn tricky_text() -> impl Strategy<Value = String> {
    proptest::collection::vec(
        proptest::sample::select(vec![
            "'", "?", "\"", "`", "--", "/*", "*/", "\\", "\n", "\r", "a", " ",
        ]),
        0..20,
    )
    .prop_map(|parts: Vec<&str>| parts.concat())
}

proptest! {
    #[test]
    fn placeholders_in_tricky_encoded_text_never_count(s in tricky_text(), n in 0_usize..5) {
        let literal = crate::literal::Param::Text(s).to_sql();
        let sql = format!("SELECT {literal}{}", ", ?".repeat(n));
        prop_assert_eq!(count(&sql), n);
    }
}
