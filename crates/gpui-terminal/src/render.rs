//! Terminal rendering module.
//!
//! This module provides [`TerminalRenderer`], which handles efficient rendering of
//! terminal content using GPUI's text and drawing systems.
//!
//! # Rendering Pipeline
//!
//! The renderer processes the terminal grid in several stages:
//!
//! ```text
//! Terminal Grid → Layout Phase → Paint Phase
//!                      │              │
//!                      ├─ Collect backgrounds
//!                      ├─ Batch text runs
//!                      │              │
//!                      │              ├─ Paint default background
//!                      │              ├─ Paint non-default backgrounds
//!                      │              ├─ Paint text characters
//!                      │              └─ Paint cursor
//! ```
//!
//! # Optimizations
//!
//! The renderer includes several optimizations to minimize draw calls:
//!
//! 1. **Background Merging**: Adjacent cells with the same background color are
//!    merged into single rectangles, reducing the number of quads to paint.
//!
//! 2. **Text Batching**: Adjacent cells with identical styling (color, bold, italic)
//!    are grouped into [`BatchedTextRun`]s for efficient text shaping.
//!
//! 3. **Default Background Skip**: Cells with the default background color don't
//!    generate separate background rectangles.
//!
//! 4. **Cell Measurement**: Font metrics are measured once using the 'M' character
//!    and cached for consistent cell dimensions.
//!
//! # Cell Dimensions
//!
//! Cell size is calculated from actual font metrics:
//!
//! - **Width**: Measured from shaped 'M' character (typically widest in monospace)
//! - **Height**: `(ascent + descent) × line_height_multiplier`
//!
//! The `line_height_multiplier` (default 1.2) adds extra vertical space to
//! accommodate tall glyphs from nerd fonts and other icon fonts.
//!
//! # Example
//!
//! ```ignore
//! use gpui::px;
//! use gpui_terminal::{ColorPalette, TerminalRenderer};
//!
//! let renderer = TerminalRenderer::new(
//!     "JetBrains Mono".to_string(),
//!     px(14.0),
//!     1.2,  // line height multiplier
//!     ColorPalette::default(),
//! );
//! ```

use crate::colors::ColorPalette;
use crate::event::GpuiEventProxy;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point as AlacPoint};
use alacritty_terminal::term::Term;
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::vte::ansi::Color;
use gpui::{
    App, Bounds, Edges, Font, FontFeatures, FontStyle, FontWeight, Hsla, Pixels, Point,
    SharedString, Size, TextRun, UnderlineStyle, Window, px, quad, transparent_black,
};

/// What is drawn on top of the grid besides the cells: search matches, the link under the pointer
/// and the phase of the blinking cursor.
pub struct PaintExtras<'a> {
    /// The cursor is in its visible phase (always true when it does not blink).
    pub cursor_on: bool,
    /// Search matches in order, and which one is the current one.
    pub matches: &'a [std::ops::RangeInclusive<AlacPoint>],
    pub active_match: Option<usize>,
    /// The link the pointer is over while Cmd (Ctrl) is held: first and last cell.
    pub link: Option<(AlacPoint, AlacPoint)>,
}

impl Default for PaintExtras<'_> {
    fn default() -> Self {
        Self { cursor_on: true, matches: &[], active_match: None, link: None }
    }
}

/// The columns a range covers on one line, if it reaches that line.
fn columns_on_line(range: &std::ops::RangeInclusive<AlacPoint>, line: Line, cols: usize) -> Option<(usize, usize)> {
    let (start, end) = (range.start(), range.end());
    if line < start.line || line > end.line {
        return None;
    }
    let from = if line == start.line { start.column.0 } else { 0 };
    let to = if line == end.line { end.column.0 } else { cols.saturating_sub(1) };
    (from <= to).then_some((from, to.min(cols.saturating_sub(1))))
}


/// A batched run of text with consistent styling.
///
/// This struct groups adjacent terminal cells with identical visual attributes
/// to reduce the number of text rendering calls.
#[derive(Debug, Clone)]
pub struct BatchedTextRun {
    /// The text content to render
    pub text: String,

    /// Starting column position
    pub start_col: usize,

    /// Row position
    pub row: usize,

    /// Foreground color
    pub fg_color: Hsla,

    /// Background color
    pub bg_color: Hsla,

    /// Bold flag
    pub bold: bool,

    /// Italic flag
    pub italic: bool,

    /// Underline flag
    pub underline: bool,
}

/// Background rectangle to paint.
///
/// Represents a rectangular region with a solid color background.
#[derive(Debug, Clone)]
pub struct BackgroundRect {
    /// Starting column position
    pub start_col: usize,

    /// Ending column position (exclusive)
    pub end_col: usize,

    /// Row position
    pub row: usize,

    /// Background color
    pub color: Hsla,
}

impl BackgroundRect {
    /// Check if this rectangle can be merged with another.
    ///
    /// Two rectangles can be merged if they:
    /// - Are on the same row
    /// - Have the same color
    /// - Are horizontally adjacent
    fn can_merge_with(&self, other: &Self) -> bool {
        self.row == other.row && self.color == other.color && self.end_col == other.start_col
    }
}

/// Terminal renderer with font settings and cell dimensions.
///
/// This struct manages the rendering of terminal content, including text,
/// backgrounds, and cursor. It maintains font metrics and provides the
/// [`paint`](Self::paint) method for drawing the terminal grid.
///
/// # Font Metrics
///
/// Cell dimensions are calculated from actual font measurements via
/// [`measure_cell`](Self::measure_cell). This ensures accurate character
/// positioning regardless of the font used.
///
/// # Usage
///
/// The renderer is typically used internally by [`TerminalView`](crate::TerminalView),
/// but can also be used directly for custom rendering:
///
/// ```ignore
/// // Measure cell dimensions (call once per font change)
/// renderer.measure_cell(window);
///
/// // Paint the terminal grid
/// renderer.paint(bounds, padding, &term, window, cx);
/// ```
///
/// # Performance
///
/// For optimal performance:
/// - Call `measure_cell` only when font settings change
/// - The `paint` method is designed to be called every frame
/// - Background and text batching minimize GPU draw calls
#[derive(Clone)]
pub struct TerminalRenderer {
    /// Font family name (e.g., "Fira Code", "Menlo")
    pub font_family: String,

    /// Font size in pixels
    pub font_size: Pixels,

    /// Width of a single character cell
    pub cell_width: Pixels,

    /// Height of a single character cell (line height)
    pub cell_height: Pixels,

    /// Multiplier for line height to accommodate tall glyphs
    pub line_height_multiplier: f32,

    /// Color palette for resolving terminal colors
    pub palette: ColorPalette,
}

impl TerminalRenderer {
    /// Creates a new terminal renderer with the given font settings and color palette.
    ///
    /// # Arguments
    ///
    /// * `font_family` - The name of the font family to use
    /// * `font_size` - The font size in pixels
    /// * `line_height_multiplier` - Multiplier for line height (e.g., 1.2 for 20% extra)
    /// * `palette` - The color palette to use for terminal colors
    ///
    /// # Returns
    ///
    /// A new `TerminalRenderer` instance with default cell dimensions.
    ///
    /// # Examples
    ///
    /// ```
    /// use gpui::px;
    /// use gpui_terminal::render::TerminalRenderer;
    /// use gpui_terminal::ColorPalette;
    ///
    /// let renderer = TerminalRenderer::new("Fira Code".to_string(), px(14.0), 1.2, ColorPalette::default());
    /// ```
    pub fn new(
        font_family: String,
        font_size: Pixels,
        line_height_multiplier: f32,
        palette: ColorPalette,
    ) -> Self {
        // Default cell dimensions - will be measured on first paint
        // Using 0.6 as approximate em-width ratio for monospace fonts
        let cell_width = font_size * 0.6;
        let cell_height = font_size * 1.4; // Line height with some spacing

        Self {
            font_family,
            font_size,
            cell_width,
            cell_height,
            line_height_multiplier,
            palette,
        }
    }

    /// Measure cell dimensions based on actual font metrics.
    ///
    /// This method measures the actual width and height of characters
    /// using the GPUI text system.
    ///
    /// # Arguments
    ///
    /// * `window` - The GPUI window for text system access
    pub fn measure_cell(&mut self, window: &mut Window) {
        // Measure using a reference character (M is typically the widest)
        let font = Font {
            family: self.font_family.clone().into(),
            features: FontFeatures::default(),
            fallbacks: None,
            weight: FontWeight::NORMAL,
            style: FontStyle::Normal,
        };

        let text_run = TextRun {
            len: 1,
            font,
            color: gpui::black(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };

        // Shape a single 'M' character to get its metrics
        let shaped = window
            .text_system()
            .shape_line("M".into(), self.font_size, &[text_run], None);

        // Get the width from the shaped line (accessed via Deref to LineLayout)
        if shaped.width > px(0.0) {
            self.cell_width = shaped.width;
        }

        // Calculate height from ascent + descent with multiplier for tall glyphs (nerd fonts, etc.)
        let line_height = shaped.ascent + shaped.descent;
        if line_height > px(0.0) {
            self.cell_height = line_height * self.line_height_multiplier;
        }
    }

    /// Layout cells into batched text runs and background rects for a single row.
    ///
    /// This method processes a row of terminal cells and groups adjacent cells
    /// with identical styling into batched runs. It also collects background
    /// rectangles that need to be painted.
    ///
    /// # Arguments
    ///
    /// * `row` - The row number
    /// * `cells` - Iterator over (column, Cell) pairs
    /// * `colors` - Terminal color configuration
    ///
    /// # Returns
    ///
    /// A tuple of `(backgrounds, text_runs)` where:
    /// - `backgrounds` is a vector of merged background rectangles
    /// - `text_runs` is a vector of batched text runs
    pub fn layout_row(
        &self,
        row: usize,
        cells: impl Iterator<Item = (usize, Cell)>,
        colors: &Colors,
    ) -> (Vec<BackgroundRect>, Vec<BatchedTextRun>) {
        let mut backgrounds = Vec::new();
        let mut text_runs = Vec::new();

        let mut current_run: Option<BatchedTextRun> = None;
        let mut current_bg: Option<BackgroundRect> = None;

        for (col, cell) in cells {
            // Skip wide character spacers
            if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                continue;
            }

            // Extract cell styling
            let fg_color = self.palette.resolve(cell.fg, colors);
            let bg_color = self.palette.resolve(cell.bg, colors);
            let bold = cell.flags.contains(Flags::BOLD);
            let italic = cell.flags.contains(Flags::ITALIC);
            let underline = cell.flags.contains(Flags::UNDERLINE);

            // Get the character (or space if empty)
            let ch = if cell.c == ' ' || cell.c == '\0' {
                ' '
            } else {
                cell.c
            };

            // Handle background rectangles
            if let Some(ref mut bg_rect) = current_bg {
                if bg_rect.color == bg_color && bg_rect.end_col == col {
                    // Extend current background
                    bg_rect.end_col = col + 1;
                } else {
                    // Save current background and start new one
                    backgrounds.push(bg_rect.clone());
                    current_bg = Some(BackgroundRect {
                        start_col: col,
                        end_col: col + 1,
                        row,
                        color: bg_color,
                    });
                }
            } else {
                // Start new background
                current_bg = Some(BackgroundRect {
                    start_col: col,
                    end_col: col + 1,
                    row,
                    color: bg_color,
                });
            }

            // Handle text runs
            if let Some(ref mut run) = current_run {
                if run.fg_color == fg_color
                    && run.bg_color == bg_color
                    && run.bold == bold
                    && run.italic == italic
                    && run.underline == underline
                {
                    // Extend current run
                    run.text.push(ch);
                } else {
                    // Save current run and start new one
                    text_runs.push(run.clone());
                    current_run = Some(BatchedTextRun {
                        text: ch.to_string(),
                        start_col: col,
                        row,
                        fg_color,
                        bg_color,
                        bold,
                        italic,
                        underline,
                    });
                }
            } else {
                // Start new run
                current_run = Some(BatchedTextRun {
                    text: ch.to_string(),
                    start_col: col,
                    row,
                    fg_color,
                    bg_color,
                    bold,
                    italic,
                    underline,
                });
            }
        }

        // Push final run and background
        if let Some(run) = current_run {
            text_runs.push(run);
        }
        if let Some(bg) = current_bg {
            backgrounds.push(bg);
        }

        // Merge adjacent backgrounds with same color
        let merged_backgrounds = self.merge_backgrounds(backgrounds);

        (merged_backgrounds, text_runs)
    }

    /// Merge adjacent background rects with same color.
    ///
    /// This optimization reduces the number of rectangles to paint by
    /// combining horizontally adjacent rectangles that share the same color.
    ///
    /// # Arguments
    ///
    /// * `rects` - Vector of background rectangles to merge
    ///
    /// # Returns
    ///
    /// A new vector with merged rectangles
    fn merge_backgrounds(&self, mut rects: Vec<BackgroundRect>) -> Vec<BackgroundRect> {
        if rects.is_empty() {
            return rects;
        }

        let mut merged = Vec::new();
        let mut current = rects.remove(0);

        for rect in rects {
            if current.can_merge_with(&rect) {
                current.end_col = rect.end_col;
            } else {
                merged.push(current);
                current = rect;
            }
        }

        merged.push(current);
        merged
    }

    /// Paint terminal content to the window.
    ///
    /// This is the main rendering method that draws the terminal grid,
    /// including backgrounds, text, and cursor.
    ///
    /// # Arguments
    ///
    /// * `bounds` - The bounding box to render within
    /// * `padding` - Padding around the terminal content
    /// * `term` - The terminal state
    /// * `window` - The GPUI window
    /// * `cx` - The application context
    pub fn paint(
        &self,
        bounds: Bounds<Pixels>,
        padding: Edges<Pixels>,
        term: &Term<GpuiEventProxy>,
        focused: bool,
        extras: &PaintExtras<'_>,
        window: &mut Window,
        cx: &mut App,
    ) {
        use alacritty_terminal::term::TermMode;
        use alacritty_terminal::vte::ansi::{CursorShape, NamedColor};

        let grid = term.grid();
        let num_lines = grid.screen_lines();
        let num_cols = grid.columns();
        let colors = term.colors();
        // How far the view is scrolled back into the history (0 = at the bottom).
        let offset = grid.display_offset() as i32;

        let default_bg = self.palette.resolve(Color::Named(NamedColor::Background), colors);
        let cursor_color = self.palette.resolve(Color::Named(NamedColor::Cursor), colors);
        let mut selection_color = cursor_color;
        selection_color.a = 0.35;
        let match_color: Hsla = gpui::rgba(0xffd33d66).into();
        let match_active: Hsla = gpui::rgba(0xff8c1aaa).into();
        let link_color = self.palette.resolve(Color::Named(NamedColor::Foreground), colors);

        let fill = |window: &mut Window, rect: Bounds<Pixels>, color: Hsla| {
            window.paint_quad(quad(
                rect,
                px(0.0),
                color,
                Edges::<Pixels>::default(),
                transparent_black(),
                Default::default(),
            ));
        };

        // Default background (covers full bounds including padding)
        fill(window, bounds, default_bg);

        let origin = Point {
            x: bounds.origin.x + padding.left,
            y: bounds.origin.y + padding.top,
        };
        let selection = term.selection.as_ref().and_then(|s| s.to_range(term));

        // The text sits in the middle of the taller cell the line height multiplier makes.
        let base_height = self.cell_height / self.line_height_multiplier;
        let vertical_offset = (self.cell_height - base_height) / 2.0;
        let thin = px(1.0);

        for line_idx in 0..num_lines {
            let line = Line(line_idx as i32 - offset);
            let y = origin.y + self.cell_height * (line_idx as f32);

            // The look of every cell of the row: reverse video swaps the colors, dim fades the text.
            let cells: Vec<(char, Hsla, Hsla, Flags, bool)> = (0..num_cols)
                .map(|col_idx| {
                    let point = AlacPoint::new(line, Column(col_idx));
                    let cell = &grid[point];
                    let mut fg = self.palette.resolve(cell.fg, colors);
                    let mut bg = self.palette.resolve(cell.bg, colors);
                    if cell.flags.contains(Flags::INVERSE) {
                        std::mem::swap(&mut fg, &mut bg);
                    }
                    if cell.flags.contains(Flags::DIM) {
                        fg.a *= 0.6;
                    }
                    let selected = selection.as_ref().is_some_and(|range| range.contains(point));
                    (cell.c, fg, bg, cell.flags, selected)
                })
                .collect();

            // Backgrounds, merging neighbours that share a color.
            let mut col = 0;
            while col < num_cols {
                let bg = cells[col].2;
                let mut end = col + 1;
                while end < num_cols && cells[end].2 == bg {
                    end += 1;
                }
                if bg != default_bg {
                    fill(
                        window,
                        Bounds {
                            origin: Point { x: origin.x + self.cell_width * (col as f32), y },
                            size: Size {
                                width: self.cell_width * ((end - col) as f32),
                                height: self.cell_height,
                            },
                        },
                        bg,
                    );
                }
                col = end;
            }

            // The selection, over the backgrounds and under the text.
            let mut col = 0;
            while col < num_cols {
                if !cells[col].4 {
                    col += 1;
                    continue;
                }
                let mut end = col + 1;
                while end < num_cols && cells[end].4 {
                    end += 1;
                }
                fill(
                    window,
                    Bounds {
                        origin: Point { x: origin.x + self.cell_width * (col as f32), y },
                        size: Size {
                            width: self.cell_width * ((end - col) as f32),
                            height: self.cell_height,
                        },
                    },
                    selection_color,
                );
                col = end;
            }

            // Search matches, the current one stronger.
            let first = extras.matches.partition_point(|m| m.end().line < line);
            for (i, m) in extras.matches.iter().enumerate().skip(first) {
                if m.start().line > line {
                    break;
                }
                if let Some((from, to)) = columns_on_line(m, line, num_cols) {
                    let color = if extras.active_match == Some(i) { match_active } else { match_color };
                    fill(
                        window,
                        Bounds {
                            origin: Point { x: origin.x + self.cell_width * (from as f32), y },
                            size: Size {
                                width: self.cell_width * ((to - from + 1) as f32),
                                height: self.cell_height,
                            },
                        },
                        color,
                    );
                }
            }

            // The link under the pointer is underlined.
            if let Some((link_start, link_end)) = extras.link {
                let range = link_start..=link_end;
                if let Some((from, to)) = columns_on_line(&range, line, num_cols) {
                    fill(
                        window,
                        Bounds {
                            origin: Point { x: origin.x + self.cell_width * (from as f32), y: y + self.cell_height - px(2.0) },
                            size: Size { width: self.cell_width * ((to - from + 1) as f32), height: thin },
                        },
                        link_color,
                    );
                }
            }

            // The characters, each at its exact cell so the columns always line up.
            for (col_idx, (ch, fg, _bg, flags, _selected)) in cells.iter().enumerate() {
                if flags.contains(Flags::WIDE_CHAR_SPACER) || flags.contains(Flags::HIDDEN) {
                    continue;
                }
                let x = origin.x + self.cell_width * (col_idx as f32);
                let width = self.cell_width * if flags.contains(Flags::WIDE_CHAR) { 2.0 } else { 1.0 };

                if *ch != ' ' && *ch != '\0' {
                    let font = Font {
                        family: self.font_family.clone().into(),
                        features: FontFeatures::default(),
                        fallbacks: None,
                        weight: if flags.contains(Flags::BOLD) { FontWeight::BOLD } else { FontWeight::NORMAL },
                        style: if flags.contains(Flags::ITALIC) { FontStyle::Italic } else { FontStyle::Normal },
                    };
                    let text: SharedString = ch.to_string().into();
                    let run = TextRun {
                        len: text.len(),
                        font,
                        color: *fg,
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    };
                    let shaped = window.text_system().shape_line(text, self.font_size, &[run], None);
                    let _ = shaped.paint(Point { x, y: y + vertical_offset - vertical_offset }, self.cell_height, window, cx);
                }

                let underlined = flags.intersects(
                    Flags::UNDERLINE
                        | Flags::DOUBLE_UNDERLINE
                        | Flags::UNDERCURL
                        | Flags::DOTTED_UNDERLINE
                        | Flags::DASHED_UNDERLINE,
                );
                if underlined {
                    fill(
                        window,
                        Bounds {
                            origin: Point { x, y: y + self.cell_height - px(2.0) },
                            size: Size { width, height: thin },
                        },
                        *fg,
                    );
                }
                if flags.contains(Flags::STRIKEOUT) {
                    fill(
                        window,
                        Bounds {
                            origin: Point { x, y: y + self.cell_height / 2.0 },
                            size: Size { width, height: thin },
                        },
                        *fg,
                    );
                }
            }
        }

        // The cursor: solid when the terminal has the keyboard focus, an outline when it does not.
        let cursor = grid.cursor.point;
        let visible = term.mode().contains(TermMode::SHOW_CURSOR) && offset == 0 && (extras.cursor_on || !focused);
        let row = cursor.line.0;
        if visible && row >= 0 && (row as usize) < num_lines {
            let shape = term.cursor_style().shape;
            if shape != CursorShape::Hidden {
                let cell = &grid[cursor];
                let wide = cell.flags.contains(Flags::WIDE_CHAR);
                let cursor_bounds = Bounds {
                    origin: Point {
                        x: origin.x + self.cell_width * (cursor.column.0 as f32),
                        y: origin.y + self.cell_height * (row as f32),
                    },
                    size: Size {
                        width: self.cell_width * if wide { 2.0 } else { 1.0 },
                        height: self.cell_height,
                    },
                };
                let line_width = px(2.0);
                if !focused || shape == CursorShape::HollowBlock {
                    for edge in [
                        Bounds { origin: cursor_bounds.origin, size: Size { width: cursor_bounds.size.width, height: thin } },
                        Bounds {
                            origin: Point { x: cursor_bounds.origin.x, y: cursor_bounds.origin.y + cursor_bounds.size.height - thin },
                            size: Size { width: cursor_bounds.size.width, height: thin },
                        },
                        Bounds { origin: cursor_bounds.origin, size: Size { width: thin, height: cursor_bounds.size.height } },
                        Bounds {
                            origin: Point { x: cursor_bounds.origin.x + cursor_bounds.size.width - thin, y: cursor_bounds.origin.y },
                            size: Size { width: thin, height: cursor_bounds.size.height },
                        },
                    ] {
                        fill(window, edge, cursor_color);
                    }
                } else if shape == CursorShape::Underline {
                    fill(
                        window,
                        Bounds {
                            origin: Point { x: cursor_bounds.origin.x, y: cursor_bounds.origin.y + cursor_bounds.size.height - line_width },
                            size: Size { width: cursor_bounds.size.width, height: line_width },
                        },
                        cursor_color,
                    );
                } else if shape == CursorShape::Beam {
                    fill(
                        window,
                        Bounds { origin: cursor_bounds.origin, size: Size { width: line_width, height: cursor_bounds.size.height } },
                        cursor_color,
                    );
                } else {
                    // A block: solid, with the character under it drawn in the background color.
                    fill(window, cursor_bounds, cursor_color);
                    if cell.c != ' ' && cell.c != '\0' && !cell.flags.contains(Flags::HIDDEN) {
                        let font = Font {
                            family: self.font_family.clone().into(),
                            features: FontFeatures::default(),
                            fallbacks: None,
                            weight: if cell.flags.contains(Flags::BOLD) { FontWeight::BOLD } else { FontWeight::NORMAL },
                            style: if cell.flags.contains(Flags::ITALIC) { FontStyle::Italic } else { FontStyle::Normal },
                        };
                        let text: SharedString = cell.c.to_string().into();
                        let run = TextRun { len: text.len(), font, color: default_bg, background_color: None, underline: None, strikethrough: None };
                        let shaped = window.text_system().shape_line(text, self.font_size, &[run], None);
                        let _ = shaped.paint(cursor_bounds.origin, self.cell_height, window, cx);
                    }
                }
            }
        }
    }

    /// Draws text the system input method is still composing (what dictation has heard so far, or
    /// an accent waiting for its letter) at the cursor, underlined on a tinted background, so you
    /// can see it before it is sent to the shell.
    pub fn paint_composition(
        &self,
        text: &str,
        bounds: Bounds<Pixels>,
        padding: Edges<Pixels>,
        term: &Term<GpuiEventProxy>,
        window: &mut Window,
        cx: &mut App,
    ) {
        if text.is_empty() {
            return;
        }
        let colors = term.colors();
        let cursor = term.grid().cursor.point;
        let x = bounds.origin.x + padding.left + self.cell_width * (cursor.column.0 as f32);
        let y = bounds.origin.y + padding.top + self.cell_height * (cursor.line.0 as f32);
        let mut fg = self.palette.resolve(
            Color::Named(alacritty_terminal::vte::ansi::NamedColor::Foreground),
            colors,
        );
        let bg = self.palette.resolve(
            Color::Named(alacritty_terminal::vte::ansi::NamedColor::Background),
            colors,
        );

        let font = Font {
            family: self.font_family.clone().into(),
            features: FontFeatures::default(),
            fallbacks: None,
            weight: FontWeight::NORMAL,
            style: FontStyle::Normal,
        };
        let run = TextRun {
            len: text.len(),
            font,
            color: fg,
            background_color: None,
            underline: Some(UnderlineStyle {
                thickness: px(1.0),
                color: Some(fg),
                wavy: false,
            }),
            strikethrough: None,
        };
        let line = window
            .text_system()
            .shape_line(SharedString::from(text.to_string()), self.font_size, &[run], None);

        // A solid patch behind the text, so it does not mix with what is already on screen.
        fg.a = 0.22;
        let patch = Bounds {
            origin: Point { x, y },
            size: Size {
                width: line.width.max(self.cell_width),
                height: self.cell_height,
            },
        };
        window.paint_quad(quad(
            patch,
            px(0.0),
            bg,
            Edges::<Pixels>::default(),
            transparent_black(),
            Default::default(),
        ));
        window.paint_quad(quad(
            patch,
            px(0.0),
            fg,
            Edges::<Pixels>::default(),
            transparent_black(),
            Default::default(),
        ));
        let _ = line.paint(Point { x, y }, self.cell_height, window, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_renderer_creation() {
        let renderer = TerminalRenderer::new(
            "Fira Code".to_string(),
            px(14.0),
            1.2,
            ColorPalette::default(),
        );
        assert_eq!(renderer.font_family, "Fira Code");
        assert_eq!(renderer.font_size, px(14.0));
        assert_eq!(renderer.line_height_multiplier, 1.2);
    }

    #[test]
    fn test_background_rect_merge() {
        let black = Hsla::black();

        let rect1 = BackgroundRect {
            start_col: 0,
            end_col: 5,
            row: 0,
            color: black,
        };

        let rect2 = BackgroundRect {
            start_col: 5,
            end_col: 10,
            row: 0,
            color: black,
        };

        assert!(rect1.can_merge_with(&rect2));

        let rect3 = BackgroundRect {
            start_col: 5,
            end_col: 10,
            row: 1,
            color: black,
        };

        assert!(!rect1.can_merge_with(&rect3));
    }

    #[test]
    fn test_merge_backgrounds() {
        let renderer = TerminalRenderer::new(
            "monospace".to_string(),
            px(14.0),
            1.2,
            ColorPalette::default(),
        );
        let black = Hsla::black();

        let rects = vec![
            BackgroundRect {
                start_col: 0,
                end_col: 5,
                row: 0,
                color: black,
            },
            BackgroundRect {
                start_col: 5,
                end_col: 10,
                row: 0,
                color: black,
            },
        ];

        let merged = renderer.merge_backgrounds(rects);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].start_col, 0);
        assert_eq!(merged[0].end_col, 10);
    }
}
