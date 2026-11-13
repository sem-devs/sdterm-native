//! Small building blocks shared by the views: buttons, menus, drag previews.

use gpui::prelude::*;
use gpui::{
    AnyElement, AnyView, App, AppContext, Context, Deferred, ElementId, FontWeight, Hsla, Render, Rgba,
    SharedString, Window, deferred, div, px,
};

use crate::layout::Divider;
use crate::model::PaneColor;
use crate::theme::Theme;

/// `t` of `b` over `a`.
pub fn mix(a: Hsla, b: Hsla, t: f32) -> Hsla {
    let (a, b): (Rgba, Rgba) = (a.into(), b.into());
    Rgba {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: 1.0,
    }
    .into()
}

/// The icons of the interface: one family, one weight, like the rest of the app.
#[derive(Clone, Copy)]
pub enum Icon {
    Sidebar,
    Plus,
    Minus,
    Gear,
    Dots,
    FolderOpen,
    X,
    Play,
    CaretDown,
    CaretRight,
    Prohibit,
    ArrowSquareOut,
    TerminalWindow,
    SquaresFour,
    ArrowLeft,
}

impl Icon {
    fn path(self) -> &'static str {
        match self {
            Icon::Sidebar => "icons/sidebar.svg",
            Icon::Plus => "icons/plus.svg",
            Icon::Minus => "icons/minus.svg",
            Icon::Gear => "icons/gear.svg",
            Icon::Dots => "icons/dots.svg",
            Icon::FolderOpen => "icons/folder-open.svg",
            Icon::X => "icons/x.svg",
            Icon::Play => "icons/play.svg",
            Icon::CaretDown => "icons/caret-down.svg",
            Icon::CaretRight => "icons/caret-right.svg",
            Icon::Prohibit => "icons/prohibit.svg",
            Icon::ArrowSquareOut => "icons/arrow-square-out.svg",
            Icon::TerminalWindow => "icons/terminal-window.svg",
            Icon::SquaresFour => "icons/squares-four.svg",
            Icon::ArrowLeft => "icons/arrow-left.svg",
        }
    }
}

/// An icon at `size` pixels. It takes the text color of the element it is in.
pub fn icon(which: Icon, size: f32) -> gpui::Svg {
    gpui::svg().path(which.path()).size(px(size)).flex_none()
}

/// A square button with an icon: `side` pixels wide, the icon `glyph` pixels. `hint` is what the
/// tooltip says.
pub fn icon_button_sized(
    id: impl Into<ElementId>,
    which: Icon,
    side: f32,
    glyph: f32,
    hint: impl Into<SharedString>,
    theme: &Theme,
    on_click: impl Fn(&mut App) + 'static,
) -> gpui::Stateful<gpui::Div> {
    let (fg, hover_bg, hover_fg) = (theme.muted_foreground, theme.accent, theme.foreground);
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(side))
        .rounded_md()
        .cursor_pointer()
        .text_color(fg)
        .hover(move |s| s.bg(hover_bg).text_color(hover_fg))
        .child(icon(which, glyph))
        .tooltip(tip(hint))
        .on_click(move |_, _, cx| on_click(cx))
}

pub fn icon_button(
    id: impl Into<ElementId>,
    which: Icon,
    hint: impl Into<SharedString>,
    theme: &Theme,
    on_click: impl Fn(&mut App) + 'static,
) -> gpui::Stateful<gpui::Div> {
    icon_button_sized(id, which, 28., 16., hint, theme, on_click)
}

/// The small one, for rows and title bars of terminals.
pub fn mini_icon_button(
    id: impl Into<ElementId>,
    which: Icon,
    hint: impl Into<SharedString>,
    theme: &Theme,
    on_click: impl Fn(&mut App) + 'static,
) -> gpui::Stateful<gpui::Div> {
    icon_button_sized(id, which, 22., 14., hint, theme, on_click)
}

/// The little label that appears when the pointer rests on a button.
pub struct Tip(SharedString);

impl Render for Tip {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<Theme>().clone();
        div()
            .px_2()
            .py_1()
            .rounded_md()
            .border_1()
            .border_color(theme.input)
            .bg(mix(theme.background, theme.foreground, 0.07))
            .shadow_md()
            .text_size(px(12.))
            .text_color(theme.foreground)
            .child(self.0.clone())
    }
}

/// A tooltip builder for `.tooltip(...)`.
pub fn tip(text: impl Into<SharedString>) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    let text: SharedString = text.into();
    move |_, cx| cx.new(|_| Tip(text.clone())).into()
}

pub fn text_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    theme: &Theme,
    primary: bool,
    on_click: impl Fn(&mut App) + 'static,
) -> gpui::Stateful<gpui::Div> {
    let (bg, fg, hover) = if primary {
        (theme.primary, theme.primary_foreground, theme.primary.opacity(0.85))
    } else {
        (theme.accent, theme.foreground, theme.accent.opacity(0.7))
    };
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .h(px(30.))
        .px_3()
        .rounded_md()
        .cursor_pointer()
        .bg(bg)
        .text_color(fg)
        .hover(move |s| s.bg(hover))
        .child(label.into())
        .on_click(move |_, _, cx| on_click(cx))
}

/// The small square that shows a terminal's color.
pub fn color_chip(theme: &Theme, color: Option<PaneColor>) -> gpui::Div {
    let chip = div().size(px(10.)).flex_none().rounded(px(3.));
    if color.is_some() {
        chip.bg(theme.pane(color))
    } else {
        chip.border_1().border_color(theme.muted_foreground)
    }
}

// ---- Menus ---------------------------------------------------------------------------------

/// A floating panel. Place it with `.top()` / `.right()` inside a `.relative()` container and wrap
/// it in [`overlay`] so it draws above everything else.
pub fn menu_panel(theme: &Theme) -> gpui::Div {
    div()
        .absolute()
        .occlude()
        .flex()
        .flex_col()
        .p_1()
        .rounded_lg()
        .border_1()
        .border_color(theme.input)
        .bg(mix(theme.background, theme.foreground, 0.07))
        .shadow_lg()
        .text_size(px(13.))
        .text_color(theme.foreground)
}

pub fn overlay(content: impl IntoElement) -> Deferred {
    deferred(content).with_priority(10)
}

pub fn menu_item(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    theme: &Theme,
    danger: bool,
    on_click: impl Fn(&mut App) + 'static,
) -> gpui::Stateful<gpui::Div> {
    let hover_bg = theme.accent;
    let color = if danger { Hsla { h: 0.98, s: 0.8, l: 0.62, a: 1.0 } } else { theme.foreground };
    div()
        .id(id)
        .flex()
        .items_center()
        .h(px(28.))
        .px_2()
        .rounded_md()
        .cursor_pointer()
        .text_color(color)
        .hover(move |s| s.bg(hover_bg))
        .child(label.into())
        .on_click(move |_, _, cx| on_click(cx))
}

pub fn menu_separator(theme: &Theme) -> gpui::Div {
    div().h(px(1.)).my_1().bg(theme.border)
}

pub fn menu_title(theme: &Theme, text: impl Into<SharedString>) -> gpui::Div {
    div()
        .px_2()
        .py_1()
        .text_size(px(11.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(theme.muted_foreground)
        .child(text.into())
}

/// Closes whatever menu is open when you click anywhere else.
pub fn backdrop(on_close: impl Fn(&mut App) + 'static) -> AnyElement {
    deferred(
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| on_close(cx)),
    )
    .with_priority(5)
    .into_any_element()
}

// ---- Dragging ------------------------------------------------------------------------------

/// A terminal being dragged to another position or workspace.
#[derive(Clone)]
pub struct PaneDrag {
    pub id: String,
    pub label: String,
    pub color: Hsla,
}

/// The line between two regions of a layout being dragged.
#[derive(Clone)]
pub struct DividerDrag {
    pub workspace: String,
    pub divider: Divider,
}

/// What follows the pointer while a terminal is dragged.
pub struct DragGhost {
    pub label: String,
    pub color: Hsla,
}

impl Render for DragGhost {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_3()
            .py_1()
            .rounded_md()
            .bg(self.color.opacity(0.92))
            .text_color(Hsla { h: 0.0, s: 0.0, l: 0.05, a: 1.0 })
            .text_size(px(12.))
            .font_weight(FontWeight::SEMIBOLD)
            .child(self.label.clone())
    }
}

/// Nothing to show while a divider is dragged: the layout itself moves.
pub struct NoGhost;

impl Render for NoGhost {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size(px(0.))
    }
}

#[cfg(test)]
mod tests {
    use super::Icon;
    use crate::assets::Assets;
    use gpui::AssetSource;

    #[test]
    fn every_icon_is_in_the_bundle() {
        let all = [
            Icon::Sidebar,
            Icon::Plus,
            Icon::Minus,
            Icon::Gear,
            Icon::Dots,
            Icon::FolderOpen,
            Icon::X,
            Icon::Play,
            Icon::CaretDown,
            Icon::CaretRight,
            Icon::Prohibit,
            Icon::ArrowSquareOut,
            Icon::TerminalWindow,
            Icon::SquaresFour,
            Icon::ArrowLeft,
        ];
        for icon in all {
            let bytes = Assets.load(icon.path()).unwrap().unwrap_or_else(|| panic!("{} is missing", icon.path()));
            let text = std::str::from_utf8(&bytes).unwrap();
            assert!(text.starts_with("<svg") && text.contains("viewBox") && text.contains("<path"), "{}", icon.path());
        }
    }
}
