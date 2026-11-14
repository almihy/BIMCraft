//! A solid's volume with its curved faces as the cylinders they are, not the facets inscribed in them.
//!
//! A curved wall, a round column: the engine builds them as straight facets whose vertices are on the true circle ([`Curve`]
//! says which). Manifold's volume is that of the facets, short of the circle's by a segment's area for each facet (about
//! 0.13 % of a wall's volume at the 5 degrees a facet turns). A quantity must be the circle's, so [`Solid3::volume_on_curves`]
//! adds back what lies between each tagged facet and its arc (taken off for a concave face, the inside of a curved wall,
//! where the solid is the side the arc bulges away from): exact, in closed form, for any part of a facet that is left (a
//! door's jamb cuts through the middle of one) and for a sloped top or bottom (the height is the triangle's own).
//!
//! Which triangles are facets of a curve is decided as [`Solid3::slice`] decides it for arcs: a vertical triangle of an origin
//! with a [`Curve`], whose plane lies at a distance from the axis between `r cos(MAX_FACET_ANGLE / 2)` and `r` for the radius
//! `r` it is a chord of (the larger one where the face looks away from the axis, the smaller where it looks toward it), with
//! all of it within that cylinder.

use super::{Solid3, Tri, tol};

impl Solid3 {
    /// Enclosed volume, mm³, with every curved face the cylinder its origin's [`Curve`](super::Curve) says (see the module).
    /// Equal to [`Solid3::volume`] for a solid with no curve tag.
    pub fn volume_on_curves(&self) -> f64 {
        let facets: f64 = self.triangles().iter().map(sliver).sum();
        self.volume() + facets
    }
}

/// What lies between a facet triangle and its arc: positive where the solid is on the axis's side of the face (the outside of a
/// curved wall, a column), negative where it is on the far side (the inside of a curved wall).
fn sliver(t: &Tri) -> f64 {
    let Some(curve) = t.origin.curve else { return 0.0 };
    let n = t.normal;
    let along = n[0].hypot(n[1]);
    if n[2].abs() > 1e-6 || along < 0.5 {
        return 0.0;
    }
    let (nx, ny) = (n[0] / along, n[1] / along);
    let c = curve.centre;
    // How far the face's plane is from the axis, signed: positive where the face looks away from it.
    let h = (t.p[0][0] - c.0) * nx + (t.p[0][1] - c.1) * ny;
    let d = h.abs();
    let (lo, hi) = (curve.radii[0].min(curve.radii[1]), curve.radii[0].max(curve.radii[1]));
    let r = if h > 0.0 { hi } else { lo };
    let within = t.p.iter().all(|p| (p[0] - c.0).hypot(p[1] - c.1) <= r + tol::CURVE);
    if !within || d > r + tol::CURVE || d < r * (tol::MAX_FACET_ANGLE / 2.0).cos() {
        return 0.0;
    }
    // The plane's own axes: s along it from the foot of the perpendicular from the axis, z up.
    let (fx, fy) = (c.0 + nx * h, c.1 + ny * h);
    let (tx, ty) = (-ny, nx);
    let mut pts: [(f64, f64); 3] = t.p.map(|p| ((p[0] - fx) * tx + (p[1] - fy) * ty, p[2]));
    pts.sort_by(|a, b| a.0.total_cmp(&b.0));
    let [(s0, z0), (s1, z1), (s2, z2)] = pts;
    if s2 - s0 < 1e-12 {
        return 0.0;
    }
    // The triangle's height over s rises from nothing at s0 to `ell` at s1 and falls to nothing at s2.
    let on_long = z0 + (z2 - z0) * (s1 - s0) / (s2 - s0);
    let ell = (on_long - z1).abs();
    let r2 = r * r;
    // The antiderivatives of g(s) = sqrt(r² - s²) - d, and of s g(s).
    let root = |s: f64| (r2 - s * s).max(0.0).sqrt();
    let f0 = |s: f64| (s * root(s) + r2 * (s / r).clamp(-1.0, 1.0).asin()) / 2.0 - d * s;
    let f1 = |s: f64| -(root(s).powi(3)) / 3.0 - d * s * s / 2.0;
    // The integral of g(s) (alpha + beta s) from a to b.
    let part = |a: f64, b: f64, alpha: f64, beta: f64| alpha * (f0(b) - f0(a)) + beta * (f1(b) - f1(a));
    let mut sum = 0.0;
    if s1 - s0 > 1e-12 {
        let k = ell / (s1 - s0);
        sum += part(s0, s1, -k * s0, k);
    }
    if s2 - s1 > 1e-12 {
        let k = ell / (s2 - s1);
        sum += part(s1, s2, k * s2, -k);
    }
    if h > 0.0 { sum } else { -sum }
}
