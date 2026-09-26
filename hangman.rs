use winit::event::*;
use winit::event_loop::{EventLoop, ControlFlow};
use winit::window::{Window, WindowBuilder};
use std::collections::HashSet;
use std::time::Duration;
use rand::Rng;

// Define the hangman graphic as ASCII art
const HANGMAN_GRAPHIC: [&str; 7] = [
    "  +---+",
    "  |   |",
    "  O   |",
    " /|\\  |",
    " / \\  |",
    "      |",
    "      |",
];

// Define the word list
const WORDS: &[&str] = &[
    "APPLE", "BANANA", "ORANGE", "GRAPE", "STRAWBERRY",
    "BLUEBERRY", "WATERMELON", "PINEAPPLE", "MANGO", "PEAR",
];

struct HangmanGame {
    word: String,
    guessed_letters: HashSet<char>,
    lives: u8,
    display: String,
    current_gallows: usize,
    is_running: bool,
}

impl HangmanGame {
    fn new() -> Self {
        let word = WORDS.choose(&mut rand::thread_rng()).unwrap().to_string();
        HangmanGame {
            word,
            guessed_letters: HashSet::new(),
            lives: 6,
            display: word.chars().map(|c| if c.is_alphabetic() { '_' } else { c }).collect::<String>(),
            current_gallows: 0,
            is_running: true,
        }
    }

    fn handle_guess(&mut self, letter: char) {
        if self.guessed_letters.contains(&letter) {
            return; // Already guessed
        }

        self.guessed_letters.insert(letter);

        if !self.word.contains(letter) {
            self.lives -= 1;
            self.current_gallows = self.lives as usize;
        }

        self.display = self.word.chars().map(|c| {
            if self.guessed_letters.contains(&c) {
                c
            } else {
                '_'
            }
        }).collect::<String>();

        if self.display == self.word {
            self.is_running = false;
        }
    }

    fn is_won(&self) -> bool {
        self.display == self.word
    }

    fn is_lost(&self) -> bool {
        self.lives == 0
    }

    fn update_display(&self) -> String {
        let mut display = String::new();
        for c in self.word.chars() {
            if self.guessed_letters.contains(&c) {
                display.push(c);
            } else {
                display.push('_');
            }
        }
        display
    }

    fn draw_gallows(&self) -> Vec<String> {
        let mut result = vec![];
        for i in 0..self.current_gallows {
            result.extend(HANGMAN_GRAPHIC[i].split_whitespace());
        }
        result
    }
}

fn main() {
    let mut event_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .with_title("Hangman Game")
        .with_inner_size(winit::dpi::LogicalSize::new(640.0, 480.0))
        .build(&event_loop)
        .unwrap();

    let mut game = HangmanGame::new();
    let mut is_running = true;

    event_loop.set_control_flow(ControlFlow::Wait);

    event_loop.run(move |event, _event_loop, _control_flow| {
        if !is_running {
            return;
        }

        match event {
            Event::WindowEvent { event, .. } => match event {
                WindowEvent::CloseRequested => {
                    is_running = false;
                    *control_flow = ControlFlow::Exit;
                }
                _ => {}
            },
            Event::KeyboardInput {
                input: KeyboardInput { state, virtual_keycode, .. },
                ..
            } => {
                if state == ElementState::Pressed {
                    if let Some(key) = virtual_keycode {
                        if let Some(c) = key.to_uppercase().chars().next() {
                            if c.is_alphabetic() {
                                game.handle_guess(c);
                            }
                        }
                    }
                }
            },
            Event::RedrawRequested(_) => {
                let mut canvas = window
                    .inner_size()
                    .width
                    .max(320)
                    .max(480)
                    .max(640)
                    .min(1024)
                    .min(1280)
                    .min(1920);

                let mut w = 640.0;
                let mut h = 480.0;

                if canvas > 640.0 {
                    w = canvas;
                    h = (w * 0.75) as f32;
                }

                // Draw hangman graphic
                let hangman = game.draw_gallows();
                let display = game.update_display();

                let mut buffer = vec![0u8; (w as usize) * (h as usize) * 4];
                let mut canvas = canvas;

                // Print hangman and word display
                let mut out = String::new();
                for line in hangman {
                    out.push_str(&line);
                    out.push_str("\n");
                }

                // Add word display
                out.push_str(&format!("Word: {}", display));
                out.push_str("\n");

                // Print to terminal
                print!("{}", out);

                // Update window
                window.request_redraw();
            }
            Event::MainEventsCleared => {
                window.request_redraw();
            }
            _ => {}
        }
    });

    // Game loop
    while is_running {
        // Check if game is over
        if game.is_won() || game.is_lost() {
            // Show game over message
            println!("Game over! Word: {}", game.word);
            is_running = false;
        }
    }
}
