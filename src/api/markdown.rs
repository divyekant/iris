use axum::http::StatusCode;
use axum::Json;
use pulldown_cmark::{html, Options, Parser};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// POST /api/compose/markdown-preview — convert markdown to sanitized HTML
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct MarkdownRequest {
    pub markdown: String,
}

#[derive(Debug, Serialize)]
pub struct MarkdownResponse {
    pub html: String,
}

/// Convert markdown text to safe HTML using pulldown-cmark.
/// Pure transformation — no AppState needed.
pub async fn markdown_preview(
    Json(req): Json<MarkdownRequest>,
) -> Result<Json<MarkdownResponse>, StatusCode> {
    let html = markdown_to_safe_html(&req.markdown);
    Ok(Json(MarkdownResponse { html }))
}

/// Convert markdown to HTML with pulldown-cmark, then sanitize the output.
fn markdown_to_safe_html(input: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_HEADING_ATTRIBUTES);

    let parser = Parser::new_ext(input, options);
    let mut raw_html = String::new();
    html::push_html(&mut raw_html, parser);

    sanitize_html(&raw_html)
}

/// Basic HTML sanitization: strip dangerous tags and attributes.
fn sanitize_html(html: &str) -> String {
    let mut result = html.to_string();

    // Remove <script>...</script> tags (including multiline)
    let script_re = regex_lite::Regex::new(r"(?is)<script[\s>].*?</script>").unwrap();
    result = script_re.replace_all(&result, "").to_string();

    // Remove <iframe>...</iframe> tags
    let iframe_re = regex_lite::Regex::new(r"(?is)<iframe[\s>].*?</iframe>").unwrap();
    result = iframe_re.replace_all(&result, "").to_string();

    // Remove standalone <script> or <iframe> tags (self-closing or unclosed)
    let tag_re = regex_lite::Regex::new(r"(?i)<(script|iframe)\b[^>]*/?>").unwrap();
    result = tag_re.replace_all(&result, "").to_string();

    // Remove on* event attributes (onclick, onerror, onload, etc.)
    let on_attr_re = regex_lite::Regex::new(r#"(?i)\s+on\w+\s*=\s*("[^"]*"|'[^']*'|[^\s>]*)"#).unwrap();
    result = on_attr_re.replace_all(&result, "").to_string();

    // Remove javascript: URLs from href attributes
    let js_href_re = regex_lite::Regex::new(r#"(?i)href\s*=\s*["']?\s*javascript:"#).unwrap();
    result = js_href_re.replace_all(&result, r#"href=""#).to_string();

    result
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_heading() {
        let html = markdown_to_safe_html("# Hello World");
        assert!(html.contains("<h1>"));
        assert!(html.contains("Hello World"));
    }

    #[test]
    fn test_bold_italic() {
        let html = markdown_to_safe_html("**bold** and *italic*");
        assert!(html.contains("<strong>bold</strong>"));
        assert!(html.contains("<em>italic</em>"));
    }

    #[test]
    fn test_lists() {
        let html = markdown_to_safe_html("- item one\n- item two\n\n1. first\n2. second");
        assert!(html.contains("<ul>"));
        assert!(html.contains("<li>item one</li>"));
        assert!(html.contains("<ol>"));
        assert!(html.contains("<li>first</li>"));
    }

    #[test]
    fn test_code_blocks() {
        // Inline code
        let html = markdown_to_safe_html("Use `println!` macro");
        assert!(html.contains("<code>println!</code>"));

        // Fenced code block
        let html = markdown_to_safe_html("```rust\nfn main() {}\n```");
        assert!(html.contains("<code"));
        assert!(html.contains("fn main()"));
    }

    #[test]
    fn test_tables() {
        let md = "| A | B |\n|---|---|\n| 1 | 2 |";
        let html = markdown_to_safe_html(md);
        assert!(html.contains("<table>"));
        assert!(html.contains("<th>"));
        assert!(html.contains("<td>"));
    }

    #[test]
    fn test_links_preserved() {
        let html = markdown_to_safe_html("[Google](https://google.com)");
        assert!(html.contains(r#"href="https://google.com""#));
        assert!(html.contains("Google"));
    }

    #[test]
    fn test_xss_prevention() {
        // Script tags must be stripped
        let html = markdown_to_safe_html("<script>alert('xss')</script>");
        assert!(!html.contains("<script>"));
        assert!(!html.contains("alert("));

        // on* attributes must be stripped
        let html = markdown_to_safe_html(r#"<img src="x" onerror="alert(1)">"#);
        assert!(!html.contains("onerror"));

        // javascript: URLs must be neutralized
        let html = markdown_to_safe_html("[click](javascript:alert(1))");
        assert!(!html.to_lowercase().contains("javascript:alert"));
    }

    #[test]
    fn test_empty_input() {
        let html = markdown_to_safe_html("");
        assert!(html.is_empty() || html.trim().is_empty());
    }

    #[test]
    fn test_plain_text_passthrough() {
        let html = markdown_to_safe_html("Just some plain text.");
        assert!(html.contains("Just some plain text."));
        assert!(html.contains("<p>"));
    }
}
