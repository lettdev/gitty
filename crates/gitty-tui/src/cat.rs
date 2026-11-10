//! Gitty's cat: a small sitting silhouette drawn in braille dots (2×4 per
//! character cell), with a tail that sways in a smooth curve.

use std::f64::consts::TAU;

/// Size in character cells.
pub const WIDTH: usize = 12;
pub const HEIGHT: usize = 6;
const DOTS_W: usize = WIDTH * 2;
const DOTS_H: usize = HEIGHT * 4;
/// One full sway of the tail, in milliseconds.
const PERIOD_MS: f64 = 2_600.0;

fn ellipse(x: f64, y: f64, cx: f64, cy: f64, rx: f64, ry: f64) -> bool {
    ((x - cx) / rx).powi(2) + ((y - cy) / ry).powi(2) <= 1.0
}

fn triangle(p: (f64, f64), a: (f64, f64), b: (f64, f64), c: (f64, f64)) -> bool {
    let side = |p: (f64, f64), q: (f64, f64), r: (f64, f64)| (p.0 - r.0) * (q.1 - r.1) - (q.0 - r.0) * (p.1 - r.1);
    let (d1, d2, d3) = (side(p, a, b), side(p, b, c), side(p, c, a));
    !((d1 < 0.0 || d2 < 0.0 || d3 < 0.0) && (d1 > 0.0 || d2 > 0.0 || d3 > 0.0))
}

/// Head, ears, body and the base it sits on — everything but the tail.
fn body(x: f64, y: f64) -> bool {
    ellipse(x, y, 8.0, 8.2, 5.0, 4.4)
        || triangle((x, y), (3.4, 6.5), (4.0, 1.2), (7.6, 4.4))
        || triangle((x, y), (8.4, 4.4), (12.0, 1.2), (12.6, 6.5))
        || ellipse(x, y, 9.0, 17.5, 6.0, 6.2)
        || ellipse(x, y, 9.2, 22.6, 6.6, 1.4)
}

/// Points along the tail at `phase` (radians): it rises from the back of the
/// body in an S and sways most near the tip, thinning as it goes.
fn tail(phase: f64) -> impl Iterator<Item = (f64, f64, f64)> {
    (0..400).map(move |i| {
        let t = i as f64 / 399.0;
        let sway = (phase - t * 2.4).sin() * 2.6 * t.powf(1.3);
        let x = 14.5 + 4.6 * (t * 2.0).sin() + sway;
        let y = 22.4 - 12.5 * t;
        (x, y, 1.25 - 0.3 * t)
    })
}

/// The cat at `ms` milliseconds into its animation, as `HEIGHT` rows of braille.
pub fn frame(ms: u128) -> Vec<String> {
    let phase = (ms as f64 % PERIOD_MS) / PERIOD_MS * TAU;
    let mut dots = [[false; DOTS_W]; DOTS_H];
    for (y, row) in dots.iter_mut().enumerate() {
        for (x, d) in row.iter_mut().enumerate() {
            *d = body(x as f64 + 0.5, y as f64 + 0.5);
        }
    }
    for (tx, ty, w) in tail(phase) {
        let (x0, y0) = (tx as i64, ty as i64);
        for yy in y0 - 2..=y0 + 2 {
            for xx in x0 - 2..=x0 + 2 {
                if xx < 0 || yy < 0 || xx as usize >= DOTS_W || yy as usize >= DOTS_H {
                    continue;
                }
                let (dx, dy) = (xx as f64 + 0.5 - tx, yy as f64 + 0.5 - ty);
                if dx * dx + dy * dy <= w * w {
                    dots[yy as usize][xx as usize] = true;
                }
            }
        }
    }
    // braille dot numbering: columns of 1-2-3-7 and 4-5-6-8
    const BITS: [(usize, usize, u32); 8] = [(0, 0, 0x01), (0, 1, 0x02), (0, 2, 0x04), (1, 0, 0x08), (1, 1, 0x10), (1, 2, 0x20), (0, 3, 0x40), (1, 3, 0x80)];
    (0..HEIGHT)
        .map(|cy| {
            (0..WIDTH)
                .map(|cx| {
                    let bits = BITS.iter().filter(|(dx, dy, _)| dots[cy * 4 + dy][cx * 2 + dx]).map(|b| b.2).sum::<u32>();
                    char::from_u32(0x2800 + bits).unwrap_or(' ')
                })
                .collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_are_braille_and_the_tail_moves() {
        let a = frame(0);
        assert_eq!(a.len(), HEIGHT);
        assert!(a.iter().all(|r| r.chars().count() == WIDTH && r.chars().all(|c| ('\u{2800}'..='\u{28ff}').contains(&c))));
        let b = frame(PERIOD_MS as u128 / 2);
        assert_ne!(a, b, "the tail sways");
        // the head and ears (left half of the top rows) stay still
        let left = |f: &[String]| f.iter().take(3).map(|r| r.chars().take(6).collect::<String>()).collect::<Vec<_>>();
        assert_eq!(left(&a), left(&b));
        assert_eq!(frame(0), frame(PERIOD_MS as u128), "loops seamlessly");
    }
}
