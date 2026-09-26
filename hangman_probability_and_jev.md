# Hangman: Probability, Information Gain, and JeV

## Purpose

This Rust Hangman game uses a mathematical solver to choose letters from explicit word-list hit rates. It can also ask hosted JeV or a compatible local decision model, currently Kev in this project's local setup, for a typed letter choice. The game itself remains responsible for checking guesses, revealing letters, counting mistakes, and detecting wins or losses.

## 1. Build the candidate set

Start with a dictionary and keep only words consistent with the visible game evidence:

- Same word length.
- Same letters in every revealed position.
- No incorrectly guessed letters.
- No extra occurrences of already revealed letters in hidden positions: ordinary Hangman reveals every occurrence of a correct guess.
- A matching category, if the game supplies a reliable category.

Call the remaining candidate set **C**. Never give the solver or JeV the secret answer directly.

## 2. Calculate the chance of a hit

If each candidate word is equally likely:

$$
P(\text{hit with }\ell) = \frac{|\{w\in C : \ell\in w\}|}{|C|}
$$

Count candidate **words containing the letter**, not the total number of occurrences. Consider only letters that have not been guessed.

### Worked example

Visible pattern: **`_ A _ E`**

For illustration, suppose these are the complete remaining candidates:

```text
CAKE, CAVE, GAME, GATE, LATE, MAKE, NAME, SAME
```

| Next letter | Candidate words containing it | Hit probability |
|---|---|---:|
| M | GAME, MAKE, NAME, SAME | 50% |
| C | CAKE, CAVE | 25% |
| G | GAME, GATE | 25% |
| T | GATE, LATE | 25% |
| K | CAKE, MAKE | 25% |
| V | CAVE | 12.5% |
| L | LATE | 12.5% |
| N | NAME | 12.5% |
| S | SAME | 12.5% |

**M has the highest probability of a hit in this candidate set.** These percentages are calculated from this illustrative list, not obtained from JeV or a comprehensive dictionary.

## 3. Use Bayes' rule for unequal word probabilities

Words need not have equal starting probabilities. For example, the game's actual word-selection distribution or a reliable category can inform the prior probability of a word.

$$
P(w\mid E) = \frac{P(E\mid w)P(w)}{\sum_v P(E\mid v)P(v)}
$$

Where:

- **w** is a candidate word.
- **E** is the visible evidence: pattern, correct guesses, and incorrect guesses.
- **P(w)** is the prior probability of that word.
- **P(E | w)** is the likelihood of observing the evidence if that word were the answer.
- **P(w | E)** is the updated probability after applying the evidence.

For ordinary deterministic Hangman, the likelihood is 1 when the word matches every clue and 0 otherwise. The calculation eliminates inconsistent words and renormalizes the probabilities of the survivors.

The probability of a hit becomes:

$$
P(\text{hit with }\ell\mid E) = \sum_{w\in C:\,\ell\in w} P(w\mid E)
$$

If the game chooses uniformly from its word list, uniform candidate weights are appropriate. General language frequency is not automatically the right prior for that game.

## 4. Measure information gain

A different objective is to reduce uncertainty about the answer. A letter can be informative even if its chance of a hit is lower.

Entropy measures uncertainty in the candidate distribution:

$$
H(W\mid E) = -\sum_{w\in C} P(w\mid E)\log_2 P(w\mid E)
$$

For a proposed letter, partition the candidates by the result it would produce. A result includes either a miss or the exact set of positions revealed.

Expected information gain is:

$$
IG(\ell\mid E) = H(W\mid E) - \sum_r P(r\mid E,\ell)H(W\mid E,r,\ell)
$$

A larger value means the guess is expected to narrow the possible answers more effectively. With base-2 logarithms, information gain is measured in bits.

**A high chance of a hit and high information gain are different objectives.** Neither alone necessarily maximizes the probability of winning the entire round with a limited number of mistakes.

## 5. A practical first strategy

1. Filter the dictionary using all known clues.
2. Calculate the hit probability of every unguessed letter.
3. Choose the letter with the highest hit probability.
4. Break ties using information gain, if implemented.
5. Apply the guess through the game's existing rules.
6. Update the evidence and repeat.

This is a straightforward baseline, particularly when few mistakes remain. A more advanced solver can evaluate future game states and remaining lives to optimize the probability of winning a whole round.

If no candidate words remain, do not divide by zero or fabricate confidence. Report that the dictionary does not cover the current evidence, check the filtering logic, and fall back to a broader dictionary or an explicitly labeled heuristic.

## 6. How Rust and decision models work together

| Component | Responsibility |
|---|---|
| Rust game logic | Validate guesses, reveal all matching positions, update mistakes, and determine the outcome |
| Mathematical solver | Filter candidates and compute hit probabilities; information gain is explained here but not implemented in the game |
| Hosted JeV | Choose among supplied available letters from the visible evidence through TypeSafe's System One API; requires a TypeSafe API key |
| Local model such as Kev | Answer the same `POST /v1/systemone` Choice request using separate local model weights; the endpoint format does not mean JeV runs locally |
| Integration code | Validate the model's selected letter and pass an allowed guess to the game |

The game has several ways to play:

- **Odds Plays (F3):** Rust chooses the letter with the highest calculated hit rate from words consistent with visible clues, using alphabetical tie breaking. No model call is needed.
- **JeV Plays or Local AI Plays (F2):** the configured model proposes a letter from visible clues. Normal rounds include the public candidate words; fixed practice rounds do not. For local models only, Rust can replace a lower-quality proposal using candidate hit rates and one-turn lookahead. The terminal reports `model_pick` and the applied `letter` separately.
- **Odds vs JeV or Odds vs Local AI (F4 twice):** Odds and the configured model play independent boards with the same secret word and eligible word pool. A duel compares wins, then misses, then turns. It does not directly compare Kev with Laya.

The local correction means a duel score is for the model **with** Rust's public-clue rule, not a raw model benchmark. For a purely highest-hit strategy, Rust can choose the maximum directly; a model call is unnecessary.

### Example System One state

This is an illustrative normal-round state using the eight-word candidate set above, not a dump of a current level. The real game offers every unguessed A–Z letter, sends `candidate_words` for normal rounds, and omits that list for fixed practice words. All fields below are public clues or public game progress.

```json
{
  "game": "Hangman",
  "language": "English",
  "word_pattern": "_ A _ E",
  "guessed_letters": ["A", "E"],
  "wrong_letters": [],
  "wrong_guesses_remaining": 6,
  "available_letters": ["B", "C", "D", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z"],
  "difficulty_level": 1,
  "wins_in_level": 0,
  "candidate_words": ["CAKE", "CAVE", "GAME", "GATE", "LATE", "MAKE", "NAME", "SAME"]
}
```

### Example Choice question

```json
{
  "next_letter": {
    "type": "choice",
    "instructions": "Choose one available letter to maximize the chance of revealing a hidden position. Use the visible pattern, previous guesses, and candidate_words. Do not choose a previously guessed letter.",
    "criteria": {
      "B": null, "C": null, "D": null, "F": null, "G": null, "H": null,
      "I": null, "J": null, "K": null, "L": null, "M": null, "N": null,
      "O": null, "P": null, "Q": null, "R": null, "S": null, "T": null,
      "U": null, "V": null, "W": null, "X": null, "Y": null, "Z": null
    }
  }
}
```

For a real game, generate the Choice options from the allowed unguessed letters on each turn. Read the selected letter from `answers.next_letter.choice` and validate it before applying it.

**A model's Choice probabilities are not the same as the solver's letter-hit probabilities.** Multiple letters may all occur in one word, so their hit probabilities need not sum to 1. Choice probabilities describe a distribution over which single option the model selects.

## 7. Evaluate performance

Compare strategies over the same set of secret words and game rules:

- Percentage of rounds won.
- Average incorrect guesses per round.
- Average number of turns.
- Decision latency and API request count.
- Frequency of missing dictionary coverage or invalid model responses.

Do not infer a strong strategy from one successful round. Keep the secret word hidden from both decision systems and distinguish calculated statistics from model estimates.

## Implementation status

The Rust game bundles 200 words in ten approximate difficulty levels, twenty words per level. Each level requires ten distinct wins before advancing. After a loss, the next round draws another unsolved word from the same level when one is available. Odds filters the words eligible for the current round using visible clues and chooses the highest hit rate, with alphabetical ties. Weighted priors and a general information-gain solver are not implemented; the local model correction does simulate future Odds play to break ties between maximum-hit letters.

Hosted JeV calls `https://api.typesafe.ai/v1/systemone` and needs a TypeSafe API key. The installed local Kev-0.8B server answers the compatible API at `http://127.0.0.1:8009/v1/systemone` without a TypeSafe key. Laya's Docker HTTP server also answers the compatible API at `http://127.0.0.1:8010/v1/systemone`, using its `english` checkpoint. Its public checkpoint requires no account or API key in this setup. A local `/v1/systemone` URL identifies a request format, not TypeSafe's JeV weights. See the [README's local setup, Docker quick start, key table, and troubleshooting steps](README.md#use-a-local-decision-model).

Fixed `HANGMAN_WORD` practice rounds use the full 200-word bank only as a solver heuristic. They skip local candidate correction, so a fixed-word test such as `CASTLE` can apply Kev's `A` even when the word-list heuristic ranks `E` higher. A Laya Docker connection test on `CASTLE` applied `J` and lost a life; its HTTP request succeeded in about 3.4 seconds. These individual turns verify the local request paths, not either model's accuracy. A model's Choice probability is not its letter-hit probability; Laya's checkpoint also emitted a confidence-calibration warning.

## Further reading

- [Jev vs Laya: Hosted API or Open Weights? (2026 Guide)](https://huggingface.co/blog/sora-2/jev-vs-laya-hosted-api-or-open-weights-2026-guide) is a Hugging Face **community article** comparing hosted JeV with self-hosted Laya. Its benchmark figures describe a separate setup and do not establish either model's Hangman performance. Check the exact model versions and the [Laya project](https://github.com/NandhaKishorM/laya) before using its conclusions for this game.
