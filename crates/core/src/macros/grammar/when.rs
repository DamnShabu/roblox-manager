//! `when`: steps that play the moment something shows in the client, out of
//! turn -- `when image coin`, then a line `do tap e` for each step.
//! The macro's own steps go on around them; see `player::watch`.

use super::{FURTHEST, Step};

/// What a `when` waits for, and the steps it plays.
#[derive(Clone, Debug, PartialEq)]
pub struct Handler {
    pub when: Condition,
    pub steps: Vec<Step>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Condition {
    pub sight: Sight,
    /// `when not ...`: the moment it stops showing.
    pub not: bool,
}

/// Something a macro can see in its client.
#[derive(Clone, Debug, PartialEq)]
pub enum Sight {
    /// An image picked from the client, showing somewhere in `place` at
    /// least `least` (0 to 1) alike.
    Image { name: String, place: Place, least: f64 },
    /// The pixel at (`x`, `y`) the colour `rgb`, every channel within
    /// `within`.
    Color { x: i32, y: i32, rgb: [u8; 3], within: u8 },
}

/// Where a `when` looks for its image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    /// Anywhere in the window: `when image coin`.
    Anywhere,
    /// At (`x`, `y`), within a few pixels: `when image coin 812 40`.
    Near(i32, i32),
    /// Anywhere in a rectangle, corner and size: `when image coin in 0 0
    /// 400 300`.
    Within { x: i32, y: i32, w: u32, h: u32 },
}

/// How alike an image must be when the line does not say.
pub const IMAGE_LEAST: f64 = 0.9;
/// How far each channel of a colour may be off when the line does not say.
pub const COLOR_WITHIN: u8 = 24;

/// The steps a `do` line may play: those that play in turn. A repeat, a
/// timeline or a start belongs to the macro's own steps. An exit ends the
/// round the macro's own steps are in.
pub(super) const DOABLE: [&str; 12] = [
    "tap", "hold", "press", "release", "type", "click", "move", "scroll", "wait", "path", "turn",
    "exit",
];

/// A `when` line's condition: `[not] image NAME [X Y | in X Y W H]
/// [PERCENT%]` or `[not] color X Y #RRGGBB [WITHIN]`.
pub(super) fn condition(rest: &str) -> Result<Condition, String> {
    let words: Vec<&str> = rest.split_whitespace().collect();
    let (not, words) = match words.split_first() {
        Some((first, more)) if first.eq_ignore_ascii_case("not") => (true, more),
        _ => (false, words.as_slice()),
    };
    let sight = match words {
        [kind, name, more @ ..] if kind.eq_ignore_ascii_case("image") => {
            let (more, least) = match more.split_last() {
                Some((last, rest)) if last.ends_with('%') => (rest, percent(last)?),
                _ => (more, IMAGE_LEAST),
            };
            let place = match more {
                [] => Place::Anywhere,
                [x, y] => Place::Near(pixel(x)?, pixel(y)?),
                [within, x, y, w, h] if within.eq_ignore_ascii_case("in") => {
                    let (w, h) = (size(w)?, size(h)?);
                    Place::Within { x: pixel(x)?, y: pixel(y)?, w, h }
                }
                _ => return Err(USAGE.into()),
            };
            Sight::Image { name: image_name(name)?, place, least }
        }
        [kind, x, y, hex, more @ ..]
            if (kind.eq_ignore_ascii_case("color") || kind.eq_ignore_ascii_case("colour"))
                && more.len() <= 1 =>
        {
            let within = more.first().map_or(Ok(COLOR_WITHIN), |w| {
                w.parse().map_err(|_| format!("a colour is off by 0 to 255, not '{w}'"))
            })?;
            Sight::Color { x: pixel(x)?, y: pixel(y)?, rgb: color(hex)?, within }
        }
        _ => return Err(USAGE.into()),
    };
    Ok(Condition { sight, not })
}

const USAGE: &str = "expected when [not] image NAME [X Y | in X Y W H] [90%], or when [not] \
                     color X Y #RRGGBB [24]";

/// An image's name: what its file is saved as, so nothing that leaves the
/// images folder.
pub fn image_name(name: &str) -> Result<String, String> {
    let fine = !name.is_empty()
        && name.len() <= 64
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    if fine { Ok(name.to_owned()) } else { Err(format!("'{name}' is not an image name")) }
}

fn percent(word: &str) -> Result<f64, String> {
    let n: f64 = word
        .strip_suffix('%')
        .and_then(|n| n.parse().ok())
        .filter(|n: &f64| (1.0..=100.0).contains(n))
        .ok_or_else(|| format!("how alike is a percentage, 1% to 100%, not '{word}'"))?;
    Ok(n / 100.0)
}

fn color(hex: &str) -> Result<[u8; 3], String> {
    let bad = || format!("a colour is written #RRGGBB, not '{hex}'");
    let digits = hex.strip_prefix('#').filter(|d| d.len() == 6).ok_or_else(bad)?;
    let byte = |i: usize| u8::from_str_radix(digits.get(i..i + 2).unwrap_or(""), 16);
    Ok([byte(0).map_err(|_| bad())?, byte(2).map_err(|_| bad())?, byte(4).map_err(|_| bad())?])
}

/// A width or height: at least a pixel, and on the window.
fn size(token: &str) -> Result<u32, String> {
    match token.parse::<u32>() {
        Ok(n) if (1..=FURTHEST as u32).contains(&n) => Ok(n),
        _ => Err(format!("a size is 1 to {FURTHEST} pixels, not '{token}'")),
    }
}

fn pixel(token: &str) -> Result<i32, String> {
    let n: i32 = token.parse().map_err(|_| format!("not a number: '{token}'"))?;
    if !(0..=FURTHEST).contains(&n) {
        return Err(format!("'{token}' is off the window -- 0 to {FURTHEST}"));
    }
    Ok(n)
}

/// A `when` line's value looking for the image `name`, newly picked: any
/// `not`, rectangle to look in and how alike stay; it is looked for
/// anywhere unless the line said where.
pub fn picked_image(value: &str, name: &str) -> String {
    let (not, place, least) = match condition(value) {
        Ok(Condition { sight: Sight::Image { place, least, .. }, not }) => (not, place, least),
        Ok(c) => (c.not, Place::Anywhere, IMAGE_LEAST),
        Err(_) => (false, Place::Anywhere, IMAGE_LEAST),
    };
    let not = if not { "not " } else { "" };
    let place = match place {
        Place::Within { x, y, w, h } => format!(" in {x} {y} {w} {h}"),
        Place::Anywhere | Place::Near(..) => String::new(),
    };
    let least =
        if least == IMAGE_LEAST { String::new() } else { format!(" {}%", (least * 100.0).round()) };
    format!("{not}image {name}{place}{least}")
}

/// What a `when` waits for, for the status line.
pub fn describe(c: &Condition) -> String {
    match (&c.sight, c.not) {
        (Sight::Image { name, .. }, false) => format!("{name} shows"),
        (Sight::Image { name, .. }, true) => format!("{name} is gone"),
        (Sight::Color { x, y, rgb, .. }, not) => {
            let hex = format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2]);
            if not {
                format!("{x}, {y} is no longer {hex}")
            } else {
                format!("{x}, {y} turns {hex}")
            }
        }
    }
}

#[cfg(test)]
mod tests;
