# aso-mcp

App Store Optimization (ASO) keyword research for the **Apple App Store** and **Google Play**,
as a local MCP server (Rust, stdio). Free data sources only — no accounts, no API keys.

## Tools

| Tool | Store | What it does |
|---|---|---|
| `appstore_search_ranked` | Apple | true store-order search results (undocumented MZStore endpoint) |
| `appstore_find_rank` | Apple | an app's rank for a keyword |
| `appstore_analyze_keyword` | Apple | difficulty 0–100 + per-app match/velocity table |
| `appstore_keyword_hints` | Apple | search-bar autocomplete (demand signal) |
| `appstore_competitor_keywords` | Apple | phrases competitors target in titles |
| `appstore_search` / `appstore_lookup` | Apple | iTunes Search/Lookup wrappers |
| `play_search` / `play_find_rank` | Play | search + rank on Google Play |
| `play_analyze_keyword` | Play | difficulty 0–100 (title match + review floor) |
| `play_app_details` | Play | full listing details |
| `play_competitor_keywords` | Play | competitor title phrases |
| `keyword_report` | both | merged markdown report with target/maybe/skip verdicts |
| `version` | — | server version |

## Setup

```sh
cargo build --release          # binary at target/release/aso-mcp (~3 MB)
```

`.mcp.json` (already present) registers it at project scope for ZCode. For Claude Desktop /
Claude Code, point the same command at the binary.

## Workflow (finding growth keywords)

1. `appstore_keyword_hints` on seed terms → expansion candidates (demand signal).
2. `appstore_analyze_keyword` / `play_analyze_keyword` on candidates → difficulty.
3. `appstore_competitor_keywords` → what incumbents target.
4. `keyword_report` → one table, target/maybe/skip verdicts.
5. Optional popularity spot-checks: see `docs/volume-sources.md` (Apple planner 5–100,
   Google Keyword Planner ranges — both manual, both free).

## Docs

- `docs/SPEC.md` — tool contracts, API endpoints, scoring formulas
- `docs/PLAN.md` — milestones + verification log
- `docs/volume-sources.md` — manual demand-data workflows

## Caveats

- Apple's ranked search and Play parsing are undocumented surfaces; they can break when
  Apple/Google change their sites. Parsers are fixture-tested; breakage shows up as
  empty results, not wrong-looking data (usually).
- Global rate limiter: ~1 request/second against the stores. Reports with several seeds
  take ~5–30 s. Be polite; don't run massive parallel scans.
