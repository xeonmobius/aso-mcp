use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::http::HttpClient;

/// App metadata from the iTunes Search/Lookup API.
/// Deserialized from Apple's camelCase keys, serialized to snake_case for tool output.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct App {
    #[serde(rename = "trackId")]
    pub track_id: i64,
    #[serde(rename = "trackName")]
    pub track_name: String,
    #[serde(rename = "artistName")]
    pub artist_name: String,
    #[serde(rename = "sellerName", default)]
    pub seller_name: String,
    #[serde(rename = "averageUserRating")]
    pub average_user_rating: Option<f64>,
    #[serde(rename = "userRatingCount")]
    pub user_rating_count: Option<i64>,
    #[serde(rename = "formattedPrice", default)]
    pub formatted_price: String,
    #[serde(rename = "primaryGenreName", default)]
    pub primary_genre_name: String,
    #[serde(default)]
    pub version: String,
    #[serde(rename = "releaseDate", default)]
    pub release_date: String,
    #[serde(rename = "currentVersionReleaseDate", default)]
    pub current_version_release_date: String,
    #[serde(default)]
    pub description: String,
    #[serde(rename = "bundleId", default)]
    pub bundle_id: String,
    #[serde(rename = "trackViewUrl", default)]
    pub track_view_url: String,
    #[serde(rename = "contentAdvisoryRating", default)]
    pub content_advisory_rating: String,
    #[serde(rename = "minimumOsVersion", default)]
    pub minimum_os_version: String,
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    #[serde(rename = "resultCount", default)]
    result_count: i64,
    #[serde(default)]
    results: Vec<App>,
}

pub enum LookupRef {
    Ids(Vec<String>),
    BundleId(String),
    Url(String),
}

fn extract_id_from_url(url: &str) -> Option<String> {
    // https://apps.apple.com/us/app/yelp/id284910350 -> 284910350
    let after = url.split("id").last()?;
    let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() { None } else { Some(digits) }
}

pub async fn search(
    http: &HttpClient,
    term: &str,
    country: &str,
    limit: u32,
    attribute: Option<&str>,
) -> Result<Vec<App>> {
    let mut url = format!(
        "https://itunes.apple.com/search?term={}&media=software&entity=software&limit={}&country={}",
        urlencoded(term),
        limit,
        country.to_uppercase()
    );
    if let Some(attr) = attribute {
        url.push_str(&format!("&attribute={}", urlencoded(attr)));
    }
    let body = http.get(&url, &[]).await?;
    let parsed: SearchResponse =
        serde_json::from_str(&body).with_context(|| format!("parsing search response: {body:.200}"))?;
    Ok(parsed.results)
}

pub async fn lookup(http: &HttpClient, reference: LookupRef, country: &str) -> Result<Vec<App>> {
    let url = match reference {
        LookupRef::Ids(ids) => format!(
            "https://itunes.apple.com/lookup?id={}&country={}",
            ids.join(","),
            country.to_uppercase()
        ),
        LookupRef::Url(u) => {
            let id = extract_id_from_url(&u).context("no numeric app id found in URL")?;
            format!(
                "https://itunes.apple.com/lookup?id={id}&country={}",
                country.to_uppercase()
            )
        }
        LookupRef::BundleId(b) => format!(
            "https://itunes.apple.com/lookup?bundleId={}&country={}",
            urlencoded(&b),
            country.to_uppercase()
        ),
    };
    let body = http.get(&url, &[]).await?;
    let parsed: SearchResponse =
        serde_json::from_str(&body).with_context(|| format!("parsing lookup response: {body:.200}"))?;
    Ok(parsed.results)
}

/// Percent-encode for query strings (conservative: alphanumerics and -._~ kept).
pub fn urlencoded(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urlencoding() {
        assert_eq!(urlencoded("meditation app"), "meditation%20app");
        assert_eq!(urlencoded("café"), "caf%C3%A9");
    }

    #[test]
    fn url_id_extraction() {
        assert_eq!(
            extract_id_from_url("https://apps.apple.com/us/app/yelp/id284910350"),
            Some("284910350".into())
        );
        assert_eq!(extract_id_from_url("https://example.com/no-id"), None);
    }
}
