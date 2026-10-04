# aso-mcp — Implementation Plan

Spec: `docs/SPEC.md`. Milestones are vertical slices; each ends compiling + smoke-tested.

## M0 — Reference extraction ✅
- [x] Clone `drewster99/appstore-mcp-server` → `reference/` (read-only, depth 1)
- [x] Extract iTunes Search/Lookup contracts (`AppStoreAPI.swift`)
- [x] Extract MZStore ranked-search contract + storefront map (`ScrapeCommand.swift`)
- [x] Extract difficulty scoring (`AnalyzeCommand.swift`): word variants, match scores,
      velocity (newest-30% vs established-70%), `competitivenessV1` weights
- [x] Confirm Apple Intelligence keyword-gen is intentionally NOT ported (LLM client does it)

## M1 — Rust skeleton + Apple core (current)
- [x] Toolchain verified (rustc 1.94.1)
- [x] `cargo init`; deps: rmcp 3.5(server,macros,transport-io), tokio, reqwest(rustls), serde, serde_json, anyhow, schemars, plist
- [x] `itunes` module: search + lookup (typed `App` model, compact output)
- [x] `mzstore` module: ranked search + storefront map + global rate limiter
- [x] `hints` module: MZSearchHints plist parsing — **live spike passed**: `term=` param +
      `{storeId},24 t:native` header returns real suggestions
- [x] `scoring` module: competitivenessV1 port + top5-median/exact-ratio additions (11 unit tests green)
- [x] Tools: version, appstore_search, appstore_search_ranked, appstore_lookup,
      appstore_find_rank, appstore_analyze_keyword, appstore_keyword_hints,
      appstore_competitor_keywords
- [x] `.mcp.json` (project) + stdio smoke tests
- **Live verification:** Headspace rank #2 for "meditation" (250 results, US);
  "meditation timer" difficulty 46.8/100 with Insight Timer #1 (matches real store);
  10 autocomplete hints for "meditation"; release binary 2.9 MB (stripped, LTO, opt-size)

## M2 — Volume data (re-scoped: no ASA API)
- [ ] `docs/volume-sources.md`: manual Apple Ads **Keyword Planner** workflow (web UI, zero
      setup — popularity scores), manual Google Keyword Planner workflow (free volume ranges),
      and the first-party note (App Store Connect / Play Console search-terms reports).
- [x] Automated demand proxy shipped in M1: `appstore_keyword_hints` (live autocomplete).
- [ ] *(optional, deferred)* `asa_search_popularity` tool — build only if credentials ever
      exist; JWT (.p8) → OAuth2 → insights query. Not on the critical path.
- **Accept:** a user can, from the docs alone, get a popularity number for any keyword in <2 min.

## M3 — Google Play module ✅
- [x] Probed live Play pages: search results in `AF_initDataCallback` `ds:N` blobs
      (structural signature parsing); details via stable HTML anchors
- [x] `play.rs`: extract_data_blocks (balanced-bracket JSON), search parser, details parser
      (title/rating/reviews/installs/genre/developer/description), find_rank,
      analyze_keyword (search top-10 + details top-5 review counts)
- [x] Tools: play_search, play_app_details, play_find_rank, play_analyze_keyword,
      play_competitor_keywords — wired + fixture-tested (tests/fixtures/play_search.html)
- [x] SuggRequest spike skipped: confirmed broken upstream; competitor-title mining covers it
- **Live verification:** "meditation timer" US — Insight Timer #2 matches real Play order;
  details for de.iab.meditationtimer all correct (dev, 4.8★, 4320 reviews, 100K+,
  Health & Fitness, full description); play difficulty 47.3

## M4 — keyword_report ✅
- [x] `keyword_report(seeds, country, platform)`: per-seed hints + difficulty on
      apple/play/both → markdown table with TARGET/MAYBE/SKIP verdicts + hint expansion lists
- **Live verification:** seeds ["meditation timer","sleep sounds"], platform both →
  correct table; sleep sounds correctly splits MAYBE (apple 45.7) vs SKIP (play 91.1, crowded)

## Standing rules
- Every commit point: `cargo build` + `cargo test` green, server still speaks MCP.
- Parser tests run against saved fixtures, never live stores.
- No secrets in repo; ASA creds via env only.
