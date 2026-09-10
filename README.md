# sans

A terminal typing tutor for switching from QWERTY to [Bone](https://www.neo-layout.org/Layouts/bone/) or [Neo 2](https://www.neo-layout.org/Layouts/neo). Lesson-based, accuracy first, with stats over time and a mode for typing your own (source) files.

It's named after the character Sans from Undertale, because skeletons have a lot of visible bones, ha, I suck at naming things.

Full disclosure: This tool was mostly created with the help of agentic coding tools. It is very much built around my needs and likely wouldn't exist otherwise. I hope that's not a dealbreaker for you, but I'm sorry if it is. This readme is very much written by a human however, for whatever that's worth.

## Usage

```sh
# lesson list, see keybinds in the status bar at the bottom
sans

# type over a file, chunked
sans type [file]

# practice everything learned so far, can be restricted to specific keys via `--keys "uiae"`
sans practice

# does what it says on the box
sans stats

# echo raw key events, helpful for seeing what reaches sans
sans keys
```

A lesson has four stages, ~2 min each. An intro drill with finger position hints, syllables from the most frequent bigrams, real words, and a test at the end. Symbol lessons practice tokens and expressions instead of syllables and words. A test needs to be passed to unlock the next lesson.

A wrong key is shown in red and nothing but Backspace is accepted until it is fixed. Every wrong key counts as an error. The test at the end of a lesson has to be passed with an error rate of <=3%.

## Files

`sans type path/to/file` splits a file into chunks of about twelve lines (fewer for longer lines) and types them one after another. Progress is kept per file and content, so the next run resumes at the next chunk.

## Development

The repo ships a nix flake and a `.envrc`. `cargo test` and `cargo clippy --all-targets` should stay clean.
