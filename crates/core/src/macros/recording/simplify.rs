//! A movement cut down to the points it bends at.

use super::Cs;

/// Where the mouse was, and when.
#[derive(Clone, Copy, Debug)]
pub(super) struct Point {
    pub t: Cs,
    pub x: f64,
    pub y: f64,
}

/// The fewest points that glide within `stray` of every one of `points`,
/// at the time each was seen (Douglas-Peucker on where a glide puts the
/// mouse then). The first and last always stand; of points seen in the
/// same hundredth, the last.
pub(super) fn simplify(points: &[Point], stray: f64) -> Vec<Point> {
    let Some(last) = points.len().checked_sub(1) else { return Vec::new() };
    let mut keep = vec![false; points.len()];
    keep[0] = true;
    keep[last] = true;
    let mut spans = vec![(0, last)];
    while let Some((a, b)) = spans.pop() {
        let furthest = (a + 1..b)
            .map(|i| (i, strays(points[a], points[b], points[i])))
            .max_by(|x, y| x.1.total_cmp(&y.1));
        if let Some((i, _)) = furthest.filter(|(_, by)| *by > stray) {
            keep[i] = true;
            spans.push((a, i));
            spans.push((i, b));
        }
    }
    let kept: Vec<Point> = points.iter().zip(keep).filter(|(_, k)| *k).map(|(p, _)| *p).collect();
    kept.iter()
        .enumerate()
        .filter(|(i, p)| kept.get(i + 1).is_none_or(|next| next.t != p.t))
        .map(|(_, p)| *p)
        .collect()
}

/// How far `p` is from where a glide from `a` to `b` has the mouse at
/// `p`'s time.
fn strays(a: Point, b: Point, p: Point) -> f64 {
    let span = (b.t - a.t) as f64;
    let f = if span > 0.0 { (p.t - a.t) as f64 / span } else { 0.0 };
    let (x, y) = (a.x + (b.x - a.x) * f, a.y + (b.y - a.y) * f);
    (p.x - x).hypot(p.y - y)
}
