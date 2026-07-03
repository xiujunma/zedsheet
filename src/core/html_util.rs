//! Shared HTML-building helpers used by both print export (issue #17) and
//! clipboard copy (`text/html` flavor). Kept pure (no DOM) so the serializers
//! that use them stay host-testable.

use crate::core::data_proxy::Style;

/// Escape text for safe inclusion in HTML element content or a double-quoted
/// attribute value.
pub(crate) fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Inline CSS for one cell, from its resolved style (conditional formats are
/// expected to be applied by the caller before this is called). Defaults
/// (white fill, near-black text, left align, 10px) are omitted so a plain cell
/// yields an empty `style` attribute.
pub(crate) fn td_style(s: &Style) -> String {
    let mut css = String::new();
    if let Some(bg) = &s.bgcolor {
        if !matches!(bg.to_lowercase().as_str(), "#ffffff" | "#fff" | "white") {
            css.push_str(&format!("background:{};", esc(bg)));
        }
    }
    if !s.color.is_empty() && s.color != "#0a0a0a" {
        css.push_str(&format!("color:{};", esc(&s.color)));
    }
    if s.bold {
        css.push_str("font-weight:bold;");
    }
    if s.italic {
        css.push_str("font-style:italic;");
    }
    if s.underline {
        css.push_str("text-decoration:underline;");
    }
    if s.strike {
        css.push_str("text-decoration:line-through;");
    }
    if !s.align.is_empty() && s.align != "left" {
        css.push_str(&format!("text-align:{};", esc(&s.align)));
    }
    if s.text_wrap {
        css.push_str("white-space:normal;word-break:break-word;");
    }
    if s.font_size != 10 {
        css.push_str(&format!("font-size:{}px;", s.font_size + 2));
    }
    css
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn esc_replaces_markup_chars() {
        assert_eq!(esc("<b>&\"x\""), "&lt;b&gt;&amp;&quot;x&quot;");
    }

    #[test]
    fn td_style_is_empty_for_default_cell() {
        assert_eq!(td_style(&Style::default()), "");
    }

    #[test]
    fn td_style_emits_only_non_default_properties() {
        let s = Style {
            bold: true,
            color: "#ff0000".into(),
            ..Style::default()
        };
        let css = td_style(&s);
        assert!(css.contains("font-weight:bold;"));
        assert!(css.contains("color:#ff0000;"));
        assert!(!css.contains("background"), "white bg is omitted");
    }
}
