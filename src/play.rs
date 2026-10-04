//! Google Play scraping — public web pages, no API.
//! Search results live in the page's `AF_initDataCallback({key:'ds:N', data:[...]})`
//! blobs; app entries are located by structural signature (not fixed indexes),
//! which survives layout shifts better than key-position parsing.

use anyhow::{Context, Result};
use serde::Serialize;
use serde_json::Value;

use crate::http::HttpClient;
use crate::scoring;

#[derive(Debug, Clone, Serialize)]
pub struct PlaySearchApp {
    pub package: String,
    pub title: String,
    pub developer: String,
    pub rating: Option<f64>,
    pub genre: String,
    pub installs: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub short_description: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlayAppDetails {
    pub package: String,
    pub title: String,
    pub developer: String,
    pub rating: Option<f64>,
    pub reviews_count: Option<i64>,
    pub installs: Option<String>,
    pub genre: Option<String>,
    pub description: String,
}

/// Extract every `AF_initDataCallback({key: 'ds:N', data:[...]})` payload as JSON,
/// in document order. Handles escaped strings via balanced-bracket scanning.
pub fn extract_data_blocks(html: &str) -> Vec<Value> {
    let mut out = Vec::new();
    let marker = "AF_initDataCallback({key: 'ds:";
    let mut from = 0;
    while let Some(rel) = html[from..].find(marker) {
        let block_start = from + rel;
        // find "data:[" after the key
        let Some(ds_rel) = html[block_start..].find("data:[") else {
            break;
        };
        let arr_start = block_start + ds_rel + "data:".len();
        let bytes = html.as_bytes();
        let mut depth = 0usize;
        let mut i = arr_start;
        while i < bytes.len() {
            match bytes[i] {
                b'[' => depth += 1,
                b']' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                b'"' => {
                    // skip string literal
                    i += 1;
                    while i < bytes.len() && bytes[i] != b'"' {
                        if bytes[i] == b'\\' {
                            i += 1;
                        }
                        i += 1;
                    }
                }
                _ => {}
            }
            i += 1;
        }
        if i < bytes.len() {
            if let Ok(v) = serde_json::from_str(&html[arr_start..=i]) {
                out.push(v);
            }
        }
        from = block_start + marker.len();
    }
    out
}

fn is_package_like(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    parts.len() >= 3
        && parts.iter().all(|p| {
            !p.is_empty()
                && p.chars().next().map(|c| c.is_ascii_lowercase()).unwrap_or(false)
                && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        })
}

/// Structural signature of a search-result app entry:
/// [ [package, _], icons..., screenshots..., title, [ratingStr, rating], genre, ..., dev, installs ]
fn match_search_entry(node: &Value) -> Option<PlaySearchApp> {
    let arr = node.as_array()?;
    if arr.len() < 16 {
        return None;
    }
    let package = arr.first()?.get(0)?.as_str().filter(|s| is_package_like(s))?;
    let title = arr.get(3)?.as_str()?;
    if title.is_empty() || title.starts_with("http") {
        return None;
    }
    let rating = arr
        .get(4)?
        .get(1)
        .and_then(Value::as_f64)
        .filter(|r| (0.0..=5.0).contains(r))?;
    let genre = arr.get(5)?.as_str()?.to_string();
    let developer = arr.get(14).and_then(Value::as_str).unwrap_or("").to_string();
    let installs = arr
        .get(15)
        .and_then(Value::as_str)
        .filter(|s| s.ends_with('+'))
        .unwrap_or("")
        .to_string();
    let short_description = arr
        .get(13)
        .and_then(|v| v.get(1))
        .and_then(Value::as_str)
        .map(str::to_string);

    Some(PlaySearchApp {
        package: package.to_string(),
        title: title.to_string(),
        developer,
        rating: Some(rating),
        genre,
        installs,
        short_description,
    })
}

fn collect_search_apps(node: &Value, out: &mut Vec<PlaySearchApp>, seen: &mut Vec<String>) {
    if let Some(app) = match_search_entry(node) {
        if !seen.contains(&app.package) {
            seen.push(app.package.clone());
            out.push(app);
        }
    }
    if let Value::Array(items) = node {
        for item in items {
            collect_search_apps(item, out, seen);
        }
    }
}

/// Ranked Play search results (page order = rank). `limit` caps the result count.
pub async fn search(
    http: &HttpClient,
    term: &str,
    country: &str,
    language: &str,
    limit: usize,
) -> Result<Vec<PlaySearchApp>> {
    let url = format!(
        "https://play.google.com/store/search?q={}&c=apps&hl={}&gl={}",
        crate::itunes::urlencoded(term),
        crate::itunes::urlencoded(language),
        country.to_uppercase()
    );
    let html = http.get(&url, &[]).await?;
    let mut apps = parse_search_html(&html);
    apps.truncate(limit);
    Ok(apps)
}

pub fn parse_search_html(html: &str) -> Vec<PlaySearchApp> {
    let mut best: Vec<PlaySearchApp> = Vec::new();
    for block in extract_data_blocks(html) {
        let mut apps = Vec::new();
        let mut seen = Vec::new();
        collect_search_apps(&block, &mut apps, &mut seen);
        if apps.len() > best.len() {
            best = apps;
        }
    }
    best
}

fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.trim().to_string()
}

fn decode_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&#39;", "'")
        .replace("&quot;", "\"")
}

fn between(html: &str, open: &str, close: &str) -> Option<String> {
    let start = html.find(open)? + open.len();
    let end = html[start..].find(close)? + start;
    Some(html[start..end].to_string())
}

fn parse_compact_number(s: &str) -> Option<i64> {
    let t = s.trim().replace(',', "");
    if let Ok(v) = t.parse::<i64>() {
        return Some(v);
    }
    let (num, mult) = match t.chars().last() {
        Some('K') => (t[..t.len() - 1].parse::<f64>().ok()?, 1_000.0),
        Some('M') => (t[..t.len() - 1].parse::<f64>().ok()?, 1_000_000.0),
        _ => return None,
    };
    Some((num * mult) as i64)
}

fn find_reviews_count(html: &str) -> Option<i64> {
    let mut from = 0;
    while let Some(rel) = html[from..].find(" reviews") {
        let pos = from + rel;
        let before = &html[..pos];
        let start = before.rfind('>').map(|p| p + 1).unwrap_or(0);
        if let Some(n) = parse_compact_number(before[start..].trim()) {
            return Some(n);
        }
        from = pos + 1;
    }
    None
}

fn find_installs(html: &str) -> Option<String> {
    // The downloads chip renders like >100,000+< (or >100K+<). Prefer candidates
    // containing a thousands separator or a magnitude letter to avoid picking up
    // unrelated small counters.
    let mut first_any: Option<String> = None;
    let mut from = 0;
    while let Some(rel) = html[from..].find("+<") {
        let pos = from + rel;
        let start = html[..pos].rfind('>').map(|p| p + 1)?;
        let candidate = html[start..pos].trim();
        let core = candidate.trim_end_matches('+');
        if !core.is_empty()
            && core
                .chars()
                .all(|c| c.is_ascii_digit() || c == ',' || c == '.' || c == 'K' || c == 'M')
        {
            let strong = core.contains(',') || core.contains('K') || core.contains('M');
            if strong {
                return Some(format!("{candidate}+"));
            }
            if first_any.is_none() {
                first_any = Some(format!("{candidate}+"));
            }
        }
        from = pos + 1;
    }
    first_any
}

fn find_genre(html: &str) -> Option<String> {
    // The header genre chip is `<a href="/store/apps/category/CODE" aria-label="Name">`;
    // nav/footer category links lack the aria-label, so prefer ones that have it.
    let needle = "/store/apps/category/";
    let mut first_code: Option<String> = None;
    let mut from = 0;
    while let Some(rel) = html[from..].find(needle) {
        let code_start = from + rel + needle.len();
        let code_end = code_start + (html[code_start..].find('"')?);
        let code = &html[code_start..code_end];
        if code.len() > 2 && code.chars().all(|c| c.is_ascii_uppercase() || c == '_') {
            let window_end = (code_end + 120).min(html.len());
            if let Some(a_rel) = html[code_end..window_end].find("aria-label=\"") {
                let name_start = code_end + a_rel + "aria-label=\"".len();
                if let Some(q) = html[name_start..].find('"') {
                    let name = decode_entities(html[name_start..name_start + q].trim());
                    if !name.is_empty() {
                        return Some(name);
                    }
                }
            }
            if first_code.is_none() {
                first_code = Some(code.to_string());
            }
        }
        from = code_start;
    }
    first_code
}

fn find_developer(html: &str) -> Option<String> {
    // Developer link sits immediately after the title:
    // <a href="/store/apps/developer?id=Name"><span>Name</span></a>
    let title_pos = html.find("itemprop=\"name\"")?;
    let region = &html[title_pos..(title_pos + 3000).min(html.len())];
    let dev_rel = region
        .find("/store/apps/developer")
        .or_else(|| region.find("/store/apps/dev?"))?;
    let seg = &region[dev_rel..];
    let span_start = seg.find("<span>")? + "<span>".len();
    let span_end = span_start + (seg[span_start..].find("</span>")?);
    Some(decode_entities(seg[span_start..span_end].trim()))
}

/// Details-page fields via stable HTML anchors.
pub fn parse_details_html(html: &str, package: &str) -> PlayAppDetails {
    let title = between(html, "itemprop=\"name\">", "</span>")
        .map(|s| decode_entities(s.trim()))
        .unwrap_or_else(|| package.to_string());
    let rating = between(html, "Rated ", " stars out of five")
        .and_then(|s| s.trim().parse::<f64>().ok());
    let reviews_count = find_reviews_count(html);
    let installs = find_installs(html);
    let genre = find_genre(html);
    let description = html
        .find("data-g-id=\"description\"")
        .and_then(|pos| {
            let rest = &html[pos..];
            let start = rest.find('>')? + 1;
            let end = rest[start..].find("</div>")? + start;
            Some(decode_entities(&strip_tags(&rest[start..end])))
        })
        .unwrap_or_default();
    let developer = find_developer(html).unwrap_or_default();

    PlayAppDetails {
        package: package.to_string(),
        title,
        developer,
        rating,
        reviews_count,
        installs,
        genre,
        description,
    }
}

pub async fn app_details(
    http: &HttpClient,
    package: &str,
    country: &str,
    language: &str,
) -> Result<PlayAppDetails> {
    let url = format!(
        "https://play.google.com/store/apps/details?id={}&hl={}&gl={}",
        crate::itunes::urlencoded(package),
        crate::itunes::urlencoded(language),
        country.to_uppercase()
    );
    let html = http.get(&url, &[]).await?;
    Ok(parse_details_html(&html, package))
}

pub async fn find_rank(
    http: &HttpClient,
    term: &str,
    package: &str,
    country: &str,
    language: &str,
) -> Result<(Option<usize>, usize)> {
    let apps = search(http, term, country, language, 0).await?;
    let rank = apps.iter().position(|a| a.package == package);
    Ok((rank.map(|i| i + 1), apps.len()))
}

#[derive(Debug, Serialize)]
pub struct PlayAnalyzedApp {
    pub rank: usize,
    #[serde(flatten)]
    pub app: PlaySearchApp,
    pub title_match_score: i32,
    pub exact_match_in_title: bool,
}

#[derive(Debug, Serialize)]
pub struct PlaySummary {
    pub app_count: usize,
    pub avg_title_match_score: f64,
    pub exact_title_match_ratio: f64,
    pub top5_median_reviews: i64,
    /// 0–100, higher = harder (Play formula: review floor + title match + exact ratio)
    pub difficulty: f64,
}

/// Keyword difficulty on Play: search top-10 for title matching, then fetch details
/// for the top 5 to get review counts (Play shows no rating velocity or dates).
pub async fn analyze_keyword(
    http: &HttpClient,
    term: &str,
    country: &str,
    language: &str,
    details_top_n: usize,
) -> Result<(Vec<PlayAnalyzedApp>, PlaySummary)> {
    let apps = search(http, term, country, language, 10).await?;
    let search_words: Vec<String> = term.to_lowercase().split_whitespace().map(String::from).collect();
    let variants: Vec<Vec<String>> = search_words.iter().map(|w| scoring::word_variants(w)).collect();

    let analyzed: Vec<PlayAnalyzedApp> = apps
        .iter()
        .enumerate()
        .map(|(i, app)| {
            let title = app.title.to_lowercase();
            let exact = title.contains(&term.to_lowercase());
            PlayAnalyzedApp {
                rank: i + 1,
                app: app.clone(),
                title_match_score: if exact {
                    5
                } else {
                    scoring::count_word_matches(&title, &variants)
                },
                exact_match_in_title: exact,
            }
        })
        .collect();

    // Review counts require per-app detail pages; fetch only the top N.
    let mut reviews: Vec<i64> = Vec::new();
    for app in analyzed.iter().take(details_top_n) {
        if let Ok(d) = app_details(http, &app.app.package, country, language).await {
            if let Some(c) = d.reviews_count {
                reviews.push(c);
            }
        }
    }
    reviews.sort_unstable();
    let top5_median = if reviews.is_empty() {
        0
    } else {
        reviews[reviews.len() / 2]
    };

    let n = analyzed.len().max(1) as f64;
    let avg_title = analyzed.iter().map(|a| a.title_match_score as f64).sum::<f64>() / n;
    let exact_ratio = analyzed
        .iter()
        .filter(|a| a.exact_match_in_title)
        .count() as f64
        / n;
    let rating_pressure = (top5_median as f64 / 50_000.0).min(1.0) * 100.0;
    let difficulty = (0.45 * rating_pressure + 0.35 * avg_title * 20.0 + 0.20 * exact_ratio * 100.0)
        .clamp(0.0, 100.0);

    let app_count = analyzed.len();
    Ok((
        analyzed,
        PlaySummary {
            app_count,
            avg_title_match_score: avg_title,
            exact_title_match_ratio: exact_ratio,
            top5_median_reviews: top5_median,
            difficulty,
        },
    ))
}

#[derive(Debug, Clone, Serialize)]
pub struct PlayReview {
    pub id: String,
    pub author: String,
    pub score: u8,
    pub text: String,
    pub thumbs_up: i64,
    pub timestamp_unix: i64,
}

/// Google Play reviews via batchexecute rpcid `oCPfdb`
/// (verified against live endpoint; row layout: 0=id, 1=[name,..], 2=score,
/// 4=text, 5=[unix,nanos], 6=thumbs; token at inner[1][1]).
pub async fn reviews(
    http: &HttpClient,
    package: &str,
    country: &str,
    language: &str,
    max: usize,
) -> Result<Vec<PlayReview>> {
    let mut out: Vec<PlayReview> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut token: Option<String> = None;

    for _page in 0..10 {
        let paging = match &token {
            Some(t) => format!("[150,null,\"{}\"]", t.replace('\\', "\\\\").replace('"', "\\\"")),
            None => "[150]".to_string(),
        };
        let payload = format!(
            "[null,[2,2,{},null,null],[\"{}\",7]]",
            paging,
            package.replace('\\', "\\\\").replace('"', "\\\"")
        );
        let envelope = format!(
            "f.req={}",
            crate::itunes::urlencoded(&json_string(&[[[
                "oCPfdb".to_string(),
                payload.clone(),
                String::new(),
                "generic".to_string(),
            ]]]))
        );
        let url = format!(
            "https://play.google.com/_/PlayStoreUi/data/batchexecute?rpcids=oCPfdb&hl={}&gl={}",
            crate::itunes::urlencoded(language),
            country.to_uppercase()
        );
        let body = http
            .post(&url, &[("Content-Type", "application/x-www-form-urlencoded;charset=UTF-8".to_string())], envelope)
            .await?;

        let (page_reviews, next_token) = parse_reviews_response(&body)?;
        for r in page_reviews {
            if seen.insert(r.id.clone()) {
                out.push(r);
            }
        }
        match next_token {
            Some(t) if out.len() < max => token = Some(t),
            _ => break,
        }
    }
    out.truncate(max);
    Ok(out)
}

fn json_string<T: serde::Serialize>(v: &T) -> String {
    serde_json::to_string(v).unwrap_or_default()
}

/// Strips the `)]}'` prefix, finds the `wrb.fr`/`oCPfdb` line, parses the
/// inner JSON, and returns (reviews, next_page_token).
pub fn parse_reviews_response(body: &str) -> Result<(Vec<PlayReview>, Option<String>)> {
    let mut reviews = Vec::new();
    let mut token = None;
    for line in body.lines() {
        if !line.contains("\"wrb.fr\"") || !line.contains("oCPfdb") {
            continue;
        }
        let outer: Value = serde_json::from_str(line)
            .with_context(|| format!("parsing batchexecute line: {line:.120}"))?;
        let Some(inner_str) = outer.get(0).and_then(|w| w.get(2)).and_then(Value::as_str) else {
            continue;
        };
        let inner: Value = serde_json::from_str(inner_str)
            .with_context(|| format!("parsing inner payload: {inner_str:.120}"))?;
        for row in inner.get(0).and_then(Value::as_array).into_iter().flatten() {
            let id = row.get(0).and_then(Value::as_str).unwrap_or_default().to_string();
            if id.is_empty() {
                continue;
            }
            reviews.push(PlayReview {
                id,
                author: row
                    .get(1)
                    .and_then(|a| a.get(0))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                score: row.get(2).and_then(Value::as_u64).unwrap_or(0) as u8,
                text: row
                    .get(4)
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                thumbs_up: row.get(6).and_then(Value::as_i64).unwrap_or(0),
                timestamp_unix: row
                    .get(5)
                    .and_then(|t| t.get(0))
                    .and_then(Value::as_i64)
                    .unwrap_or(0),
            });
        }
        token = inner
            .get(1)
            .and_then(|t| t.get(1))
            .and_then(Value::as_str)
            .map(str::to_string);
        break;
    }
    Ok((reviews, token))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reviews_response_fixture() {
        let body = ")]}'\n\n[[\"er\",2]]\n[[\"wrb.fr\",\"oCPfdb\",\"[[[\\\"abc\\\",[\\\"Ann\\\"],5,null,\\\"great app\\\",[1790566706,0],3]],[null,\\\"TOKEN1\\\"]]\",null,\"generic\"]]\n";
        let (reviews, token) = parse_reviews_response(body).unwrap();
        assert_eq!(reviews.len(), 1);
        assert_eq!(reviews[0].id, "abc");
        assert_eq!(reviews[0].author, "Ann");
        assert_eq!(reviews[0].score, 5);
        assert_eq!(reviews[0].text, "great app");
        assert_eq!(reviews[0].thumbs_up, 3);
        assert_eq!(reviews[0].timestamp_unix, 1790566706);
        assert_eq!(token.as_deref(), Some("TOKEN1"));
    }

    #[test]
    fn package_like() {
        assert!(is_package_like("de.iab.meditationtimer"));
        assert!(is_package_like("uk.co.telesense.tm.free"));
        assert!(!is_package_like("https://example.com"));
        assert!(!is_package_like("com"));
        assert!(!is_package_like("not a package"));
    }

    #[test]
    fn search_fixture() {
        let html = std::fs::read_to_string("tests/fixtures/play_search.html")
            .expect("fixture");
        let apps = parse_search_html(&html);
        assert!(apps.len() >= 3, "expected >=3 apps, got {}", apps.len());
        assert!(apps[0].package.contains('.'));
        assert!(!apps[0].title.is_empty());
        assert!(apps[0].rating.unwrap_or(0.0) > 0.0);
    }

    #[test]
    fn compact_numbers() {
        assert_eq!(parse_compact_number("4.32K"), Some(4320));
        assert_eq!(parse_compact_number("2.5M"), Some(2_500_000));
        assert_eq!(parse_compact_number("100,000"), Some(100_000));
    }
}
