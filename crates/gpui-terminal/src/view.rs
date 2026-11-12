//! Main terminal view component for GPUI.
//!
//! This module provides [`TerminalView`], the primary component for embedding terminals
//! in GPUI applications. It manages:
//!
//! - **I/O Streams**: Accepts arbitrary [`Read`]/[`Write`]
//!   streams, allowing integration with any PTY implementation
//! - **Event Handling**: Keyboard and mouse input, with configurable callbacks
//! - **Rendering**: Efficient canvas-based rendering via [`TerminalRenderer`]
//! - **Configuration**: Font, colors, dimensions, and padding via [`TerminalConfig`]
//!
//! # Architecture
//!
//! The terminal uses a push-based async I/O architecture:
//!
//! 1. A background thread reads bytes from the PTY stdout in 4KB chunks
//! 2. Bytes are sent through a [flume](https://docs.rs/flume) channel to an async task
//! 3. The async task processes bytes through the VTE parser and calls `cx.notify()`
//! 4. GPUI repaints the terminal with the updated grid
//!
//! This approach ensures the terminal only wakes when data arrives, avoiding polling.
//!
//! # Thread Safety
//!
//! - [`TerminalView`] itself is not `Send` (it contains GPUI handles)
//! - The stdin writer is wrapped in `Arc<parking_lot::Mutex<>>` for thread-safe writes
//! - Callbacks ([`ResizeCallback`], [`KeyHandler`]) must be `Send + Sync`
//!
//! # Example
//!
//! ```ignore
//! use gpui::{Context, Edges, px};
//! use gpui_terminal::{ColorPalette, TerminalConfig, TerminalView};
//!
//! // In a GPUI window context:
//! let terminal = cx.new(|cx| {
//!     TerminalView::new(pty_writer, pty_reader, TerminalConfig::default(), cx)
//!         .with_resize_callback(move |cols, rows| {
//!             // Notify PTY of new dimensions
//!         })
//!         .with_exit_callback(|_, cx| {
//!             cx.quit();
//!         })
//! });
//!
//! // Focus the terminal to receive keyboard input
//! terminal.read(cx).focus_handle().focus(window);
//! ```

use crate::colors::ColorPalette;
use crate::event::{GpuiEventProxy, TerminalEvent};
use crate::input::keystroke_to_bytes;
use crate::links::{self, Link};
use crate::render::{PaintExtras, TerminalRenderer};
use crate::terminal::TerminalState;
use alacritty_terminal::index::Point as AlacPoint;
use gpui::prelude::FluentBuilder;
use gpui::{Edges, *};
use std::io::{Read, Write};
use std::ops::{Range, RangeInclusive};
use std::sync::Arc;
use std::time::{Duration, Instant};
use std::sync::mpsc;
use std::thread;

/// Configuration for terminal creation and runtime updates.
///
/// This struct defines the terminal's appearance and behavior, including
/// grid dimensions, font settings, scrollback buffer, and color scheme.
///
/// # Default Values
///
/// | Field | Default |
/// |-------|---------|
/// | `cols` | 80 |
/// | `rows` | 24 |
/// | `font_family` | "monospace" |
/// | `font_size` | 14px |
/// | `scrollback` | 10000 |
/// | `line_height_multiplier` | 1.2 |
/// | `padding` | 0px all sides |
/// | `colors` | Default palette |
///
/// # Example
///
/// ```ignore
/// use gpui::{Edges, px};
/// use gpui_terminal::{ColorPalette, TerminalConfig};
///
/// let config = TerminalConfig {
///     cols: 120,
///     rows: 40,
///     font_family: "JetBrains Mono".into(),
///     font_size: px(13.0),
///     scrollback: 50000,
///     line_height_multiplier: 1.1,
///     padding: Edges::all(px(10.0)),
///     colors: ColorPalette::builder()
///         .background(0x1a, 0x1a, 0x1a)
///         .foreground(0xe0, 0xe0, 0xe0)
///         .build(),
/// };
/// ```
///
/// # Runtime Updates
///
/// Configuration can be updated at runtime via [`TerminalView::update_config`].
/// This is useful for implementing features like dynamic font sizing:
///
/// ```ignore
/// terminal.update(cx, |terminal, cx| {
///     let mut config = terminal.config().clone();
///     config.font_size += px(1.0);
///     terminal.update_config(config, cx);
/// });
/// ```
#[derive(Clone, Debug)]
pub struct TerminalConfig {
    /// Number of columns (character width) in the terminal
    pub cols: usize,

    /// Number of rows (lines) in the terminal
    pub rows: usize,

    /// Font family name (e.g., "Fira Code", "JetBrains Mono")
    pub font_family: String,

    /// Font size in pixels
    pub font_size: Pixels,

    /// Maximum number of scrollback lines to keep in history
    pub scrollback: usize,

    /// The cursor shape programs get unless they ask for another.
    pub cursor_shape: alacritty_terminal::vte::ansi::CursorShape,

    /// The cursor blinks while the terminal has the keyboard focus (programs can ask for it too).
    pub cursor_blink: bool,

    /// Multiplier for line height to accommodate tall glyphs (e.g., nerd fonts)
    /// Default is 1.2 (20% extra height)
    pub line_height_multiplier: f32,

    /// Padding around the terminal content (top, right, bottom, left)
    /// The padding area renders with the terminal's background color
    pub padding: Edges<Pixels>,

    /// Color palette for terminal colors (16 ANSI colors, 256 extended colors,
    /// foreground, background, and cursor colors)
    pub colors: ColorPalette,
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            cols: 80,
            rows: 24,
            font_family: "monospace".into(),
            font_size: px(14.0),
            scrollback: 10000,
            cursor_shape: alacritty_terminal::vte::ansi::CursorShape::Block,
            cursor_blink: false,
            line_height_multiplier: 1.2,
            padding: Edges::all(px(0.0)),
            colors: ColorPalette::default(),
        }
    }
}

/// Callback type for PTY resize notifications.
///
/// This callback is invoked when the terminal grid dimensions change,
/// typically due to window resizing. The callback receives the new
/// column and row counts.
///
/// # Arguments
///
/// * `cols` - New number of columns (characters wide)
/// * `rows` - New number of rows (lines tall)
///
/// # Thread Safety
///
/// This callback must be `Send + Sync` as it may be called from the render thread.
///
/// # Example
///
/// ```ignore
/// use portable_pty::PtySize;
///
/// let pty = Arc::new(Mutex::new(pty_master));
/// let pty_clone = pty.clone();
///
/// terminal.with_resize_callback(move |cols, rows| {
///     pty_clone.lock().resize(PtySize {
///         cols: cols as u16,
///         rows: rows as u16,
///         pixel_width: 0,
///         pixel_height: 0,
///     }).ok();
/// });
/// ```
pub type ResizeCallback = Box<dyn Fn(usize, usize) + Send + Sync>;

/// Callback type for key event interception.
///
/// This callback is invoked before the terminal processes a key event,
/// allowing you to intercept and handle specific key combinations.
///
/// # Arguments
///
/// * `event` - The key down event from GPUI
///
/// # Returns
///
/// * `true` - Consume the event (terminal will not process it)
/// * `false` - Let the terminal handle the event normally
///
/// # Thread Safety
///
/// This callback must be `Send + Sync`.
///
/// # Example
///
/// ```ignore
/// terminal.with_key_handler(|event| {
///     let keystroke = &event.keystroke;
///
///     // Intercept Ctrl++ for font size increase
///     if keystroke.modifiers.control && (keystroke.key == "+" || keystroke.key == "=") {
///         // Handle font size increase
///         return true; // Consume the event
///     }
///
///     // Intercept Ctrl+- for font size decrease
///     if keystroke.modifiers.control && keystroke.key == "-" {
///         // Handle font size decrease
///         return true;
///     }
///
///     false // Let terminal handle all other keys
/// });
/// ```
pub type KeyHandler = Box<dyn Fn(&KeyDownEvent) -> bool + Send + Sync>;

/// Callback for terminal bell events.
///
/// This callback is invoked when the terminal bell is triggered (BEL character,
/// ASCII 0x07), allowing you to play a sound or show a visual indicator.
///
/// # Arguments
///
/// * `window` - The GPUI window
/// * `cx` - The context for the TerminalView
///
/// # Example
///
/// ```ignore
/// terminal.with_bell_callback(|window, cx| {
///     // Option 1: Visual bell (flash the window or show an indicator)
///     // Option 2: Play a sound
///     // Option 3: Notify the user via system notification
/// });
/// ```
pub type BellCallback = Box<dyn Fn(&mut Window, &mut Context<TerminalView>)>;

/// Callback for terminal title changes.
///
/// This callback is invoked when the terminal title changes via escape sequences
/// (OSC 0, OSC 2), allowing you to update the window or tab title.
///
/// # Arguments
///
/// * `window` - The GPUI window
/// * `cx` - The context for the TerminalView
/// * `title` - The new title string
///
/// # Example
///
/// ```ignore
/// terminal.with_title_callback(|window, cx, title| {
///     // Update the window title
///     // Or update a tab label in a tabbed interface
///     println!("Terminal title changed to: {}", title);
/// });
/// ```
pub type TitleCallback = Box<dyn Fn(&mut Window, &mut Context<TerminalView>, &str)>;

/// Callback for clipboard store requests.
///
/// This callback is invoked when the terminal wants to store data to the clipboard
/// via OSC 52 escape sequence. Applications like tmux and vim can use this to
/// copy text to the system clipboard.
///
/// # Arguments
///
/// * `window` - The GPUI window
/// * `cx` - The context for the TerminalView
/// * `text` - The text to store in the clipboard
///
/// # Example
///
/// ```ignore
/// use gpui_terminal::Clipboard;
///
/// terminal.with_clipboard_store_callback(|window, cx, text| {
///     if let Ok(mut clipboard) = Clipboard::new() {
///         clipboard.copy(text).ok();
///     }
/// });
/// ```
pub type ClipboardStoreCallback = Box<dyn Fn(&mut Window, &mut Context<TerminalView>, &str)>;

/// Callback for terminal exit events.
///
/// This callback is invoked when the terminal process exits (e.g., shell exits,
/// process terminates). This is detected when the PTY reader reaches EOF.
///
/// # Arguments
///
/// * `window` - The GPUI window
/// * `cx` - The context for the TerminalView
///
/// # Example
///
/// ```ignore
/// terminal.with_exit_callback(|window, cx| {
///     // Option 1: Quit the application
///     cx.quit();
///
///     // Option 2: Close this terminal tab/pane
///     // terminal_manager.close_terminal(terminal_id);
///
///     // Option 3: Show an exit message
///     // show_notification("Terminal exited");
/// });
/// ```
pub type ExitCallback = Box<dyn Fn(&mut Window, &mut Context<TerminalView>)>;

/// The main terminal view component for GPUI applications.
///
/// `TerminalView` is a GPUI entity that implements the [`Render`] trait,
/// providing a complete terminal emulator that can be embedded in any GPUI application.
///
/// # Responsibilities
///
/// - **Terminal State**: Manages the grid, cursor, and colors via [`TerminalState`]
/// - **I/O Streams**: Reads from PTY stdout and writes to PTY stdin
/// - **Event Handling**: Processes keyboard, mouse, and resize events
/// - **Rendering**: Paints text, backgrounds, and cursor via [`TerminalRenderer`]
/// - **Callbacks**: Dispatches events to user-provided callbacks
///
/// # Creating a Terminal
///
/// Use [`TerminalView::new`] within a GPUI entity context:
///
/// ```ignore
/// let terminal = cx.new(|cx| {
///     TerminalView::new(writer, reader, config, cx)
///         .with_resize_callback(resize_callback)
///         .with_exit_callback(|_, cx| cx.quit())
/// });
/// ```
///
/// # Focus
///
/// The terminal must be focused to receive keyboard input:
///
/// ```ignore
/// terminal.read(cx).focus_handle().focus(window);
/// ```
///
/// # Callbacks
///
/// Configure behavior through builder methods:
///
/// - [`with_resize_callback`](Self::with_resize_callback) - PTY size changes
/// - [`with_exit_callback`](Self::with_exit_callback) - Process exit
/// - [`with_key_handler`](Self::with_key_handler) - Key event interception
/// - [`with_bell_callback`](Self::with_bell_callback) - Terminal bell
/// - [`with_title_callback`](Self::with_title_callback) - Title changes
/// - [`with_clipboard_store_callback`](Self::with_clipboard_store_callback) - Clipboard writes
///
/// # Thread Safety
///
/// `TerminalView` is not `Send` as it contains GPUI handles. The stdin writer
/// is internally wrapped in `Arc<parking_lot::Mutex<>>` for safe concurrent access.
/// Where the terminal was last painted and how big its cells are, so the mouse can find the cell
/// under the pointer.
#[derive(Clone, Copy, Default)]
struct PaintInfo {
    origin: Point<Pixels>,
    cell_width: Pixels,
    cell_height: Pixels,
    /// The whole painted area, to tell whether the pointer is over the terminal.
    bounds: Bounds<Pixels>,
}

/// Most matches a search keeps; more than this is not useful to step through.
const MAX_MATCHES: usize = 5000;

/// How often the matches are searched again while output keeps arriving.
const SEARCH_REFRESH: Duration = Duration::from_millis(300);

/// Find in the output (Cmd+F, Ctrl+Shift+F elsewhere).
#[derive(Default)]
struct Search {
    open: bool,
    query: String,
    matches: Arc<Vec<RangeInclusive<AlacPoint>>>,
    active: Option<usize>,
    /// New output arrived since the matches were found.
    dirty: bool,
    computed: Option<Instant>,
}

/// The query as a pattern that matches exactly that text.
fn escape_regex(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 2);
    for c in text.chars() {
        if "\\.+*?()|[]{}^$#&-~".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// The key that makes links work: Cmd on a Mac, Ctrl elsewhere.
fn link_modifier(m: &Modifiers) -> bool {
    if cfg!(target_os = "macos") { m.platform } else { m.control }
}

pub struct TerminalView {
    /// The terminal state managing the grid and VTE parser
    state: TerminalState,

    /// The renderer for drawing terminal content
    renderer: TerminalRenderer,

    /// Focus handle for keyboard event handling
    focus_handle: FocusHandle,

    /// Writer for sending input to the terminal process
    stdin_writer: Arc<parking_lot::Mutex<Box<dyn Write + Send>>>,

    /// Receiver for terminal events from the event proxy
    event_rx: mpsc::Receiver<TerminalEvent>,

    /// Configuration used to create this terminal
    config: TerminalConfig,

    /// Text the system input method is composing (dead keys, dictation in progress). It is only
    /// committed to the shell when the input method says so.
    marked_text: Option<String>,

    /// Option acts as Meta (ESC prefix) instead of typing characters. Off by default, like
    /// Terminal.app, so Option+key types @ # [ ] and accents on Italian and other layouts.
    option_as_meta: bool,

    /// Where the terminal was last painted (shared with the paint closure).
    paint_info: Arc<parking_lot::Mutex<PaintInfo>>,

    /// A mouse selection is being dragged.
    selecting: bool,

    /// Pixels scrolled by the wheel that did not add up to a whole line yet.
    scroll_accum: f32,

    /// Copy the selection to the clipboard as soon as it is made.
    copy_on_select: bool,

    /// Phase of the blinking cursor.
    blink_on: bool,

    /// The terminal has the keyboard focus (set on every render): only a focused cursor blinks.
    focused_now: bool,

    #[allow(dead_code)]
    _blink_task: Task<()>,

    /// The link under the pointer while Cmd (Ctrl) is held.
    hover_link: Option<Link>,

    /// Find in the output.
    search: Search,

    /// The title the program set (OSC 0 or 2).
    title: String,

    /// Async task that reads bytes and notifies the view (push-based)
    #[allow(dead_code)]
    _reader_task: Task<()>,

    /// Callback to notify the PTY about size changes
    resize_callback: Option<Arc<ResizeCallback>>,

    /// Optional callback to intercept key events before terminal processing
    key_handler: Option<Arc<KeyHandler>>,

    /// Callback for terminal bell events
    bell_callback: Option<BellCallback>,

    /// Callback for terminal title changes
    title_callback: Option<TitleCallback>,

    /// Callback for clipboard store requests
    clipboard_store_callback: Option<ClipboardStoreCallback>,

    /// Callback for terminal exit events
    exit_callback: Option<ExitCallback>,
}

/// Whether a key press types text, which belongs to the system input method (letters, digits,
/// symbols, accents, dictation). Enter and Tab carry "\n" and "\t" as their character, but the
/// input method never delivers those as text (macOS runs them as editing commands), so they must
/// go straight to the shell or the key would do nothing.
fn types_text(ks: &Keystroke, option_as_meta: bool) -> bool {
    let m = &ks.modifiers;
    ks.key_char.as_deref().is_some_and(|c| c.chars().any(|ch| !ch.is_control()))
        && !m.control
        && !m.platform
        && !m.function
        && !(m.alt && option_as_meta)
}

#[cfg(test)]
mod key_tests {
    // No glob import: gpui exports its own `test` macro, which would shadow the built-in one.
    use super::types_text;
    use gpui::Keystroke;

    fn key(key: &str, key_char: Option<&str>) -> Keystroke {
        Keystroke { modifiers: Default::default(), key: key.into(), key_char: key_char.map(Into::into) }
    }

    #[test]
    fn enter_and_tab_go_to_the_shell_not_to_the_input_method() {
        assert!(!types_text(&key("enter", Some("\n")), false));
        assert!(!types_text(&key("tab", Some("\t")), false));
        assert!(!types_text(&key("backspace", None), false));
    }

    #[test]
    fn letters_and_space_go_to_the_input_method() {
        assert!(types_text(&key("a", Some("a")), false));
        assert!(types_text(&key("space", Some(" ")), false));
        assert!(types_text(&key("@", Some("@")), false));
    }

    #[test]
    fn option_types_characters_unless_it_is_meta() {
        let mut alt = key("a", Some("å"));
        alt.modifiers.alt = true;
        assert!(types_text(&alt, false));
        assert!(!types_text(&alt, true));
    }

    #[test]
    fn control_and_command_chords_go_to_the_shell() {
        let mut ctrl_c = key("c", Some("c"));
        ctrl_c.modifiers.control = true;
        assert!(!types_text(&ctrl_c, false));
    }
}

impl TerminalView {
    /// Create a new terminal with provided I/O streams.
    ///
    /// This method initializes a new terminal emulator with the given stdin writer
    /// and stdout reader. It spawns a background task to read from stdout and
    /// process incoming bytes through the VTE parser.
    ///
    /// # Arguments
    ///
    /// * `stdin_writer` - Writer for sending input bytes to the terminal process
    /// * `stdout_reader` - Reader for receiving output bytes from the terminal process
    /// * `config` - Terminal configuration (dimensions, font, etc.)
    /// * `cx` - GPUI context for this view
    ///
    /// # Returns
    ///
    /// A new `TerminalView` instance ready to be rendered.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// // In a GPUI window context:
    /// let terminal = cx.new(|cx| {
    ///     TerminalView::new(stdin_writer, stdout_reader, TerminalConfig::default(), cx)
    /// });
    /// ```
    pub fn new<W, R>(
        stdin_writer: W,
        stdout_reader: R,
        config: TerminalConfig,
        cx: &mut Context<Self>,
    ) -> Self
    where
        W: Write + Send + 'static,
        R: Read + Send + 'static,
    {
        // Create event channel for terminal events
        let (event_tx, event_rx) = mpsc::channel();

        // Clone event_tx for the reader task to send Exit event when PTY closes
        let exit_event_tx = event_tx.clone();

        // Create event proxy for alacritty
        let event_proxy = GpuiEventProxy::new(event_tx);

        // Create terminal state
        let state = TerminalState::with_options(
            config.cols,
            config.rows,
            event_proxy,
            config.scrollback,
            config.cursor_shape,
            config.cursor_blink,
        );

        // Create renderer with font settings and color palette
        let renderer = TerminalRenderer::new(
            config.font_family.clone(),
            config.font_size,
            config.line_height_multiplier,
            config.colors.clone(),
        );

        // Create focus handle
        let focus_handle = cx.focus_handle();

        // Wrap stdin writer in Arc<Mutex> for thread-safe access
        let stdin_writer = Arc::new(parking_lot::Mutex::new(
            Box::new(stdin_writer) as Box<dyn Write + Send>
        ));

        // Create async channel for bytes (push-based notification)
        // Using flume instead of smol::channel because flume is executor-agnostic
        // and properly wakes GPUI's async executor when data arrives
        let (bytes_tx, bytes_rx) = flume::unbounded::<Vec<u8>>();

        // Spawn background thread to read from stdout
        // This thread sends bytes through the async channel
        thread::spawn(move || {
            Self::read_stdout_blocking(stdout_reader, bytes_tx);
        });

        // Spawn async task that awaits on the channel and notifies the view
        // This is push-based: the task blocks until bytes arrive, then immediately notifies
        let reader_task = cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            loop {
                // Wait for bytes from the background reader (blocks until data arrives)
                match bytes_rx.recv_async().await {
                    Ok(bytes) => {
                        // Process bytes and notify the view
                        let result = this.update(cx, |view: &mut Self, cx: &mut Context<Self>| {
                            view.state.process_bytes(&bytes);
                            if view.search.open {
                                view.search.dirty = true;
                            }
                            cx.notify();
                        });
                        if result.is_err() {
                            // View was dropped, exit
                            break;
                        }
                    }
                    Err(_) => {
                        // Channel closed - PTY has finished, send Exit event
                        let _ = exit_event_tx.send(TerminalEvent::Exit);
                        // Notify view to process the Exit event
                        let _ = this.update(cx, |_view, cx: &mut Context<Self>| {
                            cx.notify();
                        });
                        break;
                    }
                }
            }
        });

        let blink_task = cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            loop {
                cx.background_executor().timer(Duration::from_millis(530)).await;
                if this.update(cx, |view: &mut Self, cx: &mut Context<Self>| view.blink_tick(cx)).is_err() {
                    break;
                }
            }
        });

        Self {
            state,
            renderer,
            focus_handle,
            stdin_writer,
            event_rx,
            config,
            marked_text: None,
            option_as_meta: false,
            paint_info: Arc::new(parking_lot::Mutex::new(PaintInfo::default())),
            selecting: false,
            scroll_accum: 0.0,
            copy_on_select: false,
            blink_on: true,
            focused_now: false,
            _blink_task: blink_task,
            hover_link: None,
            search: Search::default(),
            title: String::new(),
            _reader_task: reader_task,
            resize_callback: None,
            key_handler: None,
            bell_callback: None,
            title_callback: None,
            clipboard_store_callback: None,
            exit_callback: None,
        }
    }

    /// Copy the selection to the clipboard as soon as it is made.
    pub fn with_copy_on_select(mut self, on: bool) -> Self {
        self.copy_on_select = on;
        self
    }

    /// Changes the Option behavior of a terminal that is already running.
    pub fn set_option_as_meta(&mut self, on: bool) {
        self.option_as_meta = on;
    }

    /// Changes copy-on-select of a terminal that is already running.
    pub fn set_copy_on_select(&mut self, on: bool) {
        self.copy_on_select = on;
    }

    /// Make Option act as Meta (sends ESC before the key) instead of typing characters.
    pub fn with_option_as_meta(mut self, on: bool) -> Self {
        self.option_as_meta = on;
        self
    }

    /// Set a callback to be invoked when the terminal is resized.
    ///
    /// This callback should resize the underlying PTY to match the new dimensions.
    /// The callback receives (cols, rows) as arguments.
    ///
    /// # Arguments
    ///
    /// * `callback` - A function that will be called with (cols, rows) on resize
    pub fn with_resize_callback(
        mut self,
        callback: impl Fn(usize, usize) + Send + Sync + 'static,
    ) -> Self {
        self.resize_callback = Some(Arc::new(Box::new(callback)));
        self
    }

    /// Set a callback to intercept key events before terminal processing.
    ///
    /// The callback receives the key event and should return `true` to consume
    /// the event (prevent the terminal from processing it), or `false` to allow
    /// normal terminal processing.
    ///
    /// # Arguments
    ///
    /// * `handler` - A function that receives key events and returns whether to consume them
    ///
    /// # Example
    ///
    /// ```ignore
    /// terminal.with_key_handler(|event| {
    ///     // Handle Ctrl++ to increase font size
    ///     if event.keystroke.modifiers.control && event.keystroke.key == "+" {
    ///         // Handle the event
    ///         return true; // Consume the event
    ///     }
    ///     false // Let terminal handle it
    /// })
    /// ```
    pub fn with_key_handler(
        mut self,
        handler: impl Fn(&KeyDownEvent) -> bool + Send + Sync + 'static,
    ) -> Self {
        self.key_handler = Some(Arc::new(Box::new(handler)));
        self
    }

    /// Set a callback to be invoked when the terminal bell is triggered.
    ///
    /// The callback receives a mutable reference to the window and context,
    /// allowing you to play a sound or show a visual indicator.
    ///
    /// # Arguments
    ///
    /// * `callback` - A function that will be called when the bell is triggered
    ///
    /// # Example
    ///
    /// ```ignore
    /// terminal.with_bell_callback(|window, cx| {
    ///     // Play a sound or flash the screen
    /// })
    /// ```
    pub fn with_bell_callback(
        mut self,
        callback: impl Fn(&mut Window, &mut Context<TerminalView>) + 'static,
    ) -> Self {
        self.bell_callback = Some(Box::new(callback));
        self
    }

    /// Set a callback to be invoked when the terminal title changes.
    ///
    /// The callback receives a mutable reference to the window and context,
    /// along with the new title string.
    ///
    /// # Arguments
    ///
    /// * `callback` - A function that will be called with the new title
    ///
    /// # Example
    ///
    /// ```ignore
    /// terminal.with_title_callback(|window, cx, title| {
    ///     // Update window title or tab title
    /// })
    /// ```
    pub fn with_title_callback(
        mut self,
        callback: impl Fn(&mut Window, &mut Context<TerminalView>, &str) + 'static,
    ) -> Self {
        self.title_callback = Some(Box::new(callback));
        self
    }

    /// Set a callback to be invoked when the terminal wants to store data to the clipboard.
    ///
    /// The callback receives a mutable reference to the window and context,
    /// along with the text to store. This is typically triggered by OSC 52 escape sequences.
    ///
    /// # Arguments
    ///
    /// * `callback` - A function that will be called with the text to store
    ///
    /// # Example
    ///
    /// ```ignore
    /// terminal.with_clipboard_store_callback(|window, cx, text| {
    ///     // Store text to system clipboard
    /// })
    /// ```
    pub fn with_clipboard_store_callback(
        mut self,
        callback: impl Fn(&mut Window, &mut Context<TerminalView>, &str) + 'static,
    ) -> Self {
        self.clipboard_store_callback = Some(Box::new(callback));
        self
    }

    /// Set a callback to be invoked when the terminal process exits.
    ///
    /// The callback receives a mutable reference to the window and context,
    /// allowing you to close the terminal view or show an exit message.
    ///
    /// # Arguments
    ///
    /// * `callback` - A function that will be called when the process exits
    ///
    /// # Example
    ///
    /// ```ignore
    /// terminal.with_exit_callback(|window, cx| {
    ///     // Close the terminal tab or show exit message
    /// })
    /// ```
    pub fn with_exit_callback(
        mut self,
        callback: impl Fn(&mut Window, &mut Context<TerminalView>) + 'static,
    ) -> Self {
        self.exit_callback = Some(Box::new(callback));
        self
    }

    /// Background thread that reads from stdout.
    ///
    /// This function runs in a background thread, continuously reading bytes
    /// from the stdout reader and sending them through the async channel.
    /// The async channel allows the main async task to be woken up immediately
    /// when data arrives (push-based).
    fn read_stdout_blocking<R: Read + Send + 'static>(
        mut stdout_reader: R,
        bytes_tx: flume::Sender<Vec<u8>>,
    ) {
        let mut buffer = [0u8; 4096];

        loop {
            match stdout_reader.read(&mut buffer) {
                Ok(0) => {
                    // EOF - channel will be dropped, signaling completion
                    break;
                }
                Ok(n) => {
                    // Send bytes to the async task
                    let bytes = buffer[..n].to_vec();
                    if bytes_tx.send(bytes).is_err() {
                        break; // Channel closed
                    }
                }
                Err(_) => {
                    // Read error
                    break;
                }
            }
        }
    }

    /// Handle keyboard input events.
    ///
    /// Converts GPUI keystrokes to terminal escape sequences and writes them
    /// to the stdin writer. If a key handler is set and returns true, the event
    /// is consumed and not sent to the terminal.
    fn on_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        // Check if key handler wants to consume this event
        if let Some(ref handler) = self.key_handler
            && handler(event)
        {
            return; // Event consumed by handler
        }

        let ks = &event.keystroke;
        let m = &ks.modifiers;
        // Copy, paste and find: Cmd+C / V / F on a Mac, Ctrl+Shift+C / V / F elsewhere (plain
        // Ctrl+C is the interrupt and must reach the shell).
        let chord = |key: &str| {
            ks.key == key
                && if cfg!(target_os = "macos") {
                    m.platform && !m.shift && !m.control && !m.alt
                } else {
                    m.control && m.shift && !m.alt
                }
        };
        if chord("f") {
            self.search_open(cx);
            cx.stop_propagation();
            return;
        }
        if self.search.open && self.search_key(event, chord("v"), cx) {
            return;
        }
        if chord("c") {
            self.copy_selection(cx);
            cx.stop_propagation();
            return;
        }
        if chord("v") {
            self.paste_clipboard(cx);
            cx.stop_propagation();
            return;
        }
        // Keys that type text (letters, digits, symbols, Option+key on a layout that has them,
        // dead keys) belong to the system input method: it composes accents and delivers the
        // result to `replace_text_in_range`. Writing them here too would type everything twice.
        if types_text(ks, self.option_as_meta) {
            return; // propagate: GPUI hands the key to the input method
        }
        // While finding, the other keys do not reach the shell.
        if self.search.open {
            cx.stop_propagation();
            return;
        }

        if let Some(bytes) = keystroke_to_bytes(&event.keystroke, self.state.mode()) {
            self.on_user_input();
            self.write_to_shell(&bytes);
            cx.stop_propagation(); // handled: do not feed it to the input method as well
        }
    }

    /// Sends bytes to the shell.
    fn write_to_shell(&self, bytes: &[u8]) {
        let mut writer = self.stdin_writer.lock();
        let _ = writer.write_all(bytes);
        let _ = writer.flush();
    }

    /// Where the text cursor is on screen, for placing the dictation and accent popups.
    fn cursor_bounds(&self, element_bounds: Bounds<Pixels>) -> Bounds<Pixels> {
        use alacritty_terminal::grid::Dimensions;
        let term = self.state.term_arc();
        let term = term.lock();
        let cursor = term.grid().cursor.point;
        let font: f32 = self.config.font_size.into();
        let cell_w = if f32::from(self.renderer.cell_width) > 1.0 {
            self.renderer.cell_width
        } else {
            px(font * 0.6)
        };
        let cell_h = if f32::from(self.renderer.cell_height) > 1.0 {
            self.renderer.cell_height
        } else {
            px(font * self.config.line_height_multiplier)
        };
        let _ = term.columns();
        let padding = self.config.padding;
        Bounds {
            origin: point(
                element_bounds.origin.x + padding.left + cell_w * cursor.column.0 as f32,
                element_bounds.origin.y + padding.top + cell_h * cursor.line.0 as f32,
            ),
            size: size(cell_w, cell_h),
        }
    }

    /// Typing, pasting or any key that reaches the shell: jump back to the bottom of the output
    /// and drop the selection, like every terminal.
    fn on_user_input(&mut self) {
        use alacritty_terminal::grid::Scroll;
        self.blink_on = true;
        let term = self.state.term_arc();
        let mut term = term.lock();
        term.scroll_display(Scroll::Bottom);
        term.selection = None;
    }

    /// Copies the selected text to the clipboard.
    fn copy_selection(&mut self, cx: &mut Context<Self>) {
        let text = self.state.term_arc().lock().selection_to_string();
        if let Some(text) = text.filter(|t| !t.is_empty()) {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    /// Pastes the clipboard. Programs that ask for it get the text marked as a paste, so an editor
    /// does not treat it as typing.
    fn paste_clipboard(&mut self, cx: &mut Context<Self>) {
        use alacritty_terminal::term::TermMode;
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else { return };
        if text.is_empty() {
            return;
        }
        self.on_user_input();
        let mut text = text.replace("\r\n", "\r").replace('\n', "\r");
        let mut bytes = Vec::new();
        if self.state.mode().contains(TermMode::BRACKETED_PASTE) {
            // Never let the pasted text end the paste early.
            text = text.replace("\x1b[201~", "");
            bytes.extend_from_slice(b"\x1b[200~");
            bytes.extend_from_slice(text.as_bytes());
            bytes.extend_from_slice(b"\x1b[201~");
        } else {
            bytes.extend_from_slice(text.as_bytes());
        }
        self.write_to_shell(&bytes);
    }

    /// The grid cell under a pixel position, and which half of the cell it is in.
    fn grid_point(
        &self,
        position: Point<Pixels>,
    ) -> (alacritty_terminal::index::Point, alacritty_terminal::index::Side) {
        use alacritty_terminal::grid::Dimensions;
        use alacritty_terminal::index::{Column, Line, Point as AlacPoint, Side};
        let info = *self.paint_info.lock();
        let term = self.state.term_arc();
        let term = term.lock();
        let (cols, rows) = (term.columns() as f32, term.screen_lines() as f32);
        let offset = term.grid().display_offset() as i32;
        let cell_w: f32 = info.cell_width.into();
        let cell_h: f32 = info.cell_height.into();
        if cell_w <= 0.0 || cell_h <= 0.0 {
            return (AlacPoint::new(Line(0), Column(0)), Side::Left);
        }
        let x: f32 = (position.x - info.origin.x).into();
        let y: f32 = (position.y - info.origin.y).into();
        let col = (x / cell_w).floor().clamp(0.0, cols - 1.0);
        let row = (y / cell_h).floor().clamp(0.0, rows - 1.0);
        let side = if x - col * cell_w < cell_w / 2.0 { Side::Left } else { Side::Right };
        (AlacPoint::new(Line(row as i32 - offset), Column(col as usize)), side)
    }

    /// The point as the cell on screen, for reports to programs that track the mouse.
    fn screen_point(&self, point: alacritty_terminal::index::Point) -> alacritty_terminal::index::Point {
        let offset = self.state.term_arc().lock().grid().display_offset() as i32;
        alacritty_terminal::index::Point::new(
            alacritty_terminal::index::Line(point.line.0 + offset),
            point.column,
        )
    }

    fn tracks_mouse(&self) -> bool {
        use alacritty_terminal::term::TermMode;
        self.state.mode().intersects(
            TermMode::MOUSE_REPORT_CLICK | TermMode::MOUSE_MOTION | TermMode::MOUSE_DRAG,
        )
    }

    /// Handle mouse down events: a click focuses the terminal and starts a selection, or goes to
    /// the program when it asks for the mouse (hold Shift to select anyway).
    fn on_mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        use alacritty_terminal::selection::{Selection, SelectionType};
        window.focus(&self.focus_handle);
        if link_modifier(&event.modifiers) {
            if let Some(link) = self.link_under(event.position) {
                cx.open_url(&link.url);
                return;
            }
        }
        let (point, side) = self.grid_point(event.position);

        if self.tracks_mouse() && !event.modifiers.shift {
            let mods = crate::mouse::encode_modifiers(false, event.modifiers.alt, event.modifiers.control);
            let screen = self.screen_point(point);
            if let Some(bytes) = crate::mouse::mouse_button_report(
                MouseButton::Left,
                true,
                screen,
                mods,
                self.state.mode(),
            ) {
                self.write_to_shell(&bytes);
            }
            cx.notify();
            return;
        }

        let kind = match event.click_count {
            1 => SelectionType::Simple,
            2 => SelectionType::Semantic,
            _ => SelectionType::Lines,
        };
        self.state.term_arc().lock().selection = Some(Selection::new(kind, point, side));
        self.selecting = true;
        cx.notify();
    }

    /// Handle mouse up events: the selection is done.
    fn on_mouse_up(&mut self, event: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if self.tracks_mouse() && !self.selecting {
            let (point, _) = self.grid_point(event.position);
            let mods = crate::mouse::encode_modifiers(false, event.modifiers.alt, event.modifiers.control);
            let screen = self.screen_point(point);
            if let Some(bytes) = crate::mouse::mouse_button_report(
                MouseButton::Left,
                false,
                screen,
                mods,
                self.state.mode(),
            ) {
                self.write_to_shell(&bytes);
            }
            return;
        }
        if self.selecting {
            self.selecting = false;
            // A click without dragging leaves nothing selected.
            let empty = {
                let term = self.state.term_arc();
                let term = term.lock();
                term.selection.as_ref().is_some_and(|s| s.is_empty())
            };
            if empty {
                self.state.term_arc().lock().selection = None;
            } else if self.copy_on_select {
                self.copy_selection(cx);
            }
            cx.notify();
        }
    }

    /// Handle mouse move events: drag the selection.
    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let hover = if link_modifier(&event.modifiers) && !self.selecting {
            self.link_under(event.position)
        } else {
            None
        };
        if hover != self.hover_link {
            self.hover_link = hover;
            cx.notify();
        }
        if self.selecting && event.pressed_button == Some(MouseButton::Left) {
            let (point, side) = self.grid_point(event.position);
            if let Some(selection) = self.state.term_arc().lock().selection.as_mut() {
                selection.update(point, side);
            }
            cx.notify();
        } else if self.selecting {
            self.selecting = false; // the button was released outside the window
        }
    }

    /// Handle the mouse wheel: scroll back through the output, or hand the wheel to a program that
    /// wants it (a pager, vim, tmux).
    fn on_scroll(&mut self, event: &ScrollWheelEvent, _window: &mut Window, cx: &mut Context<Self>) {
        use alacritty_terminal::grid::Scroll;
        use alacritty_terminal::term::TermMode;
        let info = *self.paint_info.lock();
        let cell_h: f32 = info.cell_height.into();
        if cell_h <= 0.0 {
            return;
        }
        let delta = event.delta.pixel_delta(info.cell_height);
        let dy: f32 = delta.y.into();
        self.scroll_accum += dy / cell_h;
        let lines = self.scroll_accum.trunc() as i32;
        if lines == 0 {
            return;
        }
        self.scroll_accum -= lines as f32;
        let mode = self.state.mode();

        if self.tracks_mouse() && !event.modifiers.shift {
            let (point, _) = self.grid_point(event.position);
            let mods = crate::mouse::encode_modifiers(false, event.modifiers.alt, event.modifiers.control);
            let screen = self.screen_point(point);
            for _ in 0..lines.unsigned_abs().min(10) {
                if let Some(bytes) = crate::mouse::scroll_report(lines, screen, mods, mode) {
                    self.write_to_shell(&bytes);
                }
            }
        } else if mode.contains(TermMode::ALT_SCREEN) {
            // No history on the alternate screen: programs there read the wheel as arrow keys.
            if mode.contains(TermMode::ALTERNATE_SCROLL) {
                let arrow: &[u8] = match (lines > 0, mode.contains(TermMode::APP_CURSOR)) {
                    (true, true) => b"\x1bOA",
                    (true, false) => b"\x1b[A",
                    (false, true) => b"\x1bOB",
                    (false, false) => b"\x1b[B",
                };
                for _ in 0..lines.unsigned_abs().min(10) {
                    self.write_to_shell(arrow);
                }
            }
        } else {
            self.state.term_arc().lock().scroll_display(Scroll::Delta(lines));
            cx.notify();
        }
    }


    /// Process pending terminal events.
    ///
    /// This method drains all available events from the event receiver
    /// and handles them appropriately. Note: bytes are processed in the
    /// async reader task, not here.
    fn process_events(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Process terminal events (from alacritty event proxy)
        while let Ok(event) = self.event_rx.try_recv() {
            match event {
                TerminalEvent::Wakeup => {
                    // Terminal has new content - already handled by async task
                }
                TerminalEvent::Bell => {
                    if let Some(ref callback) = self.bell_callback {
                        callback(window, cx);
                    }
                }
                TerminalEvent::Title(title) => {
                    self.title = title.clone();
                    if let Some(ref callback) = self.title_callback {
                        callback(window, cx, &title);
                    }
                }
                TerminalEvent::ClipboardStore(text) => {
                    if let Some(ref callback) = self.clipboard_store_callback {
                        callback(window, cx, &text);
                    }
                }
                TerminalEvent::ClipboardLoad => {
                    // Terminal wants to load data from clipboard
                    // TODO: Implement clipboard integration
                }
                TerminalEvent::Exit => {
                    if let Some(ref callback) = self.exit_callback {
                        callback(window, cx);
                    }
                }
            }
        }
    }

    /// Get the current terminal dimensions.
    ///
    /// # Returns
    ///
    /// A tuple of (columns, rows).
    pub fn dimensions(&self) -> (usize, usize) {
        (self.state.cols(), self.state.rows())
    }

    /// Resize the terminal to new dimensions.
    ///
    /// This method should be called when the terminal view size changes.
    /// It updates the internal grid and notifies the terminal process of the new size.
    ///
    /// # Arguments
    ///
    /// * `cols` - New number of columns
    /// * `rows` - New number of rows
    pub fn resize(&mut self, cols: usize, rows: usize) {
        self.state.resize(cols, rows);
    }

    /// Get the current terminal configuration.
    ///
    /// # Returns
    ///
    /// A reference to the current configuration.
    pub fn config(&self) -> &TerminalConfig {
        &self.config
    }

    /// Get the focus handle for this terminal view.
    ///
    /// # Returns
    ///
    /// A reference to the focus handle.
    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus_handle
    }

    /// Update the terminal configuration.
    ///
    /// This method updates the terminal's configuration, including font settings,
    /// padding, and color palette. Changes take effect on the next render.
    ///
    /// # Arguments
    ///
    /// * `config` - The new configuration to apply
    /// * `cx` - The context for triggering a repaint
    pub fn update_config(&mut self, config: TerminalConfig, cx: &mut Context<Self>) {
        // Update renderer with new font settings and palette
        self.renderer.font_family = config.font_family.clone();
        self.renderer.font_size = config.font_size;
        self.renderer.line_height_multiplier = config.line_height_multiplier;
        self.renderer.palette = config.colors.clone();

        // Scrollback and the default cursor shape change in the running terminal too.
        self.state
            .term_arc()
            .lock()
            .set_options(crate::terminal::TerminalState::term_config(
                config.scrollback,
                config.cursor_shape,
                config.cursor_blink,
            ));

        // Store the new config
        self.config = config;

        // Trigger a repaint - cell dimensions will be recalculated via measure_cell()
        cx.notify();
    }

    /// Calculate terminal dimensions from pixel bounds and cell size.
    ///
    /// Helper method to determine how many columns and rows fit in the given bounds.
    #[allow(dead_code)]
    fn calculate_dimensions(&self, bounds: Bounds<Pixels>) -> (usize, usize) {
        let width_f32: f32 = bounds.size.width.into();
        let height_f32: f32 = bounds.size.height.into();
        let cell_width_f32: f32 = self.renderer.cell_width.into();
        let cell_height_f32: f32 = self.renderer.cell_height.into();

        let cols = ((width_f32 / cell_width_f32) as usize).max(1);
        let rows = ((height_f32 / cell_height_f32) as usize).max(1);
        (cols, rows)
    }
}

impl TerminalView {
    /// The folder the shell last reported with OSC 7 or OSC 9;9, if it ever did.
    pub fn cwd(&self) -> Option<String> {
        self.state.cwd().map(str::to_string)
    }

    /// The title the program set (OSC 0 or 2); empty when it never did or reset it.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Every half second: show or hide a blinking cursor. Only the terminal that has the keyboard
    /// focus blinks, and the cursor is always shown while it does not.
    fn blink_tick(&mut self, cx: &mut Context<Self>) {
        let blinking = self.focused_now && self.state.with_term(|term| term.cursor_style().blinking);
        if blinking {
            self.blink_on = !self.blink_on;
            cx.notify();
        } else if !self.blink_on {
            self.blink_on = true;
            cx.notify();
        }
    }

    /// The link under a pixel position, if there is one.
    fn link_under(&self, position: Point<Pixels>) -> Option<Link> {
        let (point, _) = self.grid_point(position);
        self.state.with_term(|term| links::link_at(term, point))
    }

    // ---- Find ---------------------------------------------------------------------------

    /// Opens the find bar, starting from the selected text when it is a short single line.
    fn search_open(&mut self, cx: &mut Context<Self>) {
        if !self.search.open {
            self.search.open = true;
            let selected = self.state.with_term(|term| term.selection_to_string());
            if let Some(text) = selected.filter(|t| !t.is_empty() && !t.contains('\n') && t.chars().count() <= 100) {
                self.search.query = text;
            }
        }
        self.search_refresh(true);
        cx.notify();
    }

    fn search_close(&mut self, cx: &mut Context<Self>) {
        self.search.open = false;
        self.search.matches = Arc::default();
        self.search.active = None;
        cx.notify();
    }

    /// Every match of the query in the history and on screen, top to bottom.
    fn find_matches(&self, query: &str) -> Vec<RangeInclusive<AlacPoint>> {
        use alacritty_terminal::grid::Dimensions;
        use alacritty_terminal::index::{Column, Direction};
        use alacritty_terminal::term::search::{RegexIter, RegexSearch};
        if query.is_empty() {
            return Vec::new();
        }
        let Ok(mut regex) = RegexSearch::new(&escape_regex(query)) else { return Vec::new() };
        self.state.with_term(|term| {
            let start = AlacPoint::new(term.topmost_line(), Column(0));
            let end = AlacPoint::new(term.bottommost_line(), term.last_column());
            RegexIter::new(start, end, Direction::Right, term, &mut regex).take(MAX_MATCHES).collect()
        })
    }

    /// Searches again. With `jump` the current match is the first one on screen (or the last one
    /// in the history) and the view moves to it.
    fn search_refresh(&mut self, jump: bool) {
        let matches = self.find_matches(&self.search.query);
        let active = if matches.is_empty() {
            None
        } else if jump || self.search.active.is_none() {
            let top = self.state.with_term(|term| -(term.grid().display_offset() as i32));
            Some(matches.iter().position(|m| m.start().line.0 >= top).unwrap_or(matches.len() - 1))
        } else {
            Some(self.search.active.unwrap_or(0).min(matches.len() - 1))
        };
        self.search.matches = Arc::new(matches);
        self.search.active = active;
        self.search.dirty = false;
        self.search.computed = Some(Instant::now());
        if jump {
            self.scroll_to_active();
        }
    }

    /// Scrolls so the current match is on screen.
    fn scroll_to_active(&mut self) {
        let Some(found) = self.search.active.and_then(|i| self.search.matches.get(i)) else { return };
        let start = *found.start();
        self.state.with_term_mut(|term| term.scroll_to_point(start));
    }

    /// Moves to the next (or previous) match, wrapping around.
    fn search_step(&mut self, forward: bool, cx: &mut Context<Self>) {
        if self.search.dirty {
            self.search_refresh(false);
        }
        let len = self.search.matches.len();
        if len == 0 {
            return;
        }
        let current = self.search.active.unwrap_or(0);
        self.search.active = Some(if forward { (current + 1) % len } else { (current + len - 1) % len });
        self.scroll_to_active();
        cx.notify();
    }

    /// Keys of the find bar. Returns whether the key was used.
    fn search_key(&mut self, event: &KeyDownEvent, paste: bool, cx: &mut Context<Self>) -> bool {
        let ks = &event.keystroke;
        let m = &ks.modifiers;
        let plain = !m.control && !m.platform && !m.alt;
        match ks.key.as_str() {
            "escape" => self.search_close(cx),
            "enter" => self.search_step(!m.shift, cx),
            "up" | "down" if plain => self.search_step(ks.key == "down", cx),
            "g" if cfg!(target_os = "macos") && m.platform => self.search_step(!m.shift, cx),
            "backspace" => {
                if m.platform || m.alt {
                    self.search.query.clear();
                } else {
                    self.search.query.pop();
                }
                self.search_refresh(true);
                cx.notify();
            }
            _ if paste => {
                if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                    let line: String = text.lines().next().unwrap_or("").chars().filter(|c| !c.is_control()).collect();
                    if !line.is_empty() {
                        self.search.query.push_str(&line);
                        self.search_refresh(true);
                    }
                }
                cx.notify();
            }
            _ => return false,
        }
        cx.stop_propagation();
        true
    }

    /// The find bar, at the top right of the terminal.
    fn search_bar(&self, bg: Hsla, fg: Hsla, border: Hsla, muted: Hsla, cx: &mut Context<Self>) -> AnyElement {
        let status = if self.search.query.is_empty() {
            String::new()
        } else if self.search.matches.is_empty() {
            "No results".to_string()
        } else {
            format!("{} of {}", self.search.active.map_or(0, |a| a + 1), self.search.matches.len())
        };
        let hover = fg.opacity(0.14);
        let button = |id: &'static str, label: &'static str| {
            div()
                .id(id)
                .flex()
                .items_center()
                .justify_center()
                .size(px(22.))
                .rounded_md()
                .cursor_pointer()
                .text_color(muted)
                .hover(move |s| s.bg(hover).text_color(fg))
                .child(label)
        };
        div()
            .absolute()
            .top(px(10.))
            .right(px(16.))
            .flex()
            .items_center()
            .gap_1()
            .pl_3()
            .pr_1()
            .py_1()
            .rounded_lg()
            .border_1()
            .border_color(border)
            .bg(bg)
            .text_color(fg)
            .text_size(px(13.))
            .font_family(".SystemUIFont")
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .flex()
                    .items_center()
                    .min_w(px(150.))
                    .max_w(px(280.))
                    .overflow_hidden()
                    .child(div().child(self.search.query.clone()))
                    .child(div().w(px(1.)).h(px(15.)).bg(fg))
                    .when(self.search.query.is_empty(), |d| d.child(div().ml_1().text_color(muted).child("Find"))),
            )
            .child(div().text_size(px(12.)).text_color(muted).px_1().child(status))
            .child(button("find-prev", "\u{2191}").on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.search_step(false, cx);
                    cx.stop_propagation();
                }),
            ))
            .child(button("find-next", "\u{2193}").on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.search_step(true, cx);
                    cx.stop_propagation();
                }),
            ))
            .child(button("find-close", "\u{00d7}").on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.search_close(cx);
                    cx.stop_propagation();
                }),
            ))
            .into_any_element()
    }
}

/// Traces the text input protocol while developing (debug builds only).
macro_rules! ime_trace {
    ($($arg:tt)*) => {
        if cfg!(debug_assertions) {
            eprintln!("[ime] {}", format!($($arg)*));
        }
    };
}

/// The text input protocol. Without it macOS cannot start dictation in the window (it beeps and
/// closes), and the emoji picker, accents and other input methods do nothing.
impl EntityInputHandler for TerminalView {
    fn text_for_range(
        &mut self,
        _range: Range<usize>,
        adjusted_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        // A terminal has no editable document to read back.
        ime_trace!("text_for_range {:?}", _range);
        *adjusted_range = Some(0..0);
        Some(String::new())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        ime_trace!("selected_text_range");
        Some(UTF16Selection {
            range: 0..0,
            reversed: false,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.marked_text
            .as_ref()
            .map(|text| 0..text.encode_utf16().count())
    }

    fn unmark_text(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        ime_trace!("unmark_text");
        if self.marked_text.take().is_some() {
            cx.notify();
        }
    }

    fn replace_text_in_range(
        &mut self,
        _range: Option<Range<usize>>,
        text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Final text from the keyboard, an accent, the emoji picker or dictation. A terminal
        // takes Enter as carriage return.
        ime_trace!("replace_text_in_range range={:?} text={:?}", _range, text);
        self.marked_text = None;
        if self.search.open {
            let clean: String = text.chars().filter(|c| !c.is_control()).collect();
            if !clean.is_empty() {
                self.search.query.push_str(&clean);
                self.search_refresh(true);
            }
            cx.notify();
            return;
        }
        if !text.is_empty() {
            self.on_user_input();
            let text = text.replace("\r\n", "\r").replace('\n', "\r");
            self.write_to_shell(text.as_bytes());
        }
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _range: Option<Range<usize>>,
        new_text: &str,
        _new_selected_range: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Work in progress: a dead key waiting for its letter, or what dictation heard so far.
        ime_trace!("replace_and_mark_text_in_range text={:?}", new_text);
        self.marked_text = if new_text.is_empty() {
            None
        } else {
            Some(new_text.to_string())
        };
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        _range_utf16: Range<usize>,
        element_bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let b = self.cursor_bounds(element_bounds);
        ime_trace!("bounds_for_range -> {:?}", b);
        Some(b)
    }

    fn character_index_for_point(
        &mut self,
        _point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        Some(0)
    }
}

impl Render for TerminalView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Process any pending events
        self.process_events(window, cx);

        // Get terminal state and renderer for rendering
        let state_arc = self.state.term_arc();
        let renderer = self.renderer.clone();
        let resize_callback = self.resize_callback.clone();
        let padding = self.config.padding;
        let view = cx.entity();
        let focus_handle = self.focus_handle.clone();
        let marked_text = if self.search.open { None } else { self.marked_text.clone() };
        let focused = self.focus_handle.is_focused(window);
        self.focused_now = focused;
        let paint_info = self.paint_info.clone();

        // New output while finding: look again, but not on every frame.
        if self.search.open
            && self.search.dirty
            && self.search.computed.is_none_or(|t| t.elapsed() > SEARCH_REFRESH)
        {
            self.search_refresh(false);
        }
        let matches = self.search.matches.clone();
        let active_match = self.search.active;
        let link = self.hover_link.as_ref().map(|l| (l.start, l.end));
        let cursor_on = self.blink_on;
        let search_bar = if self.search.open {
            let (bg, fg) = {
                let term = state_arc.lock();
                let colors = term.colors();
                use alacritty_terminal::vte::ansi::{Color as TermColor, NamedColor};
                (
                    renderer.palette.resolve(TermColor::Named(NamedColor::Background), colors),
                    renderer.palette.resolve(TermColor::Named(NamedColor::Foreground), colors),
                )
            };
            Some(self.search_bar(bg.blend(fg.opacity(0.12)), fg, fg.opacity(0.25), fg.opacity(0.55), cx))
        } else {
            None
        };

        div()
            .size_full()
            .relative()
            .bg(rgb(0x1e1e1e))
            .track_focus(&self.focus_handle)
            .when(self.hover_link.is_some(), |d| d.cursor_pointer())
            .on_modifiers_changed(cx.listener(|this, event: &ModifiersChangedEvent, window, cx| {
                let position = window.mouse_position();
                let inside = this.paint_info.lock().bounds.contains(&position);
                let hover = if inside && link_modifier(&event.modifiers) && !this.selecting {
                    this.link_under(position)
                } else {
                    None
                };
                if hover != this.hover_link {
                    this.hover_link = hover;
                    cx.notify();
                }
            }))
            .on_key_down(cx.listener(Self::on_key_down))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_scroll_wheel(cx.listener(Self::on_scroll))
            .child(
                canvas(
                    move |bounds, _window, _cx| bounds,
                    move |bounds, _, window, cx| {
                        use alacritty_terminal::grid::Dimensions;

                        // Tell the system this view takes text input (dictation, accents, emoji).
                        window.handle_input(
                            &focus_handle,
                            ElementInputHandler::new(bounds, view.clone()),
                            cx,
                        );

                        // Measure actual cell dimensions from the font
                        let mut measured_renderer = renderer.clone();
                        measured_renderer.measure_cell(window);
                        *paint_info.lock() = PaintInfo {
                            origin: Point {
                                x: bounds.origin.x + padding.left,
                                y: bounds.origin.y + padding.top,
                            },
                            cell_width: measured_renderer.cell_width,
                            cell_height: measured_renderer.cell_height,
                            bounds,
                        };

                        // Calculate available space after padding
                        let available_width: f32 =
                            (bounds.size.width - padding.left - padding.right).into();
                        let available_height: f32 =
                            (bounds.size.height - padding.top - padding.bottom).into();
                        let cell_width_f32: f32 = measured_renderer.cell_width.into();
                        let cell_height_f32: f32 = measured_renderer.cell_height.into();

                        let cols = ((available_width / cell_width_f32) as usize).max(1);
                        let rows = ((available_height / cell_height_f32) as usize).max(1);

                        // Helper struct implementing Dimensions for resize
                        struct TermSize {
                            cols: usize,
                            rows: usize,
                        }
                        impl Dimensions for TermSize {
                            fn total_lines(&self) -> usize {
                                self.rows
                            }
                            fn screen_lines(&self) -> usize {
                                self.rows
                            }
                            fn columns(&self) -> usize {
                                self.cols
                            }
                            fn last_column(&self) -> alacritty_terminal::index::Column {
                                alacritty_terminal::index::Column(self.cols.saturating_sub(1))
                            }
                            fn bottommost_line(&self) -> alacritty_terminal::index::Line {
                                alacritty_terminal::index::Line(self.rows as i32 - 1)
                            }
                            fn topmost_line(&self) -> alacritty_terminal::index::Line {
                                alacritty_terminal::index::Line(0)
                            }
                        }

                        // Resize terminal if dimensions changed
                        let mut term = state_arc.lock();
                        let current_cols = term.columns();
                        let current_rows = term.screen_lines();
                        if cols != current_cols || rows != current_rows {
                            // Notify the PTY about the resize
                            if let Some(ref callback) = resize_callback {
                                callback(cols, rows);
                            }
                            term.resize(TermSize { cols, rows });
                        }

                        // Paint the terminal with measured dimensions
                        let extras = PaintExtras {
                            cursor_on,
                            matches: &matches,
                            active_match,
                            link,
                        };
                        measured_renderer.paint(bounds, padding, &term, focused, &extras, window, cx);

                        // What the input method is composing, drawn at the cursor.
                        if let Some(text) = &marked_text {
                            measured_renderer
                                .paint_composition(text, bounds, padding, &term, window, cx);
                        }
                    },
                )
                .size_full(),
            )
            .children(search_bar)
    }
}

// Tests are omitted due to macro expansion issues with the test attribute
// in this configuration. Integration tests can be added separately.
