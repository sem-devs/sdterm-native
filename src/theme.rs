//! Colors. Same palette as the Electron app, from the SEM Devs brand book: cyan on dark, Tech Blue
//! on light, one accent only. The per-terminal colors are the user's own labels, not UI accents.

use gpui::{Global, Hsla, rgb, rgba};
use gpui_terminal::ColorPalette;

use crate::model::{PaneColor, ThemeMode};

#[derive(Clone)]
pub struct Theme {
    pub dark: bool,
    pub background: Hsla,
    pub foreground: Hsla,
    /// The sidebar.
    pub card: Hsla,
    pub primary: Hsla,
    pub primary_foreground: Hsla,
    pub muted: Hsla,
    pub muted_foreground: Hsla,
    pub accent: Hsla,
    pub border: Hsla,
    pub input: Hsla,
    pane: [Hsla; 8],
}

impl Global for Theme {}

fn c(hex: u32) -> Hsla {
    rgb(hex).into()
}

fn ca(hex: u32) -> Hsla {
    rgba(hex).into()
}

impl Theme {
    pub fn dark() -> Self {
        Self {
            dark: true,
            background: c(0x141414),
            foreground: c(0xF0F0F0),
            card: c(0x0D0D0D),
            primary: c(0x52E5F1),
            primary_foreground: c(0x0D0D0D),
            muted: c(0x1F1F1F),
            muted_foreground: c(0x9A9A9A),
            accent: c(0x262626),
            border: ca(0xFFFFFF17),
            input: ca(0xFFFFFF24),
            pane: [
                c(0x52E5F1),
                c(0x5B93F0),
                c(0x00C853),
                c(0xF2B84B),
                c(0xF98A47),
                c(0xF2677C),
                c(0xA98BF5),
                c(0x9A9A9A),
            ],
        }
    }

    pub fn light() -> Self {
        Self {
            dark: false,
            background: c(0xF8F8F8),
            foreground: c(0x161616),
            card: c(0xF0F0F0),
            primary: c(0x1D56B1),
            primary_foreground: c(0xF8F8F8),
            muted: c(0xEEEEEE),
            muted_foreground: c(0x5A5A5A),
            accent: c(0xE6E6E6),
            border: ca(0x0000001C),
            input: ca(0x00000029),
            pane: [
                c(0x0B8FA1),
                c(0x1D56B1),
                c(0x007A33),
                c(0xA8680A),
                c(0xC2500F),
                c(0xC2334D),
                c(0x7040C8),
                c(0x5A5A5A),
            ],
        }
    }

    /// The theme for a setting, given whether the system is in dark mode.
    pub fn for_mode(mode: ThemeMode, system_dark: bool) -> Self {
        match mode {
            ThemeMode::Dark => Self::dark(),
            ThemeMode::Light => Self::light(),
            ThemeMode::System if system_dark => Self::dark(),
            ThemeMode::System => Self::light(),
        }
    }

    /// A terminal's label color; terminals without one use the accent.
    pub fn pane(&self, color: Option<PaneColor>) -> Hsla {
        match color {
            Some(PaneColor::Cyan) => self.pane[0],
            Some(PaneColor::Blue) => self.pane[1],
            Some(PaneColor::Green) => self.pane[2],
            Some(PaneColor::Amber) => self.pane[3],
            Some(PaneColor::Orange) => self.pane[4],
            Some(PaneColor::Rose) => self.pane[5],
            Some(PaneColor::Violet) => self.pane[6],
            Some(PaneColor::Graphite) => self.pane[7],
            None => self.primary,
        }
    }

    /// Colors for the text inside a terminal.
    pub fn terminal_palette(&self) -> ColorPalette {
        if self.dark {
            ColorPalette::builder()
                .background(0x14, 0x14, 0x14)
                .foreground(0xE6, 0xE6, 0xE6)
                .cursor(0x52, 0xE5, 0xF1)
                .black(0x1D, 0x1D, 0x1D)
                .red(0xEF, 0x7A, 0x7A)
                .green(0x6F, 0xD0, 0x86)
                .yellow(0xE6, 0xC0, 0x6A)
                .blue(0x6E, 0x9B, 0xF0)
                .magenta(0xD0, 0x8A, 0xE6)
                .cyan(0x52, 0xE5, 0xF1)
                .white(0xC4, 0xC4, 0xC4)
                .bright_black(0x5A, 0x5A, 0x5A)
                .bright_red(0xFF, 0x9A, 0x9A)
                .bright_green(0x8C, 0xE8, 0xA0)
                .bright_yellow(0xF5, 0xD8, 0x8A)
                .bright_blue(0x8F, 0xB4, 0xFF)
                .bright_magenta(0xE6, 0xA8, 0xF5)
                .bright_cyan(0x8A, 0xF0, 0xF8)
                .bright_white(0xF0, 0xF0, 0xF0)
                .build()
        } else {
            ColorPalette::builder()
                .background(0xF8, 0xF8, 0xF8)
                .foreground(0x16, 0x16, 0x16)
                .cursor(0x1D, 0x56, 0xB1)
                .black(0x16, 0x16, 0x16)
                .red(0xC2, 0x33, 0x4D)
                .green(0x00, 0x7A, 0x33)
                .yellow(0xA8, 0x68, 0x0A)
                .blue(0x1D, 0x56, 0xB1)
                .magenta(0x70, 0x40, 0xC8)
                .cyan(0x0B, 0x8F, 0xA1)
                .white(0x9A, 0x9A, 0x9A)
                .bright_black(0x5A, 0x5A, 0x5A)
                .bright_red(0xD9, 0x4A, 0x64)
                .bright_green(0x1A, 0x9A, 0x4D)
                .bright_yellow(0xC2, 0x80, 0x1A)
                .bright_blue(0x3A, 0x72, 0xCC)
                .bright_magenta(0x8A, 0x5C, 0xDD)
                .bright_cyan(0x1F, 0xA8, 0xBA)
                .bright_white(0x4A, 0x4A, 0x4A)
                .build()
        }
    }
}
