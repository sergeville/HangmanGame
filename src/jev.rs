//! One bounded JeV decision from the public game state.

use crate::{solver, HangmanGame};
use serde_json::{json, Map, Value};
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

pub const ENDPOINT: &str = "https://api.typesafe.ai/v1/systemone";
static HTTP_AGENT: OnceLock<ureq::Agent> = OnceLock::new();
static LOCAL_HTTP_AGENT: OnceLock<ureq::Agent> = OnceLock::new();

fn http_agent() -> &'static ureq::Agent {
    HTTP_AGENT.get_or_init(|| {
        ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(5))
            .timeout(Duration::from_secs(10))
            .redirects(0)
            .build()
    })
}

fn local_http_agent() -> &'static ureq::Agent {
    LOCAL_HTTP_AGENT.get_or_init(|| {
        ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(2))
            .timeout(Duration::from_secs(180))
            .redirects(0)
            .build()
    })
}

pub struct Snapshot {
    state: Value,
    available: Vec<char>,
    local_advice: Option<LocalAdvice>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LetterQuality {
    hits: usize,
    projected_wins: usize,
    projected_misses: usize,
    projected_turns: usize,
}

impl LetterQuality {
    fn compare(self, other: Self) -> Ordering {
        self.hits
            .cmp(&other.hits)
            .then_with(|| self.projected_wins.cmp(&other.projected_wins))
            .then_with(|| other.projected_misses.cmp(&self.projected_misses))
            .then_with(|| other.projected_turns.cmp(&self.projected_turns))
    }
}

struct LocalAdvice {
    quality: BTreeMap<char, LetterQuality>,
    candidate_count: usize,
    best_letter: char,
    best_quality: LetterQuality,
}

impl LocalAdvice {
    fn from_game(game: &HangmanGame, candidates: &[&str], available: &[char]) -> Option<Self> {
        if candidates.is_empty() {
            return None;
        }
        let hit_counts: BTreeMap<char, usize> = available
            .iter()
            .map(|&letter| {
                (
                    letter,
                    candidates
                        .iter()
                        .filter(|word| word.contains(letter))
                        .count(),
                )
            })
            .collect();
        let max_hits = hit_counts.values().copied().max()?;
        let mut quality = BTreeMap::new();
        let mut best = None;
        for &letter in available {
            let hits = hit_counts[&letter];
            let mut score = LetterQuality {
                hits,
                projected_wins: 0,
                projected_misses: 0,
                projected_turns: 0,
            };
            if hits == max_hits {
                for &secret in candidates {
                    // Simulate each publicly possible word; never inspect the actual secret.
                    let mut probe =
                        HangmanGame::with_word_at_level(secret.to_owned(), game.level, false);
                    probe.eligible_words = game.eligible_words.clone();
                    probe.guessed = game.guessed.clone();
                    probe.lives = game.lives;
                    probe.handle_guess(letter);
                    while !probe.is_over() {
                        // One-turn lookahead: Odds finishes each hypothetical branch.
                        let Some(next) =
                            solver::analyze(&probe.display(), &probe.guessed, probe.solver_words())
                                .best_letter()
                        else {
                            break;
                        };
                        probe.handle_guess(next.letter);
                    }
                    score.projected_wins += usize::from(probe.is_won());
                    score.projected_misses += usize::from(game.lives - probe.lives);
                    score.projected_turns += probe.guessed.len() - game.guessed.len();
                }
            }
            if best.is_none_or(|(_, current): (char, LetterQuality)| score.compare(current).is_gt())
            {
                best = Some((letter, score));
            }
            quality.insert(letter, score);
        }
        let (best_letter, best_quality) = best?;
        Some(Self {
            quality,
            candidate_count: candidates.len(),
            best_letter,
            best_quality,
        })
    }

    fn apply(&self, decision: &mut Decision) {
        if self
            .quality
            .get(&decision.letter)
            .is_some_and(|score| score.compare(self.best_quality).is_lt())
        {
            decision.letter = self.best_letter;
        }
    }
}

impl Snapshot {
    pub fn from_game(game: &HangmanGame) -> Self {
        let available: Vec<char> = ('A'..='Z').filter(|c| !game.guessed.contains(c)).collect();
        let guessed: Vec<String> = game.guessed.iter().map(char::to_string).collect();
        let wrong: Vec<String> = game
            .guessed
            .iter()
            .filter(|c| !game.word.contains(**c))
            .map(char::to_string)
            .collect();
        let pattern = game
            .display()
            .chars()
            .map(|c| c.to_string())
            .collect::<Vec<_>>()
            .join(" ");

        let mut state = json!({
            "game": "Hangman",
            "language": "English",
            "word_pattern": pattern,
            "guessed_letters": guessed,
            "wrong_letters": wrong,
            "wrong_guesses_remaining": game.lives,
            "available_letters": available.iter().map(char::to_string).collect::<Vec<_>>(),
        });
        let mut local_advice = None;
        if !game.fixed {
            state["difficulty_level"] = json!(game.level + 1);
            state["wins_in_level"] = json!(game.completed_in_level());
            // These are public dictionary candidates filtered from visible clues.
            // No secret-word field or answer-derived hint is sent to JeV.
            let candidates =
                solver::matching_words(&game.display(), &game.guessed, game.solver_words());
            local_advice = LocalAdvice::from_game(game, &candidates, &available);
            state["candidate_words"] = json!(candidates);
        }
        Snapshot {
            state,
            available,
            local_advice,
        }
    }

    fn payload(&self, model: &str) -> Value {
        let criteria: Map<String, Value> = self
            .available
            .iter()
            .map(|c| (c.to_string(), Value::Null))
            .collect();
        json!({
            "model": model,
            "state": self.state,
            "questions": {
                "next_letter": {
                    "type": "choice",
                    "instructions": "Choose one available letter to maximize the chance of revealing a hidden position before the six wrong guesses are used. Use the visible pattern, previous guesses, and candidate_words when present. Prefer a letter shared by many candidate words; when hit chances are close, prefer one that separates the candidates for later turns. Do not choose a previously guessed letter.",
                    "criteria": criteria,
                }
            }
        })
    }
}

pub struct Decision {
    pub letter: char,
    pub model_letter: char,
    pub probability: f64,
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

pub(crate) fn trace_enabled() -> bool {
    std::env::var("HANGMAN_TRACE").is_ok_and(|value| value == "1")
}

fn trace_request(snapshot: &Snapshot, endpoint: &str, model: &str) {
    let state = &snapshot.state;
    let candidates = state["candidate_words"]
        .as_array()
        .map_or("not sent".to_owned(), |words| words.len().to_string());
    eprintln!("[AI TRACE 1/4] Hangman connects to {endpoint} and requests model {model}");
    eprintln!(
        "[AI TRACE] Clues sent: pattern={} guessed={} wrong={} lives={} available_letters={} candidate_words={candidates}",
        state["word_pattern"],
        state["guessed_letters"],
        state["wrong_letters"],
        state["wrong_guesses_remaining"],
        snapshot.available.len(),
    );
}

fn trace_response(body: &Value, status: u16, elapsed: Duration) {
    let mut choices: Vec<(&str, f64)> = body["answers"]["next_letter"]["probabilities"]
        .as_object()
        .into_iter()
        .flat_map(|probabilities| probabilities.iter())
        .filter_map(|(letter, probability)| Some((letter.as_str(), probability.as_f64()?)))
        .collect();
    choices.sort_by(|a, b| b.1.total_cmp(&a.1));
    let top_choices = choices
        .iter()
        .take(5)
        .map(|(letter, probability)| format!("{letter}:{probability:.3}"))
        .collect::<Vec<_>>()
        .join(",");
    eprintln!(
        "[AI TRACE 2/4] Server replied HTTP {status} in {} ms: model={} chose={} model_preferences=[{top_choices}] tokens={}/{} (preferences are not hit chances)",
        elapsed.as_millis(),
        body["model"],
        body["answers"]["next_letter"]["choice"],
        body["usage"]["input_tokens"],
        body["usage"]["output_tokens"],
    );
}

fn trace_local_rule(snapshot: &Snapshot, decision: &Decision) {
    if let Some(advice) = &snapshot.local_advice {
        let picked = advice.quality[&decision.model_letter];
        let reason = if decision.letter == decision.model_letter {
            "kept the model's letter because no word-list option has a better result"
        } else if picked.hits < advice.best_quality.hits {
            "changed the letter because it appears in more possible words"
        } else if picked.projected_wins < advice.best_quality.projected_wins {
            "changed the letter because the lookahead predicts more wins"
        } else if picked.projected_misses > advice.best_quality.projected_misses {
            "changed the letter because the lookahead predicts fewer misses"
        } else {
            "changed the letter because the lookahead predicts fewer turns"
        };
        eprintln!(
            "[AI TRACE 3/4] Word-list check: {} possible words; model's {} appears in {}/{}; best {} appears in {}/{}; Hangman uses {}",
            advice.candidate_count,
            decision.model_letter,
            picked.hits,
            advice.candidate_count,
            advice.best_letter,
            advice.best_quality.hits,
            advice.candidate_count,
            decision.letter,
        );
        eprintln!("[AI TRACE] Why: {reason}.");
    } else {
        let reason = if snapshot.state.get("candidate_words").is_none() {
            "this is a fixed practice word, so dictionary frequencies could mislead"
        } else {
            "no built-in words match the visible clues"
        };
        eprintln!(
            "[AI TRACE 3/4] Hangman uses {} without changing it",
            decision.letter
        );
        eprintln!("[AI TRACE] Why: {reason}.");
    }
}

fn parse_decision(body: &Value, available: &[char]) -> Result<Decision, &'static str> {
    let answer = &body["answers"]["next_letter"];
    if answer["type"] != "choice" {
        return Err("BAD ANSWER TYPE");
    }
    let choice = answer["choice"].as_str().ok_or("MISSING CHOICE")?;
    let mut chars = choice.chars();
    let letter = chars.next().ok_or("INVALID LETTER")?;
    if chars.next().is_some() || !letter.is_ascii_uppercase() || !available.contains(&letter) {
        return Err("INVALID LETTER");
    }
    let probability = answer["probabilities"][choice]
        .as_f64()
        .ok_or("MISSING PROBABILITY")?;
    if !(0.0..=1.0).contains(&probability) {
        return Err("INVALID PROBABILITY");
    }
    Ok(Decision {
        letter,
        model_letter: letter,
        probability,
        model: body["model"].as_str().unwrap_or("jev-latest").to_owned(),
        input_tokens: body["usage"]["input_tokens"].as_u64().unwrap_or(0),
        output_tokens: body["usage"]["output_tokens"].as_u64().unwrap_or(0),
    })
}

pub fn choose_letter(
    api_key: &str,
    endpoint: &str,
    model: &str,
    snapshot: Snapshot,
) -> Result<Decision, String> {
    let trace = trace_enabled();
    if trace {
        trace_request(&snapshot, endpoint, model);
    }
    let agent = if endpoint == ENDPOINT {
        http_agent()
    } else {
        local_http_agent()
    };
    let started = Instant::now();
    let response = agent
        .post(endpoint)
        .set("Authorization", &format!("Bearer {api_key}"))
        .send_json(snapshot.payload(model));
    let response = match response {
        Ok(response) => response,
        Err(ureq::Error::Status(code, _)) => {
            if trace {
                eprintln!(
                    "[AI TRACE 2/4] Server replied HTTP {code} after {} ms",
                    started.elapsed().as_millis()
                );
            }
            return Err(format!("HTTP {code}"));
        }
        Err(ureq::Error::Transport(error)) => {
            if trace {
                eprintln!(
                    "[AI TRACE 2/4] Connection failed after {} ms: {error}",
                    started.elapsed().as_millis()
                );
            }
            return Err("NETWORK ERROR".into());
        }
    };
    let status = response.status();
    let body: Value = response.into_json().map_err(|_| {
        if trace {
            eprintln!("[AI TRACE] Hangman could not read the server's JSON response");
        }
        "BAD JSON RESPONSE"
    })?;
    if trace {
        trace_response(&body, status, started.elapsed());
    }
    let mut decision = parse_decision(&body, &snapshot.available).map_err(|error| {
        if trace {
            eprintln!("[AI TRACE 3/4] Hangman rejected the server's choice: {error}");
        }
        error.to_owned()
    })?;
    if endpoint != ENDPOINT {
        if let Some(advice) = &snapshot.local_advice {
            advice.apply(&mut decision);
        }
        if trace {
            trace_local_rule(&snapshot, &decision);
        }
    } else if trace {
        eprintln!(
            "[AI TRACE 3/4] Hangman uses {} without a local word-list correction (hosted JeV)",
            decision.letter
        );
    }
    Ok(decision)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_contains_only_visible_clues_and_all_remaining_letters() {
        let mut game = HangmanGame::with_word("MANGO".into());
        for c in "ANOET".chars() {
            game.handle_guess(c);
        }
        let snapshot = Snapshot::from_game(&game);
        let payload = snapshot.payload("jev-latest");
        assert_eq!(payload["model"], "jev-latest");
        assert_eq!(payload["state"]["word_pattern"], "_ A N _ O");
        assert_eq!(payload["state"]["wrong_guesses_remaining"], 4);
        assert_eq!(payload["state"]["wrong_letters"], json!(["E", "T"]));
        assert_eq!(
            payload["questions"]["next_letter"]["criteria"]
                .as_object()
                .unwrap()
                .len(),
            21
        );
        assert!(payload["questions"]["next_letter"]["criteria"]
            .get("A")
            .is_none());
        assert!(payload["questions"]["next_letter"]["criteria"]
            .get("M")
            .is_some());
        assert!(!payload.to_string().contains("MANGO"));
    }

    #[test]
    fn normal_round_sends_public_candidates_without_looking_at_secret() {
        let game = HangmanGame::new_normal_at_level(0, Default::default(), None);
        let payload = Snapshot::from_game(&game).payload("jev-latest");
        let candidates = payload["state"]["candidate_words"].as_array().unwrap();
        let expected = solver::matching_words(&game.display(), &game.guessed, game.solver_words());
        assert_eq!(candidates.len(), expected.len());
        assert!(!candidates.is_empty());
        assert!(candidates.contains(&json!(game.word)));
        assert!(payload["state"].get("secret_word").is_none());
    }

    #[test]
    fn rejects_a_choice_that_was_not_offered() {
        let body = json!({
            "model": "jev-1.13.0",
            "answers": {"next_letter": {
                "type": "choice", "choice": "A", "probabilities": {"A": 0.9}
            }}
        });
        assert!(parse_decision(&body, &['M', 'G']).is_err());
    }

    #[test]
    fn reads_a_valid_choice_and_probability() {
        let body = json!({
            "model": "jev-1.13.0",
            "answers": {"next_letter": {
                "type": "choice", "choice": "M", "probabilities": {"M": 0.8, "G": 0.2}
            }},
            "usage": {"input_tokens": 300, "output_tokens": 25}
        });
        let decision = parse_decision(&body, &['M', 'G']).unwrap();
        assert_eq!(decision.letter, 'M');
        assert_eq!(decision.model_letter, 'M');
        assert_eq!(decision.probability, 0.8);
        assert_eq!(decision.input_tokens, 300);
    }

    #[test]
    fn local_advice_corrects_a_lower_hit_rate_without_relabeling_model_probability() {
        const CANDIDATES: &[&str] = &["APPLE", "GRAPE", "MANGO"];
        let mut game = HangmanGame::with_word_at_level("APPLE".into(), 0, false);
        game.eligible_words = CANDIDATES.to_vec();
        let available: Vec<char> = ('A'..='Z').collect();
        let advice = LocalAdvice::from_game(&game, CANDIDATES, &available).unwrap();
        let mut decision = Decision {
            letter: 'P',
            model_letter: 'P',
            probability: 0.7,
            model: "kev-latest".into(),
            input_tokens: 1,
            output_tokens: 1,
        };
        advice.apply(&mut decision);
        assert_eq!(advice.best_quality.hits, 3);
        assert_eq!(decision.letter, advice.best_letter);
        assert_eq!(decision.model_letter, 'P');
        assert_eq!(decision.probability, 0.7);
    }

    #[test]
    fn local_advice_looks_ahead_when_hit_rates_tie() {
        let pool = crate::word_bank::words_for_level(3);
        let mut game = HangmanGame::with_word_at_level("GLACIER".into(), 3, false);
        game.eligible_words = pool.to_vec();
        let candidates = solver::matching_words(&game.display(), &game.guessed, pool);
        let available: Vec<char> = ('A'..='Z').collect();
        let advice = LocalAdvice::from_game(&game, &candidates, &available).unwrap();
        assert_eq!(advice.quality[&'A'].hits, advice.quality[&'M'].hits);
        assert_eq!(advice.best_letter, 'M');
        assert!(advice.quality[&'M'].projected_misses < advice.quality[&'A'].projected_misses);
        let mut decision = Decision {
            letter: 'A',
            model_letter: 'A',
            probability: 0.6,
            model: "kev-latest".into(),
            input_tokens: 1,
            output_tokens: 1,
        };
        advice.apply(&mut decision);
        assert_eq!(decision.letter, 'M');
    }

    #[test]
    fn local_model_keeps_a_choice_with_equal_projected_result() {
        const CANDIDATES: &[&str] = &["LETTER"];
        let mut game = HangmanGame::with_word_at_level("LETTER".into(), 0, false);
        game.eligible_words = CANDIDATES.to_vec();
        let available: Vec<char> = ('A'..='Z').collect();
        let advice = LocalAdvice::from_game(&game, CANDIDATES, &available).unwrap();
        let mut decision = Decision {
            letter: 'T',
            model_letter: 'T',
            probability: 0.4,
            model: "kev-latest".into(),
            input_tokens: 1,
            output_tokens: 1,
        };
        advice.apply(&mut decision);
        assert_eq!(decision.letter, 'T');
    }

    #[test]
    fn custom_practice_word_does_not_claim_exact_candidate_odds() {
        let game = HangmanGame::with_word("MANGO".into());
        assert!(Snapshot::from_game(&game).local_advice.is_none());
    }

    #[test]
    fn local_policy_vs_odds_across_the_word_bank() {
        fn play(word: &str, pool: &'static [&'static str], local: bool) -> (bool, usize, usize) {
            let mut game = HangmanGame::with_word_at_level(word.to_owned(), 0, false);
            game.eligible_words = pool.to_vec();
            while !game.is_over() {
                let pattern = game.display();
                let letter = if local {
                    let candidates = solver::matching_words(&pattern, &game.guessed, pool);
                    let available: Vec<char> = ('A'..='Z')
                        .filter(|letter| !game.guessed.contains(letter))
                        .collect();
                    LocalAdvice::from_game(&game, &candidates, &available)
                        .unwrap()
                        .best_letter
                } else {
                    solver::analyze(&pattern, &game.guessed, pool)
                        .best_letter()
                        .unwrap()
                        .letter
                };
                game.handle_guess(letter);
            }
            (
                game.is_won(),
                usize::from(crate::MAX_LIVES - game.lives),
                game.guessed.len(),
            )
        }
        let mut odds = (0, 0, 0);
        let mut local = (0, 0, 0);
        for level in 0..crate::word_bank::LEVELS {
            let pool = crate::word_bank::words_for_level(level);
            for &word in pool {
                let a = play(word, pool, false);
                let b = play(word, pool, true);
                odds.0 += usize::from(a.0);
                odds.1 += a.1;
                odds.2 += a.2;
                local.0 += usize::from(b.0);
                local.1 += b.1;
                local.2 += b.2;
            }
        }
        assert_eq!(odds.0, 200);
        assert_eq!(local.0, 200);
        assert!(local.1 <= odds.1, "local {local:?} versus Odds {odds:?}");
        assert!(local.2 <= odds.2, "local {local:?} versus Odds {odds:?}");
        eprintln!("current bank: Odds {odds:?}; local lookahead {local:?} (wins, misses, turns)");
    }
}
