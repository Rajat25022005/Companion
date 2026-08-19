//! Web Scraping Capability.
//!
//! Provides built-in tools for fetching & parsing web pages:
//! - `web.fetch` — Fetch HTML from a URL, strip noise (script/style/nav/footer),
//!   extract the main article body, and convert it to clean Markdown (headers,
//!   paragraphs, links, lists, tables).
//! - `web.extract_links` — Extract all internal & external hyperlinks with
//!   anchor text from a web page.
//!
//! Both tools accept either a `url` (remote fetch via `reqwest`) or an inline
//! `html` string (useful for tests and chained pipelines).

use async_trait::async_trait;
use scraper::{element_ref::ElementRef, Html, Selector};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::time::Instant;
use url::Url;

use companion_domain::*;
use crate::registry::Capability;

// ===========================================================================
// Constants
// ===========================================================================

/// Tags whose entire subtree is dropped during conversion.
const STRIP_TAGS: &[&str] = &["script", "style", "nav", "footer"];

/// CSS selector list, tried in order, to find the "main" article body.
/// Falls back to <body> if none match.
const MAIN_SELECTORS: &[&str] = &[
    "main",
    "article",
    "[role=\"main\"]",
    "#content",
    "#main-content",
    ".post-content",
    ".article-body",
    ".entry-content",
    ".content",
];
const BODY_FALLBACK: &str = "body";

// ===========================================================================
// Shared Response Types
// ===========================================================================

/// A hyperlink extracted from a page.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExtractedLink {
    /// The resolved absolute URL.
    pub url: String,
    /// The anchor text (trimmed, may be empty).
    pub text: String,
    /// Whether the link points to the same origin as the page.
    pub internal: bool,
}

// ===========================================================================
// Markdown Rendering
// ===========================================================================

/// Internal rendering state for list rendering.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ListMode {
    None,
    Unordered,
    Ordered,
}

/// Clean HTML processing pipeline used for `web.fetch` output.
///
/// Picks a "main" subtree, otherwise falls back to `<body>`, otherwise returns
/// an empty string. The chosen subtree is rendered to clean Markdown.
pub fn clean_html_to_markdown(html: &str) -> String {
    let document = Html::parse_document(html);

    let main_root: Option<ElementRef<'_>> = MAIN_SELECTORS
        .iter()
        .filter_map(|sel| Selector::parse(sel).ok())
        .find_map(|sel| document.select(&sel).next())
        .or_else(|| {
            Selector::parse(BODY_FALLBACK)
                .ok()
                .and_then(|sel| document.select(&sel).next())
        });

    let main = match main_root {
        Some(r) => r,
        None => return String::new(),
    };

    let mut out = String::new();
    walk(main, &mut out, ListMode::None, 0);
    collapse_blank_lines(&out)
}

fn walk(node: ElementRef<'_>, out: &mut String, mode: ListMode, depth: usize) {
    // ElementRef derefs to NodeRef<'a, Node>, and `children()` returns
    // an iterator over child NodeRef<'a, Node>.
    for child in node.children() {
        // Text nodes carry a string value; element nodes carry an Element
        // struct. We dispatch via wrapper helpers.
        let value = child.value();
        if value.is_text() {
            // `Text` derefs to a str slice — borrow as &str.
            let s: &str = value.as_text().expect("is_text").as_ref();
            let s = collapse_whitespace(s);
            if !s.is_empty() {
                out.push_str(&s);
            }
        } else if value.is_element() {
            if let Some(el) = ElementRef::wrap(child) {
                let tag = el.value().name.local.as_ref().to_lowercase();
                emit_element(el, &tag, out, mode, depth);
            }
        }
        // Document / Comment / Doctype / ProcessingInstruction: skip.
    }
}

fn emit_element(
    el: ElementRef<'_>,
    tag: &str,
    out: &mut String,
    mode: ListMode,
    depth: usize,
) {
    match tag {
        // Headings
        "h1" => emit_heading(out, "# ", el, mode, depth),
        "h2" => emit_heading(out, "## ", el, mode, depth),
        "h3" => emit_heading(out, "### ", el, mode, depth),
        "h4" => emit_heading(out, "#### ", el, mode, depth),
        "h5" => emit_heading(out, "##### ", el, mode, depth),
        "h6" => emit_heading(out, "###### ", el, mode, depth),

        // Paragraph
        "p" => {
            ensure_newline(out, 1);
            walk(el, out, mode, depth);
            out.push('\n');
        }

        // Hard line break
        "br" => {
            out.push_str("  \n");
        }

        // Inline emphasis
        "strong" | "b" => {
            out.push_str("**");
            walk(el, out, mode, depth);
            out.push_str("**");
        }
        "em" | "i" => {
            out.push('*');
            walk(el, out, mode, depth);
            out.push('*');
        }
        "code" => {
            out.push('`');
            walk(el, out, mode, depth);
            out.push('`');
        }
        "pre" => {
            ensure_newline(out, 2);
            out.push_str("```\n");
            let raw: String = el
                .text()
                .map(|t| t.to_string())
                .collect::<Vec<_>>()
                .join("");
            out.push_str(raw.trim_end());
            if !out.ends_with('\n') {
                out.push('\n');
            }
            out.push_str("```\n");
        }
        "blockquote" => {
            ensure_newline(out, 2);
            let mut inner = String::new();
            walk(el, &mut inner, mode, depth);
            for line in inner.lines() {
                if line.trim().is_empty() {
                    out.push('\n');
                } else {
                    out.push_str("> ");
                    out.push_str(line.trim_end());
                    out.push('\n');
                }
            }
        }

        // Anchors
        "a" => {
            let href = el.value().attr("href").unwrap_or("");
            let mut label_buf = String::new();
            walk(el, &mut label_buf, mode, depth);
            let label = collapse_whitespace(&label_buf);

            if href.is_empty() {
                out.push_str(&label);
            } else if label.is_empty() {
                out.push('<');
                out.push_str(href);
                out.push('>');
            } else {
                out.push('[');
                out.push_str(&label);
                out.push_str("](");
                out.push_str(href);
                out.push(')');
            }
        }

        // Lists
        "ul" => {
            ensure_newline(out, 2);
            walk(el, out, ListMode::Unordered, depth);
        }
        "ol" => {
            ensure_newline(out, 2);
            walk(el, out, ListMode::Ordered, depth);
        }
        "li" => {
            ensure_newline(out, 1);
            let indent = "  ".repeat(depth);
            out.push_str(&indent);
            match mode {
                ListMode::Ordered => out.push_str("1. "),
                _ => out.push_str("- "),
            }
            walk(el, out, mode, depth + 1);
            out.push('\n');
        }

        // Tables
        "table" => {
            ensure_newline(out, 2);
            render_table(el, out);
            out.push('\n');
        }

        // Horizontal rule
        "hr" => {
            ensure_newline(out, 2);
            out.push_str("---\n");
        }

        // Images
        "img" => {
            let src = el.value().attr("src").unwrap_or("");
            let alt = el.value().attr("alt").unwrap_or("");
            if !src.is_empty() {
                out.push_str("![");
                out.push_str(alt);
                out.push_str("](");
                out.push_str(src);
                out.push(')');
            }
        }

        // Noise tags: silently drop the subtree.
        t if STRIP_TAGS.contains(&t) => {}

        // Default: just walk children.
        _ => walk(el, out, mode, depth),
    }
}

fn emit_heading(
    out: &mut String,
    prefix: &str,
    el: ElementRef<'_>,
    mode: ListMode,
    depth: usize,
) {
    ensure_newline(out, 2);
    out.push_str(prefix);
    walk(el, out, mode, depth);
    out.push('\n');
}

fn collapse_whitespace(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_ws = false;
    for ch in s.chars() {
        if ch.is_whitespace() {
            if !prev_ws {
                out.push(' ');
            }
            prev_ws = true;
        } else {
            out.push(ch);
            prev_ws = false;
        }
    }
    out
}

fn ensure_newline(out: &mut String, min: usize) {
    let mut needed = min;
    while needed > 0 && out.ends_with('\n') {
        needed -= 1;
    }
    for _ in 0..needed {
        out.push('\n');
    }
}

fn collapse_blank_lines(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut blank_run = 0u32;
    for line in s.lines() {
        if line.trim().is_empty() {
            blank_run += 1;
            if blank_run > 1 {
                continue;
            }
            out.push('\n');
        } else {
            blank_run = 0;
            out.push_str(line.trim_end());
            out.push('\n');
        }
    }
    out.trim_end_matches('\n').to_string()
}

fn render_table(table: ElementRef<'_>, out: &mut String) {
    let header_sel = Selector::parse("thead tr th").unwrap();
    let header_cells: Vec<String> = table.select(&header_sel).map(cell_text).collect();

    let row_sel = Selector::parse("tr").unwrap();
    let cell_sel = Selector::parse("th, td").unwrap();

    let tbody_sel = Selector::parse("tbody").unwrap();
    let has_tbody = table.select(&tbody_sel).next().is_some();

    let rows: Vec<Vec<String>> = if has_tbody {
        let tbody_row_sel = Selector::parse("tbody tr").unwrap();
        table.select(&tbody_row_sel).map(|r| row_cells(r, &cell_sel)).collect()
    } else {
        Vec::new()
    };

    let header = if !header_cells.is_empty() {
        header_cells.clone()
    } else if !rows.is_empty() {
        rows[0].clone()
    } else if let Some(first) = table.select(&row_sel).next() {
        row_cells(first, &cell_sel)
    } else {
        Vec::new()
    };

    if header.is_empty() {
        return;
    }

    out.push_str("| ");
    out.push_str(&header.join(" | "));
    out.push_str(" |\n");
    out.push('|');
    for _ in &header {
        out.push_str(" --- |");
    }
    out.push('\n');

    let body_start: usize = if !header_cells.is_empty() { 0 } else { 1 };
    for (i, row) in rows.iter().enumerate() {
        if i < body_start {
            continue;
        }
        out.push_str("| ");
        out.push_str(&row.join(" | "));
        out.push_str(" |\n");
    }
}

fn row_cells(row: ElementRef<'_>, cell_sel: &Selector) -> Vec<String> {
    row.select(cell_sel).map(cell_text).collect()
}

fn cell_text(c: ElementRef<'_>) -> String {
    let mut buf = String::new();
    walk(c, &mut buf, ListMode::None, 0);
    collapse_whitespace(&buf.replace('\n', " "))
}

// ===========================================================================
// Link Extraction
// ===========================================================================

/// Extract all hyperlinks with anchor text from an HTML document.
///
/// `base_url` is used to resolve relative URLs and determine internal-vs-external.
/// URLs that fail to resolve without a base are kept verbatim.
pub fn extract_links(html: &str, base_url: Option<&str>) -> Vec<ExtractedLink> {
    let document = Html::parse_document(html);
    let sel = Selector::parse("a[href]").expect("static selector");
    let base = base_url.and_then(|s| Url::parse(s).ok());

    let mut seen: HashSet<String> = HashSet::new();
    let mut out: Vec<ExtractedLink> = Vec::new();

    for a in document.select(&sel) {
        let raw_href = a.value().attr("href").unwrap_or("").trim();
        if raw_href.is_empty()
            || raw_href.starts_with('#')
            || raw_href.starts_with("javascript:")
            || raw_href.starts_with("mailto:")
            || raw_href.starts_with("tel:")
        {
            continue;
        }

        let abs = match &base {
            Some(b) => b
                .join(raw_href)
                .map(|u| u.to_string())
                .unwrap_or_else(|_| raw_href.to_string()),
            None => raw_href.to_string(),
        };

        if !seen.insert(abs.clone()) {
            continue;
        }

        let mut label_buf = String::new();
        walk(a, &mut label_buf, ListMode::None, 0);
        let label = collapse_whitespace(&label_buf);

        let internal = base
            .as_ref()
            .and_then(|b| Url::parse(&abs).ok().map(|u| u.origin() == b.origin()))
            .unwrap_or(false);

        out.push(ExtractedLink {
            url: abs,
            text: label,
            internal,
        });
    }

    out
}

// ===========================================================================
// Fetcher (Network Layer)
// ===========================================================================

/// Build a shared `reqwest::Client` with the Companion user-agent and
/// redirect-following enabled (up to 10 hops).
pub fn build_http_client() -> Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder()
        .user_agent(
            "Mozilla/5.0 (compatible; CompanionBot/0.1; +https://github.com/Rajat25022005/Companion)",
        )
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
}

/// Fetch the HTML body of a URL as text.
pub async fn fetch_html(client: &reqwest::Client, url: &str) -> Result<String, reqwest::Error> {
    let resp = client.get(url).send().await?.error_for_status()?;
    resp.text().await
}

// ===========================================================================
// Capability: web.fetch
// ===========================================================================

pub struct WebFetch {
    definition: CapabilityDefinition,
    client: reqwest::Client,
}

impl WebFetch {
    pub fn new() -> Self {
        let client = build_http_client().expect("reqwest client must build");
        Self {
            definition: CapabilityDefinition::new(
                "web.fetch",
                "Fetch HTML from a URL using reqwest (desktop user-agent, redirect-following), strip <script>/<style>/<nav>/<footer>, extract the main article body, and convert to clean Markdown preserving headers, paragraphs, links, lists, and tables.",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "url": {
                            "type": "string",
                            "description": "URL to fetch (http or https only)."
                        },
                        "html": {
                            "type": "string",
                            "description": "Optional inline HTML payload. If provided, skips the network fetch and processes this content directly (useful for chained pipelines and tests)."
                        },
                        "max_bytes": {
                            "type": "integer",
                            "description": "Maximum bytes to read from the network response (default 5 MB)."
                        }
                    },
                    "anyOf": [
                        { "required": ["url"] },
                        { "required": ["html"] }
                    ]
                }),
                vec![CapabilityPermission::NetworkRead],
                RiskLevel::Medium,
            ),
            client,
        }
    }
}

#[async_trait]
impl Capability for WebFetch {
    fn definition(&self) -> &CapabilityDefinition {
        &self.definition
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult, ToolError> {
        let start = Instant::now();

        let max_bytes = args
            .get("max_bytes")
            .and_then(|v| v.as_u64())
            .unwrap_or(5 * 1024 * 1024);

        let html = if let Some(inline) = args.get("html").and_then(|v| v.as_str()) {
            inline.to_string()
        } else {
            let url = args.get("url").and_then(|v| v.as_str()).ok_or_else(|| ToolError {
                tool_call_id: String::new(),
                message: "missing 'url' parameter".into(),
                retryable: false,
            })?;

            let parsed = Url::parse(url).map_err(|e| ToolError {
                tool_call_id: String::new(),
                message: format!("invalid url: {e}"),
                retryable: false,
            })?;
            match parsed.scheme() {
                "http" | "https" => {}
                other => {
                    return Err(ToolError {
                        tool_call_id: String::new(),
                        message: format!("unsupported scheme: {other}"),
                        retryable: false,
                    });
                }
            }

            let raw = fetch_html(&self.client, url).await.map_err(|e| ToolError {
                tool_call_id: String::new(),
                message: format!("fetch failed: {e}"),
                retryable: true,
            })?;

            if raw.len() as u64 > max_bytes {
                raw[..max_bytes as usize].to_string()
            } else {
                raw
            }
        };

        let markdown = clean_html_to_markdown(&html);
        let hash = format!("{:x}", Sha256::digest(markdown.as_bytes()));
        let elapsed = start.elapsed().as_millis() as u64;

        Ok(ToolResult {
            tool_call_id: String::new(),
            success: true,
            output: serde_json::json!({
                "markdown": markdown,
                "length": markdown.len(),
                "content_hash": hash,
            }),
            content_hash: Some(hash),
            execution_ms: elapsed,
        })
    }
}

// ===========================================================================
// Capability: web.extract_links
// ===========================================================================

pub struct WebExtractLinks {
    definition: CapabilityDefinition,
    client: reqwest::Client,
}

impl WebExtractLinks {
    pub fn new() -> Self {
        let client = build_http_client().expect("reqwest client must build");
        Self {
            definition: CapabilityDefinition::new(
                "web.extract_links",
                "Extract all internal and external hyperlinks with their anchor text from a web page. Provide either a `url` to fetch, or an inline `html` payload plus `base_url` for relative-resolution.",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "url": {
                            "type": "string",
                            "description": "URL of the page to fetch and parse."
                        },
                        "html": {
                            "type": "string",
                            "description": "Optional inline HTML payload (skips network fetch)."
                        },
                        "base_url": {
                            "type": "string",
                            "description": "Base URL used to resolve relative <a href> values when using inline `html`."
                        },
                        "max_bytes": {
                            "type": "integer",
                            "description": "Maximum bytes to read from a network response (default 5 MB)."
                        },
                        "internal_only": {
                            "type": "boolean",
                            "description": "If true, only return links that share the same origin as the page."
                        }
                    },
                    "anyOf": [
                        { "required": ["url"] },
                        { "required": ["html"] }
                    ]
                }),
                vec![CapabilityPermission::NetworkRead],
                RiskLevel::Low,
            ),
            client,
        }
    }
}

#[async_trait]
impl Capability for WebExtractLinks {
    fn definition(&self) -> &CapabilityDefinition {
        &self.definition
    }

    async fn execute(&self, args: serde_json::Value) -> Result<ToolResult, ToolError> {
        let start = Instant::now();

        let max_bytes = args
            .get("max_bytes")
            .and_then(|v| v.as_u64())
            .unwrap_or(5 * 1024 * 1024);

        let internal_only = args
            .get("internal_only")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let (html, base) = if let Some(inline) = args.get("html").and_then(|v| v.as_str()) {
            let b = args
                .get("base_url")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            (inline.to_string(), b)
        } else {
            let url = args.get("url").and_then(|v| v.as_str()).ok_or_else(|| ToolError {
                tool_call_id: String::new(),
                message: "missing 'url' parameter".into(),
                retryable: false,
            })?;
            let parsed = Url::parse(url).map_err(|e| ToolError {
                tool_call_id: String::new(),
                message: format!("invalid url: {e}"),
                retryable: false,
            })?;
            if !matches!(parsed.scheme(), "http" | "https") {
                return Err(ToolError {
                    tool_call_id: String::new(),
                    message: format!("unsupported scheme: {}", parsed.scheme()),
                    retryable: false,
                });
            }
            let raw = fetch_html(&self.client, url).await.map_err(|e| ToolError {
                tool_call_id: String::new(),
                message: format!("fetch failed: {e}"),
                retryable: true,
            })?;
            let body = if raw.len() as u64 > max_bytes {
                raw[..max_bytes as usize].to_string()
            } else {
                raw
            };
            (body, Some(url.to_string()))
        };

        let base_str = base.as_deref();
        let mut links = extract_links(&html, base_str);
        if internal_only {
            links.retain(|l| l.internal);
        }

        let hash_input = serde_json::to_string(&links).unwrap_or_default();
        let hash = format!("{:x}", Sha256::digest(hash_input.as_bytes()));
        let elapsed = start.elapsed().as_millis() as u64;

        Ok(ToolResult {
            tool_call_id: String::new(),
            success: true,
            output: serde_json::json!({
                "links": links,
                "count": links.len(),
                "base_url": base_str,
            }),
            content_hash: Some(hash),
            execution_ms: elapsed,
        })
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_HTML: &str = r#"
<!doctype html>
<html>
  <head>
    <title>Sample</title>
    <style>body { color: red; }</style>
    <script>console.log('hi');</script>
  </head>
  <body>
    <nav><a href="/home">Home</a></nav>
    <header><h1>Site Header</h1></header>
    <main>
      <article>
        <h2>Article Title</h2>
        <p>Hello <strong>world</strong>, this is a <a href="/docs/intro">link</a>.</p>
        <ul>
          <li>First <em>item</em></li>
          <li>Second item with <a href="https://example.com">example</a></li>
        </ul>
        <table>
          <thead>
            <tr><th>Name</th><th>Value</th></tr>
          </thead>
          <tbody>
            <tr><td>alpha</td><td>1</td></tr>
            <tr><td>beta</td><td>2</td></tr>
          </tbody>
        </table>
      </article>
    </main>
    <footer><p>Footer banner</p></footer>
  </body>
</html>
"#;

    #[test]
    fn markdown_extraction_strips_noise_and_keeps_structure() {
        let md = clean_html_to_markdown(SAMPLE_HTML);

        // Article content present.
        assert!(md.contains("## Article Title"), "h2 should render: {md}");
        assert!(
            md.contains("Hello **world**, this is a [link](/docs/intro)."),
            "inline formatting should render: {md}"
        );

        // Lists preserved.
        assert!(md.contains("- First *item*"), "ul should render: {md}");
        assert!(
            md.contains("- Second item with [example](https://example.com)"),
            "second li link should render: {md}"
        );

        // Table headers & rows preserved.
        assert!(md.contains("| Name | Value |"), "table header: {md}");
        assert!(md.contains("| --- | --- |"), "table separator: {md}");
        assert!(md.contains("| alpha | 1 |"), "table row 1: {md}");
        assert!(md.contains("| beta | 2 |"), "table row 2: {md}");

        // Noise removed.
        assert!(!md.contains("console.log"), "script body should be stripped");
        assert!(!md.contains("color: red"), "style body should be stripped");
        assert!(!md.contains("Footer banner"), "footer should be stripped");
        assert!(
            !md.contains("Site Header"),
            "header outside <main> should be stripped: {md}"
        );
        assert!(!md.contains("> Home"), "nav should be stripped: {md}");

        // No triple+ blank lines.
        assert!(!md.contains("\n\n\n"), "excess blank lines: {md}");
    }

    #[test]
    fn extract_links_returns_internal_and_external_with_anchor_text() {
        let links = extract_links(SAMPLE_HTML, Some("https://example.com/page"));

        let doc_link = links
            .iter()
            .find(|l| l.url.ends_with("/docs/intro"))
            .expect("internal link should be present");
        assert_eq!(doc_link.text, "link");
        assert!(doc_link.internal);

        assert!(
            links.iter().any(|l| l.url.contains("example.com")),
            "external link should be present"
        );

        // Deduplication across repeated hrefs.
        let dup_count = links.iter().filter(|l| l.url.contains("/home")).count();
        assert_eq!(dup_count, 1, "duplicate URLs should be deduplicated");
    }

    #[test]
    fn extract_links_dedupes_and_skips_mailto_javascript() {
        let html = r##"
            <a href="mailto:foo@bar.com">email</a>
            <a href="javascript:void(0)">js</a>
            <a href="#">hash</a>
            <a href="/a">A</a>
            <a href="/a">A again</a>
            <a href="https://other.com/">External</a>
        "##;
        let links = extract_links(html, Some("https://base.com/"));
        assert_eq!(links.len(), 2);
        assert!(links[0].internal);
        assert!(!links[1].internal);
        assert_eq!(links[0].url, "https://base.com/a");
        assert_eq!(links[1].url, "https://other.com/");
    }

    #[test]
    fn markdown_extraction_handles_malformed_html_gracefully() {
        let html = "<p>Unclosed paragraph<div>nested</div>";
        let md = clean_html_to_markdown(html);
        assert!(md.contains("Unclosed paragraph"), "got: {md}");
        assert!(md.contains("nested"), "got: {md}");
    }

    #[test]
    fn markdown_extraction_handles_empty_document() {
        let md = clean_html_to_markdown("<html><head></head><body></body></html>");
        assert!(md.is_empty() || md.trim().is_empty(), "got: {md:?}");
    }

    #[tokio::test]
    async fn web_fetch_capability_accepts_inline_html() {
        let tool = WebFetch::new();
        let res = tool
            .execute(serde_json::json!({ "html": SAMPLE_HTML }))
            .await
            .expect("inline fetch should succeed");

        assert!(res.success);
        let markdown = res.output["markdown"].as_str().unwrap();
        assert!(markdown.contains("## Article Title"));
        assert!(!markdown.contains("console.log"));
        assert!(res.content_hash.is_some());
        assert!(res.success);
    }

    #[tokio::test]
    async fn web_fetch_rejects_unsupported_scheme() {
        let tool = WebFetch::new();
        let res = tool
            .execute(serde_json::json!({ "url": "file:///etc/passwd" }))
            .await;
        assert!(res.is_err(), "file:// must be rejected");
    }

    #[tokio::test]
    async fn web_fetch_rejects_missing_url_and_html() {
        let tool = WebFetch::new();
        let res = tool.execute(serde_json::json!({})).await;
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn web_extract_links_capability_works_inline() {
        let tool = WebExtractLinks::new();
        let res = tool
            .execute(serde_json::json!({
                "html": SAMPLE_HTML,
                "base_url": "https://example.com/page",
            }))
            .await
            .expect("extract should succeed");

        assert!(res.success);
        let count = res.output["count"].as_u64().unwrap();
        assert!(count >= 2, "should find at least /docs/intro and example.com");
        assert!(res.content_hash.is_some());
    }

    #[tokio::test]
    async fn web_extract_links_internal_only_filter() {
        let tool = WebExtractLinks::new();
        let res = tool
            .execute(serde_json::json!({
                "html": SAMPLE_HTML,
                "base_url": "https://example.com/page",
                "internal_only": true,
            }))
            .await
            .expect("internal_only filter should succeed");

        let links = res.output["links"].as_array().unwrap();
        assert!(!links.is_empty(), "should retain at least the internal link");
        for l in links {
            assert_eq!(l["internal"], serde_json::json!(true));
        }
    }
}
