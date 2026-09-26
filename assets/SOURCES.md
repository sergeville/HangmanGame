# Assets

Derived from `~/Dev/HTML/hangman.jpeg` (Serge's reference render) by
`~/Dev/HTML/hangman-assets/build_assets.py` on 2026-09-06:

- `bg.jpg` — the reference with the man, sign text, baked word row and hint patched out.
- `man.png` — the man, cut out by macOS Vision, gold letters scrubbed off the shirt.
  Sits at image box (656, 165)-(851, 730) of `bg.jpg`.
- `regions.png` — part map for the reveal, R channel = part id × 40:
  0 right leg, 1 left leg, 2 right arm, 3 left arm, 4 torso, 5 head.
