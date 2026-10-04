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
mod store;

pub struct Ctx {
    http: http::HttpClient,
    history: store::History,
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

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct TrackDiffArgs {
    /// Apple numeric app IDs to diff
    pub apple_ids: Option<Vec<String>>,
    /// Play package names to diff
    pub play_ids: Option<Vec<String>>,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct TrackListArgs {}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct AppReviewsArgs {
    /// Which store: "apple" or "play"
    pub store: String,
    /// Apple numeric app ID or Play package name
    pub app_id: String,
    /// Two-letter ISO country code (default US)
    pub country: Option<String>,
    /// Language for Play reviews, e.g. "en" (default en)
    pub language: Option<String>,
    /// Max reviews to fetch, 1-500 (default 200)
    pub max: Option<u32>,
}

#[tool_router]
impl Asomcp {
    pub fn new() -> Self {
        Self {
            ctx: Arc::new(Ctx {
                http: http::HttpClient::new().expect("HTTP client"),
                history: store::History::open().expect("history db"),
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
        for app in &apps {
            let _ = self.ctx.history.save(&store::Snapshot {
                store: "apple".into(),
                app_id: app.track_id.to_string(),
                title: app.track_name.clone(),
                seller: app.seller_name.clone(),
                price: Some(app.formatted_price.clone()),
                rating: app.average_user_rating,
                rating_count: app.user_rating_count,
                description_hash: store::fnv1a(&app.description),
                fetched_at: store::now_secs(),
            });
        }
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
        let _ = self.ctx.history.save(&store::Snapshot {
            store: "play".into(),
            app_id: details.package.clone(),
            title: details.title.clone(),
            seller: details.developer.clone(),
            price: None,
            rating: details.rating,
            rating_count: details.reviews_count,
            description_hash: store::fnv1a(&details.description),
            fetched_at: store::now_secs(),
        });
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

    #[tool(description = "Diff tracked competitors' metadata against their previous snapshot: title/seller/price/description changes plus rating-count velocity. Snapshots accumulate automatically from appstore_lookup and play_app_details calls. Run those on your watch list weekly, then run this.")]
    async fn track_diff(
        &self,
        Parameters(args): Parameters<TrackDiffArgs>,
    ) -> Result<CallToolResult, McpError> {
        let mut results: Vec<serde_json::Value> = Vec::new();
        for id in args.apple_ids.unwrap_or_default() {
            let d = self
                .ctx
                .history
                .diff("apple", &id)
                .map_err(err)?;
            results.push(serde_json::to_value(&d).map_err(err)?);
        }
        for id in args.play_ids.unwrap_or_default() {
            let d = self.ctx.history.diff("play", &id).map_err(err)?;
            results.push(serde_json::to_value(&d).map_err(err)?);
        }
        to_result(&json!({ "results": results }))
    }

    #[tool(description = "List tracked apps with their snapshot counts and last-seen time.")]
    async fn track_list(
        &self,
        Parameters(_args): Parameters<TrackListArgs>,
    ) -> Result<CallToolResult, McpError> {
        let tracked = self.ctx.history.tracked().map_err(err)?;
        let out: Vec<serde_json::Value> = tracked
            .into_iter()
            .map(|(store, app_id, count, last_seen)| {
                json!({
                    "store": store,
                    "app_id": app_id,
                    "snapshots": count,
                    "last_seen_unix": last_seen,
                })
            })
            .collect();
        to_result(&json!({ "tracked": out }))
    }

    #[tool(description = "Fetch customer reviews for an app (Apple via RSS feed, Play via web endpoint). Returns reviews with scores plus a score histogram and the most frequent 1-2 star phrases (complaint vocabulary -> keyword and positioning candidates). Reviews are logged to history.db (deduped by review ID) so repeated calls accumulate a review archive.")]
    async fn app_reviews(
        &self,
        Parameters(args): Parameters<AppReviewsArgs>,
    ) -> Result<CallToolResult, McpError> {
        let country = args.country.as_deref().unwrap_or("US");
        let max = args.max.unwrap_or(200).clamp(1, 500) as usize;
        let store_kind = args.store.to_lowercase();
        if !["apple", "play"].contains(&store_kind.as_str()) {
            return Err(McpError::invalid_params("store must be 'apple' or 'play'", None));
        }

        let reviews_json = match store_kind.as_str() {
            "apple" => {
                let reviews = itunes::reviews(&self.ctx.http, &args.app_id, country, max)
                    .await
                    .map_err(err)?;
                let logged: Vec<(String, u8, String)> = reviews
                    .iter()
                    .map(|r| (r.id.clone(), r.score, format!("{} {}", r.title, r.text)))
                    .collect();
                let new_saved = self.ctx.history.save_reviews("apple", &args.app_id, &logged).unwrap_or(0);
                let histogram = hist(&reviews.iter().map(|r| r.score as i64).collect::<Vec<_>>());
                json!({
                    "store": "apple", "app_id": args.app_id, "fetched": reviews.len(),
                    "new_logged": new_saved,
                    "histogram": histogram,
                    "negative_phrases": negative_phrases(&reviews.iter().filter(|r| r.score <= 2).map(|r| (r.title.as_str(), r.text.as_str())).collect::<Vec<_>>()),
                    "reviews": reviews,
                })
            }
            _ => {
                let reviews = play::reviews(
                    &self.ctx.http,
                    &args.app_id,
                    country,
                    args.language.as_deref().unwrap_or("en"),
                    max,
                )
                .await
                .map_err(err)?;
                let logged: Vec<(String, u8, String)> = reviews
                    .iter()
                    .map(|r| (r.id.clone(), r.score, r.text.clone()))
                    .collect();
                let new_saved = self.ctx.history.save_reviews("play", &args.app_id, &logged).unwrap_or(0);
                let histogram = hist(&reviews.iter().map(|r| r.score as i64).collect::<Vec<_>>());
                json!({
                    "store": "play", "app_id": args.app_id, "fetched": reviews.len(),
                    "new_logged": new_saved,
                    "histogram": histogram,
                    "negative_phrases": negative_phrases(&reviews.iter().filter(|r| r.score <= 2).map(|r| ("", r.text.as_str())).collect::<Vec<_>>()),
                    "reviews": reviews,
                })
            }
        };
        to_result(&reviews_json)
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

fn hist(scores: &[i64]) -> serde_json::Value {
    let mut h = [0i64; 5];
    for s in scores {
        if (1..=5).contains(s) {
            h[(s - 1) as usize] += 1;
        }
    }
    json!({ "1": h[0], "2": h[1], "3": h[2], "4": h[3], "5": h[4] })
}

/// Frequent 1-3 word phrases across negative review text — complaint vocabulary.
fn negative_phrases(texts: &[(&str, &str)]) -> Vec<serde_json::Value> {
    const REVIEW_STOPWORDS: [&str; 46] = [
        "this", "that", "have", "has", "had", "but", "they", "them", "their", "there", "with",
        "was", "were", "are", "you", "your", "all", "can", "just", "would", "could", "what",
        "when", "been", "than", "its", "it's", "even", "only", "also", "very", "will", "from",
        "which", "who", "how", "get", "got", "one", "out", "not", "now", "use", "using", "used",
        "the",
    ];
    let mut freq: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    for (title, text) in texts {
        let words: Vec<String> = format!("{title} {text}")
            .to_lowercase()
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { ' ' })
            .collect::<String>()
            .split_whitespace()
            .filter(|w| w.len() > 2 && !PHRASE_STOPWORDS.contains(w) && !REVIEW_STOPWORDS.contains(w))
            .map(String::from)
            .collect();
        for n in 1..=3 {
            for w in words.windows(n) {
                *freq.entry(w.join(" ")).or_insert(0) += 1;
            }
        }
    }
    let mut phrases: Vec<(String, u32)> = freq
        .into_iter()
        .filter(|(_, c)| *c >= 3)
        .collect();
    phrases.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    phrases
        .into_iter()
        .take(25)
        .map(|(phrase, count)| json!({ "phrase": phrase, "count": count }))
        .collect()
}

fn verdict(difficulty: f64, exact_ratio: f64, top5_median: i64) -> String {    let base = if difficulty < 35.0 {
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
