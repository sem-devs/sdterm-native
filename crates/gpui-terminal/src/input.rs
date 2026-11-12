//! Keyboard input handling for the terminal emulator.
//!
//! This module provides [`keystroke_to_bytes`], which converts GPUI keyboard
//! events into terminal escape sequences that can be written to the PTY.
//!
//! # Key Mappings
//!
//! ## Special Keys
//!
//! | Key | Sequence | Notes |
//! |-----|----------|-------|
//! | Enter | `\r` (0x0D) | Carriage return |
//! | Escape | `\x1b` (0x1B) | ESC |
//! | Backspace | `\x7f` (0x7F) | DEL |
//! | Tab | `\t` (0x09) | Horizontal tab |
//! | Shift+Tab | `\x1b[Z` | Backtab |
//! | Space | ` ` (0x20) | Space |
//! | Ctrl+Space | `\x00` | NUL |
//!
//! ## Arrow Keys
//!
//! Arrow key sequences depend on application cursor mode:
//!
//! | Key | Normal Mode | App Cursor Mode |
//! |-----|-------------|-----------------|
//! | Up | `\x1b[A` | `\x1bOA` |
//! | Down | `\x1b[B` | `\x1bOB` |
//! | Right | `\x1b[C` | `\x1bOC` |
//! | Left | `\x1b[D` | `\x1bOD` |
//!
//! ## Navigation Keys
//!
//! | Key | Sequence |
//! |-----|----------|
//! | Home | `\x1b[H` |
//! | End | `\x1b[F` |
//! | PageUp | `\x1b[5~` |
//! | PageDown | `\x1b[6~` |
//! | Insert | `\x1b[2~` |
//! | Delete | `\x1b[3~` |
//!
//! ## Function Keys
//!
//! | Key | Sequence |
//! |-----|----------|
//! | F1-F4 | `\x1bOP` - `\x1bOS` |
//! | F5-F12 | `\x1b[15~` - `\x1b[24~` |
//!
//! ## Control Combinations
//!
//! Ctrl+A through Ctrl+Z map to ASCII control characters 0x01-0x1A:
//!
//! | Combination | Byte |
//! |-------------|------|
//! | Ctrl+A | 0x01 |
//! | Ctrl+C | 0x03 (interrupt) |
//! | Ctrl+D | 0x04 (EOF) |
//! | Ctrl+Z | 0x1A (suspend) |
//!
//! ## Alt Combinations
//!
//! Alt+key sends ESC followed by the key: `\x1b` + key
//!
//! # Terminal Mode Effects
//!
//! The [`TermMode`] flags affect key sequences:
//!
//! - **APP_CURSOR**: Changes arrow key sequences from CSI to SS3 format
//!
//! # Example
//!
//! ```
//! use gpui::Keystroke;
//! use alacritty_terminal::term::TermMode;
//! use gpui_terminal::input::keystroke_to_bytes;
//!
//! // Enter key
//! let keystroke = Keystroke::parse("enter").unwrap();
//! assert_eq!(keystroke_to_bytes(&keystroke, TermMode::empty()), Some(b"\r".to_vec()));
//!
//! // Ctrl+C (interrupt)
//! let keystroke = Keystroke::parse("ctrl-c").unwrap();
//! assert_eq!(keystroke_to_bytes(&keystroke, TermMode::empty()), Some(vec![0x03]));
//! ```

use alacritty_terminal::term::TermMode;
use gpui::Keystroke;

/// Convert a GPUI keystroke to terminal escape sequence bytes.
///
/// This function translates GPUI keyboard events into the appropriate byte sequences
/// expected by terminal applications. It handles special keys, control characters,
/// and application cursor mode.
///
/// # Arguments
///
/// * `keystroke` - The GPUI keystroke to convert
/// * `mode` - The current terminal mode (affects arrow key sequences)
///
/// # Returns
///
/// An optional vector of bytes representing the terminal escape sequence.
/// Returns `None` if the keystroke should not produce any output.
///
/// # Examples
///
/// ```
/// use gpui::Keystroke;
/// use alacritty_terminal::term::TermMode;
/// use gpui_terminal::input::keystroke_to_bytes;
///
/// let keystroke = Keystroke::parse("enter").unwrap();
/// let bytes = keystroke_to_bytes(&keystroke, TermMode::empty());
/// assert_eq!(bytes, Some(b"\r".to_vec()));
/// ```
pub fn keystroke_to_bytes(keystroke: &Keystroke, mode: TermMode) -> Option<Vec<u8>> {
    // Arrows and navigation keys with Shift, Alt or Ctrl: the xterm form programs understand, such
    // as ESC [ 1 ; 5 C for Ctrl+Right (word jumps in readline, selections in editors).
    if let Some(bytes) = modified_navigation_bytes(keystroke) {
        return Some(bytes);
    }

    // Handle special keys first
    match keystroke.key.as_str() {
        // Basic control characters
        "space" => {
            if keystroke.modifiers.control {
                return Some(b"\x00".to_vec()); // Ctrl+Space = NUL
            }
            return Some(b" ".to_vec());
        }
        "enter" => return Some(b"\r".to_vec()),
        "escape" => return Some(b"\x1b".to_vec()),
        "backspace" => return Some(b"\x7f".to_vec()),
        "tab" => {
            // Shift+Tab sends a different sequence
            if keystroke.modifiers.shift {
                return Some(b"\x1b[Z".to_vec());
            }
            return Some(b"\t".to_vec());
        }

        // Arrow keys - check APP_CURSOR mode
        "up" => {
            if mode.contains(TermMode::APP_CURSOR) {
                return Some(b"\x1bOA".to_vec());
            }
            return Some(b"\x1b[A".to_vec());
        }
        "down" => {
            if mode.contains(TermMode::APP_CURSOR) {
                return Some(b"\x1bOB".to_vec());
            }
            return Some(b"\x1b[B".to_vec());
        }
        "right" => {
            if mode.contains(TermMode::APP_CURSOR) {
                return Some(b"\x1bOC".to_vec());
            }
            return Some(b"\x1b[C".to_vec());
        }
        "left" => {
            if mode.contains(TermMode::APP_CURSOR) {
                return Some(b"\x1bOD".to_vec());
            }
            return Some(b"\x1b[D".to_vec());
        }

        // Navigation keys
        "home" => return Some(b"\x1b[H".to_vec()),
        "end" => return Some(b"\x1b[F".to_vec()),
        "pageup" => return Some(b"\x1b[5~".to_vec()),
        "pagedown" => return Some(b"\x1b[6~".to_vec()),
        "insert" => return Some(b"\x1b[2~".to_vec()),
        "delete" => return Some(b"\x1b[3~".to_vec()),

        // Function keys
        "f1" => return Some(b"\x1bOP".to_vec()),
        "f2" => return Some(b"\x1bOQ".to_vec()),
        "f3" => return Some(b"\x1bOR".to_vec()),
        "f4" => return Some(b"\x1bOS".to_vec()),
        "f5" => return Some(b"\x1b[15~".to_vec()),
        "f6" => return Some(b"\x1b[17~".to_vec()),
        "f7" => return Some(b"\x1b[18~".to_vec()),
        "f8" => return Some(b"\x1b[19~".to_vec()),
        "f9" => return Some(b"\x1b[20~".to_vec()),
        "f10" => return Some(b"\x1b[21~".to_vec()),
        "f11" => return Some(b"\x1b[23~".to_vec()),
        "f12" => return Some(b"\x1b[24~".to_vec()),

        _ => {}
    }

    // Handle Ctrl+key combinations
    if keystroke.modifiers.control {
        let key = keystroke.key.as_str();

        // Ctrl+A through Ctrl+Z map to 0x01 through 0x1a
        if key.len() == 1 {
            let ch = key.chars().next().unwrap();
            if ch.is_ascii_alphabetic() {
                // Convert to uppercase and then to control character
                let upper = ch.to_ascii_uppercase();
                let ctrl_char = (upper as u8) - b'@';
                return Some(vec![ctrl_char]);
            }

            // Special Ctrl combinations
            match ch {
                '[' => return Some(b"\x1b".to_vec()),  // Ctrl+[
                '\\' => return Some(b"\x1c".to_vec()), // Ctrl+\
                ']' => return Some(b"\x1d".to_vec()),  // Ctrl+]
                '^' => return Some(b"\x1e".to_vec()),  // Ctrl+^
                '_' => return Some(b"\x1f".to_vec()),  // Ctrl+_
                '?' => return Some(b"\x7f".to_vec()),  // Ctrl+?
                _ => {}
            }
        }
    }

    // Handle Alt+key combinations
    if keystroke.modifiers.alt {
        let key = keystroke.key.as_str();
        if key.len() == 1 {
            // Alt+key sends ESC followed by the key
            let ch = key.chars().next().unwrap();
            if ch.is_ascii() {
                let mut bytes = vec![b'\x1b'];
                bytes.push(ch as u8);
                return Some(bytes);
            }
        }
    }

    // Handle regular printable characters
    // Use key_char if available (contains the actual typed character with modifiers like Shift)
    if let Some(key_char) = &keystroke.key_char
        && !keystroke.modifiers.control
        && !keystroke.modifiers.alt
    {
        return Some(key_char.as_bytes().to_vec());
    }

    // Fallback to key for single characters
    let key = keystroke.key.as_str();
    if key.len() == 1 {
        let ch = key.chars().next().unwrap();
        if ch.is_ascii() && !keystroke.modifiers.control {
            // Handle shift modifier for uppercase
            let ch = if keystroke.modifiers.shift {
                ch.to_ascii_uppercase()
            } else {
                ch
            };
            return Some(vec![ch as u8]);
        }
        // For non-ASCII characters, encode as UTF-8
        if !keystroke.modifiers.control && !keystroke.modifiers.alt {
            return Some(key.as_bytes().to_vec());
        }
    }

    // If we get here, the keystroke doesn't produce any output
    None
}

/// The xterm modifier number: 1, plus 1 for Shift, 2 for Alt, 4 for Ctrl.
fn modifier_number(m: &gpui::Modifiers) -> u8 {
    1 + m.shift as u8 + 2 * m.alt as u8 + 4 * m.control as u8
}

/// Arrows, Home, End, Insert, Delete, Page Up and Page Down pressed with Shift, Alt or Ctrl.
/// Cmd (the platform key) is not part of it: it never reaches the shell as a modifier.
fn modified_navigation_bytes(keystroke: &Keystroke) -> Option<Vec<u8>> {
    let m = &keystroke.modifiers;
    if !(m.shift || m.alt || m.control) {
        return None;
    }
    let n = modifier_number(m);
    let letter = match keystroke.key.as_str() {
        "up" => 'A',
        "down" => 'B',
        "right" => 'C',
        "left" => 'D',
        "home" => 'H',
        "end" => 'F',
        _ => {
            let code = match keystroke.key.as_str() {
                "insert" => 2,
                "delete" => 3,
                "pageup" => 5,
                "pagedown" => 6,
                _ => return None,
            };
            return Some(format!("\x1b[{code};{n}~").into_bytes());
        }
    };
    Some(format!("\x1b[1;{n}{letter}").into_bytes())
}

/// The text-editing shortcuts macOS users expect from Terminal.app, as the bytes a shell reads
/// (readline and zsh bind them): Cmd+Left and Cmd+Right go to the start and end of the line,
/// Cmd+Backspace deletes to the start of the line, Option+Left and Option+Right move by a word,
/// Option+Backspace deletes the previous word and Option+Delete the next one.
pub fn mac_editing_bytes(keystroke: &Keystroke) -> Option<Vec<u8>> {
    let m = &keystroke.modifiers;
    if m.shift || m.control || m.platform == m.alt {
        return None; // exactly one of Cmd and Option
    }
    let bytes: &[u8] = match (m.platform, keystroke.key.as_str()) {
        (true, "left") => b"\x01",
        (true, "right") => b"\x05",
        (true, "backspace") => b"\x15",
        (false, "left") => b"\x1bb",
        (false, "right") => b"\x1bf",
        (false, "backspace") => b"\x17",
        (false, "delete") => b"\x1bd",
        _ => return None,
    };
    Some(bytes.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_enter_key() {
        let keystroke = Keystroke::parse("enter").unwrap();
        let bytes = keystroke_to_bytes(&keystroke, TermMode::empty());
        assert_eq!(bytes, Some(b"\r".to_vec()));
    }

    #[test]
    fn test_escape_key() {
        let keystroke = Keystroke::parse("escape").unwrap();
        let bytes = keystroke_to_bytes(&keystroke, TermMode::empty());
        assert_eq!(bytes, Some(b"\x1b".to_vec()));
    }

    #[test]
    fn test_backspace_key() {
        let keystroke = Keystroke::parse("backspace").unwrap();
        let bytes = keystroke_to_bytes(&keystroke, TermMode::empty());
        assert_eq!(bytes, Some(b"\x7f".to_vec()));
    }

    #[test]
    fn test_tab_key() {
        let keystroke = Keystroke::parse("tab").unwrap();
        let bytes = keystroke_to_bytes(&keystroke, TermMode::empty());
        assert_eq!(bytes, Some(b"\t".to_vec()));
    }

    #[test]
    fn test_shift_tab() {
        let keystroke = Keystroke::parse("shift-tab").unwrap();
        let bytes = keystroke_to_bytes(&keystroke, TermMode::empty());
        assert_eq!(bytes, Some(b"\x1b[Z".to_vec()));
    }

    #[test]
    fn test_arrow_keys_normal_mode() {
        let mode = TermMode::empty();

        let up = Keystroke::parse("up").unwrap();
        assert_eq!(keystroke_to_bytes(&up, mode), Some(b"\x1b[A".to_vec()));

        let down = Keystroke::parse("down").unwrap();
        assert_eq!(keystroke_to_bytes(&down, mode), Some(b"\x1b[B".to_vec()));

        let right = Keystroke::parse("right").unwrap();
        assert_eq!(keystroke_to_bytes(&right, mode), Some(b"\x1b[C".to_vec()));

        let left = Keystroke::parse("left").unwrap();
        assert_eq!(keystroke_to_bytes(&left, mode), Some(b"\x1b[D".to_vec()));
    }

    #[test]
    fn test_arrow_keys_app_cursor_mode() {
        let mode = TermMode::APP_CURSOR;

        let up = Keystroke::parse("up").unwrap();
        assert_eq!(keystroke_to_bytes(&up, mode), Some(b"\x1bOA".to_vec()));

        let down = Keystroke::parse("down").unwrap();
        assert_eq!(keystroke_to_bytes(&down, mode), Some(b"\x1bOB".to_vec()));

        let right = Keystroke::parse("right").unwrap();
        assert_eq!(keystroke_to_bytes(&right, mode), Some(b"\x1bOC".to_vec()));

        let left = Keystroke::parse("left").unwrap();
        assert_eq!(keystroke_to_bytes(&left, mode), Some(b"\x1bOD".to_vec()));
    }

    #[test]
    fn test_navigation_keys() {
        let mode = TermMode::empty();

        let home = Keystroke::parse("home").unwrap();
        assert_eq!(keystroke_to_bytes(&home, mode), Some(b"\x1b[H".to_vec()));

        let end = Keystroke::parse("end").unwrap();
        assert_eq!(keystroke_to_bytes(&end, mode), Some(b"\x1b[F".to_vec()));

        let pageup = Keystroke::parse("pageup").unwrap();
        assert_eq!(keystroke_to_bytes(&pageup, mode), Some(b"\x1b[5~".to_vec()));

        let pagedown = Keystroke::parse("pagedown").unwrap();
        assert_eq!(
            keystroke_to_bytes(&pagedown, mode),
            Some(b"\x1b[6~".to_vec())
        );

        let insert = Keystroke::parse("insert").unwrap();
        assert_eq!(keystroke_to_bytes(&insert, mode), Some(b"\x1b[2~".to_vec()));

        let delete = Keystroke::parse("delete").unwrap();
        assert_eq!(keystroke_to_bytes(&delete, mode), Some(b"\x1b[3~".to_vec()));
    }

    #[test]
    fn test_function_keys() {
        let mode = TermMode::empty();

        let f1 = Keystroke::parse("f1").unwrap();
        assert_eq!(keystroke_to_bytes(&f1, mode), Some(b"\x1bOP".to_vec()));

        let f2 = Keystroke::parse("f2").unwrap();
        assert_eq!(keystroke_to_bytes(&f2, mode), Some(b"\x1bOQ".to_vec()));

        let f5 = Keystroke::parse("f5").unwrap();
        assert_eq!(keystroke_to_bytes(&f5, mode), Some(b"\x1b[15~".to_vec()));

        let f12 = Keystroke::parse("f12").unwrap();
        assert_eq!(keystroke_to_bytes(&f12, mode), Some(b"\x1b[24~".to_vec()));
    }

    #[test]
    fn test_ctrl_combinations() {
        let mode = TermMode::empty();

        // Ctrl+A = 0x01
        let ctrl_a = Keystroke::parse("ctrl-a").unwrap();
        assert_eq!(keystroke_to_bytes(&ctrl_a, mode), Some(vec![0x01]));

        // Ctrl+C = 0x03
        let ctrl_c = Keystroke::parse("ctrl-c").unwrap();
        assert_eq!(keystroke_to_bytes(&ctrl_c, mode), Some(vec![0x03]));

        // Ctrl+Z = 0x1a
        let ctrl_z = Keystroke::parse("ctrl-z").unwrap();
        assert_eq!(keystroke_to_bytes(&ctrl_z, mode), Some(vec![0x1a]));

        // Ctrl+Space = 0x00
        let ctrl_space = Keystroke::parse("ctrl-space").unwrap();
        assert_eq!(keystroke_to_bytes(&ctrl_space, mode), Some(vec![0x00]));
    }

    #[test]
    fn test_alt_combinations() {
        let mode = TermMode::empty();

        // Alt+a sends ESC followed by 'a'
        let alt_a = Keystroke::parse("alt-a").unwrap();
        assert_eq!(keystroke_to_bytes(&alt_a, mode), Some(b"\x1ba".to_vec()));

        // Alt+x sends ESC followed by 'x'
        let alt_x = Keystroke::parse("alt-x").unwrap();
        assert_eq!(keystroke_to_bytes(&alt_x, mode), Some(b"\x1bx".to_vec()));
    }

    #[test]
    fn test_regular_characters() {
        let mode = TermMode::empty();

        let a = Keystroke::parse("a").unwrap();
        assert_eq!(keystroke_to_bytes(&a, mode), Some(b"a".to_vec()));

        let z = Keystroke::parse("z").unwrap();
        assert_eq!(keystroke_to_bytes(&z, mode), Some(b"z".to_vec()));

        let zero = Keystroke::parse("0").unwrap();
        assert_eq!(keystroke_to_bytes(&zero, mode), Some(b"0".to_vec()));
    }

    #[test]
    fn test_space_key() {
        let mode = TermMode::empty();

        let space = Keystroke::parse("space").unwrap();
        assert_eq!(keystroke_to_bytes(&space, mode), Some(b" ".to_vec()));
    }

    fn with(key: &str, f: impl FnOnce(&mut gpui::Modifiers)) -> Keystroke {
        let mut k = Keystroke::parse(key).unwrap();
        f(&mut k.modifiers);
        k
    }

    #[test]
    fn mac_editing_keys() {
        let cmd = |key| with(key, |m| m.platform = true);
        let opt = |key| with(key, |m| m.alt = true);
        assert_eq!(mac_editing_bytes(&cmd("left")), Some(b"\x01".to_vec()));
        assert_eq!(mac_editing_bytes(&cmd("right")), Some(b"\x05".to_vec()));
        assert_eq!(mac_editing_bytes(&cmd("backspace")), Some(b"\x15".to_vec()));
        assert_eq!(mac_editing_bytes(&opt("left")), Some(b"\x1bb".to_vec()));
        assert_eq!(mac_editing_bytes(&opt("right")), Some(b"\x1bf".to_vec()));
        assert_eq!(mac_editing_bytes(&opt("backspace")), Some(b"\x17".to_vec()));
        assert_eq!(mac_editing_bytes(&opt("delete")), Some(b"\x1bd".to_vec()));
        // Not these: plain keys, other letters, other modifiers on top.
        assert_eq!(mac_editing_bytes(&Keystroke::parse("left").unwrap()), None);
        assert_eq!(mac_editing_bytes(&cmd("a")), None);
        assert_eq!(mac_editing_bytes(&with("left", |m| { m.platform = true; m.shift = true })), None);
        assert_eq!(mac_editing_bytes(&with("left", |m| { m.platform = true; m.alt = true })), None);
    }

    #[test]
    fn arrows_and_navigation_keys_carry_their_modifiers() {
        let mode = TermMode::empty();
        let ctrl = |key| with(key, |m| m.control = true);
        assert_eq!(keystroke_to_bytes(&ctrl("right"), mode), Some(b"\x1b[1;5C".to_vec()));
        assert_eq!(keystroke_to_bytes(&ctrl("left"), mode), Some(b"\x1b[1;5D".to_vec()));
        assert_eq!(keystroke_to_bytes(&with("up", |m| m.shift = true), mode), Some(b"\x1b[1;2A".to_vec()));
        assert_eq!(keystroke_to_bytes(&with("left", |m| m.alt = true), mode), Some(b"\x1b[1;3D".to_vec()));
        assert_eq!(keystroke_to_bytes(&with("end", |m| { m.shift = true; m.control = true }), mode), Some(b"\x1b[1;6F".to_vec()));
        assert_eq!(keystroke_to_bytes(&ctrl("delete"), mode), Some(b"\x1b[3;5~".to_vec()));
        assert_eq!(keystroke_to_bytes(&with("pageup", |m| m.shift = true), mode), Some(b"\x1b[5;2~".to_vec()));
        // Without modifiers nothing changes.
        assert_eq!(keystroke_to_bytes(&Keystroke::parse("right").unwrap(), mode), Some(b"\x1b[C".to_vec()));
    }
}
