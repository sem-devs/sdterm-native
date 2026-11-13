//! Files that ship inside the program: the logo, the icons and the terminal font.

use std::borrow::Cow;

use anyhow::Result;
use gpui::{App, AssetSource, SharedString};

/// The icons (Phosphor, MIT: see resources/icons/LICENSE-phosphor.txt), by the path the views use.
const ICONS: [(&str, &[u8]); 15] = [
    ("icons/sidebar.svg", include_bytes!("../resources/icons/sidebar.svg")),
    ("icons/plus.svg", include_bytes!("../resources/icons/plus.svg")),
    ("icons/minus.svg", include_bytes!("../resources/icons/minus.svg")),
    ("icons/gear.svg", include_bytes!("../resources/icons/gear.svg")),
    ("icons/dots.svg", include_bytes!("../resources/icons/dots.svg")),
    ("icons/folder-open.svg", include_bytes!("../resources/icons/folder-open.svg")),
    ("icons/x.svg", include_bytes!("../resources/icons/x.svg")),
    ("icons/play.svg", include_bytes!("../resources/icons/play.svg")),
    ("icons/caret-down.svg", include_bytes!("../resources/icons/caret-down.svg")),
    ("icons/caret-right.svg", include_bytes!("../resources/icons/caret-right.svg")),
    ("icons/prohibit.svg", include_bytes!("../resources/icons/prohibit.svg")),
    ("icons/arrow-square-out.svg", include_bytes!("../resources/icons/arrow-square-out.svg")),
    ("icons/terminal-window.svg", include_bytes!("../resources/icons/terminal-window.svg")),
    ("icons/squares-four.svg", include_bytes!("../resources/icons/squares-four.svg")),
    ("icons/arrow-left.svg", include_bytes!("../resources/icons/arrow-left.svg")),
];

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        let bytes: &'static [u8] = match path {
            "brand/wordmark.svg" => include_bytes!("../resources/brand/wordmark.svg"),
            "brand/icon-back.svg" => include_bytes!("../resources/brand/icon-back.svg"),
            "brand/icon-front.svg" => include_bytes!("../resources/brand/icon-front.svg"),
            _ => match ICONS.iter().find(|(name, _)| *name == path) {
                Some((_, bytes)) => bytes,
                None => return Ok(None),
            },
        };
        Ok(Some(Cow::Borrowed(bytes)))
    }

    fn list(&self, _path: &str) -> Result<Vec<SharedString>> {
        Ok(Vec::new())
    }
}

/// Geist Mono, the terminal font, bundled so it looks the same everywhere.
pub fn load_fonts(cx: &App) {
    let fonts: Vec<Cow<'static, [u8]>> = vec![
        Cow::Borrowed(include_bytes!("../resources/fonts/GeistMono-Regular.ttf")),
        Cow::Borrowed(include_bytes!("../resources/fonts/GeistMono-Bold.ttf")),
        Cow::Borrowed(include_bytes!("../resources/fonts/GeistMono-Italic.ttf")),
    ];
    if let Err(err) = cx.text_system().add_fonts(fonts) {
        eprintln!("could not load the bundled font: {err}");
    }
}
