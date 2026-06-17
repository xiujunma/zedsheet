//! Hyperlink normalization for cell links (issue #23).
//!
//! A user can type a bare host (`example.com`), an email (`bob@example.com`),
//! or a full URL; this module canonicalizes that into an openable target so the
//! renderer and click handler don't have to guess.

/// Normalize a user-entered hyperlink target into a canonical, openable URL.
/// Returns `None` for blank input (used to clear a link).
///
/// - Blank / whitespace-only → `None`.
/// - Already has a known scheme (`http`, `https`, `mailto`, `tel`, `ftp`,
///   `ftps`) → kept as typed (trimmed).
/// - Looks like an email address → `mailto:` prefix.
/// - Anything else (a bare host/path like `example.com/x`) → `https://` prefix.
pub fn normalize_link(raw: &str) -> Option<String> {
    let s = raw.trim();
    if s.is_empty() {
        return None;
    }
    const SCHEMES: [&str; 6] = [
        "http://", "https://", "mailto:", "tel:", "ftp://", "ftps://",
    ];
    let lower = s.to_ascii_lowercase();
    if SCHEMES.iter().any(|p| lower.starts_with(p)) {
        return Some(s.to_string());
    }
    if is_email_like(s) {
        return Some(format!("mailto:{s}"));
    }
    Some(format!("https://{s}"))
}

/// A loose `local@domain.tld` check: exactly one `@`, no whitespace, and a dot
/// inside (not bounding) the domain part.
fn is_email_like(s: &str) -> bool {
    let mut parts = s.split('@');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(local), Some(domain), None) => {
            !local.is_empty()
                && !s.chars().any(char::is_whitespace)
                && domain.contains('.')
                && !domain.starts_with('.')
                && !domain.ends_with('.')
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_is_none() {
        assert_eq!(normalize_link(""), None);
        assert_eq!(normalize_link("   "), None);
    }

    #[test]
    fn keeps_existing_scheme() {
        assert_eq!(
            normalize_link("https://a.com"),
            Some("https://a.com".into())
        );
        assert_eq!(
            normalize_link("http://a.com/x?y=1"),
            Some("http://a.com/x?y=1".into())
        );
        assert_eq!(
            normalize_link("mailto:x@a.com"),
            Some("mailto:x@a.com".into())
        );
        assert_eq!(normalize_link("  tel:+1-555  "), Some("tel:+1-555".into()));
        // Scheme match is case-insensitive but the value is preserved verbatim.
        assert_eq!(
            normalize_link("HTTPS://A.com"),
            Some("HTTPS://A.com".into())
        );
    }

    #[test]
    fn bare_host_gets_https() {
        assert_eq!(
            normalize_link("example.com"),
            Some("https://example.com".into())
        );
        assert_eq!(
            normalize_link("example.com/path"),
            Some("https://example.com/path".into())
        );
        assert_eq!(
            normalize_link("www.example.com"),
            Some("https://www.example.com".into())
        );
    }

    #[test]
    fn email_gets_mailto() {
        assert_eq!(
            normalize_link("bob@example.com"),
            Some("mailto:bob@example.com".into())
        );
        assert_eq!(
            normalize_link("first.last@sub.example.co"),
            Some("mailto:first.last@sub.example.co".into())
        );
    }

    #[test]
    fn at_without_domain_dot_is_not_email() {
        // No dot in the domain → not email-like → treated as a path.
        assert_eq!(
            normalize_link("user@localhost"),
            Some("https://user@localhost".into())
        );
    }
}
