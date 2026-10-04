use std::sync::Arc;

use rmcp::{
    ErrorData as McpError, ServerHandler, ServiceExt,
    handler::server::router::tool::ToolRouter,
    handler::server::wrapper::Parameters,
    model::{CallToolResult, ContentBlock, Implementation, ServerCapabilities, ServerConfig},
    tool, tool_handler, tool_router,
    transport::stdio,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

mod hints;
mod http;
mod itunes;
mod mzstore;
mod play;
mod scoring;

pub struct Ctx {
    http: http::HttpClient,
}

#[derive(Clone)]
pub struct Asomcp {
    ctx: Arc<Ctx>,
    tool_router: ToolRouter<Asomcp>,
}

fn err(e: impl std::fmt::Display) -> McpError {
    McpError::internal_error(e.to_string(), None)
}

// ---- tool argument structs (doc comments become schema descriptions) ----

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct SearchArgs {
    /// Search term(s)
    pub term: String,
    /// Two-letter ISO country code, e.g. "US" (default US)
    pub country: Option<String>,
    /// Max results, 1-200 (default 25)
    pub limit: Option<u32>,
    /// Optional iTunes search attribute filter, e.g. "TitleTerm"
    pub attribute: Option<String>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct LookupArgs {
    /// Numeric app ID(s), comma-separated (e.g. "742044935" or "1,2,3")
    pub id: Option<String>,
    /// Bundle ID (e.g. "com.headspace.Happify")
    pub bundle_id: Option<String>,
    /// apps.apple.com URL (ID extracted from it)
    pub url: Option<String>,
    /// Two-letter ISO country code (default US)
    pub country: Option<String>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct SearchRankedArgs {
    /// Search term
    pub term: String,
    /// Two-letter ISO country code (default US)
    pub country: Option<String>,
    /// Max results, 1-200 (default 25)
    pub limit: Option<u32>,
    /// Language code like "en-us" (default en-us)
    pub language: Option<String>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct FindRankArgs {
    /// Keyword to check
    pub term: String,
    /// Numeric app ID to locate
    pub app_id: String,
    /// Two-letter ISO country code (default US)
    pub country: Option<String>,
    /// Language code like "en-us" (default en-us)
    pub language: Option<String>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct AnalyzeKeywordArgs {
    /// Keyword to analyze
    pub term: String,
    /// Two-letter ISO country code (default US)
    pub country: Option<String>,
    /// Language code like "en-us" (default en-us)
    pub language: Option<String>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct KeywordHintsArgs {
    /// Seed term for autocomplete suggestions
    pub term: String,
    /// Two-letter ISO country code (default US)
    pub country: Option<String>,
    /// BCP-47 language like "en-us", "de-de", "ja-jp" (default en-us)
    pub language: Option<String>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct CompetitorKeywordsArgs {
    /// Keyword whose top results to mine
    pub term: String,
    /// Two-letter ISO country code (default US)
    pub country: Option<String>,
    /// How many top apps to analyze, 1-50 (default 10)
    pub top_n: Option<u32>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct PlaySearchArgs {
    /// Search term
    pub term: String,
    /// Two-letter ISO country code (default US)
    pub country: Option<String>,
    /// Language, e.g. "en" (default en)
    pub language: Option<String>,
    /// Max results (default 25)
    pub limit: Option<u32>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct PlayAppDetailsArgs {
    /// Play package name, e.g. "com.headspace.happify"
    pub package: String,
    /// Two-letter ISO country code (default US)
    pub country: Option<String>,
    /// Language, e.g. "en" (default en)
    pub language: Option<String>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct PlayFindRankArgs {
    /// Keyword to check
    pub term: String,
    /// Play package name to locate
    pub package: String,
    /// Two-letter ISO country code (default US)
    pub country: Option<String>,
    /// Language, e.g. "en" (default en)
    pub language: Option<String>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct PlayAnalyzeKeywordArgs {
    /// Keyword to analyze
    pub term: String,
    /// Two-letter ISO country code (default US)
    pub country: Option<String>,
    /// Language, e.g. "en" (default en)
    pub language: Option<String>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct PlayCompetitorKeywordsArgs {
    /// Keyword whose top results to mine
    pub term: String,
    /// Two-letter ISO country code (default US)
    pub country: Option<String>,
    /// Language, e.g. "en" (default en)
    pub language: Option<String>,
    /// How many top apps to analyze, 1-50 (default 10)
    pub top_n: Option<u32>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct KeywordReportArgs {
    /// Seed keywords to evaluate (max 8)
    pub seeds: Vec<String>,
    /// Two-letter ISO country code (default US)
    pub country: Option<String>,
    /// Which store: "apple", "play", or "both" (default "apple")
    pub platform: Option<String>,
}

#[tool_router]
impl Asomcp {
    pub fn new() -> Self {
        Self {
            ctx: Arc::new(Ctx {
                http: http::HttpClient::new().expect("HTTP client"),
            }),
            tool_router: Self::tool_router(),
        }
    }

    #[tool(description = "Return the aso-mcp server version")]
    fn version(&self) -> Result<CallToolResult, McpError> {
        Ok(CallToolResult::success(vec![ContentBlock::text(
            env!("CARGO_PKG_VERSION"),
        )]))
    }

    #[tool(description = "Search the Apple App Store via the iTunes Search API. WARNING: result order is NOT the true store ranking; use appstore_search_ranked for rank-accurate results.")]
    async fn appstore_search(
        &self,
        Parameters(args): Parameters<SearchArgs>,
    ) -> Result<CallToolResult, McpError> {
        let apps = itunes::search(
            &self.ctx.http,
            &args.term,
            args.country.as_deref().unwrap_or("US"),
            args.limit.unwrap_or(25).min(200),
            args.attribute.as_deref(),
        )
        .await
        .map_err(err)?;
        to_result(&apps)
    }

    #[tool(description = "Look up Apple App Store apps by numeric ID (comma-separated list allowed), bundle ID, or apps.apple.com URL. Returns full metadata (title, developer, ratings, description, dates).")]
    async fn appstore_lookup(
        &self,
        Parameters(args): Parameters<LookupArgs>,
    ) -> Result<CallToolResult, McpError> {
        let reference = if let Some(ids) = args.id {
            itunes::LookupRef::Ids(ids.split(',').map(|s| s.trim().to_string()).collect())
        } else if let Some(b) = args.bundle_id {
            itunes::LookupRef::BundleId(b)
        } else if let Some(u) = args.url {
            itunes::LookupRef::Url(u)
        } else {
            return Err(McpError::invalid_params(
                "provide one of: id, bundle_id, url",
                None,
            ));
        };
        let apps = itunes::lookup(
            &self.ctx.http,
            reference,
            args.country.as_deref().unwrap_or("US"),
        )
        .await
        .map_err(err)?;
        to_result(&apps)
    }

    #[tool(description = "Search the App Store in TRUE ranked order (the order users actually see). Returns apps with their 1-based rank. Slower: two network calls.")]
    async fn appstore_search_ranked(
        &self,
        Parameters(args): Parameters<SearchRankedArgs>,
    ) -> Result<CallToolResult, McpError> {
        let storefront = args.country.as_deref().unwrap_or("US");
        let ranked = mzstore::ranked_apps(
            &self.ctx.http,
            &args.term,
            storefront,
            args.language.as_deref().unwrap_or("en-us"),
            args.limit.unwrap_or(25).min(200) as usize,
        )
        .await
        .map_err(err)?;
        let out: Vec<serde_json::Value> = ranked
            .into_iter()
            .map(|(rank, app)| json!({ "rank": rank, "app": app }))
            .collect();
        to_result(&out)
    }

    #[tool(description = "Find an app's 1-based rank in the App Store for a keyword (true store order). rank is null if the app is not in the top 200.")]
    async fn appstore_find_rank(
        &self,
        Parameters(args): Parameters<FindRankArgs>,
    ) -> Result<CallToolResult, McpError> {
        let storefront = args.country.as_deref().unwrap_or("US");
        let ids = mzstore::ranked_app_ids(
            &self.ctx.http,
            &args.term,
            storefront,
            args.language.as_deref().unwrap_or("en-us"),
        )
        .await
        .map_err(err)?;
        let rank = ids.iter().position(|x| x == &args.app_id).map(|i| i as u64 + 1);
        to_result(&json!({
            "term": args.term,
            "app_id": args.app_id,
            "rank": rank,
            "total_results": ids.len(),
        }))
    }

    #[tool(description = "Analyze keyword difficulty: fetches the true ranked top-20 for a keyword and scores competitiveness (difficulty 0-100, higher = harder) from title matching, review velocity, freshness, and incumbent dominance. Slow: two network calls.")]
    async fn appstore_analyze_keyword(
        &self,
        Parameters(args): Parameters<AnalyzeKeywordArgs>,
    ) -> Result<CallToolResult, McpError> {
        let storefront = args.country.as_deref().unwrap_or("US");
        let ranked = mzstore::ranked_apps(
            &self.ctx.http,
            &args.term,
            storefront,
            args.language.as_deref().unwrap_or("en-us"),
            20,
        )
        .await
        .map_err(err)?;
        let analyzed: Vec<scoring::AnalyzedApp> = ranked
            .iter()
            .map(|(rank, app)| scoring::analyze_app(&args.term, *rank, app))
            .collect();
        let summary = scoring::summarize(&analyzed);
        to_result(&json!({
            "term": args.term,
            "storefront": storefront,
            "summary": summary,
            "apps": analyzed,
        }))
    }

    #[tool(description = "Get App Store search-bar autocomplete suggestions for a term (live long-tail keyword mining). Suggestions are input-script driven: for DE/JP/BR markets pass a localized seed (e.g. メディテーション, meditação) to get that market's suggestions; latin seeds return what latin-typing users there see.")]
    async fn appstore_keyword_hints(
        &self,
        Parameters(args): Parameters<KeywordHintsArgs>,
    ) -> Result<CallToolResult, McpError> {
        let list = hints::keyword_hints(
            &self.ctx.http,
            &args.term,
            args.country.as_deref().unwrap_or("US"),
            args.language.as_deref().unwrap_or("en-us"),
        )
        .await
        .map_err(err)?;
        to_result(&json!({ "term": args.term, "hints": list }))
    }

    #[tool(description = "Extract keyword phrases (1-3 words) that the top-ranked competitors target in their titles for a keyword, with per-phrase frequency across apps.")]
    async fn appstore_competitor_keywords(
        &self,
        Parameters(args): Parameters<CompetitorKeywordsArgs>,
    ) -> Result<CallToolResult, McpError> {
        let storefront = args.country.as_deref().unwrap_or("US");
        let ranked = mzstore::ranked_apps(
            &self.ctx.http,
            &args.term,
            storefront,
            "en-us",
            args.top_n.unwrap_or(10).min(50) as usize,
        )
        .await
        .map_err(err)?;
        let mut freq: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
        for (_, app) in &ranked {
            for phrase in title_phrases(&app.track_name) {
                *freq.entry(phrase).or_insert(0) += 1;
            }
        }
        let mut phrases: Vec<(String, u32)> = freq.into_iter().collect();
        phrases.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        let out: Vec<serde_json::Value> = phrases
            .into_iter()
            .map(|(phrase, count)| json!({ "phrase": phrase, "apps": count }))
            .collect();
        to_result(&json!({ "term": args.term, "apps_analyzed": ranked.len(), "phrases": out }))
    }

    #[tool(description = "Google Play: search apps (page order = Play ranking). Returns package, title, developer, rating, installs per result. Slower: one network call per page.")]
    async fn play_search(
        &self,
        Parameters(args): Parameters<PlaySearchArgs>,
    ) -> Result<CallToolResult, McpError> {
        let apps = play::search(
            &self.ctx.http,
            &args.term,
            args.country.as_deref().unwrap_or("US"),
            args.language.as_deref().unwrap_or("en"),
            args.limit.unwrap_or(25).min(50) as usize,
        )
        .await
        .map_err(err)?;
        to_result(&json!({ "term": args.term, "results": apps }))
    }

    #[tool(description = "Google Play: full details for a package (title, developer, rating, review count, installs, genre, full description).")]
    async fn play_app_details(
        &self,
        Parameters(args): Parameters<PlayAppDetailsArgs>,
    ) -> Result<CallToolResult, McpError> {
        let details = play::app_details(
            &self.ctx.http,
            &args.package,
            args.country.as_deref().unwrap_or("US"),
            args.language.as_deref().unwrap_or("en"),
        )
        .await
        .map_err(err)?;
        to_result(&details)
    }

    #[tool(description = "Google Play: find an app's 1-based rank for a keyword. rank is null if not on the first page of results.")]
    async fn play_find_rank(
        &self,
        Parameters(args): Parameters<PlayFindRankArgs>,
    ) -> Result<CallToolResult, McpError> {
        let (rank, total) = play::find_rank(
            &self.ctx.http,
            &args.term,
            &args.package,
            args.country.as_deref().unwrap_or("US"),
            args.language.as_deref().unwrap_or("en"),
        )
        .await
        .map_err(err)?;
        to_result(&json!({
            "term": args.term,
            "package": args.package,
            "rank": rank,
            "results_on_page": total,
        }))
    }

    #[tool(description = "Google Play: analyze keyword difficulty. Fetches the top-10 ranked results for title matching and pulls review counts for the top 5. Returns difficulty 0-100 (higher = harder). Slow: ~6 network calls.")]
    async fn play_analyze_keyword(
        &self,
        Parameters(args): Parameters<PlayAnalyzeKeywordArgs>,
    ) -> Result<CallToolResult, McpError> {
        let storefront = args.country.as_deref().unwrap_or("US");
        let (apps, summary) = play::analyze_keyword(
            &self.ctx.http,
            &args.term,
            storefront,
            args.language.as_deref().unwrap_or("en"),
            5,
        )
        .await
        .map_err(err)?;
        to_result(&json!({
            "term": args.term,
            "storefront": storefront,
            "summary": summary,
            "apps": apps,
        }))
    }

    #[tool(description = "Google Play: extract keyword phrases (1-3 words) that top-ranked competitors target in their titles for a keyword, with frequencies.")]
    async fn play_competitor_keywords(
        &self,
        Parameters(args): Parameters<PlayCompetitorKeywordsArgs>,
    ) -> Result<CallToolResult, McpError> {
        let apps = play::search(
            &self.ctx.http,
            &args.term,
            args.country.as_deref().unwrap_or("US"),
            args.language.as_deref().unwrap_or("en"),
            args.top_n.unwrap_or(10).min(50) as usize,
        )
        .await
        .map_err(err)?;
        let mut freq: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
        for app in &apps {
            for phrase in title_phrases(&app.title) {
                *freq.entry(phrase).or_insert(0) += 1;
            }
        }
        let mut phrases: Vec<(String, u32)> = freq.into_iter().collect();
        phrases.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        let out: Vec<serde_json::Value> = phrases
            .into_iter()
            .map(|(phrase, count)| json!({ "phrase": phrase, "apps": count }))
            .collect();
        to_result(&json!({ "term": args.term, "apps_analyzed": apps.len(), "phrases": out }))
    }

    #[tool(description = "Merged keyword report across seeds: runs discovery (autocomplete hints where available) + difficulty scoring per seed and returns a markdown table with target/maybe/skip verdicts. platform: 'apple', 'play', or 'both'. Slow: ~2-7 network calls per seed.")]
    async fn keyword_report(
        &self,
        Parameters(args): Parameters<KeywordReportArgs>,
    ) -> Result<CallToolResult, McpError> {
        let storefront = args.country.as_deref().unwrap_or("US");
        let platform = args.platform.as_deref().unwrap_or("apple").to_lowercase();
        if !["apple", "play", "both"].contains(&platform.as_str()) {
            return Err(McpError::invalid_params(
                "platform must be 'apple', 'play', or 'both'",
                None,
            ));
        }
        let seeds: Vec<String> = args.seeds.iter().take(8).cloned().collect();
        let do_apple = platform != "play";
        let do_play = platform == "play" || platform == "both";

        let mut rows: Vec<serde_json::Value> = Vec::new();
        let mut hint_sections: Vec<String> = Vec::new();

        for seed in &seeds {
            if do_apple {
                let summary = match mzstore::ranked_apps(
                    &self.ctx.http, seed, storefront, "en-us", 20,
                )
                .await
                {
                    Ok(ranked) => {
                        let analyzed: Vec<scoring::AnalyzedApp> = ranked
                            .iter()
                            .map(|(rank, app)| scoring::analyze_app(seed, *rank, app))
                            .collect();
                        Some(scoring::summarize(&analyzed))
                    }
                    Err(_) => None,
                };
                let hints = hints::keyword_hints(&self.ctx.http, seed, storefront, "en-us")
                    .await
                    .unwrap_or_default();
                if let Some(s) = summary {
                    rows.push(json!({
                        "keyword": seed,
                        "platform": "apple",
                        "difficulty": (s.difficulty * 10.0).round() / 10.0,
                        "exact_title_ratio": (s.exact_title_match_ratio * 100.0).round() / 10.0,
                        "top5_median_ratings": s.top5_median_rating_count,
                        "hints": hints.len(),
                        "verdict": verdict(s.difficulty, s.exact_title_match_ratio, s.top5_median_rating_count),
                    }));
                }
                if !hints.is_empty() {
                    hint_sections.push(format!(
                        "**{seed}** (Apple hints): {}",
                        hints.iter().map(|h| format!("`{h}`")).collect::<Vec<_>>().join(", ")
                    ));
                }
            }
            if do_play {
                if let Ok((_apps, s)) = play::analyze_keyword(
                    &self.ctx.http, seed, storefront, "en", 5,
                )
                .await
                {
                    rows.push(json!({
                        "keyword": seed,
                        "platform": "play",
                        "difficulty": (s.difficulty * 10.0).round() / 10.0,
                        "exact_title_ratio": (s.exact_title_match_ratio * 100.0).round() / 10.0,
                        "top5_median_ratings": s.top5_median_reviews,
                        "hints": "-",
                        "verdict": verdict(s.difficulty, s.exact_title_match_ratio, s.top5_median_reviews),
                    }));
                }
            }
        }

        let mut md = String::from("# Keyword report\n\n");
        md.push_str(&format!(
            "Storefront: {storefront} · Platform: {platform} · Seeds: {}\n\n",
            seeds.join(", ")
        ));
        md.push_str("| Keyword | Platform | Difficulty | Exact-title match % | Top-5 median ratings | Hints | Verdict |\n|---|---|---|---|---|---|---|\n");
        for row in &rows {
            md.push_str(&format!(
                "| {k} | {p} | {d} | {e}% | {t} | {h} | **{v}** |\n",
                k = row["keyword"].as_str().unwrap_or(""),
                p = row["platform"].as_str().unwrap_or(""),
                d = row["difficulty"],
                e = row["exact_title_ratio"],
                t = row["top5_median_ratings"],
                h = row["hints"],
                v = row["verdict"].as_str().unwrap_or(""),
            ));
        }
        md.push_str("\nVerdicts: TARGET = difficulty < 35, MAYBE = 35–60, SKIP > 60. ");
        md.push_str("\"weak\" notes a low top-5 median (rankable niche); \"crowded\" notes most top-10 titles already contain the phrase.\n\n");
        if !hint_sections.is_empty() {
            md.push_str("## Expansion candidates (store autocomplete)\n\n");
            for h in hint_sections {
                md.push_str(&format!("- {h}\n"));
            }
        }
        Ok(CallToolResult::success(vec![ContentBlock::text(md)]))
    }
}

fn verdict(difficulty: f64, exact_ratio: f64, top5_median: i64) -> String {
    let base = if difficulty < 35.0 {
        "TARGET"
    } else if difficulty <= 60.0 {
        "MAYBE"
    } else {
        "SKIP"
    };
    let mut notes: Vec<String> = Vec::new();
    if top5_median > 0 && top5_median < 2_000 {
        notes.push("weak incumbents".into());
    }
    if exact_ratio >= 0.8 {
        notes.push("crowded titles".into());
    }
    if notes.is_empty() {
        base.to_string()
    } else {
        format!("{base} ({})", notes.join(", "))
    }
}

fn to_result<T: serde::Serialize>(value: &T) -> Result<CallToolResult, McpError> {
    ContentBlock::json(value)
        .map(|block| CallToolResult::success(vec![block]))
        .map_err(err)
}

const PHRASE_STOPWORDS: [&str; 17] = [
    "a", "an", "the", "and", "or", "for", "with", "of", "in", "on", "at", "to", "from", "by", "my",
    "your", "app",
];

/// 1-3 word phrases from a title, minus stopwords (per-app keyword targeting signals).
pub fn title_phrases(title: &str) -> Vec<String> {
    let words: Vec<String> = title
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .to_lowercase()
        .split_whitespace()
        .map(String::from)
        .collect();
    let mut out = Vec::new();
    for n in 1..=3 {
        for w in words.windows(n) {
            if w.iter().any(|x| PHRASE_STOPWORDS.contains(&x.as_str())) {
                continue;
            }
            out.push(w.join(" "));
        }
    }
    out.sort();
    out.dedup();
    out
}

#[tool_handler]
impl ServerHandler for Asomcp {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions(
                "App Store & Google Play keyword research (ASO). appstore_search_ranked gives true store-order results; appstore_analyze_keyword gives difficulty scores; appstore_keyword_hints gives autocomplete demand signals.",
            )
            .with_server_info(Implementation::new("aso-mcp", env!("CARGO_PKG_VERSION")))
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let service = Asomcp::new().serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phrase_extraction() {
        let phrases = title_phrases("Calm - Sleep & Meditation");
        assert!(phrases.contains(&"calm".to_string()));
        assert!(phrases.contains(&"sleep".to_string()));
        assert!(phrases.contains(&"meditation".to_string()));
        assert!(phrases.contains(&"calm sleep".to_string()));
        assert!(!phrases.iter().any(|p| p == "app"));
    }
}
