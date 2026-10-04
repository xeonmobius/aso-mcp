use anyhow::Result;

use crate::http::HttpClient;
use crate::itunes;
use crate::mzstore;

/// App Store search-bar autocomplete (MZSearchHints). Undocumented; response is an
/// XML plist `{"title": "Suggestions", "hints": [{"displayTerm": ..., "term": ...}]}`.
/// The endpoint returns structurally-valid but EMPTY hints without a storefront header
/// (verified live), so we send storefront headers and fall back across param variants.
pub async fn keyword_hints(
    http: &HttpClient,
    term: &str,
    storefront: &str,
    language: &str,
) -> Result<Vec<String>> {
    let sid = mzstore::store_id_for(storefront);
    let encoded = itunes::urlencoded(term);
    let lang = if language.is_empty() {
        "en-us".to_string()
    } else {
        language.to_lowercase()
    };

    let attempts: [(&str, String); 2] = [
        (
            "term",
            format!("{sid},24 t:native"),
        ),
        ("q", format!("{sid}-1")),
    ];

    let mut last_body = String::new();
    for (param, front) in attempts {
        let url = format!(
            "https://search.itunes.apple.com/WebObjects/MZSearchHints.woa/wa/hints?clientApplication=Software&{param}={encoded}"
        );
        let headers = [
            ("X-Apple-Store-Front", front),
            ("Accept-Language", lang.clone()),
        ];
        let body = http.get(&url, &headers).await?;
        let hints = parse_hints(&body);
        if !hints.is_empty() {
            return Ok(hints);
        }
        last_body = body;
    }
    let _ = last_body;
    Ok(Vec::new())
}

/// Extract suggestion terms from either an XML plist or a JSON response body.
pub fn parse_hints(body: &str) -> Vec<String> {
    let trimmed = body.trim_start();
    if trimmed.starts_with('<') {
        return parse_plist_hints(body);
    }
    // Some storefronts/responders return JSON: {"hints":[{"displayTerm":..}]} etc.
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) {
        let arr = v
            .get("hints")
            .and_then(|h| h.as_array())
            .or_else(|| v.as_array());
        if let Some(arr) = arr {
            let mut out = Vec::new();
            for item in arr {
                if let Some(s) = string_from_hint(&serde_json::to_value(item).unwrap_or_default()) {
                    out.push(s);
                }
            }
            return out;
        }
    }
    Vec::new()
}

fn string_from_hint(v: &serde_json::Value) -> Option<String> {
    if let Some(s) = v.as_str() {
        return Some(s.to_string());
    }
    for key in ["displayTerm", "term"] {
        if let Some(s) = v.get(key).and_then(|x| x.as_str()) {
            return Some(s.to_string());
        }
    }
    None
}

fn parse_plist_hints(body: &str) -> Vec<String> {
    let cursor = std::io::Cursor::new(body.as_bytes());
    let Ok(value) = plist::Value::from_reader(cursor) else {
        return Vec::new();
    };
    let Some(dict) = value.as_dictionary() else {
        return Vec::new();
    };
    let Some(hints) = dict.get("hints").and_then(|h| h.as_array()) else {
        return Vec::new();
    };
    hints
        .iter()
        .filter_map(|h| {
            let d = h.as_dictionary()?;
            for key in ["displayTerm", "term"] {
                if let Some(s) = d.get(key).and_then(plist::Value::as_string) {
                    return Some(s.to_string());
                }
            }
            None
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plist_fixture() {
        let body = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple Computer//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict><key>title</key><string>Suggestions</string><key>hints</key>
<array><dict><key>displayTerm</key><string>meditation free</string><key>term</key><string>meditation free</string></dict>
<dict><key>displayTerm</key><string>meditation timer</string><key>term</key><string>meditation timer</string></dict></array></dict></plist>"#;
        assert_eq!(
            parse_hints(body),
            vec!["meditation free".to_string(), "meditation timer".to_string()]
        );
    }

    #[test]
    fn empty_plist_fixture() {
        let body = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple Computer//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict><key>title</key><string>Suggestions</string><key>hints</key><array/></dict></plist>"#;
        assert!(parse_hints(body).is_empty());
    }
}
