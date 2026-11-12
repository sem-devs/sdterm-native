//! A text field: for renaming workspaces and terminals, writing a note, and the settings.
//!
//! GPUI has no ready made text field, so this one is built on the same input protocol the
//! terminal uses. That is what makes dictation, accents and the emoji picker work in it too.

use std::ops::Range;

use gpui::prelude::*;
use gpui::{
    App, Bounds, ClipboardItem, Context, ElementInputHandler, EntityInputHandler, EventEmitter,
    FocusHandle, Focusable, KeyDownEvent, Pixels, Point, Render, SharedString, Subscription,
    UTF16Selection, Window, canvas, div, px,
};
use unicode_segmentation::UnicodeSegmentation;

use crate::theme::Theme;

// ---- Pure text operations (tested below) ---------------------------------------------------

fn offset_to_utf16(text: &str, offset: usize) -> usize {
    text[..offset].encode_utf16().count()
}

fn offset_from_utf16(text: &str, utf16: usize) -> usize {
    let mut count = 0;
    for (i, ch) in text.char_indices() {
        if count >= utf16 {
            return i;
        }
        count += ch.len_utf16();
    }
    text.len()
}

fn prev_boundary(text: &str, i: usize) -> usize {
    text[..i].grapheme_indices(true).next_back().map_or(0, |(j, _)| j)
}

fn next_boundary(text: &str, i: usize) -> usize {
    text[i..].graphemes(true).next().map_or(text.len(), |g| i + g.len())
}

/// The start of the word before `i` (skipping spaces first).
fn prev_word(text: &str, i: usize) -> usize {
    let head = &text[..i];
    let trimmed = head.trim_end();
    trimmed.rfind(char::is_whitespace).map_or(0, |j| j + trimmed[j..].chars().next().map_or(1, char::len_utf8))
}

/// The end of the word after `i` (skipping spaces first).
fn next_word(text: &str, i: usize) -> usize {
    let tail = &text[i..];
    let skipped = tail.len() - tail.trim_start().len();
    let rest = &tail[skipped..];
    i + skipped + rest.find(char::is_whitespace).unwrap_or(rest.len())
}

fn line_start(text: &str, i: usize) -> usize {
    text[..i].rfind('\n').map_or(0, |j| j + 1)
}

fn line_end(text: &str, i: usize) -> usize {
    text[i..].find('\n').map_or(text.len(), |j| i + j)
}

/// The offset in the line above or below, keeping the column as far as it fits.
fn vertical(text: &str, i: usize, up: bool) -> usize {
    let start = line_start(text, i);
    let column = text[start..i].chars().count();
    let target_start = if up {
        if start == 0 {
            return 0;
        }
        line_start(text, start - 1)
    } else {
        let end = line_end(text, i);
        if end >= text.len() {
            return text.len();
        }
        end + 1
    };
    let target_end = line_end(text, target_start);
    let mut offset = target_start;
    for (n, ch) in text[target_start..target_end].chars().enumerate() {
        if n == column {
            break;
        }
        offset += ch.len_utf8();
    }
    offset
}

// ---- The field -----------------------------------------------------------------------------

pub enum InputEvent {
    /// Enter (or Cmd+Enter in a note).
    Submit(String),
    Cancel,
    /// The field lost the focus: what is written counts, like leaving a rename box.
    Blur(String),
}

pub struct TextInput {
    focus: FocusHandle,
    text: String,
    cursor: usize,
    anchor: Option<usize>,
    marked: Option<Range<usize>>,
    placeholder: SharedString,
    multiline: bool,
    _blur: Subscription,
}

impl EventEmitter<InputEvent> for TextInput {}

impl Focusable for TextInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl TextInput {
    /// A field with `text` already in it. `select_all` selects it, ready to be replaced.
    pub fn new(
        text: &str,
        placeholder: &str,
        multiline: bool,
        select_all: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        let blur = cx.on_blur(&focus, window, |this, _window, cx| {
            cx.emit(InputEvent::Blur(this.text.clone()));
        });
        Self {
            focus,
            text: text.to_string(),
            cursor: text.len(),
            anchor: if select_all && !text.is_empty() { Some(0) } else { None },
            marked: None,
            placeholder: placeholder.to_string().into(),
            multiline,
            _blur: blur,
        }
    }

    pub fn focus(&self, window: &mut Window) {
        window.focus(&self.focus);
    }

    pub fn is_focused(&self, window: &Window) -> bool {
        self.focus.is_focused(window)
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    fn selection(&self) -> Range<usize> {
        match self.anchor {
            Some(a) => a.min(self.cursor)..a.max(self.cursor),
            None => self.cursor..self.cursor,
        }
    }

    fn replace(&mut self, range: Range<usize>, new: &str, cx: &mut Context<Self>) {
        self.text.replace_range(range.clone(), new);
        self.cursor = range.start + new.len();
        self.anchor = None;
        self.marked = None;
        cx.notify();
    }

    fn move_to(&mut self, index: usize, extend: bool, cx: &mut Context<Self>) {
        if extend {
            self.anchor.get_or_insert(self.cursor);
        } else {
            self.anchor = None;
        }
        self.cursor = index;
        cx.notify();
    }

    fn insert(&mut self, text: &str, cx: &mut Context<Self>) {
        let text = if self.multiline { text.replace("\r\n", "\n") } else { text.replace(['\n', '\r'], " ") };
        let range = self.selection();
        self.replace(range, &text, cx);
    }

    fn delete(&mut self, forward: bool, word: bool, line: bool, cx: &mut Context<Self>) {
        let sel = self.selection();
        if !sel.is_empty() {
            return self.replace(sel, "", cx);
        }
        let i = self.cursor;
        let range = match (forward, line, word) {
            (false, true, _) => line_start(&self.text, i)..i,
            (false, _, true) => prev_word(&self.text, i)..i,
            (false, _, _) => prev_boundary(&self.text, i)..i,
            (true, true, _) => i..line_end(&self.text, i),
            (true, _, true) => i..next_word(&self.text, i),
            (true, _, _) => i..next_boundary(&self.text, i),
        };
        if !range.is_empty() {
            self.replace(range, "", cx);
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let ks = &event.keystroke;
        let m = &ks.modifiers;
        let mac = cfg!(target_os = "macos");
        let command = if mac { m.platform } else { m.control };
        let word = if mac { m.alt } else { m.control };
        let shift = m.shift;

        // Text keys belong to the input method, which composes accents and delivers the result to
        // `replace_text_in_range`.
        if ks.key_char.is_some() && !m.control && !m.platform && !m.function {
            return;
        }

        match ks.key.as_str() {
            "enter" => {
                if self.multiline && !command {
                    self.insert("\n", cx);
                } else {
                    cx.emit(InputEvent::Submit(self.text.clone()));
                }
            }
            "escape" => cx.emit(InputEvent::Cancel),
            "backspace" => self.delete(false, word, mac && m.platform, cx),
            "delete" => self.delete(true, word, mac && m.platform, cx),
            "left" => {
                let to = if mac && m.platform {
                    line_start(&self.text, self.cursor)
                } else if word {
                    prev_word(&self.text, self.cursor)
                } else if !shift && self.anchor.is_some() {
                    self.selection().start
                } else {
                    prev_boundary(&self.text, self.cursor)
                };
                self.move_to(to, shift, cx);
            }
            "right" => {
                let to = if mac && m.platform {
                    line_end(&self.text, self.cursor)
                } else if word {
                    next_word(&self.text, self.cursor)
                } else if !shift && self.anchor.is_some() {
                    self.selection().end
                } else {
                    next_boundary(&self.text, self.cursor)
                };
                self.move_to(to, shift, cx);
            }
            "up" if self.multiline => self.move_to(vertical(&self.text, self.cursor, true), shift, cx),
            "down" if self.multiline => self.move_to(vertical(&self.text, self.cursor, false), shift, cx),
            "home" => self.move_to(line_start(&self.text, self.cursor), shift, cx),
            "end" => self.move_to(line_end(&self.text, self.cursor), shift, cx),
            "a" if command => {
                self.anchor = Some(0);
                self.cursor = self.text.len();
                cx.notify();
            }
            "c" if command => {
                let sel = self.selection();
                if !sel.is_empty() {
                    cx.write_to_clipboard(ClipboardItem::new_string(self.text[sel].to_string()));
                }
            }
            "x" if command => {
                let sel = self.selection();
                if !sel.is_empty() {
                    cx.write_to_clipboard(ClipboardItem::new_string(self.text[sel.clone()].to_string()));
                    self.replace(sel, "", cx);
                }
            }
            "v" if command => {
                if let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) {
                    self.insert(&text, cx);
                }
            }
            _ => return,
        }
        cx.stop_propagation();
    }
}

impl Render for TextInput {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<Theme>().clone();
        let focused = self.focus.is_focused(window);
        let entity = cx.entity();
        let focus = self.focus.clone();
        let sel = self.selection();
        let (primary, selection_bg) = (theme.primary, theme.primary.opacity(0.3));

        // One row per line. The caret and the selection are drawn by splitting the text around
        // them, so no text measuring is needed.
        let mut rows = div().flex().flex_col().w_full();
        let mut start = 0;
        let lines: Vec<&str> = if self.text.is_empty() { vec![""] } else { self.text.split('\n').collect() };
        for line in &lines {
            let end = start + line.len();
            let mut row = div().flex().items_center().min_h(px(18.)).whitespace_nowrap();
            let overlap = sel.start.max(start)..sel.end.min(end);
            if focused && !sel.is_empty() && overlap.start < overlap.end {
                row = row
                    .child(self.text[start..overlap.start].to_string())
                    .child(div().bg(selection_bg).child(self.text[overlap.clone()].to_string()))
                    .child(self.text[overlap.end..end].to_string());
            } else if sel.is_empty() && self.cursor >= start && self.cursor <= end && focused {
                row = row
                    .child(self.text[start..self.cursor].to_string())
                    .child(div().w(px(1.5)).h(px(15.)).flex_none().bg(primary))
                    .child(self.text[self.cursor..end].to_string());
            } else {
                row = row.child(line.to_string());
            }
            rows = rows.child(row);
            start = end + 1;
        }

        let empty = self.text.is_empty();
        div()
            .id("text-input")
            .relative()
            .w_full()
            .px_2()
            .py_1()
            .rounded_md()
            .border_1()
            .border_color(if focused { theme.primary } else { theme.input })
            .bg(theme.background)
            .text_color(theme.foreground)
            .overflow_hidden()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key_down))
            .on_mouse_down(gpui::MouseButton::Left, cx.listener(|this, _, window, _| window.focus(&this.focus)))
            .child(
                canvas(
                    |bounds, _, _| bounds,
                    move |bounds, _, window, cx| {
                        window.handle_input(&focus, ElementInputHandler::new(bounds, entity.clone()), cx);
                    },
                )
                .absolute()
                .size_full(),
            )
            .when(empty, |d| {
                d.child(
                    div()
                        .absolute()
                        .top_1()
                        .left_2()
                        .text_color(theme.muted_foreground)
                        .child(self.placeholder.clone()),
                )
            })
            .child(rows)
    }
}

impl EntityInputHandler for TextInput {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let start = offset_from_utf16(&self.text, range.start);
        let end = offset_from_utf16(&self.text, range.end);
        *adjusted = Some(offset_to_utf16(&self.text, start)..offset_to_utf16(&self.text, end));
        Some(self.text[start..end].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let sel = self.selection();
        Some(UTF16Selection {
            range: offset_to_utf16(&self.text, sel.start)..offset_to_utf16(&self.text, sel.end),
            reversed: self.anchor.is_some_and(|a| a > self.cursor),
        })
    }

    fn marked_text_range(&self, _window: &mut Window, _cx: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked
            .as_ref()
            .map(|r| offset_to_utf16(&self.text, r.start)..offset_to_utf16(&self.text, r.end))
    }

    fn unmark_text(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.marked = None;
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range
            .map(|r| offset_from_utf16(&self.text, r.start)..offset_from_utf16(&self.text, r.end))
            .or_else(|| self.marked.clone())
            .unwrap_or_else(|| self.selection());
        let text = if self.multiline { text.to_string() } else { text.replace(['\n', '\r'], " ") };
        self.replace(range, &text, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        new_text: &str,
        new_selected: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range
            .map(|r| offset_from_utf16(&self.text, r.start)..offset_from_utf16(&self.text, r.end))
            .or_else(|| self.marked.clone())
            .unwrap_or_else(|| self.selection());
        self.text.replace_range(range.clone(), new_text);
        self.marked = (!new_text.is_empty()).then(|| range.start..range.start + new_text.len());
        self.cursor = new_selected
            .map(|r| range.start + offset_from_utf16(new_text, r.end))
            .unwrap_or(range.start + new_text.len());
        self.anchor = None;
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        _range: Range<usize>,
        element_bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        // Close to the text, which is enough to place the dictation and accent popups.
        Some(Bounds { origin: element_bounds.origin + Point { x: px(8.), y: px(4.) }, size: element_bounds.size })
    }

    fn character_index_for_point(
        &mut self,
        _point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        Some(offset_to_utf16(&self.text, self.cursor))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moves_over_whole_characters_not_bytes() {
        let t = "añb😀c";
        assert_eq!(next_boundary(t, 0), 1);
        assert_eq!(next_boundary(t, 1), 3); // ñ is two bytes
        assert_eq!(prev_boundary(t, 3), 1);
        let emoji = t.find('😀').unwrap();
        assert_eq!(next_boundary(t, emoji), emoji + 4);
        assert_eq!(prev_boundary(t, emoji + 4), emoji);
        assert_eq!(prev_boundary(t, 0), 0);
        assert_eq!(next_boundary(t, t.len()), t.len());
    }

    #[test]
    fn a_family_emoji_is_one_character() {
        let t = "a👨‍👩‍👧b";
        let family_end = t.len() - 1;
        assert_eq!(prev_boundary(t, family_end), 1);
    }

    #[test]
    fn utf16_offsets_round_trip() {
        let t = "a😀ñ";
        for (i, _) in t.char_indices() {
            assert_eq!(offset_from_utf16(t, offset_to_utf16(t, i)), i);
        }
        assert_eq!(offset_to_utf16(t, t.len()), 4); // the emoji takes two UTF-16 units
        assert_eq!(offset_from_utf16(t, 99), t.len());
    }

    #[test]
    fn words() {
        let t = "hello big  world";
        assert_eq!(next_word(t, 0), 5);
        assert_eq!(next_word(t, 5), 9);
        assert_eq!(next_word(t, 9), 16);
        assert_eq!(prev_word(t, 16), 11);
        assert_eq!(prev_word(t, 11), 6);
        assert_eq!(prev_word(t, 5), 0);
    }

    #[test]
    fn lines_and_vertical_moves() {
        let t = "abc\nde\nfghij";
        assert_eq!(line_start(t, 5), 4);
        assert_eq!(line_end(t, 5), 6);
        // From column 2 of line 1 up to line 0, and down to the short line 1, then line 2.
        assert_eq!(vertical(t, 6, true), 2); // line 1 col 2 -> line 0 col 2
        assert_eq!(vertical(t, 2, false), 6); // line 0 col 2 -> line 1 is only 2 long: its end
        assert_eq!(vertical(t, 6, false), 9); // line 1 end (col 2) -> line 2 col 2
        assert_eq!(vertical(t, 1, true), 0); // top line: start of the text
        assert_eq!(vertical(t, t.len(), false), t.len()); // bottom line: end of the text
    }
}
