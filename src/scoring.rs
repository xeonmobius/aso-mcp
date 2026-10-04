//! Difficulty / competitiveness scoring — port of the reference implementation's
//! `AnalyzeCommand` (word variants, match scores, velocity split, competitivenessV1),
//! plus two additions from the design doc: top5_median_rating_count and
//! exact_title_match_ratio.

use serde::Serialize;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::itunes::App;

const FILTER_WORDS: [&str; 15] = [
    "a", "an", "the", "and", "or", "but", "&", "for", "with", "of", "in", "on", "at", "to", "from",
];

#[derive(Debug, Clone, Serialize)]
pub struct AnalyzedApp {
    pub rank: usize,
    #[serde(flatten)]
    pub app: App,
    pub title_match_score: i32,
    pub description_match_score: i32,
    pub exact_match_in_title: bool,
    pub age_days: i64,
    pub freshness_days: i64,
    pub ratings_per_day: f64,
}

#[derive(Debug, Serialize)]
pub struct Summary {
    pub app_count: usize,
    pub avg_age_days: i64,
    pub median_age_days: i64,
    pub age_ratio: f64,
    pub avg_freshness_days: i64,
    pub avg_rating: f64,
    pub avg_rating_count: i64,
    pub avg_title_match_score: f64,
    pub avg_description_match_score: f64,
    pub avg_ratings_per_day: f64,
    pub newest_velocity: f64,
    pub established_velocity: f64,
    pub velocity_ratio: f64,
    pub newest_percent_of_ratings: f64,
    pub established_percent_of_ratings: f64,
    /// 0–100, higher = harder to rank (reference "competitivenessV1")
    pub difficulty: f64,
    /// Additions: median rating count of the top 5 (50k+ floor heuristic)
    pub top5_median_rating_count: i64,
    /// Additions: fraction of top 10 whose title contains the exact phrase
    pub exact_title_match_ratio: f64,
}

pub fn word_variants(word: &str) -> Vec<String> {
    let w = word.to_lowercase();
    let mut v = vec![w.clone()];
    v.push(format!("{w}s"));
    v.push(format!("{w}es"));
    if w.ends_with('y') && w.len() > 1 {
        let stem = &w[..w.len() - 1];
        if let Some(c) = stem.chars().last() {
            if !"aeiou".contains(c) {
                v.push(format!("{stem}ies"));
            }
        }
    }
    v.push(format!("{w}ing"));
    if w.ends_with('e') && w.len() > 1 {
        v.push(format!("{}ing", &w[..w.len() - 1]));
    }
    v.push(format!("{w}ed"));
    if w.ends_with('e') && w.len() > 1 {
        v.push(format!("{}ed", &w[..w.len() - 1]));
    }
    v
}

trait WordSplit {
    fn lowercase_words(&self) -> Vec<String>;
}

impl WordSplit for str {
    fn lowercase_words(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut cur = String::new();
        for c in self.chars() {
            if c.is_alphanumeric() {
                cur.extend(c.to_lowercase());
            } else if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
        }
        if !cur.is_empty() {
            out.push(cur);
        }
        out
    }
}

pub fn remove_filter_words(text: &str) -> String {
    text.lowercase_words()
        .into_iter()
        .filter(|w| !FILTER_WORDS.contains(&w.as_str()))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Count how many search words (any variant) appear as whole words in the text.
pub fn count_word_matches(text: &str, word_variants: &[Vec<String>]) -> i32 {
    let words = text.lowercase_words();
    let tokens: std::collections::HashSet<&str> = words.iter().map(String::as_str).collect();
    word_variants
        .iter()
        .filter(|variants| variants.iter().any(|v| tokens.contains(v.as_str())))
        .count() as i32
}

/// Days since epoch for "YYYY-MM-DD..." strings; None if unparseable.
pub fn days_since_epoch(date_str: &str) -> Option<i64> {
    let date_part = date_str.split(|c: char| c == 'T' || c == ' ').next()?;
    let mut parts = date_part.split('-');
    let year: i64 = parts.next()?.parse().ok()?;
    let month: u32 = parts.next()?.parse().ok()?;
    let day: u32 = parts.next()?.parse().ok()?;
    Some(days_from_civil(year, month, day))
}

fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = ((m + 9) % 12) as i64;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn now_days() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64 / 86400)
        .unwrap_or(0)
}

pub fn analyze_app(term: &str, rank: usize, app: &App) -> AnalyzedApp {
    let title = app.track_name.to_lowercase();
    let description = app.description.to_lowercase();
    let term_lower = term.to_lowercase();
    let title_filtered = remove_filter_words(&title);

    let exact = title.contains(&term_lower) || title_filtered.contains(&term_lower);
    let search_words: Vec<String> = term_lower.split_whitespace().map(String::from).collect();
    let variants: Vec<Vec<String>> = search_words.iter().map(|w| word_variants(w)).collect();

    let title_match_score = if exact {
        5
    } else {
        count_word_matches(&title, &variants)
    };
    let description_match_score = count_word_matches(&description, &variants);

    let age_days = app
        .release_date
        .as_str()
        .pipe(days_since_epoch)
        .map(|d| now_days() - d)
        .unwrap_or(0);
    let freshness_days = app
        .current_version_release_date
        .as_str()
        .pipe(days_since_epoch)
        .map(|d| now_days() - d)
        .unwrap_or(0);

    let rating_count = app.user_rating_count.unwrap_or(0);
    let ratings_per_day = if age_days > 0 {
        rating_count as f64 / age_days as f64
    } else {
        0.0
    };

    AnalyzedApp {
        rank,
        app: app.clone(),
        title_match_score,
        description_match_score,
        exact_match_in_title: exact,
        age_days,
        freshness_days,
        ratings_per_day,
    }
}

trait Pipe: Sized {
    fn pipe<T>(self, f: impl FnOnce(Self) -> T) -> T {
        f(self)
    }
}
impl<T> Pipe for T {}

fn avg_over(xs: &[AnalyzedApp], f: impl Fn(&AnalyzedApp) -> f64) -> f64 {
    if xs.is_empty() {
        0.0
    } else {
        xs.iter().map(f).sum::<f64>() / xs.len() as f64
    }
}

pub fn summarize(analyzed: &[AnalyzedApp]) -> Summary {
    let n = analyzed.len();
    if n == 0 {
        return Summary {
            app_count: 0,
            avg_age_days: 0,
            median_age_days: 0,
            age_ratio: 1.0,
            avg_freshness_days: 0,
            avg_rating: 0.0,
            avg_rating_count: 0,
            avg_title_match_score: 0.0,
            avg_description_match_score: 0.0,
            avg_ratings_per_day: 0.0,
            newest_velocity: 0.0,
            established_velocity: 0.0,
            velocity_ratio: 0.0,
            newest_percent_of_ratings: 0.0,
            established_percent_of_ratings: 0.0,
            difficulty: 0.0,
            top5_median_rating_count: 0,
            exact_title_match_ratio: 0.0,
        };
    }

    let avg_age = avg_over(analyzed, |a| a.age_days as f64) as i64;
    let avg_freshness = avg_over(analyzed, |a| a.freshness_days as f64) as i64;
    let avg_rating = avg_over(analyzed, |a| a.app.average_user_rating.unwrap_or(0.0));
    let avg_rating_count = avg_over(analyzed, |a| a.app.user_rating_count.unwrap_or(0) as f64) as i64;
    let avg_title = avg_over(analyzed, |a| a.title_match_score as f64);
    let avg_desc = avg_over(analyzed, |a| a.description_match_score as f64);
    let avg_rpd = avg_over(analyzed, |a| a.ratings_per_day);

    let mut ages: Vec<i64> = analyzed.iter().map(|a| a.age_days).collect();
    ages.sort_unstable();
    let median_age = if n % 2 == 0 {
        (ages[n / 2 - 1] + ages[n / 2]) / 2
    } else {
        ages[n / 2]
    };

    // Newest 30% vs established remainder (by age)
    let mut by_age: Vec<&AnalyzedApp> = analyzed.iter().collect();
    by_age.sort_by_key(|a| a.age_days);
    let new_count = ((n as f64) * 0.3).ceil() as usize;
    let (newest, established) = by_age.split_at(new_count.max(1).min(n));
    let avg_f = |xs: &[&AnalyzedApp], f: fn(&AnalyzedApp) -> f64| -> f64 {
        if xs.is_empty() {
            0.0
        } else {
            xs.iter().map(|a| f(a)).sum::<f64>() / xs.len() as f64
        }
    };
    let newest_age = avg_f(newest, |a| a.age_days as f64).max(1.0);
    let established_age = avg_f(established, |a| a.age_days as f64).max(1.0);
    let age_ratio = established_age / newest_age;

    let total_ratings: i64 = analyzed.iter().map(|a| a.app.user_rating_count.unwrap_or(0)).sum();
    let newest_ratings: i64 = newest.iter().map(|a| a.app.user_rating_count.unwrap_or(0)).sum();
    let established_ratings: i64 = established.iter().map(|a| a.app.user_rating_count.unwrap_or(0)).sum();
    let pct = |x: i64| if total_ratings > 0 { x as f64 / total_ratings as f64 * 100.0 } else { 0.0 };

    let newest_velocity = avg_f(newest, |a| a.ratings_per_day);
    let established_velocity = avg_f(established, |a| a.ratings_per_day);
    let velocity_ratio = if established_velocity > 0.0 {
        newest_velocity / established_velocity
    } else {
        0.0
    };

    // competitivenessV1 (reference formula)
    let normalized_traffic = avg_rpd.min(100.0);
    let normalized_freshness = 100.0 - (avg_freshness.min(365) as f64) / 365.0 * 100.0;
    let normalized_title_match = avg_title * 20.0;
    let normalized_velocity = 100.0 - velocity_ratio.min(5.0) * 20.0;
    let difficulty = (normalized_traffic * 0.35
        + normalized_freshness.max(0.0) * 0.25
        + normalized_title_match * 0.20
        + normalized_velocity.max(0.0) * 0.20)
        .clamp(0.0, 100.0);

    // Additions
    let mut top5: Vec<i64> = analyzed
        .iter()
        .take(5)
        .filter_map(|a| a.app.user_rating_count)
        .collect();
    top5.sort_unstable();
    let top5_median = top5.get(top5.len() / 2).copied().unwrap_or(0);
    let top10 = analyzed.iter().take(10).count().max(1) as f64;
    let exact_ratio = analyzed
        .iter()
        .take(10)
        .filter(|a| a.exact_match_in_title)
        .count() as f64
        / top10;

    Summary {
        app_count: n,
        avg_age_days: avg_age,
        median_age_days: median_age,
        age_ratio,
        avg_freshness_days: avg_freshness,
        avg_rating,
        avg_rating_count,
        avg_title_match_score: avg_title,
        avg_description_match_score: avg_desc,
        avg_ratings_per_day: avg_rpd,
        newest_velocity,
        established_velocity,
        velocity_ratio,
        newest_percent_of_ratings: pct(newest_ratings),
        established_percent_of_ratings: pct(established_ratings),
        difficulty,
        top5_median_rating_count: top5_median,
        exact_title_match_ratio: exact_ratio,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variants() {
        assert_eq!(word_variants("meditate")[0], "meditate");
        assert!(word_variants("study").contains(&"studies".to_string()));
        assert!(word_variants("track").contains(&"tracking".to_string()));
    }

    #[test]
    fn matching() {
        let variants = vec![word_variants("meditation")];
        assert_eq!(count_word_matches("a meditation app for calm", &variants), 1);
        assert_eq!(count_word_matches("meditations daily", &variants), 1);
        assert_eq!(count_word_matches("meditationally speaking", &variants), 0);
    }

    #[test]
    fn dates() {
        assert!(days_since_epoch("2024-01-02T03:04:05Z").is_some());
        assert_eq!(
            days_since_epoch("2024-01-02T03:04:05Z"),
            Some(days_from_civil(2024, 1, 2))
        );
        assert_eq!(days_since_epoch("garbage"), None);
    }

    #[test]
    fn summary_bounds() {
        let empty = summarize(&[]);
        assert_eq!(empty.difficulty, 0.0);
    }
}
