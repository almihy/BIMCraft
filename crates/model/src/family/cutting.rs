//! What a family's voids cut from its forms: a closed form cut by a closed void is cut on the solid kernel
//! ([`bimcraft_geom::solid`], Manifold), with no limit on how many triangles either has; a form or a void that is not a closed
//! solid (a triangle soup with holes, as Revit's result can be) is cut by [`csg::subtract`], the older cutter, which has one
//! ([`csg::MAX_TRIANGLES`]). The family check says which forms are not closed ([`Family::open_forms`]).

use bimcraft_geom::solid::{Origin, Solid3};

use super::csg;
use crate::geom::v3::P3;

/// What a form of the family's solid is cut from, and what a void is, to the kernel: told apart in the solids' origins.
const FORM: Origin = Origin { element: 0, layer: 0, curve: None };
const VOID: Origin = Origin { element: 0, layer: 1, curve: None };

/// How near equal two volumes are, as a share, for a cut to have taken nothing.
const SAME: f64 = 1e-12;

/// A void ready to cut with: its triangles, its box, and its solid where it is a closed one (else why not).
pub(super) struct Void<'a> {
    /// Its form's index in the family's list.
    pub form: usize,
    tris: &'a [[P3; 3]],
    bounds: (P3, P3),
    solid: Result<Solid3, String>,
}

impl<'a> Void<'a> {
    pub fn new(form: usize, tris: &'a [[P3; 3]]) -> Void<'a> {
        Void { form, tris, bounds: bounds(tris), solid: as_solid(tris, VOID) }
    }
}

/// `tris` as a closed solid, or why it isn't one.
pub(super) fn as_solid(tris: &[[P3; 3]], origin: Origin) -> Result<Solid3, String> {
    Solid3::from_triangles(tris, origin).map_err(|e| e.to_string())
}

/// The corners of the box round `tris`.
fn bounds(tris: &[[P3; 3]]) -> (P3, P3) {
    tris.iter().flatten().fold(([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]), |(lo, hi), p| {
        (std::array::from_fn(|k| lo[k].min(p[k])), std::array::from_fn(|k| hi[k].max(p[k])))
    })
}

/// Whether two boxes touch or overlap.
fn meet(a: (P3, P3), b: (P3, P3)) -> bool {
    const EPS: f64 = 1e-5;
    (0..3).all(|k| b.0[k] <= a.1[k] + EPS && b.1[k] >= a.0[k] - EPS)
}

/// `form`, a closed surface of triangles turned outward, less what each of `voids` takes of it: what is left, as triangles
/// (none if the voids swallow it); the form as it was where no void reaches it. Its cut faces are the form's own surface, so
/// they take its material. Closed voids go to the kernel in one operation; the rest, and every void where the form is not
/// closed, to [`csg::subtract`], which `meter` counts.
pub(super) fn cut(form: &[[P3; 3]], voids: &[&Void], meter: &mut csg::Meter) -> Result<Vec<[P3; 3]>, String> {
    let reach = bounds(form);
    let voids: Vec<&&Void> = voids.iter().filter(|v| meet(reach, v.bounds)).collect();
    if voids.is_empty() {
        return Ok(form.to_vec());
    }
    let (mut tris, mut old): (Vec<[P3; 3]>, Vec<&&Void>) = (form.to_vec(), voids.clone());
    if let Ok(solid) = as_solid(form, FORM) {
        let (closed, open): (Vec<&&Void>, Vec<&&Void>) = voids.into_iter().partition(|v| v.solid.is_ok());
        old = open;
        if !closed.is_empty() {
            let cutters: Vec<&Solid3> = closed.iter().filter_map(|v| v.solid.as_ref().ok()).collect();
            let left = solid.difference_all(cutters);
            let (before, after) = (solid.volume(), left.volume());
            // A void that took nothing leaves the form's own triangles, not the kernel's.
            if left.triangle_count() != solid.triangle_count() || (before - after).abs() > SAME * before.abs() {
                tris = left.triangles().into_iter().map(|t| t.p).collect();
            }
        }
    }
    for v in old {
        tris = csg::subtract(&tris, v.tris, meter).map_err(|e| format!("form {}: {e}", v.form + 1))?;
    }
    Ok(tris)
}

#[cfg(test)]
pub(super) mod tests {
    use std::collections::BTreeMap;

    use serde_json::{Value, json};

    use super::super::shapes::volume;
    use super::super::{Family, Piece};
    use super::*;

    /// A one-type family of `forms`.
    fn family(forms: Value) -> Family {
        let f: Family = serde_json::from_value(json!({"types": {"A": {}}, "forms": forms})).unwrap();
        f.check(&|_| true).unwrap();
        f
    }

    fn pieces(f: &Family) -> Vec<Piece> {
        f.pieces("A", &f.values("A", &BTreeMap::new()).unwrap()).unwrap()
    }

    /// The area of a regular polygon of `n` corners on the circle of radius `r`.
    fn polygon(n: f64, r: f64) -> f64 {
        n / 2.0 * (std::f64::consts::TAU / n).sin() * r * r
    }

    /// A box from `lo` to `hi` as triangles turned out, less the first `leave` of the 12 where it is to be left open.
    pub(in crate::family) fn cuboid(lo: P3, hi: P3, leave: usize) -> Vec<[P3; 3]> {
        let v = |x: usize, y: usize, z: usize| [[lo[0], hi[0]][x], [lo[1], hi[1]][y], [lo[2], hi[2]][z]];
        [
            [v(0, 0, 1), v(1, 0, 1), v(1, 1, 1), v(0, 1, 1)],
            [v(0, 0, 0), v(0, 1, 0), v(1, 1, 0), v(1, 0, 0)],
            [v(0, 0, 0), v(1, 0, 0), v(1, 0, 1), v(0, 0, 1)],
            [v(1, 1, 0), v(0, 1, 0), v(0, 1, 1), v(1, 1, 1)],
            [v(0, 1, 0), v(0, 0, 0), v(0, 0, 1), v(0, 1, 1)],
            [v(1, 0, 0), v(1, 1, 0), v(1, 1, 1), v(1, 0, 1)],
        ]
        .iter()
        .flat_map(|[a, b, c, d]| [[*a, *b, *c], [*a, *c, *d]])
        .skip(leave)
        .collect()
    }

    /// A cylinder of `n` sides about the z axis, from 0 to `h`, its caps fans from their centres.
    fn cylinder(n: usize, r: f64, h: f64) -> Vec<[P3; 3]> {
        let at = |k: usize, z: f64| {
            let (s, c) = (k as f64 * std::f64::consts::TAU / n as f64).sin_cos();
            [r * c, r * s, z]
        };
        let mut out = Vec::with_capacity(4 * n);
        for k in 0..n {
            let j = (k + 1) % n;
            out.push([at(k, 0.0), at(j, 0.0), at(j, h)]);
            out.push([at(k, 0.0), at(j, h), at(k, h)]);
            out.push([[0.0, 0.0, h], at(k, h), at(j, h)]);
            out.push([[0.0, 0.0, 0.0], at(j, 0.0), at(k, 0.0)]);
        }
        out
    }

    #[test]
    fn a_curved_tube_with_two_voids_is_cut_exactly() {
        // A tube, 100 to 300 out and 500 high; a ring-shaped void through its outer wall (250 to 350 out, 100 to 200 up) and a
        // slot through its top, wholly in its wall (80 by 100 and clear of the ring). A revolve is drawn with as many sides as keep
        // a chord within half a millimetre of the circle (`family::sides`) at the largest radius of its profile: 55 for the tube
        // (out to 300), 59 for the ring (out to 350), so the ring's inner polygon is not the tube's.
        let tube = json!({"plane": "xz", "profile": [[100, 0], [300, 0], [300, 500], [100, 500]], "axis": [[0, 0], [0, 1]], "to": 360});
        let ring = json!({"plane": "xz", "profile": [[250, 100], [350, 100], [350, 200], [250, 200]], "axis": [[0, 0], [0, 1]], "to": 360, "void": true});
        let slot = json!({"profile": [[150, -50], [230, -50], [230, 50], [150, 50]], "from": -10, "to": 510, "void": true});
        let f = family(json!([tube, ring, slot]));
        let cut = pieces(&f);
        let [p] = cut.as_slice() else { panic!("{cut:?}") };
        let tau = std::f64::consts::TAU;
        let (n_tube, n_ring) = (super::super::sides(tau, 300.0), super::super::sides(tau, 350.0));
        assert_eq!((n_tube, n_ring), (55, 59));
        let (outer, inner, sleeve) = (polygon(n_tube as f64, 300.0), polygon(n_tube as f64, 100.0), polygon(n_ring as f64, 250.0));
        // The ring takes what lies outside its inner polygon (the tube's outer polygon is wholly inside its outer one).
        let exact = (outer - inner) * 500.0 - (outer - sleeve) * 100.0 - 80.0 * 100.0 * 500.0;
        let v = volume(&p.tris);
        assert!((v / exact - 1.0).abs() < 1e-9, "{v} not {exact}");
        // The tube's own circles are no more than a facet off the true ones: the true volume is a hair more.
        let round = (std::f64::consts::PI * (300.0f64.powi(2) - 100.0f64.powi(2))) * 500.0
            - std::f64::consts::PI * (300.0f64.powi(2) - 250.0f64.powi(2)) * 100.0
            - 80.0 * 100.0 * 500.0;
        assert!((v / round - 1.0).abs() < 0.01, "{v} against the circles' {round}");
        // What came back is one closed solid, cut by the kernel, with the same volume as the older cutter makes of it.
        assert!(as_solid(&p.tris, FORM).is_ok());
        assert_eq!((p.bottom, p.top), (0.0, 500.0));
        let mut by_csg = pieces(&family(json!([tube])))[0].tris.clone();
        for void in [&ring, &slot] {
            let mut solid = void.clone();
            solid["void"] = json!(false);
            by_csg = csg::subtract(&by_csg, &pieces(&family(json!([solid])))[0].tris, &mut csg::Meter::default()).unwrap();
        }
        assert!((volume(&by_csg) / v - 1.0).abs() < 1e-6, "csg {} against the kernel {v}", volume(&by_csg));
    }

    #[test]
    fn a_form_of_fifty_thousand_triangles_is_cut_past_the_old_limit() {
        // A round block of 12,500 sides (50,000 triangles) with a slot across it, 400 wide.
        let (n, r, h) = (12_500, 1000.0, 1000.0);
        let block = cylinder(n, r, h);
        assert_eq!(block.len(), 50_000);
        let slot = json!({"profile": [[-200, -2000], [200, -2000], [200, 2000], [-200, 2000]], "from": -10, "to": 1010, "void": true});
        let f = family(json!([{"mesh": block}, slot]));
        // The old cutter refuses a form this size.
        let one = cuboid([-200.0, -2000.0, -10.0], [200.0, 2000.0, 1010.0], 0);
        let refused = csg::subtract(&block, &one, &mut csg::Meter::default()).unwrap_err();
        assert!(refused.contains("50000 triangles") || refused.contains("can't have more than"), "{refused}");
        let started = std::time::Instant::now();
        let cut = pieces(&f);
        let took = started.elapsed();
        eprintln!("50,000-triangle form cut by a void: {took:?}, {} triangles out", cut[0].tris.len());
        let [p] = cut.as_slice() else { panic!("{cut:?}") };
        // The slot takes the strip |x| < 200 of the disc: 2 (x sqrt(R^2 - x^2) + R^2 asin(x / R)) at 200.
        let strip = 2.0 * (200.0 * (r * r - 200.0 * 200.0_f64).sqrt() + r * r * (200.0 / r).asin());
        let exact = (polygon(n as f64, r) - strip) * h;
        let v = volume(&p.tris);
        assert!((v / exact - 1.0).abs() < 1e-6, "{v} not {exact}");
        assert!(as_solid(&p.tris, FORM).is_ok());
        assert!(took.as_secs_f64() < 60.0, "{took:?}");
    }

    #[test]
    fn a_void_that_is_not_closed_is_cut_by_the_old_cutter_and_the_check_names_it() {
        let slab = json!({"profile": [[0, 0], [1000, 0], [1000, 1000], [0, 1000]], "from": 0, "to": 100});
        // A box of a void with one triangle missing: a hole in its surface.
        let open = cuboid([400.0, 400.0, -10.0], [600.0, 600.0, 110.0], 1);
        let closed = cuboid([400.0, 400.0, -10.0], [600.0, 600.0, 110.0], 0);
        let f = family(json!([slab, {"mesh": open, "void": true}]));
        let by_csg = csg::subtract(&pieces(&family(json!([slab])))[0].tris, &open, &mut csg::Meter::default()).unwrap();
        let cut = pieces(&f);
        assert_eq!(cut[0].tris, by_csg, "the open void is cut by csg::subtract, as it always was");
        let named = f.open_forms("A", &f.values("A", &BTreeMap::new()).unwrap());
        assert_eq!(named.len(), 1, "{named:?}");
        assert!(named[0].starts_with("void 2 ") && named[0].contains("not a closed solid") && named[0].contains("open edge"), "{named:?}");
        // The same void closed is cut by the kernel, and nothing is said of it.
        let g = family(json!([slab, {"mesh": closed, "void": true}]));
        assert!(g.open_forms("A", &g.values("A", &BTreeMap::new()).unwrap()).is_empty());
        assert!((volume(&pieces(&g)[0].tris) - (1e6 - 200.0 * 200.0) * 100.0).abs() < 1e-3);
        // An open form is named too, as a form.
        let h = family(json!([{"mesh": cuboid([0.0, 0.0, 0.0], [10.0, 10.0, 10.0], 2)}]));
        let named = h.open_forms("A", &h.values("A", &BTreeMap::new()).unwrap());
        assert!(named.len() == 1 && named[0].starts_with("form 1 "), "{named:?}");
    }
}
