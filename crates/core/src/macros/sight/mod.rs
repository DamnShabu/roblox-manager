//! What a macro sees of its client: areas of the frame cage composes,
//! copied out by the compositor on request, and the checks a `when` line
//! makes of them -- an image back where it was picked, a colour at a point.
//!
//! Nothing reaches into the client: cage hands over a copy of what it shows,
//! as it would to a screenshot tool, through [`screencopy`].

pub mod screencopy;

use std::io;
use std::path::{Path, PathBuf};

/// How far, either way, an image may have shifted from where it was picked
/// and still be found: a pixel or two of jitter in a game's own layout, or
/// a window resized by a hair.
pub const SLACK: i32 = 4;

/// A rectangle of the display, from its top-left corner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Area {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl Area {
    /// The smallest area holding both.
    pub fn union(self, other: Area) -> Area {
        let right = self.right().max(other.right());
        let bottom = self.bottom().max(other.bottom());
        let (x, y) = (self.x.min(other.x), self.y.min(other.y));
        Area { x, y, w: (right - i64::from(x)) as u32, h: (bottom - i64::from(y)) as u32 }
    }

    fn right(self) -> i64 {
        i64::from(self.x) + i64::from(self.w)
    }

    fn bottom(self) -> i64 {
        i64::from(self.y) + i64::from(self.h)
    }

    /// The part of it on the display: nothing left of or above its corner.
    pub fn on_display(self) -> Area {
        let (x, y) = (self.x.max(0), self.y.max(0));
        let w = (self.right() - i64::from(x)).max(0) as u32;
        let h = (self.bottom() - i64::from(y)).max(0) as u32;
        Area { x, y, w, h }
    }
}

/// Pixels as red, green, blue bytes, row after row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<u8>,
}

impl Image {
    /// The pixel at (`x`, `y`); None outside it.
    pub fn pixel(&self, x: i64, y: i64) -> Option<[u8; 3]> {
        if x < 0 || y < 0 || x >= i64::from(self.width) || y >= i64::from(self.height) {
            return None;
        }
        let i = (y as usize * self.width as usize + x as usize) * 3;
        let p = self.rgb.get(i..i + 3)?;
        Some([p[0], p[1], p[2]])
    }

    /// The part of it inside `area`, given the area it shows.
    pub fn crop(&self, shows: Area, area: Area) -> Option<Image> {
        let (dx, dy) = (i64::from(area.x - shows.x), i64::from(area.y - shows.y));
        let fits = dx >= 0
            && dy >= 0
            && dx + i64::from(area.w) <= i64::from(self.width)
            && dy + i64::from(area.h) <= i64::from(self.height);
        if !fits || area.w == 0 || area.h == 0 {
            return None;
        }
        let mut rgb = Vec::with_capacity(area.w as usize * area.h as usize * 3);
        for row in 0..i64::from(area.h) {
            let start = ((dy + row) as usize * self.width as usize + dx as usize) * 3;
            rgb.extend_from_slice(self.rgb.get(start..start + area.w as usize * 3)?);
        }
        Some(Image { width: area.w, height: area.h, rgb })
    }
}

/// Where the image a `when` calls `name` is kept, in `dir`.
pub fn image_file(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!("{name}.png"))
}

/// A name for a newly picked image no image in `dir` has: `image1`,
/// `image2`, ... Picking again never overwrites one another macro may use.
pub fn free_image_name(dir: &Path) -> String {
    (1u32..)
        .map(|n| format!("image{n}"))
        .find(|name| !image_file(dir, name).exists())
        .unwrap_or_else(|| "image".to_owned())
}

/// What a macro sees through. Adapters: [`screencopy::Screencopy`], and
/// frames made up in the tests.
pub trait Eyes {
    /// The display's pixels in `area`, as they are on its next frame. A
    /// frame that does not come in time is `TimedOut`, and worth asking for
    /// again: a client that is loading draws nothing for a while.
    fn look(&mut self, area: Area) -> io::Result<Image>;
}

/// How alike `template` is to the frame at (`x`, `y`), from 0 (nothing
/// alike) to 1 (the same pixels), at the first place within [`SLACK`] of
/// there that scores at least `least` -- None when none does. `frame`
/// shows `shows`.
pub fn image_score(
    frame: &Image,
    shows: Area,
    template: &Image,
    (x, y): (i32, i32),
    least: f64,
) -> Option<f64> {
    let pixels = u64::from(template.width) * u64::from(template.height);
    if pixels == 0 {
        return None;
    }
    let most = 255.0 * 3.0 * pixels as f64;
    // The most difference, summed over every channel, that still scores
    // `least`: a place is given up on as soon as it is past it.
    let budget = ((1.0 - least).max(0.0) * most) as u64;
    offsets().into_iter().find_map(|(ox, oy)| {
        let (left, top) = (i64::from(x + ox - shows.x), i64::from(y + oy - shows.y));
        let fits = left >= 0
            && top >= 0
            && left + i64::from(template.width) <= i64::from(frame.width)
            && top + i64::from(template.height) <= i64::from(frame.height);
        if !fits {
            return None;
        }
        let diff = difference(frame, template, left as usize, top as usize, budget)?;
        Some(1.0 - diff as f64 / most)
    })
}

/// Every offset within [`SLACK`], nearest first: the place it was picked is
/// tried before any other, and is where an image that has not moved is.
fn offsets() -> Vec<(i32, i32)> {
    let mut all: Vec<(i32, i32)> =
        (-SLACK..=SLACK).flat_map(|y| (-SLACK..=SLACK).map(move |x| (x, y))).collect();
    all.sort_by_key(|(x, y)| x * x + y * y);
    all
}

/// The summed difference of every channel of `template` against the frame
/// at (`left`, `top`); None once it passes `limit`.
fn difference(frame: &Image, template: &Image, left: usize, top: usize, limit: u64) -> Option<u64> {
    let (fw, tw) = (frame.width as usize * 3, template.width as usize * 3);
    let mut diff = 0u64;
    for row in 0..template.height as usize {
        let f = (top + row) * fw + left * 3;
        let a = frame.rgb.get(f..f + tw)?;
        let b = template.rgb.get(row * tw..(row + 1) * tw)?;
        diff += a.iter().zip(b).map(|(p, q)| u64::from(p.abs_diff(*q))).sum::<u64>();
        if diff > limit {
            return None;
        }
    }
    Some(diff)
}

/// Whether the frame's pixel at (`x`, `y`) is `rgb`, every channel within
/// `within`. `frame` shows `shows`.
pub fn color_matches(
    frame: &Image,
    shows: Area,
    (x, y): (i32, i32),
    rgb: [u8; 3],
    within: u8,
) -> bool {
    let at = frame.pixel(i64::from(x - shows.x), i64::from(y - shows.y));
    at.is_some_and(|p| p.iter().zip(rgb).all(|(a, b)| a.abs_diff(b) <= within))
}

#[cfg(test)]
mod tests;
