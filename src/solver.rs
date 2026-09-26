//! Exact letter-hit rates over the words consistent with public Hangman clues.

use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LetterOdds {
    pub letter: char,
    pub hits: usize,
    pub candidates: usize,
}

impl LetterOdds {
    pub fn probability(self) -> f64 {
        self.hits as f64 / self.candidates as f64
    }
}

pub struct Analysis {
    pub candidate_count: usize,
    odds: Vec<LetterOdds>,
}

impl Analysis {
    pub fn odds_for(&self, letter: char) -> Option<LetterOdds> {
        self.odds.iter().copied().find(|odds| odds.letter == letter)
    }

    pub fn best_letter(&self) -> Option<LetterOdds> {
        // On equal hit rates, choose the earlier letter for a stable result.
        self.odds
            .iter()
            .copied()
            .max_by(|a, b| a.hits.cmp(&b.hits).then_with(|| b.letter.cmp(&a.letter)))
    }
}

/// Uses only visible evidence. Hidden positions must contain unguessed letters:
/// a correct Hangman guess reveals every occurrence of that letter.
pub fn matching_words<'a>(
    pattern: &str,
    guessed: &BTreeSet<char>,
    words: &[&'a str],
) -> Vec<&'a str> {
    let visible: Vec<char> = pattern.chars().collect();
    words
        .iter()
        .copied()
        .filter(|word| {
            word.chars().count() == visible.len()
                && word.chars().zip(&visible).all(|(letter, shown)| {
                    if guessed.contains(&letter) {
                        letter == *shown
                    } else {
                        *shown == '_'
                    }
                })
        })
        .collect()
}

pub fn analyze(pattern: &str, guessed: &BTreeSet<char>, words: &[&str]) -> Analysis {
    let candidates = matching_words(pattern, guessed, words);

    let odds = if candidates.is_empty() {
        Vec::new()
    } else {
        ('A'..='Z')
            .filter(|letter| !guessed.contains(letter))
            .map(|letter| LetterOdds {
                letter,
                hits: candidates
                    .iter()
                    .filter(|word| word.contains(letter))
                    .count(),
                candidates: candidates.len(),
            })
            .collect()
    };

    Analysis {
        candidate_count: candidates.len(),
        odds,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_rates_count_words_with_letter_once() {
        let analysis = analyze("_____", &BTreeSet::new(), &["APPLE", "GRAPE", "MANGO"]);
        assert_eq!(analysis.candidate_count, 3);
        assert_eq!(analysis.odds_for('A').unwrap().hits, 3);
        assert_eq!(analysis.odds_for('P').unwrap().hits, 2);
    }

    #[test]
    fn revealed_letters_must_match_all_positions_and_wrong_letters_are_absent() {
        let words = [
            "CAKE", "CAVE", "GAME", "GATE", "LATE", "MAKE", "NAME", "SAME", "MAMA",
        ];
        let guessed = ['A', 'E'].into_iter().collect();
        let analysis = analyze("_A_E", &guessed, &words);
        assert_eq!(analysis.candidate_count, 8);
        assert_eq!(analysis.odds_for('M').unwrap().hits, 4);
        assert_eq!(analysis.best_letter().unwrap().letter, 'M');

        let guessed = ['A', 'E', 'C'].into_iter().collect();
        let analysis = analyze("_A_E", &guessed, &words);
        assert_eq!(analysis.candidate_count, 6);
        assert!(analysis.odds_for('C').is_none());
    }

    #[test]
    fn repeated_revealed_letter_filters_hidden_occurrences() {
        let words = ["BANANA", "CABANA", "BAAANA"];
        let guessed = ['A'].into_iter().collect();
        let analysis = analyze("_A_A_A", &guessed, &words);
        assert_eq!(analysis.candidate_count, 2);
        assert_eq!(analysis.odds_for('N').unwrap().hits, 2);
    }

    #[test]
    fn uncovered_word_has_no_fabricated_probability() {
        let guessed = ['X'].into_iter().collect();
        let analysis = analyze("X____", &guessed, &["APPLE", "GRAPE", "MANGO"]);
        assert_eq!(analysis.candidate_count, 0);
        assert!(analysis.best_letter().is_none());
    }
}
