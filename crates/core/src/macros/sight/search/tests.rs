use std::time::Instant;

use super::*;

/// A frame that looks like a game more than a test card does: every pixel
/// its own, from a small generator, so no two places are alike.
fn noise(w: u32, h: u32, seed: u32) -> Image {
    let mut x = seed.wrapping_mul(2_654_435_761).max(1);
    let rgb = (0..w * h * 3)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            (x >> 24) as u8
        })
        .collect();
    Image { width: w, height: h, rgb }
}

/// Soften `image` so neighbouring pixels are alike, as they are on screen.
fn smooth(image: &Image) -> Image {
    let mut out = image.clone();
    let w = image.width as i64;
    for y in 0..i64::from(image.height) {
        for x in 0..w {
            for c in 0..3 {
                let mut sum = 0u32;
                let mut n = 0u32;
                for (dx, dy) in [(0, 0), (1, 0), (-1, 0), (0, 1), (0, -1)] {
                    if let Some(p) = image.pixel(x + dx, y + dy) {
                        sum += u32::from(p[c]);
                        n += 1;
                    }
                }
                out.rgb[((y * w + x) * 3) as usize + c] = (sum / n) as u8;
            }
        }
    }
    out
}

fn cut(image: &Image, x: u32, y: u32, w: u32, h: u32) -> Image {
    image.crop(whole(image), Area { x: x as i32, y: y as i32, w, h }).unwrap()
}

fn whole(image: &Image) -> Area {
    Area { x: 0, y: 0, w: image.width, h: image.height }
}

#[test]
fn an_image_is_found_anywhere_in_the_frame() {
    let frame = smooth(&noise(320, 200, 7));
    let coin = cut(&frame, 213, 131, 40, 24);
    assert_eq!(find(&frame, whole(&frame), &coin, whole(&frame), 0.9, None), Some((213, 131)));
    // Off the grid it is shrunk to, and near an edge.
    let edge = cut(&frame, 277, 3, 37, 19);
    assert_eq!(find(&frame, whole(&frame), &edge, whole(&frame), 0.9, None), Some((277, 3)));
}

#[test]
fn an_image_that_is_not_there_is_not_found() {
    let frame = smooth(&noise(320, 200, 7));
    let other = cut(&smooth(&noise(320, 200, 99)), 50, 50, 40, 24);
    assert_eq!(find(&frame, whole(&frame), &other, whole(&frame), 0.9, None), None);
}

#[test]
fn an_image_near_on_average_but_unlike_pixel_by_pixel_is_not_found() {
    // What a game's frame does: a place whose colours are close overall,
    // every other pixel well off -- 91% alike by summed difference.
    let frame = smooth(&noise(320, 200, 7));
    let mut speckled = cut(&frame, 213, 131, 40, 24);
    for (i, px) in speckled.rgb.chunks_exact_mut(3).enumerate() {
        let (x, y) = (i % 40, i / 40);
        if (x + y) % 2 == 0 {
            px.iter_mut().for_each(|c| *c = if *c < 128 { *c + 45 } else { *c - 45 });
        }
    }
    assert_eq!(find(&frame, whole(&frame), &speckled, whole(&frame), 0.9, None), None);
    assert_eq!(near(&frame, whole(&frame), &speckled, (213, 131), 0.9), None);
    // Half its pixels are alike, though.
    let half = find(&frame, whole(&frame), &speckled, whole(&frame), 0.45, None);
    assert_eq!(half, Some((213, 131)));
}

#[test]
fn a_rectangle_keeps_the_search_inside_it() {
    let frame = smooth(&noise(320, 200, 7));
    let coin = cut(&frame, 213, 131, 40, 24);
    let left = Area { x: 0, y: 0, w: 160, h: 200 };
    assert_eq!(find(&frame, whole(&frame), &coin, left, 0.9, None), None);
    let right = Area { x: 160, y: 100, w: 160, h: 100 };
    assert_eq!(find(&frame, whole(&frame), &coin, right, 0.9, None), Some((213, 131)));
}

#[test]
fn a_frame_of_part_of_the_window_is_searched_in_window_coordinates() {
    let frame = smooth(&noise(320, 200, 7));
    let coin = cut(&frame, 213, 131, 40, 24);
    let shows = Area { x: 1000, y: 500, w: 320, h: 200 };
    let everywhere = Area { x: 0, y: 0, w: 1 << 16, h: 1 << 16 };
    assert_eq!(find(&frame, shows, &coin, everywhere, 0.9, None), Some((1213, 631)));
}

#[test]
fn a_tiny_image_is_found_without_shrinking() {
    let frame = smooth(&noise(120, 80, 3));
    let dot = cut(&frame, 61, 17, 6, 5);
    assert_eq!(shrink_by(&dot), 1, "too small to shrink");
    assert_eq!(find(&frame, whole(&frame), &dot, whole(&frame), 0.95, None), Some((61, 17)));
}

#[test]
fn where_it_was_last_is_tried_first() {
    let frame = smooth(&noise(320, 200, 7));
    let coin = cut(&frame, 213, 131, 40, 24);
    let last = Some((211, 133));
    assert_eq!(find(&frame, whole(&frame), &coin, whole(&frame), 0.9, last), Some((213, 131)));
}

#[test]
fn halving_averages_each_block_of_four() {
    let image =
        Image { width: 2, height: 2, rgb: vec![0, 0, 0, 100, 100, 100, 200, 0, 0, 0, 0, 0] };
    assert_eq!(halve(&image).rgb, [75, 25, 25]);
}

/// How long a whole-window look takes, which twenty a second has to fit:
/// `cargo test --release -p rbxmgr-core -- --ignored --nocapture timing`.
#[test]
#[ignore = "a measurement, in release"]
fn timing_a_whole_window_search() {
    let frame = smooth(&noise(1280, 720, 11));
    for (w, h) in [(64, 40), (120, 80), (24, 24), (12, 12)] {
        let coin = cut(&frame, 900, 500, w, h);
        let absent = cut(&smooth(&noise(1280, 720, 12)), 900, 500, w, h);
        for (what, image, last) in [
            ("found", &coin, None),
            ("absent", &absent, None),
            ("found, last", &coin, Some((900, 500))),
        ] {
            let start = Instant::now();
            let runs = 20;
            for _ in 0..runs {
                std::hint::black_box(find(&frame, whole(&frame), image, whole(&frame), 0.9, last));
            }
            println!("{w}x{h} {what}: {:?} a look", start.elapsed() / runs);
        }
    }
}
