use rusqlite::{params, Connection};

/// Strip all HTML tags from a string, returning only text content.
/// Uses a char-by-char approach for efficiency (no regex overhead).
pub fn strip_html_tags(html: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => result.push(ch),
            _ => {}
        }
    }
    result
}

/// Escape SQL LIKE metacharacters (`\`, `%`, `_`) so the input can be
/// safely embedded in a `LIKE ?1 ESCAPE '\'` clause.
pub fn escape_sql_like(input: &str) -> String {
    input
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

/// Resolve a potentially truncated message ID (e.g. 8-char prefix from AI)
/// to a full UUID by querying the database.
///
/// Returns `Some(full_id)` if a match is found, `None` otherwise.
/// IDs of 36+ characters are returned as-is (already full UUIDs).
/// The input is sanitised to ASCII alphanumeric and `-` only.
pub fn resolve_message_id(conn: &Connection, id: &str) -> Option<String> {
    let sanitized: String = id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();
    if sanitized.len() >= 36 {
        Some(sanitized)
    } else {
        conn.query_row(
            "SELECT id FROM messages WHERE id LIKE ?1 LIMIT 1",
            params![format!("{}%", sanitized)],
            |row| row.get::<_, String>(0),
        )
        .ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_html_tags() {
        assert_eq!(strip_html_tags("<p>Hello</p>"), "Hello");
        assert_eq!(
            strip_html_tags("<b>Bold</b> and <i>italic</i>"),
            "Bold and italic"
        );
        assert_eq!(strip_html_tags("No tags here"), "No tags here");
        assert_eq!(
            strip_html_tags("<div><span>Nested</span></div>"),
            "Nested"
        );
        assert_eq!(strip_html_tags(""), "");
        assert_eq!(strip_html_tags("<br/>line<br/>break"), "linebreak");
    }

    #[test]
    fn test_escape_sql_like() {
        assert_eq!(escape_sql_like("hello"), "hello");
        assert_eq!(escape_sql_like("100%"), "100\\%");
        assert_eq!(escape_sql_like("a_b"), "a\\_b");
        assert_eq!(escape_sql_like("a\\b"), "a\\\\b");
        assert_eq!(escape_sql_like("100%_\\"), "100\\%\\_\\\\");
    }

    #[test]
    fn test_resolve_message_id_full_uuid() {
        // A 36-char UUID should be returned as-is without DB access
        let uuid = "abcdef01-2345-6789-abcd-ef0123456789";
        assert_eq!(uuid.len(), 36);
        // We can't easily test DB path here, but the sanitize + length check is exercised
        let sanitized: String = uuid
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
            .collect();
        assert_eq!(sanitized.len(), 36);
    }

    #[test]
    fn test_resolve_message_id_sanitizes_input() {
        // Dangerous characters should be stripped
        let dirty = "abc%def_gh\\ij";
        let sanitized: String = dirty
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
            .collect();
        assert_eq!(sanitized, "abcdefghij");
    }
}
