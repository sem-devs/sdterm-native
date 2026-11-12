//! Links in the output: web addresses written as plain text, and the hyperlinks a program marks
//! with OSC 8. Holding Cmd (Ctrl elsewhere) and clicking opens them.

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::Term;
use alacritty_terminal::term::cell::Flags;

/// A link and the cells it covers (both ends included, on the same line or across wrapped lines).
#[derive(Clone, Debug, PartialEq)]
pub struct Link {
    pub start: Point,
    pub end: Point,
    pub url: String,
}

/// How far a wrapped line is followed up and down when looking for an address.
const MAX_WRAPPED_ROWS: i32 = 8;

const SCHEMES: [&str; 5] = ["https://", "http://", "ftp://", "file://", "mailto:"];

/// A hyperlink set by a program can say one thing and point to another, so only the schemes that
/// open a web page or a mail window are followed.
fn hyperlink_allowed(uri: &str) -> bool {
    let lower = uri.to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("mailto:")
}

fn starts_with_ignore_case(chars: &[char], text: &str) -> bool {
    let n = text.chars().count();
    chars.len() >= n && chars.iter().zip(text.chars()).all(|(a, b)| a.eq_ignore_ascii_case(&b))
}

/// Characters that end an address.
fn ends_url(c: char) -> bool {
    c.is_whitespace() || c.is_control() || matches!(c, '<' | '>' | '"' | '`')
}

/// Punctuation at the end of an address that belongs to the sentence around it.
fn trailing_junk(span: &[char]) -> bool {
    let count = |c: char| span.iter().filter(|&&x| x == c).count();
    match span.last() {
        Some('.' | ',' | ';' | ':' | '!' | '?' | '\'') => true,
        Some(')') => count('(') < count(')'),
        Some(']') => count('[') < count(']'),
        Some('}') => count('{') < count('}'),
        _ => false,
    }
}

/// The address that covers `index` in a line of text: where it starts and ends (both included in
/// character positions) and the URL to open.
pub fn find_in_text(chars: &[char], index: usize) -> Option<(usize, usize, String)> {
    let mut i = 0;
    while i < chars.len() {
        // An address starts at a word boundary: "xhttp://" is not one.
        if i > 0 && chars[i - 1].is_alphanumeric() {
            i += 1;
            continue;
        }
        let rest = &chars[i..];
        let (prefix_len, add_scheme) = if let Some(scheme) = SCHEMES.iter().find(|s| starts_with_ignore_case(rest, s)) {
            (scheme.chars().count(), false)
        } else if starts_with_ignore_case(rest, "www.") && rest.get(4).is_some_and(|c| c.is_alphanumeric()) {
            (4, true)
        } else {
            i += 1;
            continue;
        };
        let mut end = i + prefix_len;
        while end < chars.len() && !ends_url(chars[end]) {
            end += 1;
        }
        while end > i + prefix_len && trailing_junk(&chars[i..end]) {
            end -= 1;
        }
        if end > i + prefix_len {
            if (i..end).contains(&index) {
                let text: String = chars[i..end].iter().collect();
                return Some((i, end - 1, if add_scheme { format!("https://{text}") } else { text }));
            }
            i = end;
        } else {
            i += 1;
        }
    }
    None
}

/// The link under a cell, if there is one.
pub fn link_at<T>(term: &Term<T>, point: Point) -> Option<Link> {
    let grid = term.grid();
    let cols = term.columns();
    if cols == 0 {
        return None;
    }

    // A hyperlink the program marked itself (OSC 8).
    if let Some(link) = grid[point].hyperlink() {
        let uri = link.uri().to_string();
        if !hyperlink_allowed(&uri) {
            return None;
        }
        let same = |col: usize| {
            grid[Point::new(point.line, Column(col))].hyperlink().is_some_and(|other| other.uri() == uri)
        };
        let mut start = point.column.0;
        while start > 0 && same(start - 1) {
            start -= 1;
        }
        let mut end = point.column.0;
        while end + 1 < cols && same(end + 1) {
            end += 1;
        }
        return Some(Link {
            start: Point::new(point.line, Column(start)),
            end: Point::new(point.line, Column(end)),
            url: uri,
        });
    }

    // An address written in the text. A long one wraps onto the next row, so the whole logical
    // line is read.
    let last = Column(cols - 1);
    let wraps = |line: i32| grid[Point::new(Line(line), last)].flags.contains(Flags::WRAPLINE);
    let (top, bottom) = (term.topmost_line().0, term.bottommost_line().0);
    let mut first = point.line.0;
    while first > top && point.line.0 - first < MAX_WRAPPED_ROWS && wraps(first - 1) {
        first -= 1;
    }
    let mut final_row = point.line.0;
    while final_row < bottom && final_row - point.line.0 < MAX_WRAPPED_ROWS && wraps(final_row) {
        final_row += 1;
    }

    let mut cells: Vec<(char, Point)> = Vec::new();
    for line in first..=final_row {
        for col in 0..cols {
            let at = Point::new(Line(line), Column(col));
            let cell = &grid[at];
            if cell.flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
                continue;
            }
            cells.push((if cell.c == '\0' { ' ' } else { cell.c }, at));
        }
    }
    let index = cells.iter().position(|(_, p)| *p == point).or_else(|| {
        // The second half of a wide character belongs to the one before it.
        point.column.0.checked_sub(1).and_then(|col| {
            let before = Point::new(point.line, Column(col));
            cells.iter().position(|(_, p)| *p == before)
        })
    })?;
    let chars: Vec<char> = cells.iter().map(|(c, _)| *c).collect();
    let (start, end, url) = find_in_text(&chars, index)?;
    Some(Link { start: cells[start].1, end: cells[end].1, url })
}

#[cfg(test)]
mod tests {
    use super::find_in_text;

    fn find(text: &str, at: usize) -> Option<(usize, usize, String)> {
        let chars: Vec<char> = text.chars().collect();
        find_in_text(&chars, at)
    }

    #[test]
    fn finds_an_address_under_the_pointer() {
        let text = "open https://sem-devs.com/sdterm now";
        assert_eq!(find(text, 10), Some((5, 31, "https://sem-devs.com/sdterm".into())));
        assert_eq!(find(text, 0), None);
        assert_eq!(find(text, 34), None);
    }

    #[test]
    fn leaves_the_punctuation_of_the_sentence_out() {
        assert_eq!(find("see https://a.dev/x.", 6).unwrap().2, "https://a.dev/x");
        assert_eq!(find("(https://a.dev/x)", 3).unwrap().2, "https://a.dev/x");
        assert_eq!(find("https://en.wikipedia.org/wiki/Rust_(language)", 3).unwrap().2, "https://en.wikipedia.org/wiki/Rust_(language)");
        assert_eq!(find("\"http://localhost:3000\",", 3).unwrap().2, "http://localhost:3000");
    }

    #[test]
    fn www_and_mail_addresses() {
        assert_eq!(find("go to www.sem-devs.com today", 8).unwrap().2, "https://www.sem-devs.com");
        assert_eq!(find("write mailto:info@sem-devs.com", 9).unwrap().2, "mailto:info@sem-devs.com");
    }

    #[test]
    fn not_an_address() {
        assert_eq!(find("xhttp://not.a.link", 8), None);
        assert_eq!(find("just http:// nothing", 6), None);
        assert_eq!(find("wwwx.example", 2), None);
    }

    #[test]
    fn two_addresses_on_a_line() {
        let text = "http://a.dev http://b.dev";
        assert_eq!(find(text, 2).unwrap().2, "http://a.dev");
        assert_eq!(find(text, 16).unwrap().2, "http://b.dev");
    }
}
