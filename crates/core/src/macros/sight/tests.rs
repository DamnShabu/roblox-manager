use super::*;

/// A frame `w` by `h` of one colour.
fn plain(w: u32, h: u32, rgb: [u8; 3]) -> Image {
    Image { width: w, height: h, rgb: rgb.repeat((w * h) as usize) }
}

/// `frame` with `stamp` drawn on it at (`x`, `y`).
fn with(mut frame: Image, stamp: &Image, x: u32, y: u32) -> Image {
    for row in 0..stamp.height {
        for col in 0..stamp.width {
            let to = (((y + row) * frame.width + x + col) * 3) as usize;
            let from = ((row * stamp.width + col) * 3) as usize;
            frame.rgb[to..to + 3].copy_from_slice(&stamp.rgb[from..from + 3]);
        }
    }
    frame
}

/// A small picture with something in it: a gradient, not one colour.
fn coin() -> Image {
    let rgb = (0..36u32).flat_map(|i| [(i * 7) as u8, 200, (255 - i * 5) as u8]).collect();
    Image { width: 6, height: 6, rgb }
}

const SHOWS: Area = Area { x: 100, y: 50, w: 40, h: 30 };

#[test]
fn an_image_is_found_where_it_was_picked_and_a_few_pixels_off() {
    let at = (110, 60);
    let frame = with(plain(40, 30, [0, 0, 0]), &coin(), 10, 10);
    assert_eq!(image_score(&frame, SHOWS, &coin(), at, 0.9), Some(1.0));
    let moved = with(plain(40, 30, [0, 0, 0]), &coin(), 13, 8);
    assert_eq!(image_score(&moved, SHOWS, &coin(), at, 0.9), Some(1.0), "within the slack");
    let far = with(plain(40, 30, [0, 0, 0]), &coin(), 20, 20);
    assert_eq!(image_score(&far, SHOWS, &coin(), at, 0.9), None, "too far to be it");
}

#[test]
fn an_image_a_little_off_colour_still_matches_but_another_does_not() {
    let at = (110, 60);
    let mut dim = coin();
    dim.rgb.iter_mut().for_each(|c| *c = c.saturating_sub(10));
    let frame = with(plain(40, 30, [0, 0, 0]), &dim, 10, 10);
    let score = image_score(&frame, SHOWS, &coin(), at, 0.9).unwrap();
    assert!((0.95..1.0).contains(&score), "{score}");
    assert_eq!(image_score(&frame, SHOWS, &coin(), at, 0.99), None, "not that alike");
    let other = with(plain(40, 30, [0, 0, 0]), &plain(6, 6, [255, 0, 0]), 10, 10);
    assert_eq!(image_score(&other, SHOWS, &coin(), at, 0.9), None);
}

#[test]
fn a_colour_matches_within_its_tolerance_and_nowhere_off_the_frame() {
    let frame = with(plain(40, 30, [0, 0, 0]), &plain(1, 1, [250, 10, 10]), 5, 5);
    assert!(color_matches(&frame, SHOWS, (105, 55), [255, 0, 0], 24));
    assert!(!color_matches(&frame, SHOWS, (105, 55), [255, 0, 0], 5));
    assert!(!color_matches(&frame, SHOWS, (106, 55), [255, 0, 0], 24));
    assert!(!color_matches(&frame, SHOWS, (99, 55), [0, 0, 0], 255), "left of the frame");
}

#[test]
fn areas_join_and_keep_to_the_display() {
    let a = Area { x: -3, y: 10, w: 10, h: 5 };
    let b = Area { x: 20, y: 2, w: 1, h: 1 };
    assert_eq!(a.union(b), Area { x: -3, y: 2, w: 24, h: 13 });
    assert_eq!(a.on_display(), Area { x: 0, y: 10, w: 7, h: 5 });
    assert_eq!(Area { x: -9, y: 0, w: 4, h: 4 }.on_display().w, 0);
}

#[test]
fn a_crop_is_the_pixels_of_the_area_it_names() {
    let frame = with(plain(40, 30, [0, 0, 0]), &coin(), 10, 10);
    let got = frame.crop(SHOWS, Area { x: 110, y: 60, w: 6, h: 6 });
    assert_eq!(got, Some(coin()));
    assert_eq!(frame.crop(SHOWS, Area { x: 138, y: 60, w: 6, h: 6 }), None, "runs off it");
}

#[test]
fn a_picked_image_gets_a_name_no_other_has() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(free_image_name(dir.path()), "image1");
    std::fs::write(image_file(dir.path(), "image1"), b"").unwrap();
    std::fs::write(image_file(dir.path(), "image3"), b"").unwrap();
    assert_eq!(free_image_name(dir.path()), "image2");
}
