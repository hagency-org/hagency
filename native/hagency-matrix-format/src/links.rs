pub(crate) fn normalize(url: &str) -> String {
    // Preserve relative and Matrix URI semantics: generic browser URL parsers
    // resolve or normalize these differently. Encode only the host for schemes
    // for which the retained JS formatter performs IDN conversion.
    let lower = url.to_ascii_lowercase();
    let start = ["http://", "https://", "mailto:", "//"]
        .iter()
        .find_map(|prefix| lower.starts_with(prefix).then_some(prefix.len()));
    let mut value = url.to_owned();
    if let Some(start) = start {
        let end = url[start..]
            .find(['/', '?', '#'])
            .map_or(url.len(), |i| start + i);
        let auth = url[start..end].rfind('@').map_or(start, |i| start + i + 1);
        let host_end = url[auth..end].find(':').map_or(end, |i| auth + i);
        let host = &url[auth..host_end];
        if !host.is_ascii() {
            // JS uses raw punycode, not UTS46: do not map fullwidth letters or
            // otherwise change the destination's hostname during formatting.
            let labels: Option<Vec<_>> = host
                .split(['.', '\u{3002}', '\u{ff0e}', '\u{ff61}'])
                .map(|label| {
                    if label.is_ascii() {
                        Some(label.to_owned())
                    } else {
                        idna::punycode::encode_str(label).map(|s| format!("xn--{s}"))
                    }
                })
                .collect();
            if let Some(labels) = labels {
                value.replace_range(auth..host_end, &labels.join("."));
            }
        }
    }
    let encode = |s: &str| {
        mdurl::urlencode::encode(s, mdurl::urlencode::ENCODE_DEFAULT_CHARS, true).into_owned()
    };
    if let Some(start) = start {
        let end = value[start..]
            .find(['/', '?', '#'])
            .map_or(value.len(), |i| start + i);
        let host = value[start..end]
            .rfind('@')
            .map_or(start, |i| start + i + 1);
        if value[host..end].starts_with('[')
            && let Some(close) = value[host..end].find(']')
        {
            let close = host + close;
            if value[host + 1..close].parse::<std::net::Ipv6Addr>().is_ok() {
                return format!(
                    "{}[{}]{}",
                    encode(&value[..host]),
                    &value[host + 1..close],
                    encode(&value[close + 1..])
                );
            }
        }
    }
    encode(&value)
}

pub(crate) const TAGS: &[&str] = &[
    "p",
    "br",
    "strong",
    "em",
    "del",
    "s",
    "blockquote",
    "ul",
    "ol",
    "li",
    "pre",
    "code",
    "a",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "hr",
    "table",
    "thead",
    "tbody",
    "tr",
    "th",
    "td",
];
// Equivalent to sanitize-html 2.17.7 / launder's scheme check, after entity
// decoding by the Markdown parser. The original normalized href is emitted.
pub(crate) fn safe_href(url: &str) -> bool {
    let mut clean: String = url.chars().filter(|c| *c > '\u{20}').collect();
    while let Some(start) = clean.find("<!--") {
        let Some(end) = clean[start + 4..].find("-->") else {
            break;
        };
        clean.replace_range(start..start + 4 + end + 3, "");
    }
    if let Some((scheme, _)) = clean.split_once(':')
        && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+'))
    {
        return matches!(
            scheme.to_ascii_lowercase().as_str(),
            "http" | "https" | "mailto" | "matrix"
        );
    }
    !clean
        .as_bytes()
        .get(..2)
        .is_some_and(|s| s.iter().all(|b| matches!(b, b'/' | b'\\')))
}

pub(crate) fn markdown_link(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    !["javascript:", "vbscript:", "file:", "data:"]
        .iter()
        .any(|p| lower.starts_with(p))
        || [
            "data:image/gif;",
            "data:image/png;",
            "data:image/jpeg;",
            "data:image/webp;",
        ]
        .iter()
        .any(|p| lower.starts_with(p))
}
pub(crate) fn display(url: &str) -> String {
    let mut value = url.to_owned();
    let lower = url.to_ascii_lowercase();
    if let Some(start) = ["http://", "https://", "mailto:", "//"]
        .iter()
        .find_map(|p| lower.starts_with(p).then_some(p.len()))
    {
        let end = url[start..]
            .find(['/', '?', '#'])
            .map_or(url.len(), |i| start + i);
        let auth = url[start..end].rfind('@').map_or(start, |i| start + i + 1);
        let host_end = url[auth..end].find(':').map_or(end, |i| auth + i);
        let labels: Option<Vec<_>> = url[auth..host_end]
            .split('.')
            .map(|label| {
                if label.to_ascii_lowercase().starts_with("xn--") {
                    idna::punycode::decode_to_string(&label[4..])
                } else {
                    Some(label.into())
                }
            })
            .collect();
        if let Some(labels) = labels {
            value.replace_range(auth..host_end, &labels.join("."));
        }
    }
    mdurl::urlencode::decode(&value, mdurl::urlencode::DECODE_DEFAULT_CHARS).into_owned()
}
pub(crate) fn unicode_punctuation(c: char) -> bool {
    use unicode_general_category::{GeneralCategory::*, get_general_category};
    !c.is_ascii()
        && matches!(
            get_general_category(c),
            ClosePunctuation
                | ConnectorPunctuation
                | DashPunctuation
                | FinalPunctuation
                | InitialPunctuation
                | OpenPunctuation
                | OtherPunctuation
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_matrix_format_security_href_normalization() {
        for i in 0u8..=32 {
            let inserted = char::from(i);
            assert!(!safe_href(&format!("java{inserted}script:evil")));
            assert!(!safe_href(&format!("/{inserted}/evil.example")));
        }
        for input in [
            "javascript:bad",
            "JaVaScRiPt:bad",
            "java<!--ignored-->script:bad",
            "//evil",
            "\\\\evil",
            "/\\evil",
            "\\/evil",
            "ftp://example.org",
            "data:image/png;base64,AAA",
            "f.o+o-bar:unknown",
        ] {
            assert!(!safe_href(input), "{input}");
        }
        for input in [
            "https://example.org",
            "HTTP://example.org",
            "mailto:a@example.org",
            "matrix:u/alice:example.org",
            "/path",
            "../path",
            "#anchor",
            "?query=1",
            "relative",
        ] {
            assert!(safe_href(input), "{input}");
        }
    }
}
