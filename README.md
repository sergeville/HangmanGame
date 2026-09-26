# Hangman

A Rust desktop Hangman game with 200 words arranged into ten approximate difficulty levels. Guess the hidden word before you run out of lives. For the probability model and decision-model boundaries, see [Hangman: Probability, Information Gain, and JeV](hangman_probability_and_jev.md).

## Install

Install a Rust toolchain with Cargo, then open a terminal in this project folder. Build the game with:

```sh
cargo build --release --locked
```

This downloads the Rust dependencies on the first build and creates the executable at `target/release/hangman_game` (or `target\release\hangman_game.exe` on Windows). A graphical desktop session is required to play.

## Start

From the project folder, run:

```sh
cargo run --release --locked
```

If you have already built it, you can also run `./target/release/hangman_game` on macOS or Linux, or `.\target\release\hangman_game.exe` in Windows PowerShell.

## Play

- Type a letter or left-click a letter tile to guess it.
- Correct guesses reveal every occurrence of that letter in the word. Incorrect guesses cost one life and reveal part of the figure.
- You have six lives. Reveal the whole word to win; after six incorrect guesses, the word is shown and the round ends.
- A letter you have already guessed does not cost another life. The letter tiles and status line show your progress.
- Press **Enter** or click **New Game** to start another round. A win counts toward the current level. You advance after winning ten distinct words from that level's twenty-word pool. A loss or an unfinished round keeps you at the same level.
- Press **Esc** or close the window to quit.
- Press **F2** or click **JeV Plays** to let JeV guess automatically; press it again to pause. With a local model configured, the button reads **Local AI Plays**.
- Press **F3** or click **Odds Plays** to let the built-in probability solver guess automatically; press it again to pause.
- Press **F4** or click **Odds vs JeV** twice to start a match. The first press shows the call limit; the second starts the match. With a local model configured, the button reads **Odds vs Local AI**.

For a repeatable first round on macOS or Linux, set a word when starting the game, for example:

```sh
HANGMAN_WORD=MANGO cargo run --release --locked
```

`HANGMAN_WORD` accepts letters A–Z only. It sets the first round to a practice word. **New Game** then returns to random level play. If the variable is unset or invalid, the game chooses a word from the current level immediately. To start with a random word, run the plain `cargo run --release --locked` command above.

## Words and difficulty

The game bundles 200 distinct English words, twenty per level, so normal play works offline. The original 100 words were checked against the [English-word-frequencies list](https://github.com/brekker23/English-word-frequencies), which counts words in public-domain books. The added 100 words were hand-curated for variety and letter difficulty. The levels are approximate; actual Hangman difficulty also depends on your guesses.

You start at level 1. The screen shows your level and wins toward the ten-win goal. Each level draws from twenty words, and won words are removed from that level's draw. After a loss, **New Game** chooses a different unsolved word at the same level. Win ten distinct words to reach the next level. Finishing level 10 completes the 100-win challenge; **New Game** then restarts at level 1. Progress lasts for the current app session and starts again at level 1 after you quit.

### Word-list source and licensing

[WORD_LIST_LICENSE](WORD_LIST_LICENSE) contains the Apache 2.0 license text from the English-word-frequencies source used to check the original word list. It records that source's license; it does not license this game's Rust code. No project-wide code license has been chosen or added. GitHub may display an Apache-2.0 badge because it detects the standalone license text, so that badge should not be read as a license for the whole project.

## Let the probability solver play

Start a round with the solver playing automatically:

```sh
cargo run --release --locked -- --solver-auto
```

The solver needs no API key or network access. It keeps only eligible words in the current level that fit the visible pattern and previous guesses. Won words, and the immediately previous word after a loss when alternatives remain, are excluded. It chooses the unguessed letter found in the most remaining words. Repeating an opening letter in an identical state is expected for a highest-hit strategy. Tied letters are resolved alphabetically.

To inspect one calculated turn in the terminal, run `cargo run --release --locked -- --solver-once`. It prints the chosen letter and the count of matching words that contain it, such as `4/5`. During automatic play, the status line shows the letter's current-level hit rate, and the terminal shows the counts. The solver stops at the end of the round or if no words match the clues. **New Game** starts manually after a completed round; press **F3** to run the solver again.

These rates are exact for a normal round, because the game selects uniformly from the eligible words in its current level. With `HANGMAN_WORD`, the solver uses the full 200-word bank as a heuristic; even a word in the bank is no longer a random draw. A custom word outside the bank may have no matching candidates. The solver never reads the secret word to choose a letter.

## Odds vs JeV

Press **F4** or click **Odds vs JeV** once to arm a match. The screen shows the maximum number of JeV requests for that match. Press it again to start. Both players then get the same word and the same eligible word pool, but keep separate guesses and lives. The two word rows show their progress. A win beats a loss; if both win, fewer misses wins, then fewer turns. Equal results are a draw. **New Game** exits the match and returns to regular play without counting the match toward level progress. F2, F3, and letter input are disabled during the match.

JeV now receives the candidate words that fit its visible clues, drawn from the same level pool Odds uses. Fixed practice words do not get this level-specific list. JeV never receives a secret-word field. This gives JeV useful evidence, but does not guarantee it will beat the exact local solver. A hosted match can make up to `HANGMAN_JEV_MAX_CALLS` paid JeV requests (26 by default). If the model runs out of calls or the service fails before the word is solved, the match stops without declaring a winner. No model request is sent until the second press.

Odds calculates a letter locally. Its 750 ms pause between guesses is for watching the board, not calculation time. JeV needs a separate API request after each guess to react to the new clues. The game now reuses its HTTP connection when possible and waits only 100 ms between JeV requests. During a match, the top line shows the latest JeV request time; the terminal logs each request time and the total for a completed JeV round. Those times include network and response processing, not just model inference. Match winners are still decided by game results, misses, and turns rather than elapsed time.

## Let JeV play automatically

On macOS, the game reads the TypeSafe API key from the Keychain entry with account `jev` and service `typesafe-jev` when you activate JeV. If `TYPESAFE_API_KEY` is already set, it uses that instead. The key is not printed or stored in the project.

Start the game with JeV ready to play immediately:

```sh
cargo run --release --locked -- --jev-auto
```

For a first trial that makes **at most one** JeV request:

```sh
HANGMAN_JEV_MAX_CALLS=1 cargo run --release --locked -- --jev-auto
```

You can also start with `cargo run --release --locked` and then press **F2** or click **JeV Plays**. JeV chooses one available letter at a time; the game applies each guess. The status line shows JeV's letter, its hit rate in the eligible word list, and the latest request time. The terminal also shows JeV's Choice probability, the best list hit rate, model, token usage, and request time. JeV's Choice probability compares the offered letters; it is not the chance of a correct guess. Human letter input is paused while JeV plays. Press **F2** again to resume manual play. JeV stops when the round ends; **New Game** starts a manual round, and you can activate JeV again if you want another automatic round. Switching between **JeV Plays** and **Odds Plays** pauses the other player.

To test one JeV turn in the terminal without opening the game window, run `cargo run --release --locked -- --jev-once`. This sends one API request, applies the chosen letter, and prints the resulting pattern.

If the Keychain entry is unavailable, get an API key from the [TypeSafe console](https://console.typesafe.ai/) and set `TYPESAFE_API_KEY` in your environment. In macOS or Linux Terminal, run this line by itself, paste the key when input waits, and press **Enter**. The key will not be displayed or placed in your shell history:

```sh
read -s TYPESAFE_API_KEY
```

Then run:

```sh
export TYPESAFE_API_KEY
cargo run --release --locked
```

JeV is off by default. Each automatic guess sends one TypeSafe API request, which may incur charges; a round can make up to 26 requests unless `HANGMAN_JEV_MAX_CALLS` is set to a number from 1 to 26. The game does not retry failed requests. Pausing prevents new requests, although a request already in progress may still finish. If the key is missing or a request fails, the game shows an error instead of guessing. Restart the game after setting or changing the key.

## Use a local decision model

**What is running locally?** The game can send its visible-clue Choice request to a local server that implements `POST /v1/systemone`. That path is the *System One API format*, not proof that TypeSafe's System One service or JeV model is running on your computer. In this project's local setup, [Kev-0.8B](https://github.com/jaredpalmer/kev) serves the endpoint at `http://127.0.0.1:8009/v1/systemone`. Kev is a separate model; it does not contain TypeSafe's JeV weights. Other compatible servers include [Laya](https://github.com/NandhaKishorM/laya), [CLM](https://github.com/Contrastive-LM/CLM), and [LocalJev](https://github.com/githubnext/localjev).

The game selects the provider when it starts:

| Setting | Server | Model |
| --- | --- | --- |
| Neither local model variable is set | `https://api.typesafe.ai/v1/systemone` | Hosted TypeSafe JeV, using your API key |
| `HANGMAN_LOCAL_MODEL_PORT=8009` | `http://127.0.0.1:8009/v1/systemone` | The model served on that port; `kev-latest` selects the local Kev model below |

Setting `HANGMAN_LOCAL_MODEL_NAME` without a port is a configuration error; it does not switch to the hosted service.

Kev-0.8B is installed in `.local-ai/kev` on this Mac. To start it, open Terminal in the HangmanGame directory and run:

```sh
HF_HOME="$PWD/.local-ai/hf-cache" HF_HUB_OFFLINE=1 .local-ai/kev/.venv/bin/python -m kev.serve --run jaredpalmer/kev-0.8b --port 8009
```

Leave that Terminal open. Wait for `Uvicorn running on http://127.0.0.1:8009`, then open a second Terminal in the HangmanGame directory and run:

```sh
HANGMAN_LOCAL_MODEL_PORT=8009 HANGMAN_LOCAL_MODEL_NAME=kev-latest cargo run --release --locked
```

Press **F2** for **Local AI Plays** or **F4** twice for **Odds vs Local AI**. In local mode the game sends requests only to `127.0.0.1` at the selected port. It sends a placeholder local key, never reads the TypeSafe key, and never falls back to the hosted endpoint if the local server is down or the configuration is invalid. The default model name is `jev-latest`, which LocalJev accepts as an alias; use `HANGMAN_LOCAL_MODEL_NAME=kev-latest` for Kev. The 26-call round limit still applies. Kev-0.8B uses no TypeSafe requests. Other local servers may have their own upstream connections. The first request may take longer while the local model loads. Keep the server Terminal open while playing; closing it stops the local endpoint.

### API keys

| Model | Key for this setup |
| --- | --- |
| Hosted JeV | Requires a TypeSafe API key from `TYPESAFE_API_KEY` or the macOS Keychain entry described above |
| [Local Kev](https://github.com/jaredpalmer/kev/blob/main/kev/serve.py) | No key by default; its server can optionally require `KEV_API_KEY` |
| [Local Laya](https://github.com/NandhaKishorM/laya/blob/main/laya/serve.py) | No key by default; its server can optionally require `LAYA_API_KEY` |

Kev's and Laya's optional keys protect **their own servers**; they are not TypeSafe keys. Hangman currently sends the fixed placeholder `Authorization: Bearer local` to a local server and has no setting for a different local server key. If local server authentication requires another key, Hangman gets HTTP 401. If you run Laya for local testing without a key, set `LAYA_HOST=127.0.0.1`: its documented default binds to all network interfaces.

### Check the local connection

On this Mac, `lsof -nP -iTCP:8009 -sTCP:LISTEN` should show the Python server listening on `127.0.0.1:8009`. The server Terminal should show `Uvicorn running on http://127.0.0.1:8009`. From a second Terminal in the project folder, make one bounded local game request without opening the window:

```sh
HANGMAN_LOCAL_MODEL_PORT=8009 HANGMAN_LOCAL_MODEL_NAME=kev-latest HANGMAN_WORD=CASTLE cargo run --release --locked -- --jev-once
```

This is a one-turn connection test, not a duel. The three `HANGMAN_...` settings apply only to this command: use the local server on port 8009, request its `kev-latest` model, and make the first practice word `CASTLE`. `--release --locked` uses the optimized Cargo build and locked dependencies. The standalone `--` passes `--jev-once` to the game; despite that older flag name, the local settings make this a Kev request. The game asks for one letter, applies it, prints the result, and exits without opening a window. `HANGMAN_WORD` is not sent to the model as a secret-word field.

For example, one run printed:

```text
Local AI model=kev-latest model_pick=A letter=A api_ms=480 model_choice_probability=0.142 list_hit_rate=34/74 list_best=E(43/74) hit=true pattern=______ -> _A____ lives=6 tokens=293/314
```

| Output | Meaning in this example |
| --- | --- |
| `model=kev-latest model_pick=A letter=A` | Kev proposed `A`, and the game applied `A`. `model_pick` and `letter` can differ when the game's local word-list rule corrects a proposal. |
| `api_ms=480` | The complete local request took 480 ms; this is not just model inference time. |
| `model_choice_probability=0.142` | Kev assigned `A` a 14.2% choice probability among the offered letters. This is **not** the probability of hitting the secret word. |
| `list_hit_rate=34/74 list_best=E(43/74)` | Of the 74 six-letter words matching the blank pattern in the bundled word bank, 34 contain `A` and 43 contain `E`. The word-list heuristic prefers `E`. Because this is a fixed practice word, the game does not override Kev's `A`; normal rounds can apply the local correction. |
| `hit=true pattern=______ -> _A____ lives=6` | `A` occurs in `CASTLE`, so the second position is revealed and no life is lost. |
| `tokens=293/314` | Input/output usage reported by the local server. This request does not call the hosted TypeSafe service. |

Cargo's `Finished` and `Running` lines report that the executable is ready and that `hangman_game --jev-once` started. The example shows that the local connection works for one turn; it does not measure Kev's accuracy across full games. Values such as `api_ms`, model choice, and token usage vary between runs.

If the terminal prints `Local AI error: NETWORK ERROR api_ms=0`, check that the server Terminal is still open and listening on the same port as `HANGMAN_LOCAL_MODEL_PORT`. `NETWORK ERROR` means the HTTP request had a transport failure; the game currently does not print the underlying socket error. `api_ms=0` means the failure was measured at less than one millisecond, not that the model evaluated a letter in zero time. In the observed case, nothing was listening on port 8009, and starting Kev resolved it. A running server with the wrong port or a blocked local connection can produce the same generic error.

The game does not retry a failed request. If a duel has stopped, press **Enter** or **New Game** to leave that duel, then press **F4** twice for a fresh match. This discards the stopped match; pressing F4 inside it does not resume it. If **Local AI Plays** stopped outside a duel, press **F2** to start it again. Restart the game after changing the local port or model name, since those settings are read at launch.

### Kev and Laya in a duel

**Duel** is the game mode, not a model. It pits the built-in Odds solver against the local model selected when Hangman starts. The setup above runs **Odds vs Kev**. [Laya](https://github.com/NandhaKishorM/laya) is a different model family that can serve the same `/v1/systemone` request format; it is not installed or running in this project's local setup. The game does not currently offer a direct Kev vs Laya match.

| | Kev-0.8B used here | Laya |
| --- | --- | --- |
| Model | [Qwen3.5-0.8B base with a trained adapter and decision head](https://github.com/jaredpalmer/kev/blob/main/docs/model-cards/kev-0.8b.md) | [English ModernBERT and multilingual mmBERT checkpoints, selected by a router](https://github.com/NandhaKishorM/laya#readme) |
| Hangman connection | The installed Kev server on port 8009 | Requires a separate Laya server and matching Hangman port setting |
| What the game does | Sends a Choice question over visible clues | Sends the same Choice question over visible clues |

The two models may propose different letters, but neither is automatically stronger at Hangman. The game applies its local candidate-word correction to either model's proposal in normal rounds, so a duel result measures the model **with** that rule. The terminal's `model_pick` shows the model's original letter; `letter` shows the applied guess. A fair Kev versus Laya accuracy claim would need both models tested on the same words and clues. Laya has not been tested here.

For normal rounds, the game checks the local model's proposed letter against the words still possible from visible clues. It first keeps the letters with the highest exact chance of a hit. For ties, it simulates each possible word and lets Odds finish the hypothetical round, then prefers more projected wins, fewer misses, and fewer turns. The model decides when those results tie. This prevents a guess like the earlier Kev trial's 3/11 letter when a 6/11 letter was available. The terminal reports `model_pick`, the applied `letter`, and the model's probability for its own pick; the status line says `CORRECTED` when they differ. The Odds vs Local AI duel now compares Odds against a model assisted by this visible-clue rule.

In an offline simulation over all 200 current words, the deterministic guided rule won all 200 with 71 misses and 1,242 turns. Odds also won all 200, with 73 misses and 1,244 turns. This checks the letter rule with every eligible word as the answer; real Kev may choose differently when projected results tie.

Fixed `HANGMAN_WORD` practice rounds keep the model's choice because their word was selected deliberately, so dictionary frequencies are only a heuristic. A custom word may be outside the game's dictionary. The model's Choice probability is a preference among offered letters, not the chance of hitting the word. One real Kev-0.8B test took 2.4 seconds on its first request and 348 ms on a warm request on this Mac; those are full game request times, and they do not establish its accuracy over many rounds.

## Play with JeV in the Playground

You can also choose each letter manually in the TypeSafe Playground, then enter that letter in the game. Leave automatic JeV mode off for this walkthrough.

For a repeatable first turn on macOS or Linux, start the game with `HANGMAN_WORD=MANGO cargo run --release --locked`. Guess **A**, **N**, **O**, **E**, and **T**. The visible pattern will be `_ A N _ O`, with four lives remaining. In the Playground, paste this into **State**:

```json
{
  "game": "Hangman",
  "language": "English",
  "word_pattern": "_ A N _ O",
  "guessed_letters": ["A", "N", "O", "E", "T"],
  "wrong_letters": ["E", "T"],
  "wrong_guesses_remaining": 4,
  "available_letters": ["B", "C", "D", "F", "G", "H", "I", "J", "K", "L", "M", "P", "Q", "R", "S", "U", "V", "W", "X", "Y", "Z"]
}
```

Paste this into **Questions**:

```json
{
  "next_letter": {
    "type": "choice",
    "instructions": "Choose the available letter most likely to reveal an unrevealed position in the word. Use the word pattern and previous guesses. Do not choose a letter already guessed.",
    "criteria": {
      "B": null, "C": null, "D": null, "F": null, "G": null,
      "H": null, "I": null, "J": null, "K": null, "L": null,
      "M": null, "P": null, "Q": null, "R": null, "S": null,
      "U": null, "V": null, "W": null, "X": null, "Y": null, "Z": null
    }
  }
}
```

Click **Run request**, then type or click the letter in `answers.next_letter.choice`. Update the pattern, guesses, lives, and available letters before the next request. For a random game, use the clues currently shown on screen. Keep the secret word out of JeV's state.
