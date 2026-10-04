# aso-mcp — Specification

**Version:** 0.1.0 (M1 scope)
**Status:** Active development
**Upstream reference:** `reference/appstore-mcp-server` (drewster99, Swift, MIT) — read-only

## 1. Purpose

A single MCP server (Rust, stdio) for App Store Optimization keyword research across the
Apple App Store and Google Play. It answers, per keyword:

1. **Demand** — do people search this? (Apple Search Ads popularity; autocomplete hints)
2. **Supply/Difficulty** — how hard is top-10 placement? (ranked results analysis)
3. **Competition** — who ranks, and what keywords are they targeting?

and finally merges all three into a **keyword report** with target/skip verdicts.

## 2. Non-goals

- No listing management (writing titles/subtitles to stores) — research only.
- No exact Google Play search volumes — no free source exists anywhere.
- No Apple Intelligence / on-device LLM features (the MCP client's LLM does that better).
- No paid ASO vendor APIs.

## 3. Architecture

- **Language:** Rust (chosen for: smallest binary, lowest memory, official `rmcp` SDK).
- **Transport:** MCP over stdio (works in ZCode, Claude Desktop/Code, any MCP client).
- **HTTP:** one shared `reqwest` client (rustls), global rate limiter (~1 req/s + jitter)
  across all store endpoints.
- **Modules:**
  - `itunes` — iTunes Search API (search, lookup)
  - `mzstore` — undocumented ranked-search endpoint + storefront ID map
  - `hints` — MZSearchHints autocomplete
  - `asa` — Apple Search Ads insights (popularity) [M2]
  - `play` — Google Play batchexecute scraping [M3]
  - `scoring` — difficulty/competitiveness math
  - `report` — merged keyword report [M4]

## 4. Data source contracts (extracted from reference implementation)

### 4.1 iTunes Search — `GET https://itunes.apple.com/search`
Params: `term`, `media=software`, `entity=software`, `limit` (1–200), `country` (ISO-2, default US),
optional `attribute`, `genreId`, `lang`.
Response: `{"resultCount": N, "results": [App]}`. ~20 req/min documented limit.
Ranking order is **NOT** store-accurate — never use for rank checks.

### 4.2 iTunes Lookup — `GET https://itunes.apple.com/lookup`
Params: `id` (comma-joined IDs), or `bundleId`; `country`, `lang`.
Used to enrich ranked IDs with full metadata. `App` fields we keep:
`trackId, trackName, artistName, sellerName, averageUserRating, userRatingCount,
formattedPrice, primaryGenreName, version, releaseDate, currentVersionReleaseDate,
description, bundleId, trackViewUrl, contentAdvisoryRating, minimumOsVersion, fileSizeBytes`.
Note: **no `subtitle` field** in this API.

### 4.3 MZStore ranked search (undocumented, store-accurate order)
`GET https://search.itunes.apple.com/WebObjects/MZStore.woa/wa/search?clientApplication=Software&media=software&term={urlencoded}`

Headers:
- `X-Apple-Store-Front: {storeId},24 t:native`  (e.g. US → `143441,24 t:native`)
- `Accept-Language: {lang}` (lowercased, e.g. `en-us`)
- `User-Agent: AppStore/3.0 iOS/18.0 model/iPhone16,2 hwp/t8130 build/22A3354 (6; dt:326) AMS/1`
- `Accept: application/json`

Response: `{"bubbles": [{"results": [{"id": "...", ...}, ...]}]}` —
**array position = App Store rank**. IDs are then enriched via 4.2.
Risk: undocumented; may change without notice (same exposure as upstream repo).
Full storefront-ID map is in `src/mzstore.rs` (ported verbatim from reference).

### 4.4 MZSearchHints autocomplete
`GET https://search.itunes.apple.com/WebObjects/MZSearchHints.woa/wa/hints?clientApplication=Software&term={q}`
with `X-Apple-Store-Front: {storeId}` (+ same UA family). Returns XML plist; empty in
header-less requests (verified). Must include storefront header to yield `hints` array.

### 4.5 Apple Search Ads popularity [OPTIONAL — deferred]
`POST https://api.searchads.apple.com/api/v4/insights/search-terms/keyword` (verify path at
implementation). Auth: ES256 JWT (`.p8` key) → OAuth2 token → Bearer call.
Returns Apple's raw search popularity (5–100 scale) per storefront.
Env vars: `ASA_CLIENT_ID`, `ASA_TEAM_ID`, `ASA_KEY_ID`, `ASA_P8_PATH`.
**Deferred:** setup requires a second Apple Account (API-user role cannot be self-assigned).
Demand is covered instead by §4.4 hints (automated) + manual ASA Keyword Planner lookups in
the web UI (zero setup) + first-party App Store Connect data once the app is live. The tool
can be retrofitted at any time if credentials ever exist.

### 4.6 Google Play [M3]
HTML/batchexecute scraping following `google-play-scraper` conventions:
search results, app details (title, installs, rating count, description).
`SuggRequest` autocomplete endpoint is dead (broken upstream since 2022) — replaced by
competitor title/short-description mining. N-gram density (2–3 grams, 1–3% band) from
listing descriptions signals competitor keyword targets.

## 5. Tools (MCP)

| Tool | Status | Input | Output |
|---|---|---|---|
| `version` | M1 | — | server version string |
| `appstore_search` | M1 | term, country?, limit? | app list (iTunes order) |
| `appstore_search_ranked` | M1 | term, country?, limit?, lang? | ranked apps (true order) + rank per app |
| `appstore_lookup` | M1 | id \| bundleId \| url, country? | full app metadata |
| `appstore_find_rank` | M1 | term, app_id, country? | rank (1-based) or null, total results |
| `appstore_analyze_keyword` | M1 | term, country? | per-app CSV-like rows + summary + difficulty |
| `appstore_keyword_hints` | M1 | term, country? | autocomplete suggestions |
| `appstore_competitor_keywords` | M1.1 | term, country?, top_n? | phrases competitors target in titles |
| `asa_search_popularity` | optional | term, country? | popularity 5–100 or unconfigured error |
| `play_search` | M3 ✅ | term, country?, language?, limit? | ranked results (page order = rank) |
| `play_app_details` | M3 ✅ | package, country?, language? | full listing details |
| `play_find_rank` | M3 ✅ | term, package, country? | rank or null |
| `play_analyze_keyword` | M3 ✅ | term, country? | per-app rows + difficulty 0–100 |
| `play_competitor_keywords` | M3 ✅ | term, country?, top_n? | competitor title phrases |
| `keyword_report` | M4 ✅ | seeds[], country?, platform ("apple"/"play"/"both") | markdown table + verdicts |

All tools return **compact JSON** (no pretty-printing) to minimize LLM token usage.
Errors: MCP tool errors with human-readable messages; HTTP/status surfaced verbatim.

## 6. Difficulty scoring (port of reference `competitivenessV1` + additions)

Per top-20 ranked app: title match 0–5 (exact phrase → 5, else whole-word variant matches),
description match (whole-word variants: plural/-es/-ies/-ing/-ed forms), age/freshness days,
ratings/day velocity.

Summary → competitiveness 0–100 (higher = harder):
```
traffic     = min(avg_ratings_per_day, 100)
freshness   = 100 - min(avg_freshness_days, 365)/365*100      // actively-maintained field
titleMatch  = avg_title_match_score * 20
velocityInv = 100 - min(velocity_ratio, 5)*20                 // newest-30% vs established-70%
difficulty  = clamp(0.35*traffic + 0.25*freshness + 0.20*titleMatch + 0.20*velocityInv, 0, 100)
```
Additions over reference: `top5_median_rating_count` and `exact_title_match_ratio` reported
raw so the LLM can apply the >50k-reviews / 8-of-10 heuristics from the design doc.

**Play difficulty formula** (no rating velocity or release dates on Play pages):
```
ratingPressure = min(top5_median_reviews / 50000, 1) * 100
difficulty     = clamp(0.45*ratingPressure + 0.35*avg_title_match*20 + 0.20*exact_title_ratio*100, 0, 100)
```
**Report verdicts** (both stores): TARGET < 35, MAYBE 35–60, SKIP > 60;
notes: "weak incumbents" when top-5 median < 2000, "crowded titles" when exact-match ratio ≥ 0.8.

## 7. Parsing contracts (Play)

- Search: results live in `AF_initDataCallback({key:'ds:N', data:[...]})` blobs; entries are
  located by **structural signature** (pkg at [0][0], title [3], rating [4][1], genre [5],
  developer [14], installs [15]) — not fixed ds keys, which shift between page variants.
- Details: anchored extraction — title `itemprop="name"`, rating `Rated X stars out of five`,
  reviews `N reviews` (K/M-scaled), installs `NNN,+` chip, genre `category/CODE" aria-label=`,
  developer `/store/apps/developer` link after title, description `data-g-id="description"`.

## 7. Rate limiting & etiquette

Single global limiter: min 1.1 s between outbound requests (+ jitter ≤ 400 ms).
No parallel fan-out against Apple. Lookup enrichment batches up to ~200 IDs per call.

## 8. Acceptance

- M1: all M1 tools live; `appstore_find_rank` returns correct rank for a known app/keyword
  (e.g. Headspace for "meditation", US); server registered in ZCode `.mcp.json` and callable.
- M2: popularity score returned for a sample keyword with configured creds; clean error without.
- M3: Play parity for search/details/difficulty.
- M4: end-to-end report for 5 seed keywords reads as a decision-ready table.
