//! Hangman in a window, styled after `~/Dev/HTML/hangman.jpeg`.
//!
//! The scene is a software composite: the reference photo (man, sign and hint patched out) is
//! the backdrop, the Vision cutout of the man is drawn in front and revealed part by part as
//! lives are lost, and the text, word row and key-tile tints are painted at the pixel anchors
//! of the original image. winit for the window and input, softbuffer for the pixels.
//!
//! Type a letter or click a tile to guess, Enter (or the New Game plank) restarts, Esc quits.
//! F1 opens the help screen; F2 starts and pauses JeV; F3 starts and pauses the local solver.
//! `HANGMAN_WORD=mango` fixes the first round for testing.

mod jev;
mod solver;
mod word_bank;

use word_bank::WORDS;

use rand::seq::SliceRandom;
use std::collections::BTreeSet;
use std::num::NonZeroU32;
#[cfg(target_os = "macos")]
use std::process::Command;
use std::rc::Rc;
use std::time::{Duration, Instant};
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{ElementState, Event, KeyEvent, MouseButton, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use winit::keyboard::{Key, NamedKey};
use winit::window::WindowBuilder;

const MAX_LIVES: u8 = 6;
const ODDS_TURN_DELAY: Duration = Duration::from_millis(750);
const JEV_TURN_DELAY: Duration = Duration::from_millis(100);

// ---------------------------------------------------------------------------
// Game logic
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct HangmanGame {
    word: String,
    level: usize,
    fixed: bool,
    solved_words: BTreeSet<String>,
    eligible_words: Vec<&'static str>,
    guessed: BTreeSet<char>,
    lives: u8,
    last: Option<(char, bool)>, // last guess and whether it hit
}

impl HangmanGame {
    fn new() -> Self {
        // HANGMAN_WORD=mango fixes the first round (letters only); used for testing.
        let fixed = std::env::var("HANGMAN_WORD")
            .ok()
            .map(|w| w.trim().to_ascii_uppercase())
            .filter(|w| !w.is_empty() && w.chars().all(|c| c.is_ascii_uppercase()));
        if let Some(word) = fixed {
            Self::with_word_at_level(word, 0, true)
        } else {
            Self::new_normal_at_level(0, BTreeSet::new(), None)
        }
    }

    fn new_normal_at_level(
        level: usize,
        solved_words: BTreeSet<String>,
        exclude: Option<&str>,
    ) -> Self {
        let level = level.min(word_bank::LEVELS - 1);
        let mut eligible_words: Vec<&str> = word_bank::words_for_level(level)
            .iter()
            .copied()
            .filter(|word| !solved_words.contains(*word))
            .collect();
        // Avoid an immediate repeat after a loss, unless it is the last unsolved word.
        if eligible_words.len() > 1 {
            eligible_words.retain(|word| Some(*word) != exclude);
        }
        let word = eligible_words
            .choose(&mut rand::thread_rng())
            .expect("level has an unsolved word")
            .to_string();
        let mut game = Self::with_word_at_level(word, level, false);
        game.solved_words = solved_words;
        game.eligible_words = eligible_words;
        game
    }

    #[cfg(test)]
    fn with_word(word: String) -> Self {
        Self::with_word_at_level(word, 0, true)
    }

    fn with_word_at_level(word: String, level: usize, fixed: bool) -> Self {
        let eligible_words = if fixed {
            WORDS.to_vec()
        } else {
            word_bank::words_for_level(level).to_vec()
        };
        HangmanGame {
            word,
            level,
            fixed,
            solved_words: BTreeSet::new(),
            eligible_words,
            guessed: BTreeSet::new(),
            lives: MAX_LIVES,
            last: None,
        }
    }

    fn next_round(&self) -> Self {
        if self.fixed {
            return Self::new_normal_at_level(self.level, BTreeSet::new(), None);
        }
        let mut solved_words = self.solved_words.clone();
        if self.is_won() {
            solved_words.insert(self.word.clone());
        }
        if solved_words.len() == word_bank::WINS_TO_ADVANCE {
            let next_level = (self.level + 1) % word_bank::LEVELS;
            Self::new_normal_at_level(next_level, BTreeSet::new(), None)
        } else {
            Self::new_normal_at_level(self.level, solved_words, Some(&self.word))
        }
    }

    fn completed_in_level(&self) -> usize {
        self.solved_words.len()
            + usize::from(self.is_won() && !self.solved_words.contains(&self.word))
    }

    fn solver_words(&self) -> &[&str] {
        &self.eligible_words
    }

    /// Returns true if the guess changed the game state.
    fn handle_guess(&mut self, letter: char) -> bool {
        if self.is_over() || !letter.is_ascii_alphabetic() {
            return false;
        }
        let letter = letter.to_ascii_uppercase();
        if !self.guessed.insert(letter) {
            return false; // already guessed
        }
        let hit = self.word.contains(letter);
        if !hit {
            self.lives = self.lives.saturating_sub(1);
        }
        self.last = Some((letter, hit));
        true
    }

    fn display(&self) -> String {
        self.word
            .chars()
            .map(|c| if self.guessed.contains(&c) { c } else { '_' })
            .collect()
    }

    fn is_won(&self) -> bool {
        self.word.chars().all(|c| self.guessed.contains(&c))
    }

    fn is_lost(&self) -> bool {
        self.lives == 0
    }

    fn is_over(&self) -> bool {
        self.is_won() || self.is_lost()
    }

    /// Body part `pid` (0 right leg … 5 head) is shown once lives have dropped to it.
    /// Losing the first life reveals the head, the last one the right leg.
    fn part_visible(&self, pid: u8) -> bool {
        pid >= self.lives
    }

    fn status(&self) -> (String, u32) {
        if !self.fixed
            && self.level == word_bank::LEVELS - 1
            && self.completed_in_level() == word_bank::WINS_TO_ADVANCE
        {
            return ("ALL 10 LEVELS COMPLETE!".into(), STATUS_GOOD);
        }
        if self.is_lost() {
            (format!("HANGED. WORD: {}", self.word), STATUS_BAD)
        } else if self.is_won() {
            (format!("SAVED! WORD: {}", self.word), STATUS_GOOD)
        } else {
            match self.last {
                Some((c, true)) => (
                    format!("GOOD GUESS: {}. LIVES LEFT: {}", c, self.lives),
                    STATUS_GOOD,
                ),
                Some((c, false)) => (
                    format!("WRONG: {}. LIVES LEFT: {}", c, self.lives),
                    STATUS_BAD,
                ),
                None => (format!("LIVES LEFT: {}", self.lives), STATUS_NEUTRAL),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Scene geometry, in pixels of the 1178x896 reference image
// ---------------------------------------------------------------------------

const IMG_W: f32 = 1178.0;
const IMG_H: f32 = 896.0;
const MAN_BOX: (f32, f32) = (656.0, 165.0); // top-left of man.png in the backdrop
const TITLE: (f32, f32) = (598.0, 46.0);
const STATUS: (f32, f32) = (598.0, 69.0);
const WORD_ROW: (f32, f32) = (590.0, 418.0);
const NEW_GAME: (f32, f32) = (595.0, 849.0);
const NEW_GAME_HIT: (f32, f32) = (110.0, 26.0);
const SOLVER_BUTTON: (f32, f32) = (435.0, 849.0);
const SOLVER_BUTTON_HIT: (f32, f32) = (130.0, 26.0);
const JEV_BUTTON: (f32, f32) = (755.0, 849.0);
const JEV_BUTTON_HIT: (f32, f32) = (130.0, 26.0);
const DUEL_BUTTON: (f32, f32) = (944.0, 849.0);
const DUEL_BUTTON_HIT: (f32, f32) = (150.0, 26.0);
const HINT: (f32, f32) = (598.0, 876.0);
const TILE: f32 = 27.0;

struct KeyRow {
    letters: &'static str,
    x0: f32,
    y: f32,
    pitch: f32,
}

const KEY_ROWS: [KeyRow; 2] = [
    KeyRow {
        letters: "ABCDEFGHIJKLMNO",
        x0: 377.0,
        y: 780.0,
        pitch: 31.3,
    },
    KeyRow {
        letters: "PQRSTUVWXYZ",
        x0: 438.0,
        y: 815.0,
        pitch: 31.5,
    },
];

/// Centre of a letter's tile in image pixels.
fn tile_centre(letter: char) -> Option<(f32, f32)> {
    for row in &KEY_ROWS {
        if let Some(i) = row.letters.find(letter) {
            return Some((row.x0 + i as f32 * row.pitch, row.y));
        }
    }
    None
}

/// Which tile, if any, contains this image-pixel point.
fn tile_at(x: f32, y: f32) -> Option<char> {
    for row in &KEY_ROWS {
        for (i, letter) in row.letters.chars().enumerate() {
            let cx = row.x0 + i as f32 * row.pitch;
            if (x - cx).abs() <= TILE / 2.0 && (y - row.y).abs() <= TILE / 2.0 {
                return Some(letter);
            }
        }
    }
    None
}

fn on_new_game(x: f32, y: f32) -> bool {
    (x - NEW_GAME.0).abs() <= NEW_GAME_HIT.0 / 2.0 && (y - NEW_GAME.1).abs() <= NEW_GAME_HIT.1 / 2.0
}

fn on_jev_button(x: f32, y: f32) -> bool {
    (x - JEV_BUTTON.0).abs() <= JEV_BUTTON_HIT.0 / 2.0
        && (y - JEV_BUTTON.1).abs() <= JEV_BUTTON_HIT.1 / 2.0
}

fn on_solver_button(x: f32, y: f32) -> bool {
    (x - SOLVER_BUTTON.0).abs() <= SOLVER_BUTTON_HIT.0 / 2.0
        && (y - SOLVER_BUTTON.1).abs() <= SOLVER_BUTTON_HIT.1 / 2.0
}

fn on_duel_button(x: f32, y: f32) -> bool {
    (x - DUEL_BUTTON.0).abs() <= DUEL_BUTTON_HIT.0 / 2.0
        && (y - DUEL_BUTTON.1).abs() <= DUEL_BUTTON_HIT.1 / 2.0
}

// ---------------------------------------------------------------------------
// Colours (0x00RRGGBB)
// ---------------------------------------------------------------------------

const BARS: u32 = 0x00_0b_0a_09;
const GOLD_HI: u32 = 0x00_f3_d8_8a;
const GOLD_LO: u32 = 0x00_c4_92_36;
const SHADOW: u32 = 0x00_1a_0f_06;
const STATUS_BAD: u32 = 0x00_ff_a3_8a;
const STATUS_GOOD: u32 = 0x00_ff_e4_8f;
const STATUS_NEUTRAL: u32 = 0x00_f7_e8_c0;
const LEVEL_TEXT: u32 = 0x00_ff_f8_df;
const PLANK_INK: u32 = 0x00_f2_d8_8c;
const HINT_INK: u32 = 0x00_d9_a8_4a;
const CHALK: u32 = 0x00_e8_e7_d9;
const CHALK_DIM: u32 = 0x00_b8_c8_b7;

// ---------------------------------------------------------------------------
// Images
// ---------------------------------------------------------------------------

struct Rgba {
    w: usize,
    h: usize,
    px: Vec<[u8; 4]>,
}

impl Rgba {
    fn decode(bytes: &[u8]) -> Self {
        let img = image::load_from_memory(bytes)
            .expect("decode embedded image")
            .to_rgba8();
        let (w, h) = (img.width() as usize, img.height() as usize);
        let px = img.pixels().map(|p| p.0).collect();
        Rgba { w, h, px }
    }

    fn at(&self, x: isize, y: isize) -> [u8; 4] {
        let x = x.clamp(0, self.w as isize - 1) as usize;
        let y = y.clamp(0, self.h as isize - 1) as usize;
        self.px[y * self.w + x]
    }

    /// Bilinear sample; coordinates in this image's pixels.
    fn sample(&self, x: f32, y: f32) -> [u8; 4] {
        let (x, y) = (x - 0.5, y - 0.5);
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let (x0, y0) = (x0 as isize, y0 as isize);
        let p = [
            self.at(x0, y0),
            self.at(x0 + 1, y0),
            self.at(x0, y0 + 1),
            self.at(x0 + 1, y0 + 1),
        ];
        let mut out = [0u8; 4];
        for c in 0..4 {
            let top = p[0][c] as f32 * (1.0 - fx) + p[1][c] as f32 * fx;
            let bot = p[2][c] as f32 * (1.0 - fx) + p[3][c] as f32 * fx;
            out[c] = (top * (1.0 - fy) + bot * fy).round() as u8;
        }
        out
    }
}

fn pack(p: [u8; 4]) -> u32 {
    (p[0] as u32) << 16 | (p[1] as u32) << 8 | p[2] as u32
}

fn blend(dst: u32, src: [u8; 4]) -> u32 {
    let a = src[3] as u32;
    if a == 0 {
        return dst;
    }
    if a == 255 {
        return pack(src);
    }
    let mix = |d: u32, s: u8| ((d * (255 - a) + s as u32 * a) / 255) & 0xff;
    mix(dst >> 16, src[0]) << 16 | mix(dst >> 8 & 0xff, src[1]) << 8 | mix(dst & 0xff, src[2])
}

// ---------------------------------------------------------------------------
// Renderer
// ---------------------------------------------------------------------------

/// Maps image pixels to physical window pixels: the photo is fitted inside the window
/// ("contain"), centred, with dark bars where the aspect differs.
#[derive(Clone, Copy)]
struct Layout {
    s: f32,
    ox: f32,
    oy: f32,
}

impl Layout {
    fn fit(w: usize, h: usize) -> Self {
        let s = (w as f32 / IMG_W).min(h as f32 / IMG_H).max(1e-3);
        Layout {
            s,
            ox: (w as f32 - IMG_W * s) / 2.0,
            oy: (h as f32 - IMG_H * s) / 2.0,
        }
    }
    fn to_phys(self, x: f32, y: f32) -> (f32, f32) {
        (self.ox + x * self.s, self.oy + y * self.s)
    }
    fn to_img(self, px: f32, py: f32) -> (f32, f32) {
        ((px - self.ox) / self.s, (py - self.oy) / self.s)
    }
}

struct Renderer {
    bg: Rgba,
    man: Rgba,
    regions: Rgba,
    /// backdrop already scaled to the current window, rebuilt on resize
    backdrop: Vec<u32>,
    backdrop_size: (usize, usize),
}

struct RenderState<'a> {
    game: &'a HangmanGame,
    jev_mode: &'a JeVMode,
    solver_mode: &'a SolverMode,
    duel: Option<&'a Duel>,
    duel_armed: bool,
    help_open: bool,
}

impl Renderer {
    fn new() -> Self {
        Renderer {
            bg: Rgba::decode(include_bytes!("../assets/bg.jpg")),
            man: Rgba::decode(include_bytes!("../assets/man.png")),
            regions: Rgba::decode(include_bytes!("../assets/regions.png")),
            backdrop: Vec::new(),
            backdrop_size: (0, 0),
        }
    }

    fn rebuild_backdrop(&mut self, w: usize, h: usize, lay: Layout) {
        if self.backdrop_size == (w, h) {
            return;
        }
        let mut out = vec![BARS; w * h];
        let (x0, y0) = lay.to_phys(0.0, 0.0);
        let (x1, y1) = lay.to_phys(IMG_W, IMG_H);
        let (x0, y0) = (x0.round().max(0.0) as usize, y0.round().max(0.0) as usize);
        let (x1, y1) = ((x1.round() as usize).min(w), (y1.round() as usize).min(h));
        for py in y0..y1 {
            let iy = (py as f32 + 0.5 - lay.oy) / lay.s;
            for px in x0..x1 {
                let ix = (px as f32 + 0.5 - lay.ox) / lay.s;
                out[py * w + px] = pack(self.bg.sample(ix, iy));
            }
        }
        self.backdrop = out;
        self.backdrop_size = (w, h);
    }

    fn render(&mut self, state: RenderState<'_>, buf: &mut [u32], w: usize, h: usize) {
        let RenderState {
            game,
            jev_mode,
            solver_mode,
            duel,
            duel_armed,
            help_open,
        } = state;
        let lay = Layout::fit(w, h);
        self.rebuild_backdrop(w, h, lay);
        buf.copy_from_slice(&self.backdrop);
        let mut c = Canvas { buf, w, h, lay };

        if help_open {
            self.draw_help(&mut c, jev_mode.is_local());
            return;
        }

        self.draw_man(&mut c, game);
        self.draw_scoreboard(&mut c, game, duel, jev_mode.is_local());

        // plaque
        c.text_centred(TITLE.0, TITLE.1, 3.0, Ink::Gold, "HANGMAN");
        let (status, color) = duel.map_or_else(
            || game.status(),
            |match_state| match_state.status(game, jev_mode),
        );
        let local = jev_mode.is_local();
        let status = if local {
            status.replace("JEV", "LOCAL AI")
        } else {
            status
        };
        c.text_centred(STATUS.0, STATUS.1, 2.1, Ink::Contrast(color), &status);
        let message = if duel_armed {
            format!(
                "DUEL READY - F4 AGAIN - MAX {} {} CALLS",
                jev_mode.max_calls_in_round,
                if local { "LOCAL AI" } else { "JEV" }
            )
        } else if duel.is_none() && game.fixed {
            "PRACTICE: NEW GAME FOR RANDOM".to_string()
        } else {
            String::new()
        };
        let auto_status = if duel.is_some() || duel_armed {
            ""
        } else if !solver_mode.status.is_empty() {
            &solver_mode.status
        } else if jev_mode.status == "JEV OFF" {
            ""
        } else {
            &jev_mode.status
        };
        let auto_status = if local {
            auto_status.replace("JEV", "LOCAL AI")
        } else {
            auto_status.to_owned()
        };
        c.text_centred(
            598.0,
            105.0,
            1.9,
            Ink::Contrast(LEVEL_TEXT),
            &format!("{message}  {auto_status}"),
        );

        if let Some(match_state) = duel {
            c.text_centred(WORD_ROW.0, 328.0, 2.0, Ink::Gold, "ODDS");
            draw_word_row(&mut c, &match_state.odds.display(), 365.0);
            c.text_centred(
                WORD_ROW.0,
                418.0,
                2.0,
                Ink::Gold,
                if local { "LOCAL AI" } else { "JEV" },
            );
            draw_word_row(&mut c, &game.display(), 455.0);
        } else {
            draw_word_row(&mut c, &game.display(), WORD_ROW.1);
        }

        // key tiles: tint the ones already played
        for &letter in &game.guessed {
            if let Some((cx, cy)) = tile_centre(letter) {
                let hit = game.word.contains(letter);
                let tint = if hit {
                    [255, 190, 70, 96]
                } else {
                    [0, 0, 0, 150]
                };
                c.fill_rect(cx - TILE / 2.0, cy - TILE / 2.0, TILE, TILE, tint);
                if !hit {
                    c.text_centred(cx, cy, 2.4, Ink::Flat(0x00_b0_30_24), "X");
                }
            }
        }

        c.text_centred(
            NEW_GAME.0,
            NEW_GAME.1,
            1.8,
            Ink::Flat(PLANK_INK),
            "NEW GAME",
        );
        c.text_centred(
            SOLVER_BUTTON.0,
            SOLVER_BUTTON.1,
            1.8,
            Ink::Flat(PLANK_INK),
            if duel.is_some() {
                "ODDS IN DUEL"
            } else if solver_mode.enabled {
                "ODDS PAUSE"
            } else {
                "ODDS PLAYS"
            },
        );
        c.text_centred(
            JEV_BUTTON.0,
            JEV_BUTTON.1,
            1.8,
            Ink::Flat(PLANK_INK),
            if duel.is_some() {
                if local {
                    "AI IN DUEL"
                } else {
                    "JEV IN DUEL"
                }
            } else if jev_mode.enabled {
                if local {
                    "LOCAL AI PAUSE"
                } else {
                    "JEV PAUSE"
                }
            } else if local {
                "LOCAL AI PLAYS"
            } else {
                "JEV PLAYS"
            },
        );
        c.text_centred(
            DUEL_BUTTON.0,
            DUEL_BUTTON.1,
            1.8,
            Ink::Flat(PLANK_INK),
            if duel.is_some() {
                "DUEL ACTIVE"
            } else if duel_armed {
                "START DUEL"
            } else if local {
                "ODDS VS LOCAL"
            } else {
                "ODDS VS JEV"
            },
        );
        c.text_centred(
            HINT.0,
            HINT.1,
            1.3,
            Ink::Flat(HINT_INK),
            if local {
                "TYPE OR CLICK   F1: HELP   ENTER: NEW GAME   F2: LOCAL AI   F3: ODDS   F4: DUEL   ESC: QUIT"
            } else {
                "TYPE OR CLICK   F1: HELP   ENTER: NEW GAME   F2: JEV   F3: ODDS   F4: DUEL   ESC: QUIT"
            },
        );
    }

    fn draw_scoreboard(
        &self,
        c: &mut Canvas<'_>,
        game: &HangmanGame,
        duel: Option<&Duel>,
        local: bool,
    ) {
        // A framed slate sits in the empty, upper-left part of the backdrop.
        c.rect(26.0, 30.0, 328.0, 198.0, SHADOW);
        c.rect(20.0, 24.0, 328.0, 198.0, 0x00_4a_2c_19);
        c.rect(24.0, 28.0, 320.0, 190.0, 0x00_9c_6b_38);
        c.rect(30.0, 34.0, 308.0, 178.0, 0x00_09_17_12);
        c.rect(36.0, 80.0, 296.0, 2.0, CHALK_DIM);
        c.text_centred(
            184.0,
            57.0,
            3.1,
            Ink::Flat(CHALK),
            if duel.is_some() {
                "DUEL SCORE"
            } else {
                "SCOREBOARD"
            },
        );

        if let Some(match_state) = duel {
            let sides = [
                ("ODDS", &match_state.odds),
                (if local { "LOCAL AI" } else { "JEV" }, game),
            ];
            for (i, (label, side)) in sides.into_iter().enumerate() {
                let y = 91.0 + i as f32 * 56.0;
                let outcome = if side.is_won() {
                    "WIN"
                } else if side.is_lost() {
                    "LOSS"
                } else {
                    "PLAY"
                };
                c.text(
                    43.0,
                    y,
                    2.6,
                    &Ink::Flat(CHALK),
                    &format!("{label}  {outcome}"),
                );
                c.text(
                    43.0,
                    y + 24.0,
                    1.9,
                    &Ink::Flat(CHALK_DIM),
                    &format!(
                        "LIVES {}  MISS {}  TURNS {}",
                        side.lives,
                        MAX_LIVES - side.lives,
                        side.guessed.len()
                    ),
                );
            }
        } else {
            c.text(
                48.0,
                94.0,
                2.8,
                &Ink::Flat(CHALK),
                &format!("LEVEL  {} / {}", game.level + 1, word_bank::LEVELS),
            );
            c.text(
                48.0,
                128.0,
                2.8,
                &Ink::Flat(CHALK),
                &format!(
                    "WINS   {} / {}",
                    game.completed_in_level(),
                    word_bank::WINS_TO_ADVANCE
                ),
            );
            c.text(
                48.0,
                162.0,
                2.8,
                &Ink::Flat(CHALK),
                &format!("LIVES  {} / {}", game.lives, MAX_LIVES),
            );
            if game.fixed {
                c.text(48.0, 195.0, 1.7, &Ink::Flat(CHALK_DIM), "PRACTICE ROUND");
            }
        }
    }

    fn draw_help(&self, c: &mut Canvas<'_>, local: bool) {
        c.fill_rect(0.0, 0.0, IMG_W, IMG_H, [8, 7, 6, 238]);
        c.rect(90.0, 64.0, 998.0, 768.0, 0x00_20_18_12);
        c.rect(90.0, 64.0, 998.0, 4.0, GOLD_LO);
        c.rect(90.0, 828.0, 998.0, 4.0, GOLD_LO);
        c.rect(90.0, 64.0, 4.0, 768.0, GOLD_LO);
        c.rect(1084.0, 64.0, 4.0, 768.0, GOLD_LO);

        c.text_centred(589.0, 122.0, 5.0, Ink::Gold, "HOW TO PLAY");
        c.text_centred(
            589.0,
            164.0,
            2.2,
            Ink::Flat(LEVEL_TEXT),
            "HANGMAN RULES AND CONTROLS",
        );
        c.rect(145.0, 187.0, 888.0, 2.0, GOLD_LO);

        c.text(157.0, 205.0, 3.0, &Ink::Gold, "RULES");
        for (i, line) in [
            "TYPE A-Z OR CLICK A LETTER TILE TO GUESS",
            "A CORRECT LETTER REVEALS EVERY MATCH",
            "A WRONG LETTER COSTS ONE OF 6 LIVES",
            "REVEAL THE WORD BEFORE LIVES REACH 0",
            "WIN 10 DIFFERENT WORDS TO ADVANCE A LEVEL",
            "THERE ARE 10 LEVELS WITH 20 WORDS EACH",
        ]
        .iter()
        .enumerate()
        {
            c.text(
                157.0,
                244.0 + i as f32 * 34.0,
                2.8,
                &Ink::Flat(LEVEL_TEXT),
                line,
            );
        }

        c.text(157.0, 464.0, 3.0, &Ink::Gold, "CONTROLS");
        let model = if local { "LOCAL AI" } else { "JEV" };
        let requirement = if local { "SERVER" } else { "API KEY" };
        let controls = [
            "F1     OPEN OR CLOSE THIS HELP".to_owned(),
            "ENTER  NEW GAME OR LEAVE A DUEL".to_owned(),
            format!("F2     {model} PLAYS / PAUSES - NEEDS {requirement}"),
            "F3     ODDS PLAYS / PAUSES - NO NETWORK".to_owned(),
            format!("F4     ODDS VS {model} - PRESS TWICE TO START"),
            "ESC    CLOSE HELP; OUTSIDE HELP, QUIT".to_owned(),
            "CLICK  LETTER TILES, NEW GAME OR MODE BUTTONS".to_owned(),
        ];
        for (i, line) in controls.iter().enumerate() {
            c.text(
                157.0,
                502.0 + i as f32 * 34.0,
                2.8,
                &Ink::Flat(LEVEL_TEXT),
                line,
            );
        }
        c.text_centred(
            589.0,
            766.0,
            2.2,
            Ink::Gold,
            "DUEL: SAME WORD; WIN, THEN FEWER MISSES AND TURNS",
        );
        c.text_centred(
            589.0,
            801.0,
            2.5,
            Ink::Flat(LEVEL_TEXT),
            "PRESS F1 OR ESC TO RETURN TO YOUR GAME",
        );
    }

    fn draw_man(&self, c: &mut Canvas, game: &HangmanGame) {
        if !(0..6).any(|p| game.part_visible(p)) {
            return;
        }
        let lay = c.lay;
        let (bx0, by0) = lay.to_phys(MAN_BOX.0, MAN_BOX.1);
        let (bx1, by1) = lay.to_phys(MAN_BOX.0 + self.man.w as f32, MAN_BOX.1 + self.man.h as f32);
        let (px0, py0) = (bx0.floor().max(0.0) as usize, by0.floor().max(0.0) as usize);
        let (px1, py1) = (
            (bx1.ceil() as usize).min(c.w),
            (by1.ceil() as usize).min(c.h),
        );
        for py in py0..py1 {
            let my = (py as f32 + 0.5 - by0) / lay.s;
            for px in px0..px1 {
                let mx = (px as f32 + 0.5 - bx0) / lay.s;
                let pid = (self.regions.at(mx as isize, my as isize)[0] as u32 + 20) / 40;
                if pid > 5 || !game.part_visible(pid as u8) {
                    continue;
                }
                let src = self.man.sample(mx, my);
                let i = py * c.w + px;
                c.buf[i] = blend(c.buf[i], src);
            }
        }
    }
}

fn draw_word_row(c: &mut Canvas<'_>, display: &str, y: f32) {
    let n = display.chars().count() as f32;
    let (size, pitch) = if n > 8.0 { (4.0, 52.0) } else { (4.5, 60.0) };
    let start = WORD_ROW.0 - (n - 1.0) * pitch / 2.0;
    for (i, ch) in display.chars().enumerate() {
        c.text_centred(
            start + i as f32 * pitch,
            y,
            size,
            Ink::Gold,
            &ch.to_string(),
        );
    }
}

enum Ink {
    Gold,
    Flat(u32),
    Contrast(u32),
}

struct Canvas<'a> {
    buf: &'a mut [u32],
    w: usize,
    h: usize,
    lay: Layout,
}

impl<'a> Canvas<'a> {
    fn put(&mut self, x: i32, y: i32, color: u32) {
        if x >= 0 && y >= 0 && (x as usize) < self.w && (y as usize) < self.h {
            self.buf[y as usize * self.w + x as usize] = color;
        }
    }

    /// Solid rectangle, image-pixel coordinates.
    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: u32) {
        let (x0, y0) = self.lay.to_phys(x, y);
        let (x1, y1) = self.lay.to_phys(x + w, y + h);
        let (x0, y0, x1, y1) = (
            x0.round() as i32,
            y0.round() as i32,
            x1.round() as i32,
            y1.round() as i32,
        );
        for py in y0..y1.max(y0 + 1) {
            for px in x0..x1.max(x0 + 1) {
                self.put(px, py, color);
            }
        }
    }

    /// Translucent rectangle, image-pixel coordinates.
    fn fill_rect(&mut self, x: f32, y: f32, w: f32, h: f32, rgba: [u8; 4]) {
        let (x0, y0) = self.lay.to_phys(x, y);
        let (x1, y1) = self.lay.to_phys(x + w, y + h);
        let (x0, y0) = (x0.round().max(0.0) as usize, y0.round().max(0.0) as usize);
        let (x1, y1) = (
            (x1.round() as usize).min(self.w),
            (y1.round() as usize).min(self.h),
        );
        for py in y0..y1 {
            for px in x0..x1 {
                let i = py * self.w + px;
                self.buf[i] = blend(self.buf[i], rgba);
            }
        }
    }

    /// 5x7 bitmap text; `size` is the image-pixel size of one font pixel. Gold ink gets a
    /// two-tone face and a drop shadow so it reads like the embossed letters in the reference.
    fn text(&mut self, x: f32, y: f32, size: f32, ink: &Ink, s: &str) {
        if let Ink::Contrast(color) = ink {
            let edge = size * 0.5;
            for (dx, dy) in [(-edge, 0.0), (edge, 0.0), (0.0, -edge), (0.0, edge)] {
                self.text(x + dx, y + dy, size, &Ink::Flat(SHADOW), s);
            }
            self.text(x, y, size, &Ink::Flat(*color), s);
            return;
        }
        let shadow = size * 0.35;
        let mut cx = x;
        for ch in s.chars() {
            let g = glyph(ch);
            for (row, bits) in g.iter().enumerate() {
                for col in 0..5 {
                    if bits & (0b1_0000 >> col) == 0 {
                        continue;
                    }
                    let (gx, gy) = (cx + col as f32 * size, y + row as f32 * size);
                    match ink {
                        Ink::Gold => {
                            self.rect(gx + shadow, gy + shadow, size, size, SHADOW);
                            let face = if row < 3 { GOLD_HI } else { GOLD_LO };
                            self.rect(gx, gy, size, size, face);
                        }
                        Ink::Flat(color) => self.rect(gx, gy, size, size, *color),
                        Ink::Contrast(_) => unreachable!(),
                    }
                }
            }
            cx += size * 6.0;
        }
    }

    fn text_centred(&mut self, cx: f32, cy: f32, size: f32, ink: Ink, s: &str) {
        let w = s.chars().count() as f32 * size * 6.0 - size;
        self.text(cx - w / 2.0, cy - size * 3.5, size, &ink, s);
    }
}

/// 5x7 glyphs, one byte per row, bit 4 = leftmost column.
fn glyph(ch: char) -> [u8; 7] {
    match ch.to_ascii_uppercase() {
        'A' => [0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'B' => [0x1E, 0x11, 0x11, 0x1E, 0x11, 0x11, 0x1E],
        'C' => [0x0E, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0E],
        'D' => [0x1E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x1E],
        'E' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x1F],
        'F' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x10],
        'G' => [0x0E, 0x11, 0x10, 0x17, 0x11, 0x11, 0x0F],
        'H' => [0x11, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'I' => [0x0E, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0E],
        'J' => [0x07, 0x02, 0x02, 0x02, 0x02, 0x12, 0x0C],
        'K' => [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11],
        'L' => [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1F],
        'M' => [0x11, 0x1B, 0x15, 0x15, 0x11, 0x11, 0x11],
        'N' => [0x11, 0x19, 0x15, 0x13, 0x11, 0x11, 0x11],
        'O' => [0x0E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'P' => [0x1E, 0x11, 0x11, 0x1E, 0x10, 0x10, 0x10],
        'Q' => [0x0E, 0x11, 0x11, 0x11, 0x15, 0x12, 0x0D],
        'R' => [0x1E, 0x11, 0x11, 0x1E, 0x14, 0x12, 0x11],
        'S' => [0x0F, 0x10, 0x10, 0x0E, 0x01, 0x01, 0x1E],
        'T' => [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        'U' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'V' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x0A, 0x04],
        'W' => [0x11, 0x11, 0x11, 0x15, 0x15, 0x1B, 0x11],
        'X' => [0x11, 0x11, 0x0A, 0x04, 0x0A, 0x11, 0x11],
        'Y' => [0x11, 0x11, 0x0A, 0x04, 0x04, 0x04, 0x04],
        'Z' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1F],
        '0' => [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E],
        '1' => [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E],
        '2' => [0x0E, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1F],
        '3' => [0x1F, 0x02, 0x04, 0x02, 0x01, 0x11, 0x0E],
        '4' => [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02],
        '5' => [0x1F, 0x10, 0x1E, 0x01, 0x01, 0x11, 0x0E],
        '6' => [0x06, 0x08, 0x10, 0x1E, 0x11, 0x11, 0x0E],
        '7' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
        '8' => [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E],
        '9' => [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x02, 0x0C],
        '_' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x1F],
        ':' => [0x00, 0x0C, 0x0C, 0x00, 0x0C, 0x0C, 0x00],
        '!' => [0x04, 0x04, 0x04, 0x04, 0x04, 0x00, 0x04],
        '?' => [0x0E, 0x11, 0x01, 0x02, 0x04, 0x00, 0x04],
        '-' => [0x00, 0x00, 0x00, 0x1F, 0x00, 0x00, 0x00],
        '/' => [0x01, 0x02, 0x02, 0x04, 0x08, 0x08, 0x10],
        '.' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x0C, 0x0C],
        ',' => [0x00, 0x00, 0x00, 0x00, 0x0C, 0x04, 0x08],
        _ => [0x00; 7], // space and anything unknown
    }
}

// ---------------------------------------------------------------------------
// Window and input
// ---------------------------------------------------------------------------

fn typesafe_api_key() -> Result<String, &'static str> {
    if let Ok(key) = std::env::var("TYPESAFE_API_KEY") {
        if !key.trim().is_empty() {
            return Ok(key.trim().to_owned());
        }
    }

    #[cfg(target_os = "macos")]
    {
        let output = Command::new("security")
            .args([
                "find-generic-password",
                "-a",
                "jev",
                "-s",
                "typesafe-jev",
                "-w",
            ])
            .output()
            .map_err(|_| "KEYCHAIN TOOL MISSING")?;
        if !output.status.success() {
            return Err("KEYCHAIN ACCESS FAILED");
        }
        let key = String::from_utf8(output.stdout).map_err(|_| "KEYCHAIN KEY INVALID")?;
        if key.trim().is_empty() {
            return Err("KEYCHAIN KEY EMPTY");
        }
        Ok(key.trim().to_owned())
    }

    #[cfg(not(target_os = "macos"))]
    {
        Err("SET TYPESAFE API KEY")
    }
}

#[derive(Clone)]
struct LocalModel {
    port: u16,
    model: String,
}

impl LocalModel {
    fn endpoint(&self) -> String {
        format!("http://127.0.0.1:{}/v1/systemone", self.port)
    }
}

fn local_model_from_values(
    port: Option<&str>,
    model: Option<&str>,
) -> Result<Option<LocalModel>, &'static str> {
    let Some(port) = port else {
        return if model.is_some() {
            Err("SET LOCAL MODEL PORT")
        } else {
            Ok(None)
        };
    };
    let port = port
        .parse::<u16>()
        .ok()
        .filter(|port| *port != 0)
        .ok_or("BAD LOCAL MODEL PORT")?;
    let model = model.unwrap_or("jev-latest").trim();
    if model.is_empty()
        || !model
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err("BAD LOCAL MODEL NAME");
    }
    Ok(Some(LocalModel {
        port,
        model: model.to_owned(),
    }))
}

fn local_model_from_env() -> Result<Option<LocalModel>, &'static str> {
    let port = std::env::var_os("HANGMAN_LOCAL_MODEL_PORT")
        .map(|value| value.into_string().map_err(|_| "BAD LOCAL MODEL PORT"))
        .transpose()?;
    let model = std::env::var_os("HANGMAN_LOCAL_MODEL_NAME")
        .map(|value| value.into_string().map_err(|_| "BAD LOCAL MODEL NAME"))
        .transpose()?;
    local_model_from_values(port.as_deref(), model.as_deref())
}

struct JeVMode {
    api_key: Option<String>,
    local_model: Result<Option<LocalModel>, &'static str>,
    enabled: bool,
    busy: bool,
    round: u64,
    epoch: u64,
    calls_in_round: u8,
    max_calls_in_round: u8,
    ready_at: Option<Instant>,
    last_api_ms: Option<u128>,
    api_total_ms: u128,
    status: String,
}

impl JeVMode {
    fn new() -> Self {
        JeVMode {
            api_key: None,
            local_model: local_model_from_env(),
            enabled: false,
            busy: false,
            round: 0,
            epoch: 0,
            calls_in_round: 0,
            max_calls_in_round: parse_jev_call_limit(
                std::env::var("HANGMAN_JEV_MAX_CALLS").ok().as_deref(),
            ),
            ready_at: None,
            last_api_ms: None,
            api_total_ms: 0,
            status: "JEV OFF".into(),
        }
    }

    fn toggle(&mut self) {
        match &self.local_model {
            Err(error) => self.status = (*error).into(),
            Ok(Some(_)) => self.toggle_with(|| Ok("local".into())),
            Ok(None) => self.toggle_with(typesafe_api_key),
        }
    }

    fn is_local(&self) -> bool {
        matches!(self.local_model.as_ref(), Ok(Some(_)))
    }

    fn toggle_with(&mut self, load_key: impl FnOnce() -> Result<String, &'static str>) {
        self.epoch = self.epoch.wrapping_add(1);
        self.ready_at = None;
        if self.enabled {
            self.enabled = false;
            self.status = "JEV PAUSED".into();
        } else {
            if self.api_key.is_none() {
                match load_key() {
                    Ok(key) => self.api_key = Some(key),
                    Err(error) => {
                        self.status = error.into();
                        return;
                    }
                }
            }
            self.enabled = true;
            self.status = if self.busy {
                "JEV WAITING"
            } else {
                "JEV THINKING"
            }
            .into();
        }
    }

    fn new_round(&mut self) {
        self.round = self.round.wrapping_add(1);
        self.calls_in_round = 0;
        self.ready_at = None;
        self.last_api_ms = None;
        self.api_total_ms = 0;
        self.status = if self.enabled {
            if self.busy {
                "JEV WAITING"
            } else {
                "JEV THINKING"
            }
        } else {
            "JEV OFF"
        }
        .into();
    }

    fn stop(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        self.enabled = false;
        self.ready_at = None;
        self.last_api_ms = None;
        self.status = "JEV OFF".into();
    }
}

struct SolverMode {
    enabled: bool,
    ready_at: Option<Instant>,
    status: String,
}

impl SolverMode {
    fn new() -> Self {
        Self {
            enabled: false,
            ready_at: None,
            status: String::new(),
        }
    }

    fn toggle(&mut self) {
        self.enabled = !self.enabled;
        self.ready_at = None;
        self.status = if self.enabled {
            "ODDS THINKING"
        } else {
            "ODDS PAUSED"
        }
        .into();
    }

    fn stop(&mut self) {
        self.enabled = false;
        self.ready_at = None;
        self.status.clear();
    }

    fn new_round(&mut self) {
        self.ready_at = if self.enabled {
            Some(Instant::now())
        } else {
            None
        };
        if !self.enabled {
            self.status.clear();
        }
    }
}

struct Duel {
    odds: HangmanGame,
    resume_game: HangmanGame,
    odds_ready_at: Option<Instant>,
    odds_stalled: bool,
}

impl Duel {
    fn new(resume_game: HangmanGame, match_game: &HangmanGame) -> Self {
        Self {
            odds: match_game.clone(),
            resume_game,
            odds_ready_at: Some(Instant::now()),
            odds_stalled: false,
        }
    }

    fn step_odds(&mut self) {
        self.odds_ready_at = None;
        if self.odds.is_over() {
            return;
        }
        let analysis = solver::analyze(
            &self.odds.display(),
            &self.odds.guessed,
            self.odds.solver_words(),
        );
        if let Some(best) = analysis.best_letter() {
            if self.odds.handle_guess(best.letter) {
                eprintln!(
                    "Duel Odds letter={} hit_rate={}/{} pattern={} lives={}",
                    best.letter,
                    best.hits,
                    best.candidates,
                    self.odds.display(),
                    self.odds.lives
                );
                if !self.odds.is_over() {
                    self.odds_ready_at = Some(Instant::now() + ODDS_TURN_DELAY);
                }
                return;
            }
        }
        self.odds_stalled = true;
    }

    fn status(&self, jev: &HangmanGame, jev_mode: &JeVMode) -> (String, u32) {
        if self.odds_stalled {
            return ("DUEL STOPPED: ODDS HAS NO CANDIDATES".into(), STATUS_BAD);
        }
        if self.odds.is_over() && jev.is_over() {
            let result = match (self.odds.is_won(), jev.is_won()) {
                (true, false) => "ODDS WINS",
                (false, true) => "JEV WINS",
                (true, true) if self.odds.lives > jev.lives => "ODDS WINS",
                (true, true) if jev.lives > self.odds.lives => "JEV WINS",
                (true, true) if self.odds.guessed.len() < jev.guessed.len() => "ODDS WINS",
                (true, true) if jev.guessed.len() < self.odds.guessed.len() => "JEV WINS",
                _ => "DRAW",
            };
            return (format!("{result} - WORD: {}", jev.word), STATUS_GOOD);
        }
        if !jev_mode.enabled && !jev.is_over() {
            return (format!("DUEL STOPPED: {}", jev_mode.status), STATUS_BAD);
        }
        let status = jev_mode.last_api_ms.map_or_else(
            || "ODDS VS JEV - SAME WORD".to_string(),
            |ms| format!("ODDS VS JEV - LAST JEV API {ms}MS"),
        );
        (status, STATUS_NEUTRAL)
    }
}

fn toggle_duel(
    game: &mut HangmanGame,
    duel: &mut Option<Duel>,
    armed: &mut bool,
    jev_mode: &mut JeVMode,
    solver_mode: &mut SolverMode,
    proxy: &EventLoopProxy<JeVReply>,
) {
    if duel.is_some() {
        return;
    }
    if !*armed {
        *armed = true;
        jev_mode.stop();
        solver_mode.stop();
        return;
    }
    *armed = false;
    solver_mode.stop();
    jev_mode.stop();
    jev_mode.new_round();
    jev_mode.toggle();
    if !jev_mode.enabled {
        return;
    }
    let match_game = game.next_round();
    *duel = Some(Duel::new(game.clone(), &match_game));
    *game = match_game;
    start_jev_turn(game, jev_mode, proxy);
}

fn solver_turn(game: &mut HangmanGame, mode: &mut SolverMode) {
    if !mode.enabled {
        return;
    }
    if game.is_over() {
        mode.enabled = false;
        mode.ready_at = None;
        mode.status = "ODDS ROUND OVER".into();
        return;
    }
    let analysis = solver::analyze(&game.display(), &game.guessed, game.solver_words());
    let Some(best) = analysis.best_letter() else {
        mode.enabled = false;
        mode.ready_at = None;
        mode.status = "ODDS NO WORD LIST MATCH".into();
        return;
    };
    if !game.handle_guess(best.letter) {
        mode.enabled = false;
        mode.ready_at = None;
        mode.status = "ODDS INVALID GUESS".into();
        return;
    }
    eprintln!(
        "Odds letter={} hit_rate={}/{} pattern={} lives={}",
        best.letter,
        best.hits,
        best.candidates,
        game.display(),
        game.lives
    );
    mode.status = format!(
        "ODDS {} LIST HIT {} PCT",
        best.letter,
        (best.probability() * 100.0).round() as u8
    );
    if game.is_over() {
        mode.enabled = false;
        mode.ready_at = None;
        eprintln!("Odds round ended: {}", game.status().0);
    } else {
        mode.ready_at = Some(Instant::now() + ODDS_TURN_DELAY);
    }
}

fn parse_jev_call_limit(value: Option<&str>) -> u8 {
    value
        .and_then(|s| s.parse::<u8>().ok())
        .filter(|n| (1..=26).contains(n))
        .unwrap_or(26)
}

struct JeVReply {
    round: u64,
    epoch: u64,
    elapsed: Duration,
    result: Result<jev::Decision, String>,
}

fn start_jev_turn(game: &HangmanGame, mode: &mut JeVMode, proxy: &EventLoopProxy<JeVReply>) {
    if !mode.enabled || mode.busy {
        return;
    }
    if game.is_over() {
        mode.status = "JEV READY NEW GAME".into();
        return;
    }
    if mode.calls_in_round >= mode.max_calls_in_round {
        mode.enabled = false;
        mode.status = "JEV CALL LIMIT".into();
        return;
    }
    let Some(api_key) = mode.api_key.clone() else {
        return;
    };
    let (endpoint, model) = match &mode.local_model {
        Ok(Some(local)) => (local.endpoint(), local.model.clone()),
        _ => (jev::ENDPOINT.to_owned(), "jev-latest".to_owned()),
    };
    let snapshot = jev::Snapshot::from_game(game);
    let (round, epoch) = (mode.round, mode.epoch);
    let proxy = proxy.clone();
    mode.busy = true;
    mode.calls_in_round += 1;
    mode.status = "JEV THINKING".into();
    if jev::trace_enabled() {
        eprintln!(
            "[AI TRACE] Starting round {} request {} of {}",
            mode.round + 1,
            mode.calls_in_round,
            mode.max_calls_in_round
        );
    }
    std::thread::spawn(move || {
        let started = Instant::now();
        let result = jev::choose_letter(&api_key, &endpoint, &model, snapshot);
        let _ = proxy.send_event(JeVReply {
            round,
            epoch,
            elapsed: started.elapsed(),
            result,
        });
    });
}

fn solver_comparison(analysis: &solver::Analysis, letter: char) -> String {
    match (analysis.odds_for(letter), analysis.best_letter()) {
        (Some(pick), Some(best)) => format!(
            "list_hit_rate={}/{} list_best={}({}/{})",
            pick.hits, pick.candidates, best.letter, best.hits, best.candidates
        ),
        _ => format!(
            "list_candidates={} list_hit_rate=unknown list_best=unknown",
            analysis.candidate_count
        ),
    }
}

fn trace_ai_game_result(game: &HangmanGame, before: &str, letter: char) {
    if !jev::trace_enabled() {
        return;
    }
    let hit = game.last.is_some_and(|(last, hit)| last == letter && hit);
    let result = if game.is_won() {
        "word solved"
    } else if game.is_lost() {
        "no lives left"
    } else {
        "round continues"
    };
    eprintln!(
        "[AI TRACE 4/4] Board updated: letter={letter} {} pattern={before} -> {} lives={} ({result})",
        if hit { "hit" } else { "miss" },
        game.display(),
        game.lives,
    );
}

fn run_jev_once() -> Result<(), String> {
    let local = local_model_from_env().map_err(str::to_owned)?;
    let local_run = local.is_some();
    let (api_key, endpoint, model) = match local {
        Some(local) => ("local".to_owned(), local.endpoint(), local.model),
        None => (
            typesafe_api_key().map_err(str::to_owned)?,
            jev::ENDPOINT.to_owned(),
            "jev-latest".to_owned(),
        ),
    };
    let mut game = HangmanGame::new();
    let before = game.display();
    let analysis = solver::analyze(&before, &game.guessed, game.solver_words());
    if jev::trace_enabled() {
        eprintln!("[AI TRACE] Starting one-turn connection test");
    }
    let started = Instant::now();
    let decision =
        jev::choose_letter(&api_key, &endpoint, &model, jev::Snapshot::from_game(&game))?;
    let api_ms = started.elapsed().as_millis();
    let hit = game.word.contains(decision.letter);
    game.handle_guess(decision.letter);
    trace_ai_game_result(&game, &before, decision.letter);
    println!("{} model={} model_pick={} letter={} api_ms={} model_choice_probability={:.3} {} hit={} pattern={} -> {} lives={} tokens={}/{}",
        if local_run { "Local AI" } else { "JeV" },
        decision.model, decision.model_letter, decision.letter, api_ms, decision.probability,
        solver_comparison(&analysis, decision.letter), hit, before, game.display(),
        game.lives, decision.input_tokens, decision.output_tokens);
    Ok(())
}

fn run_solver_once() {
    let mut game = HangmanGame::new();
    let before = game.display();
    let analysis = solver::analyze(&before, &game.guessed, game.solver_words());
    let Some(best) = analysis.best_letter() else {
        println!("Odds: no matching built-in word for pattern={before}");
        return;
    };
    game.handle_guess(best.letter);
    println!(
        "Odds letter={} list_hit_rate={}/{} pattern={} -> {} lives={}",
        best.letter,
        best.hits,
        best.candidates,
        before,
        game.display(),
        game.lives
    );
}

fn main() {
    let launch_mode = std::env::args().nth(1);
    if launch_mode.as_deref() == Some("--jev-once") {
        if let Err(error) = run_jev_once() {
            eprintln!("Model: {error}");
            std::process::exit(1);
        }
        return;
    }
    if launch_mode.as_deref() == Some("--solver-once") {
        run_solver_once();
        return;
    }

    let event_loop = EventLoopBuilder::<JeVReply>::with_user_event()
        .build()
        .expect("create event loop");
    event_loop.set_control_flow(ControlFlow::Wait);
    let proxy = event_loop.create_proxy();

    let window = Rc::new(
        WindowBuilder::new()
            .with_title("Hangman")
            .with_inner_size(LogicalSize::new(IMG_W * 0.8, IMG_H * 0.8))
            .with_min_inner_size(LogicalSize::new(IMG_W * 0.4, IMG_H * 0.4))
            .build(&event_loop)
            .expect("create window"),
    );

    let context = softbuffer::Context::new(window.clone()).expect("softbuffer context");
    let mut surface =
        softbuffer::Surface::new(&context, window.clone()).expect("softbuffer surface");

    let mut renderer = Renderer::new();
    let mut game = HangmanGame::new();
    let mut jev_mode = JeVMode::new();
    let mut solver_mode = SolverMode::new();
    let mut duel: Option<Duel> = None;
    let mut duel_armed = false;
    let mut help_open = false;
    let mut held_jev_replies = Vec::new();
    let mut cursor: Option<PhysicalPosition<f64>> = None;
    if launch_mode.as_deref() == Some("--jev-auto") {
        jev_mode.toggle();
        start_jev_turn(&game, &mut jev_mode, &proxy);
    } else if launch_mode.as_deref() == Some("--solver-auto") {
        solver_mode.toggle();
        solver_turn(&mut game, &mut solver_mode);
    }

    event_loop
        .run(move |event, elwt| {
            if let Event::UserEvent(reply) = event {
                if help_open {
                    held_jev_replies.push(reply);
                    return;
                }
                jev_mode.busy = false;
                if !jev_mode.enabled || reply.round != jev_mode.round || reply.epoch != jev_mode.epoch {
                    start_jev_turn(&game, &mut jev_mode, &proxy);
                } else {
                    let provider = if jev_mode.is_local() { "Local AI" } else { "JeV" };
                    let api_ms = reply.elapsed.as_millis();
                    jev_mode.last_api_ms = Some(api_ms);
                    jev_mode.api_total_ms += api_ms;
                    let before = game.display();
                    let analysis = solver::analyze(&before, &game.guessed, game.solver_words());
                    match reply.result {
                        Ok(decision) if game.handle_guess(decision.letter) => {
                            trace_ai_game_result(&game, &before, decision.letter);
                            eprintln!("{} round={} call={} model={} model_pick={} letter={} api_ms={} model_choice_probability={:.3} {} tokens={}/{}",
                                provider, jev_mode.round + 1, jev_mode.calls_in_round, decision.model,
                                decision.model_letter, decision.letter, api_ms, decision.probability,
                                solver_comparison(&analysis, decision.letter),
                                decision.input_tokens, decision.output_tokens);
                            let adjusted = if decision.letter == decision.model_letter { "" } else { " CORRECTED" };
                            jev_mode.status = match (analysis.odds_for(decision.letter), analysis.best_letter()) {
                                (Some(pick), Some(_)) => format!("JEV {} LIST {} PCT API {}MS{}",
                                    decision.letter, (pick.probability() * 100.0).round() as u8,
                                    api_ms, adjusted),
                                _ => format!("JEV {} API {}MS NO WORD LIST MATCH", decision.letter, api_ms),
                            };
                            if game.is_over() {
                                jev_mode.enabled = false;
                                jev_mode.status = "JEV ROUND OVER".into();
                                eprintln!("{} round={} ended: {} api_total_ms={}",
                                    provider, jev_mode.round + 1, game.status().0, jev_mode.api_total_ms);
                            } else if jev_mode.calls_in_round >= jev_mode.max_calls_in_round {
                                jev_mode.enabled = false;
                                jev_mode.status = "JEV CALL LIMIT".into();
                            } else {
                                jev_mode.ready_at = Some(Instant::now() + JEV_TURN_DELAY);
                            }
                        }
                        Ok(_) => {
                            jev_mode.enabled = false;
                            jev_mode.status = "JEV INVALID GUESS".into();
                            eprintln!("{provider} invalid guess: api_ms={api_ms}");
                        }
                        Err(error) => {
                            jev_mode.enabled = false;
                            eprintln!("{provider} error: {error} api_ms={api_ms}");
                            jev_mode.status = format!("JEV ERROR: {error}");
                        }
                    }
                }
                window.request_redraw();
                return;
            }
            if let Event::AboutToWait = event {
                if help_open {
                    elwt.set_control_flow(ControlFlow::Wait);
                    return;
                }
                if let Some(match_state) = duel.as_mut() {
                    if match_state.odds_ready_at.is_some_and(|ready_at| Instant::now() >= ready_at) {
                        match_state.step_odds();
                        window.request_redraw();
                    }
                }
                if let Some(ready_at) = solver_mode.ready_at {
                    if Instant::now() >= ready_at {
                        solver_mode.ready_at = None;
                        solver_turn(&mut game, &mut solver_mode);
                        window.request_redraw();
                    }
                }
                if let Some(ready_at) = jev_mode.ready_at {
                    if Instant::now() >= ready_at {
                        jev_mode.ready_at = None;
                        start_jev_turn(&game, &mut jev_mode, &proxy);
                        window.request_redraw();
                    }
                }
                let next = [solver_mode.ready_at, jev_mode.ready_at,
                    duel.as_ref().and_then(|match_state| match_state.odds_ready_at)]
                    .into_iter().flatten().min();
                elwt.set_control_flow(next.map_or(ControlFlow::Wait, ControlFlow::WaitUntil));
                return;
            }
            let Event::WindowEvent { window_id, event } = event else { return };
            if window_id != window.id() {
                return;
            }
            match event {
                WindowEvent::CloseRequested => elwt.exit(),

                WindowEvent::Resized(_) => window.request_redraw(),

                WindowEvent::CursorMoved { position, .. } => cursor = Some(position),
                WindowEvent::CursorLeft { .. } => cursor = None,

                WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Left, .. } => {
                    if help_open { return; }
                    let Some(pos) = cursor else { return };
                    let size = window.inner_size();
                    let lay = Layout::fit(size.width as usize, size.height as usize);
                    let (ix, iy) = lay.to_img(pos.x as f32, pos.y as f32);
                    if std::env::var_os("HANGMAN_DEBUG").is_some() {
                        eprintln!("click phys=({:.0},{:.0}) inner={}x{} scale={} lay.s={:.3} img=({:.1},{:.1}) tile={:?} new_game={}",
                            pos.x, pos.y, size.width, size.height, window.scale_factor(), lay.s, ix, iy, tile_at(ix, iy), on_new_game(ix, iy));
                    }
                    let changed = if on_new_game(ix, iy) {
                        let leaving_duel = duel.is_some();
                        game = duel.take().map_or_else(|| game.next_round(), |match_state| match_state.resume_game.next_round());
                        duel_armed = false;
                        if leaving_duel { jev_mode.stop(); }
                        jev_mode.new_round();
                        solver_mode.new_round();
                        start_jev_turn(&game, &mut jev_mode, &proxy);
                        solver_turn(&mut game, &mut solver_mode);
                        true
                    } else if on_duel_button(ix, iy) {
                        toggle_duel(&mut game, &mut duel, &mut duel_armed,
                            &mut jev_mode, &mut solver_mode, &proxy);
                        true
                    } else if on_solver_button(ix, iy) {
                        if duel.is_some() { return; }
                        duel_armed = false;
                        if jev_mode.enabled {
                            jev_mode.toggle();
                        }
                        solver_mode.toggle();
                        solver_turn(&mut game, &mut solver_mode);
                        true
                    } else if on_jev_button(ix, iy) {
                        if duel.is_some() { return; }
                        duel_armed = false;
                        solver_mode.stop();
                        jev_mode.toggle();
                        start_jev_turn(&game, &mut jev_mode, &proxy);
                        true
                    } else if duel.is_some() || jev_mode.enabled || solver_mode.enabled {
                        false
                    } else {
                        tile_at(ix, iy).is_some_and(|c| game.handle_guess(c))
                    };
                    if changed {
                        window.request_redraw();
                    }
                }

                WindowEvent::KeyboardInput {
                    event: KeyEvent { logical_key, state: ElementState::Pressed, repeat: false, .. },
                    ..
                } => {
                    let changed = match logical_key {
                        Key::Named(NamedKey::F1) | Key::Named(NamedKey::Escape) if help_open => {
                            help_open = false;
                            for reply in held_jev_replies.drain(..) {
                                let _ = proxy.send_event(reply);
                            }
                            true
                        }
                        Key::Named(NamedKey::F1) => {
                            help_open = true;
                            true
                        }
                        _ if help_open => false,
                        Key::Named(NamedKey::Escape) => {
                            elwt.exit();
                            false
                        }
                        Key::Named(NamedKey::Enter) => {
                            let leaving_duel = duel.is_some();
                            game = duel.take().map_or_else(|| game.next_round(), |match_state| match_state.resume_game.next_round());
                            duel_armed = false;
                            if leaving_duel { jev_mode.stop(); }
                            jev_mode.new_round();
                            solver_mode.new_round();
                            start_jev_turn(&game, &mut jev_mode, &proxy);
                            solver_turn(&mut game, &mut solver_mode);
                            true
                        }
                        Key::Named(NamedKey::F2) => {
                            if duel.is_some() { return; }
                            duel_armed = false;
                            solver_mode.stop();
                            jev_mode.toggle();
                            start_jev_turn(&game, &mut jev_mode, &proxy);
                            true
                        }
                        Key::Named(NamedKey::F3) => {
                            if duel.is_some() { return; }
                            duel_armed = false;
                            if jev_mode.enabled {
                                jev_mode.toggle();
                            }
                            solver_mode.toggle();
                            solver_turn(&mut game, &mut solver_mode);
                            true
                        }
                        Key::Named(NamedKey::F4) => {
                            toggle_duel(&mut game, &mut duel, &mut duel_armed,
                                &mut jev_mode, &mut solver_mode, &proxy);
                            true
                        }
                        Key::Character(s) if duel.is_none() && !jev_mode.enabled && !solver_mode.enabled =>
                            s.chars().next().is_some_and(|c| game.handle_guess(c)),
                        _ => false,
                    };
                    if changed {
                        window.request_redraw();
                    }
                }

                WindowEvent::RedrawRequested => {
                    let size = window.inner_size();
                    let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else {
                        return;
                    };
                    surface.resize(w, h).expect("resize surface");
                    let mut buffer = surface.buffer_mut().expect("lock buffer");
                    renderer.render(RenderState { game: &game, jev_mode: &jev_mode,
                        solver_mode: &solver_mode, duel: duel.as_ref(), duel_armed, help_open },
                        &mut buffer, size.width as usize, size.height as usize);
                    buffer.present().expect("present buffer");
                }

                _ => {}
            }
        })
        .expect("event loop");
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_game_is_all_blanks_with_full_lives() {
        let g = HangmanGame::with_word("MANGO".into());
        assert_eq!(g.display(), "_____");
        assert_eq!(g.lives, MAX_LIVES);
        assert!(!g.is_over());
        assert!(
            (0..6).all(|p| !g.part_visible(p)),
            "nobody on the rope at the start"
        );
    }

    #[test]
    fn correct_guess_reveals_every_occurrence_and_keeps_lives() {
        let mut g = HangmanGame::with_word("BANANA".into());
        assert!(g.handle_guess('a'));
        assert_eq!(g.display(), "_A_A_A");
        assert_eq!(g.lives, MAX_LIVES);
        assert_eq!(g.status().0, "GOOD GUESS: A. LIVES LEFT: 6");
    }

    #[test]
    fn wrong_guess_costs_a_life_and_shows_the_head_first() {
        let mut g = HangmanGame::with_word("PEAR".into());
        assert!(g.handle_guess('Z'));
        assert_eq!(g.lives, MAX_LIVES - 1);
        assert!(g.part_visible(5), "head");
        assert!(!g.part_visible(4), "torso stays hidden");
        assert_eq!(g.status().0, "WRONG: Z. LIVES LEFT: 5");
    }

    #[test]
    fn parts_reveal_head_down_and_legs_last() {
        let mut g = HangmanGame::with_word("PEAR".into());
        let order = [5u8, 4, 3, 2, 1, 0];
        for (i, c) in "ZXQWVU".chars().enumerate() {
            g.handle_guess(c);
            for (j, &pid) in order.iter().enumerate() {
                assert_eq!(
                    g.part_visible(pid),
                    j <= i,
                    "after {} wrong, part {}",
                    i + 1,
                    pid
                );
            }
        }
    }

    #[test]
    fn repeated_and_non_letter_guesses_are_ignored() {
        let mut g = HangmanGame::with_word("PEAR".into());
        assert!(g.handle_guess('Z'));
        assert!(!g.handle_guess('z'));
        assert!(!g.handle_guess('3'));
        assert!(!g.handle_guess(' '));
        assert_eq!(g.lives, MAX_LIVES - 1);
    }

    #[test]
    fn six_wrong_guesses_lose_and_freeze_the_game() {
        let mut g = HangmanGame::with_word("PEAR".into());
        for c in "ZXQWVU".chars() {
            assert!(g.handle_guess(c));
        }
        assert!(g.is_lost());
        assert!(!g.handle_guess('P'), "no input accepted after game over");
        assert_eq!(g.display(), "____");
        assert_eq!(g.status().0, "HANGED. WORD: PEAR");
    }

    #[test]
    fn guessing_every_letter_wins() {
        let mut g = HangmanGame::with_word("APPLE".into());
        for c in "APLE".chars() {
            g.handle_guess(c);
        }
        assert!(g.is_won());
        assert_eq!(g.display(), "APPLE");
        assert_eq!(g.status().0, "SAVED! WORD: APPLE");
    }

    #[test]
    fn every_word_in_the_list_is_uppercase_ascii() {
        for w in WORDS {
            assert!(w.chars().all(|c| c.is_ascii_uppercase()), "{w}");
        }
    }

    #[test]
    fn every_letter_has_one_tile_and_tiles_do_not_overlap() {
        for c in 'A'..='Z' {
            let (x, y) = tile_centre(c).expect("tile for every letter");
            assert_eq!(tile_at(x, y), Some(c), "centre of {c} maps back to {c}");
        }
        assert_eq!(tile_at(10.0, 10.0), None);
        assert_eq!(
            tile_at(KEY_ROWS[0].x0 - TILE, KEY_ROWS[0].y),
            None,
            "left of A"
        );
    }

    #[test]
    fn new_game_plank_hit_box() {
        assert!(on_new_game(NEW_GAME.0, NEW_GAME.1));
        assert!(on_new_game(NEW_GAME.0 + 50.0, NEW_GAME.1 + 10.0));
        assert!(!on_new_game(NEW_GAME.0, NEW_GAME.1 - 40.0));
        assert!(on_jev_button(JEV_BUTTON.0, JEV_BUTTON.1));
        assert!(on_solver_button(SOLVER_BUTTON.0, SOLVER_BUTTON.1));
        assert!(on_duel_button(DUEL_BUTTON.0, DUEL_BUTTON.1));
        assert!(!on_solver_button(NEW_GAME.0, NEW_GAME.1));
        assert!(!on_jev_button(NEW_GAME.0, NEW_GAME.1));
        assert!(!on_duel_button(NEW_GAME.0, NEW_GAME.1));
    }

    #[test]
    fn jev_stays_off_without_a_key_and_can_be_paused() {
        let mut mode = JeVMode::new();
        mode.api_key = None;
        mode.toggle_with(|| Err("KEYCHAIN ACCESS FAILED"));
        assert!(!mode.enabled);
        assert_eq!(mode.status, "KEYCHAIN ACCESS FAILED");

        mode.api_key = Some("test-key".into());
        mode.toggle_with(|| unreachable!("cached key should be used"));
        assert!(mode.enabled);
        mode.toggle_with(|| unreachable!("pausing should not read a key"));
        assert!(!mode.enabled);
        assert_eq!(mode.status, "JEV PAUSED");
    }

    #[test]
    fn jev_call_limit_accepts_only_one_to_twenty_six() {
        assert_eq!(parse_jev_call_limit(Some("1")), 1);
        assert_eq!(parse_jev_call_limit(Some("26")), 26);
        assert_eq!(parse_jev_call_limit(Some("0")), 26);
        assert_eq!(parse_jev_call_limit(Some("27")), 26);
        assert_eq!(parse_jev_call_limit(Some("abc")), 26);
        assert_eq!(parse_jev_call_limit(None), 26);
    }

    #[test]
    fn local_model_configuration_never_falls_back_to_hosted_jev() {
        let local = local_model_from_values(Some("8009"), Some("kev-latest"))
            .unwrap()
            .unwrap();
        assert_eq!(local.endpoint(), "http://127.0.0.1:8009/v1/systemone");
        assert_eq!(local.model, "kev-latest");
        assert!(local_model_from_values(None, None).unwrap().is_none());
        assert!(local_model_from_values(None, Some("kev-latest")).is_err());
        assert!(local_model_from_values(Some("0"), None).is_err());
        assert!(local_model_from_values(Some("abc"), None).is_err());
        assert!(local_model_from_values(Some("8009"), Some("")).is_err());
    }

    #[test]
    fn solver_chooses_highest_hit_rate_and_stops_without_dictionary_coverage() {
        let mut game = HangmanGame::with_word("MANGO".into());
        let mut mode = SolverMode::new();
        mode.toggle();
        solver_turn(&mut game, &mut mode);
        assert_eq!(game.last, Some(('A', true)));
        assert_eq!(game.display(), "_A___");
        assert!(mode.enabled);

        let mut custom = HangmanGame::with_word("XYZZY".into());
        custom.handle_guess('X');
        solver_turn(&mut custom, &mut mode);
        assert!(!mode.enabled);
        assert_eq!(mode.status, "ODDS NO WORD LIST MATCH");
    }

    #[test]
    fn ten_distinct_wins_per_level_complete_the_challenge() {
        let mut game = HangmanGame::new_normal_at_level(0, BTreeSet::new(), None);
        let mut seen = BTreeSet::new();
        for level in 0..word_bank::LEVELS {
            for wins in 1..=word_bank::WINS_TO_ADVANCE {
                assert_eq!(game.level, level);
                assert!(
                    seen.insert(game.word.clone()),
                    "word repeated before completion"
                );
                for letter in game.word.clone().chars() {
                    game.handle_guess(letter);
                }
                assert!(game.is_won());
                assert_eq!(game.completed_in_level(), wins);
                if level == word_bank::LEVELS - 1 && wins == word_bank::WINS_TO_ADVANCE {
                    assert_eq!(game.status().0, "ALL 10 LEVELS COMPLETE!");
                }
                game = game.next_round();
                if wins < word_bank::WINS_TO_ADVANCE {
                    assert_eq!(game.level, level);
                    assert_eq!(game.solved_words.len(), wins);
                    assert_eq!(game.solver_words().len(), word_bank::WORDS_PER_LEVEL - wins);
                }
            }
            assert_eq!(game.level, (level + 1) % word_bank::LEVELS);
            assert_eq!(game.completed_in_level(), 0);
            assert_eq!(game.solver_words().len(), word_bank::WORDS_PER_LEVEL);
        }
        assert_eq!(seen.len(), word_bank::LEVELS * word_bank::WINS_TO_ADVANCE);
    }

    #[test]
    fn a_loss_keeps_the_level_and_avoids_an_immediate_repeat() {
        let mut game = HangmanGame::new_normal_at_level(2, BTreeSet::new(), None);
        let lost_word = game.word.clone();
        for letter in 'A'..='Z' {
            if !game.word.contains(letter) {
                game.handle_guess(letter);
            }
        }
        assert!(game.is_lost());
        let next = game.next_round();
        assert_eq!(next.level, 2);
        assert_eq!(next.completed_in_level(), 0);
        assert_ne!(next.word, lost_word);
        assert_eq!(next.solver_words().len(), word_bank::WORDS_PER_LEVEL - 1);
    }

    #[test]
    fn fixed_practice_word_returns_to_random_play_on_new_game() {
        let game = HangmanGame::with_word("MANGO".into());
        assert!(game.fixed);
        let next = game.next_round();
        assert!(!next.fixed);
        assert_eq!(next.level, 0);
        assert_eq!(next.completed_in_level(), 0);
        assert!(word_bank::words_for_level(0).contains(&next.word.as_str()));
        assert_ne!(next.word, "MANGO");
        assert_eq!(next.solver_words().len(), word_bank::WORDS_PER_LEVEL);
    }

    #[test]
    fn duel_uses_independent_guesses_on_the_same_word_and_pool() {
        let resume = HangmanGame::new_normal_at_level(0, BTreeSet::new(), None);
        let jev = resume.next_round();
        let mut duel = Duel::new(resume, &jev);
        assert_eq!(duel.odds.word, jev.word);
        assert_eq!(duel.odds.solver_words(), jev.solver_words());
        duel.step_odds();
        assert_eq!(duel.odds.guessed.len(), 1);
        assert!(jev.guessed.is_empty());
    }

    #[test]
    fn duel_declares_winner_only_after_both_games_finish() {
        let mut jev = HangmanGame::with_word("MANGO".into());
        let mut duel = Duel::new(jev.clone(), &jev);
        let mut mode = JeVMode::new();
        mode.enabled = true;
        for letter in "MANGO".chars() {
            duel.odds.handle_guess(letter);
        }
        assert_eq!(duel.status(&jev, &mode).0, "ODDS VS JEV - SAME WORD");
        for letter in "BCDEFH".chars() {
            jev.handle_guess(letter);
        }
        assert!(jev.is_lost());
        assert_eq!(duel.status(&jev, &mode).0, "ODDS WINS - WORD: MANGO");
    }

    #[test]
    fn layout_round_trips_and_letterboxes() {
        let lay = Layout::fit(2000, 1000); // wider than the image
        assert!(lay.ox > 0.0 && lay.oy.abs() < 1.0);
        let (px, py) = lay.to_phys(590.0, 418.0);
        let (ix, iy) = lay.to_img(px, py);
        assert!((ix - 590.0).abs() < 1e-3 && (iy - 418.0).abs() < 1e-3);
    }

    #[test]
    fn assets_decode_and_match_the_reference_geometry() {
        let r = Renderer::new();
        assert_eq!((r.bg.w, r.bg.h), (IMG_W as usize, IMG_H as usize));
        assert_eq!((r.man.w, r.man.h), (r.regions.w, r.regions.h));
        let ids: BTreeSet<u32> = r
            .regions
            .px
            .iter()
            .map(|p| (p[0] as u32 + 20) / 40)
            .collect();
        assert_eq!(ids, (0..=5).collect(), "six part ids in the region map");
    }

    #[test]
    fn render_does_not_panic_at_odd_sizes_and_reveals_pixels() {
        let mut r = Renderer::new();
        let mut g = HangmanGame::with_word("WATERMELON".into());
        let mode = JeVMode::new();
        let solver_mode = SolverMode::new();
        for (w, h) in [(1, 1), (400, 300), (1178, 896), (1920, 500)] {
            let mut buf = vec![0u32; w * h];
            r.render(
                RenderState {
                    game: &g,
                    jev_mode: &mode,
                    solver_mode: &solver_mode,
                    duel: None,
                    duel_armed: false,
                    help_open: false,
                },
                &mut buf,
                w,
                h,
            );
        }
        // a wrong guess must change pixels where the head is
        let (w, h) = (1178, 896);
        let mut before = vec![0u32; w * h];
        r.render(
            RenderState {
                game: &g,
                jev_mode: &mode,
                solver_mode: &solver_mode,
                duel: None,
                duel_armed: false,
                help_open: false,
            },
            &mut before,
            w,
            h,
        );
        g.handle_guess('Z');
        let mut after = vec![0u32; w * h];
        r.render(
            RenderState {
                game: &g,
                jev_mode: &mode,
                solver_mode: &solver_mode,
                duel: None,
                duel_armed: false,
                help_open: false,
            },
            &mut after,
            w,
            h,
        );
        let head = 735 + 200 * w; // roughly the face
        assert_ne!(before[head], after[head]);

        let duel = Duel::new(g.clone(), &g);
        let mut match_pixels = vec![0u32; w * h];
        r.render(
            RenderState {
                game: &g,
                jev_mode: &mode,
                solver_mode: &solver_mode,
                duel: Some(&duel),
                duel_armed: false,
                help_open: false,
            },
            &mut match_pixels,
            w,
            h,
        );
        assert_ne!(after, match_pixels);

        let mut help_pixels = vec![0u32; w * h];
        r.render(
            RenderState {
                game: &g,
                jev_mode: &mode,
                solver_mode: &solver_mode,
                duel: Some(&duel),
                duel_armed: false,
                help_open: true,
            },
            &mut help_pixels,
            w,
            h,
        );
        assert_ne!(help_pixels, match_pixels);
    }
}
