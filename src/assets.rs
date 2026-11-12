//! Files that ship inside the program: the logo and the terminal font.

use std::borrow::Cow;

use anyhow::Result;
use gpui::{App, AssetSource, SharedString};

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        let bytes: &'static [u8] = match path {
            "brand/wordmark.svg" => include_bytes!("../resources/brand/wordmark.svg"),
            "brand/icon-back.svg" => include_bytes!("../resources/brand/icon-back.svg"),
            "brand/icon-front.svg" => include_bytes!("../resources/brand/icon-front.svg"),
            _ => return Ok(None),
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
