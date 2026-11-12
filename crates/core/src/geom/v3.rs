//! 3D vector maths: points and directions in model millimetres, as arrays.

/// A model point in mm: x east, y north, z up.
pub type P3 = [f64; 3];

pub fn add(a: P3, b: P3) -> P3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
pub fn sub(a: P3, b: P3) -> P3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
pub fn mul(a: P3, k: f64) -> P3 {
    [a[0] * k, a[1] * k, a[2] * k]
}
pub fn dot(a: P3, b: P3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
pub fn cross(a: P3, b: P3) -> P3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
pub fn unit(a: P3) -> P3 {
    let l = dot(a, a).sqrt();
    if l > 0.0 { mul(a, 1.0 / l) } else { [0.0; 3] }
}

/// Of `points`, those that may be the farthest any way across or up: each place in plan once, and the lowest and highest
/// (of those that aren't NaN, as `f64::min` and `f64::max` leave it out). The farthest of them any way is the farthest of
/// all, to the bit.
pub fn extremes(points: impl Iterator<Item = P3>) -> Vec<P3> {
    let mut seen = std::collections::HashSet::new();
    let (mut low, mut high): (Option<P3>, Option<P3>) = (None, None);
    let mut out = Vec::new();
    for p in points {
        if !p[2].is_nan() {
            if low.is_none_or(|l| p[2] < l[2]) {
                low = Some(p);
            }
            if high.is_none_or(|h| p[2] > h[2]) {
                high = Some(p);
            }
        }
        if seen.insert((p[0].to_bits(), p[1].to_bits())) {
            out.push(p);
        }
    }
    out.extend(low);
    out.extend(high);
    out
}
