//! Fuzzy matching of library titles to catalogue entries.

use crate::models::{Game, MatchCandidate};

/// Auto-apply only when the best candidate is (nearly) identical …
pub const AUTO_MATCH_SCORE: f64 = 0.95;
/// … and clearly ahead of the runner-up.
pub const AUTO_MATCH_MARGIN: f64 = 0.05;
/// Candidates below this are not worth asking about.
pub const MIN_CANDIDATE_SCORE: f64 = 0.55;

const EDITION_SUFFIXES: &[&str] = &[
    "game of the year edition",
    "game of the year",
    "goty edition",
    "goty",
    "definitive edition",
    "deluxe edition",
    "complete edition",
    "enhanced edition",
    "anniversary edition",
    "digital deluxe",
    "standard edition",
];

fn roman_to_digit(word: &str) -> Option<&'static str> {
    Some(match word {
        "ii" => "2",
        "iii" => "3",
        "iv" => "4",
        "v" => "5",
        "vi" => "6",
        "vii" => "7",
        "viii" => "8",
        "ix" => "9",
        "x" => "10",
        _ => return None,
    })
}

/// Comparable form: lowercase, no symbols/editions, roman numerals as digits, single spaces.
pub fn normalize(title: &str) -> String {
    let mut lower = title.to_lowercase().replace('&', " and ");
    lower.retain(|c| !matches!(c, '™' | '®' | '©' | '\'' | '’'));
    let cleaned: String = lower
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect();
    let mut text = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    for suffix in EDITION_SUFFIXES {
        if let Some(stripped) = text.strip_suffix(suffix) {
            text = stripped.trim_end().to_owned();
        }
    }
    text.split(' ')
        .map(|w| roman_to_digit(w).unwrap_or(w))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Similarity 0–1 of two titles.
pub fn score(a: &str, b: &str) -> f64 {
    let (a, b) = (normalize(a), normalize(b));
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    if a == b {
        return 1.0;
    }
    let lev = strsim::normalized_levenshtein(&a, &b);
    let jw = strsim::jaro_winkler(&a, &b);
    // Never reach 1.0 for non-identical names.
    (0.6 * lev + 0.4 * jw).min(0.99)
}

#[derive(Debug, Clone, PartialEq)]
pub enum MatchDecision {
    /// Confident: apply without asking.
    Auto(Box<MatchCandidate>),
    /// Ask the user (best first).
    Ask(Vec<MatchCandidate>),
    NoMatch,
}

pub fn decide(title: &str, results: Vec<Game>) -> MatchDecision {
    let mut scored: Vec<MatchCandidate> = results
        .into_iter()
        .map(|game| MatchCandidate {
            score: score(title, &game.title),
            game,
        })
        .filter(|c| c.score >= MIN_CANDIDATE_SCORE)
        .collect();
    scored.sort_by(|a, b| b.score.total_cmp(&a.score));
    scored.dedup_by(|a, b| a.game.id == b.game.id);
    let Some(best) = scored.first() else {
        return MatchDecision::NoMatch;
    };
    let runner_up = scored.get(1).map_or(0.0, |c| c.score);
    if best.score >= AUTO_MATCH_SCORE && best.score - runner_up >= AUTO_MATCH_MARGIN {
        return MatchDecision::Auto(Box::new(best.clone()));
    }
    scored.truncate(5);
    MatchDecision::Ask(scored)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{GameArtwork, ImageRef};

    fn game(id: &str, title: &str) -> Game {
        Game {
            id: id.into(),
            title: title.into(),
            summary: String::new(),
            description: String::new(),
            genres: vec![],
            platforms: vec![],
            release_date: None,
            developer: String::new(),
            publisher: String::new(),
            rating: None,
            popularity: 0,
            artwork: GameArtwork {
                cover: ImageRef {
                    url: None,
                    alt: String::new(),
                },
                header: ImageRef {
                    url: None,
                    alt: String::new(),
                },
                screenshots: vec![],
            },
        }
    }

    #[test]
    fn normalizes_titles() {
        assert_eq!(
            normalize("The Witcher® 3: Wild Hunt – Game of the Year Edition"),
            "the witcher 3 wild hunt"
        );
        assert_eq!(normalize("Ember & Oak"), "ember and oak");
        assert_eq!(normalize("Final Fantasy VII"), "final fantasy 7");
        assert_eq!(normalize("Baldur's Gate"), "baldurs gate");
    }

    #[test]
    fn scores() {
        assert!((score("Ember And Oak", "Ember & Oak") - 1.0).abs() < f64::EPSILON);
        assert!(score("Hollow Lantern", "Hollow Lantern 2") > 0.8);
        assert!(score("Hollow Lantern", "Hollow Lantern 2") < AUTO_MATCH_SCORE);
        assert!(score("Tidewater", "Stardew Valley") < MIN_CANDIDATE_SCORE);
    }

    #[test]
    fn decides_auto_ask_or_none() {
        let results = vec![
            game("igdb-2", "Hollow Lantern 2"),
            game("igdb-1", "Hollow Lantern"),
        ];
        match decide("hollow_lantern", results) {
            MatchDecision::Auto(c) => assert_eq!(c.game.id, "igdb-1"),
            other => panic!("expected auto match, got {other:?}"),
        }

        // Two equally good candidates (e.g. remake + original) → ask.
        let twins = vec![game("igdb-1", "Tidewater"), game("igdb-9", "Tidewater")];
        match decide("Tidewater", twins) {
            MatchDecision::Ask(list) => assert_eq!(list.len(), 2),
            other => panic!("expected ask, got {other:?}"),
        }

        assert_eq!(
            decide("Tidewater", vec![game("igdb-3", "Stardew Valley")]),
            MatchDecision::NoMatch
        );
    }
}
