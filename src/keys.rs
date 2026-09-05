//! `sans keys`: print every terminal event as crossterm decodes it.
//!
//! This exists to answer setup questions the app cannot answer on its own: do Neo's layer 3
//! symbols arrive as plain characters, are umlauts precomposed, does the terminal treat
//! Option as Alt. It stays around as a diagnostic.

use std::env;
use std::io::{self, Write};

use anyhow::Result;
use crossterm::event::{
    self, DisableBracketedPaste, DisableFocusChange, EnableBracketedPaste, EnableFocusChange,
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, KeyboardEnhancementFlags,
    PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::{execute, terminal};

/// One line describing an event, with enough detail to tell apart the cases that matter.
pub fn describe(event: &Event) -> String {
    match event {
        Event::Key(key) => describe_key(key),
        Event::Paste(text) => format!("Paste  {} chars: {text:?}", text.chars().count()),
        Event::FocusGained => "FocusGained".to_string(),
        Event::FocusLost => "FocusLost".to_string(),
        Event::Resize(width, height) => format!("Resize  {width}x{height}"),
        Event::Mouse(mouse) => format!("Mouse  {mouse:?}"),
    }
}

fn describe_key(key: &KeyEvent) -> String {
    let kind = match key.kind {
        KeyEventKind::Press => "Press",
        KeyEventKind::Repeat => "Repeat",
        KeyEventKind::Release => "Release",
    };
    let codepoint = match key.code {
        KeyCode::Char(c) => format!("U+{:04X}", c as u32),
        _ => String::new(),
    };
    let mut line = format!(
        "Key  {kind:<7} {:<16} {codepoint:<7} mods: {}",
        format!("{:?}", key.code),
        modifier_names(key.modifiers)
    );
    // A character with Alt set is the ESC-prefixed form a terminal sends when it treats the
    // Option key as Alt instead of letting macOS compose the layer 3 symbol.
    if key.modifiers.contains(KeyModifiers::ALT) && matches!(key.code, KeyCode::Char(_)) {
        line.push_str("   <- ESC-prefixed: the terminal treats Option as Alt");
    }
    line
}

fn modifier_names(modifiers: KeyModifiers) -> String {
    if modifiers.is_empty() {
        return "NONE".to_string();
    }
    modifiers
        .iter_names()
        .map(|(name, _)| name)
        .collect::<Vec<_>>()
        .join("+")
}

pub fn run(enhanced: bool) -> Result<()> {
    let supported = terminal::supports_keyboard_enhancement().unwrap_or(false);
    println!(
        "TERM={} TERM_PROGRAM={}",
        env::var("TERM").unwrap_or_default(),
        env::var("TERM_PROGRAM").unwrap_or_default()
    );
    println!("kitty keyboard protocol: supported={supported} requested={enhanced}");
    println!("Type away. Ctrl-C quits.");

    let mut out = io::stdout();
    let _guard = RawGuard::enable(&mut out, enhanced && supported)?;
    loop {
        let event = event::read()?;
        if is_ctrl_c(&event) {
            break;
        }
        write!(out, "{}\r\n", describe(&event))?;
        out.flush()?;
    }
    Ok(())
}

fn is_ctrl_c(event: &Event) -> bool {
    matches!(
        event,
        Event::Key(KeyEvent { code: KeyCode::Char('c'), modifiers, .. })
            if modifiers.contains(KeyModifiers::CONTROL)
    )
}

/// Restores the terminal on drop, so a panic mid-loop does not leave raw mode behind.
struct RawGuard {
    enhanced: bool,
}

impl RawGuard {
    fn enable(out: &mut impl Write, enhanced: bool) -> Result<Self> {
        terminal::enable_raw_mode()?;
        execute!(out, EnableBracketedPaste, EnableFocusChange)?;
        if enhanced {
            execute!(
                out,
                PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
            )?;
        }
        Ok(Self { enhanced })
    }
}

impl Drop for RawGuard {
    fn drop(&mut self) {
        let mut out = io::stdout();
        if self.enhanced {
            let _ = execute!(out, PopKeyboardEnhancementFlags);
        }
        let _ = execute!(out, DisableFocusChange, DisableBracketedPaste);
        let _ = terminal::disable_raw_mode();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(code: KeyCode, modifiers: KeyModifiers) -> Event {
        Event::Key(KeyEvent::new(code, modifiers))
    }

    #[test]
    fn plain_character_shows_char_and_codepoint() {
        let line = describe(&press(KeyCode::Char('{'), KeyModifiers::NONE));
        assert!(line.contains("Char('{')"), "{line}");
        assert!(line.contains("U+007B"), "{line}");
        assert!(line.contains("Press"), "{line}");
    }

    #[test]
    fn umlaut_shows_precomposed_codepoint() {
        let line = describe(&press(KeyCode::Char('ä'), KeyModifiers::NONE));
        assert!(line.contains("U+00E4"), "{line}");
    }

    #[test]
    fn alt_modified_character_warns_about_option_as_alt() {
        let line = describe(&press(KeyCode::Char('a'), KeyModifiers::ALT));
        assert!(line.contains("ALT"), "{line}");
        assert!(line.contains("Option as Alt"), "{line}");
    }

    #[test]
    fn plain_character_does_not_warn() {
        let line = describe(&press(KeyCode::Char('a'), KeyModifiers::NONE));
        assert!(!line.contains("Option as Alt"), "{line}");
    }

    #[test]
    fn enter_and_tab_are_named() {
        assert!(describe(&press(KeyCode::Enter, KeyModifiers::NONE)).contains("Enter"));
        assert!(describe(&press(KeyCode::Tab, KeyModifiers::NONE)).contains("Tab"));
    }

    #[test]
    fn paste_is_reported_with_its_length() {
        let line = describe(&Event::Paste("hello".into()));
        assert!(line.contains("Paste"), "{line}");
        assert!(line.contains("5"), "{line}");
    }
}
