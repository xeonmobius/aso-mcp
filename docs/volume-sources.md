# Volume / demand sources — manual workflows

The automated stack covers keyword discovery (autocomplete hints), difficulty, and
competitor mining. Demand numbers come from these free manual sources.

## Apple — Search Ads Keyword Planner (popularity 5–100, no API)

Works in any Apple Ads account, no credit card, no API user:

1. Sign in at [searchads.apple.com](https://searchads.apple.com).
2. Create a campaign draft far enough to reach **Keyword Planner** (Ad Group → Keywords →
   "Add keywords" → search a term), or open an existing campaign's keyword pane.
3. Type a seed keyword → read the **popularity** column (5–100, Apple's own search
   popularity; blank ≈ too little data).

Rule of thumb: ≥ 40 worth considering for a niche app, ≥ 60 solid demand, < 20 very thin.
Paste the finalists back into a `keyword_report` conversation to merge with difficulty.

## Google — Ads Keyword Planner (web-search volume ranges)

Google publishes **no** Play-store search volumes anywhere. The free proxy:

1. Sign in at [ads.google.com](https://ads.google.com) with any Google account.
2. Choose **"Create an account without a campaign"** when prompted (skip billing).
3. Tools → Planning → **Keyword Planner** → "Discover new keywords".
4. Enter seeds, set country/language, read **Avg. monthly searches** (ranges like 10K–100K
   without billing history).

These are **web-search** ranges used as a proxy for Play demand — correlated for most
app categories, exact for none. Treat as directional.

## First-party ground truth (once your app is live)

- **Google Play Console → Store performance → Search terms**: the actual queries that
  surfaced your listing, with impressions and installs per term. Best free Play data.
- **Apple App Store Connect → Analytics → Impressions** (source: App Store Search):
  real impression/download splits for your app. Typed-query detail requires running
  Apple Search Ads (the Search Terms report); a small 2-week campaign is the classic
  ASO harvest.
- **Apple Ads Keyword Planner** popularity scores (above) work before launch;
  first-party reports only exist after.
