//! Finding a picked image in a copy of the frame: anywhere in an area, many
//! times a second, or near one place.
//!
//! Comparing the image at every place in a whole window is millions of
//! pixel comparisons a look, far too many at twenty looks a second. So the
//! frame and the image are halved alike a few times, the smallest image is
//! looked for everywhere in the smallest frame, and only the few places
//! most like it are looked at again, a size up each time, to full size. Where it was found last is tried before all
//! of that: an image that has not moved is found at once.

use super::{Area, Image};

/// How far, either way, an image may have shifted from where it is looked
/// for and still be found there: a pixel or two of jitter in a game's own
/// layout, or a window resized by a hair.
pub const SLACK: i32 = 4;

/// The fewest pixels a shrunk image keeps across and down: any fewer and
/// one image looks like many others.
const SHRUNK_LEAST: u32 = 4;
/// How many of the places most like it are carried from each size to the
/// next one up.
const CANDIDATES: usize = 8;
/// How much less alike a place may be at a small size and still be carried
/// up: shrinking blurs both, by different amounts where the image does not
/// sit on the grid it is shrunk to.
const SHRUNK_ALLOWANCE: f64 = 0.08;

/// Where in `within` the frame shows `template` at least `least` (0 to 1)
/// alike -- display coordinates of its top-left corner -- trying `last`
/// first. `frame` shows `shows`.
pub fn find(
    frame: &Image,
    shows: Area,
    template: &Image,
    within: Area,
    least: f64,
    last: Option<(i32, i32)>,
) -> Option<(i32, i32)> {
    if let Some(at) = last.and_then(|at| near(frame, shows, template, at, least)) {
        return Some(at);
    }
    // The places its corner may be, in the frame's own pixels.
    let x0 = i64::from(within.x - shows.x).max(0);
    let y0 = i64::from(within.y - shows.y).max(0);
    let x1 = (i64::from(within.x - shows.x) + i64::from(within.w)).min(i64::from(frame.width))
        - i64::from(template.width);
    let y1 = (i64::from(within.y - shows.y) + i64::from(within.h)).min(i64::from(frame.height))
        - i64::from(template.height);
    if template.width == 0 || template.height == 0 || x1 < x0 || y1 < y0 {
        return None;
    }
    // Halved again and again: the smallest is searched everywhere, and each
    // size up only around the places most like it on the one before.
    let levels = shrink_by(template).trailing_zeros();
    let (mut frames, mut templates) = (Vec::new(), Vec::new());
    for l in 0..levels as usize {
        frames.push(halve(if l == 0 { frame } else { &frames[l - 1] }));
        templates.push(halve(if l == 0 { template } else { &templates[l - 1] }));
    }
    // Level l: the frame and image halved l times.
    let level = |l: u32| match l.checked_sub(1) {
        Some(i) => (&frames[i as usize], &templates[i as usize]),
        None => (frame, template),
    };
    let loose = (least - SHRUNK_ALLOWANCE).max(0.0);
    let bounds = |l: u32| (x0 >> l, y0 >> l, x1 >> l, y1 >> l);
    let (f, t) = level(levels);
    let mut places = best_places(f, t, bounds(levels), loose);
    for l in (0..levels).rev() {
        let (f, t) = level(l);
        let least = if l == 0 { least } else { loose };
        let (bx0, by0, bx1, by1) = bounds(l);
        let mut next: Vec<(u64, i64, i64)> = places
            .iter()
            .filter_map(|&(x, y)| {
                // Each place stands for two by two here, and one beyond
                // either way for an image that sits across the halving.
                let window = ((2 * x - 1).max(bx0), (2 * y - 1).max(by0));
                let window = (window.0, window.1, (2 * x + 2).min(bx1), (2 * y + 2).min(by1));
                best_places(f, t, window, least).first().and_then(|&(x, y)| {
                    Some((difference(f, t, x as usize, y as usize, u64::MAX)?, x, y))
                })
            })
            .collect();
        next.sort_unstable();
        next.dedup_by_key(|p| (p.1, p.2));
        places = next.into_iter().map(|(_, x, y)| (x, y)).collect();
    }
    places.first().map(|&(x, y)| (x as i32 + shows.x, y as i32 + shows.y))
}

/// Where `template` is within [`SLACK`] of (`x`, `y`), at least `least`
/// alike: the place nearest there that is. `frame` shows `shows`.
pub fn near(
    frame: &Image,
    shows: Area,
    template: &Image,
    (x, y): (i32, i32),
    least: f64,
) -> Option<(i32, i32)> {
    if template.width == 0 || template.height == 0 {
        return None;
    }
    let budget = budget(template, least);
    offsets().into_iter().find_map(|(ox, oy)| {
        let (left, top) = (i64::from(x + ox - shows.x), i64::from(y + oy - shows.y));
        let fits = left >= 0
            && top >= 0
            && left + i64::from(template.width) <= i64::from(frame.width)
            && top + i64::from(template.height) <= i64::from(frame.height);
        if !fits {
            return None;
        }
        difference(frame, template, left as usize, top as usize, budget)?;
        Some((x + ox, y + oy))
    })
}

/// The most difference, summed over every channel, that still scores
/// `least`: a place is given up on as soon as it is past it.
fn budget(template: &Image, least: f64) -> u64 {
    let pixels = u64::from(template.width) * u64::from(template.height);
    ((1.0 - least).max(0.0) * 255.0 * 3.0 * pixels as f64) as u64
}

/// The places, among (`x0`..=`x1`, `y0`..=`y1`), most like `template` and
/// at least `least` alike: up to [`CANDIDATES`] of them, most alike first.
fn best_places(
    frame: &Image,
    template: &Image,
    (x0, y0, x1, y1): (i64, i64, i64, i64),
    least: f64,
) -> Vec<(i64, i64)> {
    let x1 = x1.min(i64::from(frame.width) - i64::from(template.width));
    let y1 = y1.min(i64::from(frame.height) - i64::from(template.height));
    let budget = budget(template, least);
    let mut best: Vec<(u64, i64, i64)> = Vec::with_capacity(CANDIDATES + 1);
    for y in y0..=y1 {
        for x in x0..=x1 {
            // Once there are enough, only a place better than the worst of
            // them is worth counting to the end.
            let limit = if best.len() < CANDIDATES { budget } else { best[CANDIDATES - 1].0 };
            if let Some(d) = difference(frame, template, x as usize, y as usize, limit) {
                let at = best.partition_point(|b| b.0 <= d);
                best.insert(at, (d, x, y));
                best.truncate(CANDIDATES);
            }
        }
    }
    best.into_iter().map(|(_, x, y)| (x, y)).collect()
}

/// How much to shrink by for `template`: as much as keeps it
/// [`SHRUNK_LEAST`] pixels across and down, at most sixteen times.
fn shrink_by(template: &Image) -> u32 {
    [16, 8, 4, 2]
        .into_iter()
        .find(|s| template.width / s >= SHRUNK_LEAST && template.height / s >= SHRUNK_LEAST)
        .unwrap_or(1)
}

/// `image` half the size each way, each pixel the average of the four it
/// stands for; an odd last row or column is dropped.
fn halve(image: &Image) -> Image {
    let (w, h) = ((image.width / 2) as usize, (image.height / 2) as usize);
    let row = image.width as usize * 3;
    let mut rgb = vec![0u8; w * h * 3];
    for (y, out) in rgb.chunks_exact_mut(w * 3).enumerate() {
        let top = &image.rgb[2 * y * row..2 * y * row + row];
        let bottom = &image.rgb[(2 * y + 1) * row..(2 * y + 2) * row];
        for (x, o) in out.chunks_exact_mut(3).enumerate() {
            let i = x * 6;
            for c in 0..3 {
                let sum = u16::from(top[i + c])
                    + u16::from(top[i + 3 + c])
                    + u16::from(bottom[i + c])
                    + u16::from(bottom[i + 3 + c]);
                o[c] = (sum / 4) as u8;
            }
        }
    }
    Image { width: w as u32, height: h as u32, rgb }
}

/// Every offset within [`SLACK`], nearest first: the place looked at is
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
        // Summed a row at a time in 32 bits, which the compiler does many
        // bytes at once: a row never holds more than 2^32 / 255 channels.
        diff += u64::from(a.iter().zip(b).map(|(p, q)| u32::from(p.abs_diff(*q))).sum::<u32>());
        if diff > limit {
            return None;
        }
    }
    Some(diff)
}

#[cfg(test)]
mod tests;
