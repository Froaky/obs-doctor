//! Draws the report as a single self-contained HTML page.
//!
//! The page must work offline: no external fonts, scripts, images or requests.
//! Every piece of text that comes from the PC (program names, profile names,
//! source names) is untrusted and must go through [`escape`].

use crate::model::ReportData;

/// Placeholder page so the pipeline works end to end. To be replaced by the
/// designed report; keep the signature.
pub fn render(data: &ReportData) -> String {
    let mut out = String::new();
    out.push_str("<!doctype html>\n<html lang=\"es\">\n<head>\n<meta charset=\"utf-8\">\n");
    out.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    out.push_str("<title>OBS Doctor</title>\n</head>\n<body>\n");
    out.push_str(&format!("<h1>{}</h1>\n", escape(data.verdict.headline())));
    for item in &data.advice {
        out.push_str(&format!("<h2>{}</h2>\n<p>{}</p>\n<ol>\n", escape(&item.title), escape(&item.why)));
        for step in &item.steps {
            out.push_str(&format!("<li>{}</li>\n", escape(step)));
        }
        out.push_str("</ol>\n");
    }
    out.push_str("</body>\n</html>\n");
    out
}

/// Makes any text safe to place inside HTML element content or a quoted attribute.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_everything_html_cares_about() {
        assert_eq!(
            escape(r#"<script>alert("x")</script> & 'y'"#),
            "&lt;script&gt;alert(&quot;x&quot;)&lt;/script&gt; &amp; &#39;y&#39;"
        );
    }
}
