//! The forms Revit has beside an extrusion and a revolve: an extrusion with holes in its profile, a blend (a bottom profile
//! and a top one, the sides ruled between them) and a sweep (a profile along a path, mitred at each corner of it), with a swept
//! blend's profile changing along the way. Each comes out as triangles turned out, which [`along`] and [`blend`] make sure of
//! by their volume's sign.

use super::{P2, P3, Plane, cap, embed};
use crate::geom::v3::{add, cross, mul};

fn sub(a: P3, b: P3) -> P3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: P3, b: P3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn unit(a: P3) -> Option<P3> {
    let l = dot(a, a).sqrt();
    (l > 1e-9 && l.is_finite()).then(|| mul(a, 1.0 / l))
}

/// The volume `tris` enclose, mm³: positive when they're turned out.
pub(crate) fn volume(tris: &[[P3; 3]]) -> f64 {
    tris.iter().map(|[a, b, c]| dot(*a, cross(*b, *c))).sum::<f64>() / 6.0
}

/// `tris` turned out: the other way round where they enclose a negative volume.
fn outward(mut tris: Vec<[P3; 3]>) -> Vec<[P3; 3]> {
    if volume(&tris) < 0.0 {
        tris.iter_mut().for_each(|t| t.swap(1, 2));
    }
    tris
}

/// `ring` counter-clockwise.
fn ccw(ring: Vec<P2>) -> Vec<P2> {
    if crate::geom::area(&ring) < 0.0 { ring.into_iter().rev().collect() } else { ring }
}

/// `ring` (counter-clockwise) with `holes` in it swept across `plane` from `from` to `to`: its caps cut round the holes, its
/// sides and the holes' sides, every triangle turned out.
pub(crate) fn holed(ring: &[P2], holes: &[Vec<P2>], from: f64, to: f64, plane: Plane) -> Result<Vec<[P3; 3]>, String> {
    let holes: Vec<Vec<P2>> = holes.iter().map(|h| ccw(h.clone()).into_iter().rev().collect()).collect();
    let t = crate::triangulation::constrained(ring, &holes, &[], &[]).map_err(|e| format!("its profile and its holes: {e}"))?;
    let pt = |i: usize| t.points.get(i).copied().unwrap_or_default();
    let at = |p: P2, z: f64| embed(plane, p, z);
    let mut tris = Vec::new();
    for [a, b, c] in &t.triangles {
        tris.push([at(pt(*a), to), at(pt(*b), to), at(pt(*c), to)]);
        tris.push([at(pt(*a), from), at(pt(*c), from), at(pt(*b), from)]);
    }
    for r in std::iter::once(ring).chain(holes.iter().map(Vec::as_slice)) {
        for (a, b) in crate::geom::ring_edges(r) {
            let (a0, b0, a1, b1) = (at(a, from), at(b, from), at(a, to), at(b, to));
            tris.push([a0, b0, b1]);
            tris.push([a0, b1, a1]);
        }
    }
    Ok(outward(tris))
}

/// The point of closed `ring` at `s` of the way round it (0 to 1) from its first corner, by length.
fn at_length(ring: &[P2], s: f64) -> P2 {
    let edges: Vec<(P2, P2)> = crate::geom::ring_edges(ring).collect();
    let total: f64 = edges.iter().map(|(a, b)| a.dist(*b)).sum();
    let mut left = s.rem_euclid(1.0) * total;
    for (a, b) in &edges {
        let l = a.dist(*b);
        if left <= l && l > 0.0 {
            return *a + (*b - *a) * (left / l);
        }
        left -= l;
    }
    ring.first().copied().unwrap_or_default()
}

/// Each corner's place round closed `ring`, 0 to 1 of its length from its first.
fn places(ring: &[P2]) -> Vec<f64> {
    let total: f64 = crate::geom::ring_edges(ring).map(|(a, b)| a.dist(b)).sum();
    let mut out = Vec::with_capacity(ring.len());
    let mut run = 0.0;
    for (a, b) in crate::geom::ring_edges(ring) {
        out.push(if total > 0.0 { run / total } else { 0.0 });
        run += a.dist(b);
    }
    out
}

/// Two rings paired corner to corner, as a blend's sides run: `top` turned to start at its corner nearest `bottom`'s first
/// (by their angles round their middles); one for one where they have as many corners, else each with a corner put where
/// the other has one, along their lengths.
pub(crate) fn paired(bottom: &[P2], top: &[P2]) -> (Vec<P2>, Vec<P2>) {
    let middle = |r: &[P2]| {
        let n = r.len().max(1) as f64;
        P2(r.iter().map(|p| p.0).sum::<f64>() / n, r.iter().map(|p| p.1).sum::<f64>() / n)
    };
    let (mb, mt) = (middle(bottom), middle(top));
    let angle = |p: P2, m: P2| (p.1 - m.1).atan2(p.0 - m.0);
    let start = bottom.first().map_or(0.0, |p| angle(*p, mb));
    let gap = |a: f64| {
        let d = (a - start).rem_euclid(std::f64::consts::TAU);
        d.min(std::f64::consts::TAU - d)
    };
    let first = (0..top.len()).min_by(|i, j| gap(angle(top[*i], mt)).total_cmp(&gap(angle(top[*j], mt)))).unwrap_or(0);
    let top: Vec<P2> = top[first..].iter().chain(&top[..first]).copied().collect();
    if bottom.len() == top.len() {
        return (bottom.to_vec(), top);
    }
    let mut at: Vec<f64> = places(bottom).into_iter().chain(places(&top)).collect();
    at.sort_by(f64::total_cmp);
    at.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    (at.iter().map(|s| at_length(bottom, *s)).collect(), at.iter().map(|s| at_length(&top, *s)).collect())
}

/// A blend: `bottom` at `from` and `top` at `to` across `plane`, their corners paired ([`paired`]), its sides ruled between
/// them, every triangle turned out.
pub(crate) fn blend(bottom: &[P2], top: &[P2], from: f64, to: f64, plane: Plane) -> Result<Vec<[P3; 3]>, String> {
    let (bottom, top) = (ccw(bottom.to_vec()), ccw(top.to_vec()));
    let (b, t) = paired(&bottom, &top);
    let mut tris = Vec::new();
    for [x, y, z] in cap(&bottom)? {
        tris.push([embed(plane, bottom[x], from), embed(plane, bottom[z], from), embed(plane, bottom[y], from)]);
    }
    for [x, y, z] in cap(&top)? {
        tris.push([embed(plane, top[x], to), embed(plane, top[y], to), embed(plane, top[z], to)]);
    }
    let n = b.len();
    for i in 0..n {
        let j = (i + 1) % n;
        let (a0, b0, a1, b1) = (embed(plane, b[i], from), embed(plane, b[j], from), embed(plane, t[i], to), embed(plane, t[j], to));
        if a0 != b0 || a1 != b1 {
            tris.push([a0, b0, b1]);
            tris.push([a0, b1, a1]);
        }
    }
    Ok(outward(tris))
}

/// `v` turned the way that takes unit `from` to unit `to` (Rodrigues).
fn turned(v: P3, from: P3, to: P3) -> P3 {
    let k = cross(from, to);
    let (s, c) = (dot(k, k).sqrt(), dot(from, to));
    if s < 1e-12 {
        // Straight on, or straight back: back turns it half round about any line square to `from`.
        return if c > 0.0 { v } else { sub(mul(from, 2.0 * dot(v, from)), v) };
    }
    let k = mul(k, 1.0 / s);
    add(add(mul(v, c), mul(cross(k, v), s)), mul(k, dot(k, v) * (1.0 - c)))
}

/// A profile `section` (points in 3D at `path`'s first point) swept along `path`, mitred at each of its corners, its ends
/// capped, or joined where the path closes; a swept blend's changing on the way to `section_end` (at the path's last point,
/// as many corners as `section`). Every triangle turned out.
#[cfg(test)]
pub(crate) fn along(path: &[P3], section: &[P3], section_end: &[P3]) -> Result<Vec<[P3; 3]>, String> {
    along_holed(path, section, section_end, &[])
}

/// A loop of a profile at a sweep's start, and at its end for a swept blend (as many corners; none, the same).
pub(crate) type Section = (Vec<P3>, Vec<P3>);

/// [`along`] with `holes` in its profile (loops inside it, each at the path's start and, for a swept blend, at its end): a
/// tube's inside swept with it, its caps cut round them.
pub(crate) fn along_holed(path: &[P3], section: &[P3], section_end: &[P3], holes: &[Section]) -> Result<Vec<[P3; 3]>, String> {
    let mut path: Vec<P3> = path.to_vec();
    path.dedup_by(|a, b| dot(sub(*a, *b), sub(*a, *b)) < 1e-12);
    if path.len() < 2 {
        return Err("its path needs two points or more, apart".into());
    }
    if section.len() < 3 || (!section_end.is_empty() && section_end.len() != section.len()) {
        return Err("its profile needs three corners or more, its end profile as many".into());
    }
    if holes.iter().any(|(s, e)| s.len() < 3 || (!e.is_empty() && e.len() != s.len())) {
        return Err("a loop inside its profile needs three corners or more, at its end as many".into());
    }
    let closed = path.len() > 2 && path.first().zip(path.last()).is_some_and(|(a, b)| dot(sub(*a, *b), sub(*a, *b)) < 1e-6);
    if closed {
        path.pop();
    }
    let n = path.len();
    let legs = if closed { n } else { n - 1 };
    let dirs: Vec<P3> =
        (0..legs).map(|i| unit(sub(path[(i + 1) % n], path[i]))).collect::<Option<_>>().ok_or("its path doubles back on itself")?;
    let t0 = dirs[0];
    let p0 = path[0];
    // The profile in the frame of the first leg: across it, in a plane square to it.
    let offset =
        |s: &[P3], base: P3, dir: P3| -> Vec<P3> { s.iter().map(|q| sub(*q, base)).map(|o| sub(o, mul(dir, dot(o, dir)))).collect() };
    // The profile is drawn square to one leg of the path, anywhere along it (Revit's profile at the middle of a curve of
    // the path): found by its plane and where its middle is, and taken back along the turns to the first leg's frame.
    let centre = mul(section.iter().fold([0.0; 3], |a, p| add(a, *p)), 1.0 / section.len() as f64);
    let facing = unit((0..section.len()).map(|i| cross(section[i], section[(i + 1) % section.len()])).fold([0.0; 3], add));
    let leg = facing
        .and_then(|nrm| {
            (0..legs)
                // Square to the leg, near enough for a chord of an arc (a profile at a curved path's start), and the
                // leg nearest its middle.
                .filter(|i| dot(nrm, dirs[*i]).abs() > 0.95)
                .map(|i| {
                    let len = dot(sub(path[(i + 1) % n], path[i]), dirs[i]);
                    let t = dot(sub(centre, path[i]), dirs[i]).clamp(0.0, len.max(0.0));
                    let off = sub(sub(centre, path[i]), mul(dirs[i], t));
                    (i, dot(off, off))
                })
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(i, _)| i)
        })
        .unwrap_or(0);
    // Each loop at the start and at the end, in the first leg's frame.
    let framed = |s: &[P3], e: &[P3]| -> Section {
        let mut start = offset(s, path[leg], dirs[leg]);
        for k in (1..=leg).rev() {
            start = start.into_iter().map(|o| turned(o, dirs[k], dirs[k - 1])).collect();
        }
        // The end profile, taken back to the first leg's frame by undoing the turns along the way.
        let end = if e.is_empty() {
            start.clone()
        } else {
            let last = dirs[legs - 1];
            let mut e = offset(e, if closed { p0 } else { path[n - 1] }, last);
            for i in (1..legs).rev() {
                e = e.into_iter().map(|o| turned(o, dirs[i], dirs[i - 1])).collect();
            }
            e
        };
        (start, end)
    };
    // Each loop turned about the path the way the sides and the caps take it: the outer counter-clockwise, a hole the other
    // way, so its sides face into the tube.
    let oriented = |(start, end): Section, outer: bool| -> Section {
        let turn: P3 = (0..start.len()).map(|i| cross(start[i], start[(i + 1) % start.len()])).fold([0.0; 3], add);
        if (dot(turn, t0) < 0.0) == outer { (start.into_iter().rev().collect(), end.into_iter().rev().collect()) } else { (start, end) }
    };
    let mut loops = vec![oriented(framed(section, section_end), true)];
    for (s, e) in holes {
        loops.push(oriented(framed(s, e), false));
    }
    // How far along the path each corner is, 0 to 1.
    let lengths: Vec<f64> = (0..legs).map(|i| dot(sub(path[(i + 1) % n], path[i]), dirs[i])).collect();
    let total: f64 = lengths.iter().sum();
    let mut tris = Vec::new();
    // Each loop's rings where the path starts and ends.
    let mut ends: Vec<(Vec<P3>, Vec<P3>)> = Vec::new();
    for (start, end) in &loops {
        // The loop carried along each leg, turned at each corner as the path turns (rotation-minimising): its offsets at
        // `f` of the way along, in leg `leg`'s frame.
        let carried = |f: f64, leg: usize| -> Vec<P3> {
            let mut o: Vec<P3> = start.iter().zip(end).map(|(a, b)| add(mul(*a, 1.0 - f), mul(*b, f))).collect();
            for k in 1..=leg {
                o = o.into_iter().map(|v| turned(v, dirs[k - 1], dirs[k])).collect();
            }
            o
        };
        let mut rings: Vec<Vec<P3>> = Vec::with_capacity(n);
        let mut run = 0.0;
        for i in 0..n {
            let f = if total > 0.0 { run / total } else { 0.0 };
            let corner = path[i];
            let ring: Vec<P3> = if !closed && i == 0 {
                carried(0.0, 0).into_iter().map(|o| add(corner, o)).collect()
            } else if !closed && i == n - 1 {
                carried(1.0, legs - 1).into_iter().map(|o| add(corner, o)).collect()
            } else {
                // The leg before the corner and the one after: the profile, square to the one before, meets the mitre that
                // halves the corner.
                let (prev, next) = (if i == 0 { legs - 1 } else { i - 1 }, i % legs);
                let (before, after) = (dirs[prev], dirs[next]);
                let o = if i == 0 { carried(0.0, 0).into_iter().map(|v| turned(v, dirs[0], before)).collect() } else { carried(f, prev) };
                let mitre = unit(add(before, after)).unwrap_or(after);
                let along = dot(mitre, before);
                o.into_iter()
                    .map(|v| {
                        let s = if along.abs() > 1e-9 { -dot(v, mitre) / along } else { 0.0 };
                        add(add(corner, v), mul(before, s))
                    })
                    .collect()
            };
            rings.push(ring);
            if i < legs {
                run += lengths[i];
            }
        }
        let m = start.len();
        let count = rings.len();
        let sides = if closed { count } else { count - 1 };
        for r in 0..sides {
            let (a, b) = (&rings[r], &rings[(r + 1) % count]);
            for j in 0..m {
                let k = (j + 1) % m;
                tris.push([a[j], a[k], b[k]]);
                tris.push([a[j], b[k], b[j]]);
            }
        }
        if let (Some(first), Some(last)) = (rings.first(), rings.last()) {
            ends.push((first.clone(), last.clone()));
        }
    }
    if !closed && !ends.is_empty() {
        // The caps: each end's loops laid flat in their own plane, the outer cut round the holes into triangles.
        for (first, dir) in [(true, dirs[0]), (false, dirs[legs - 1])] {
            let rings: Vec<&Vec<P3>> = ends.iter().map(|(a, b)| if first { a } else { b }).collect();
            let outer = rings[0];
            let o = outer[0];
            let u = unit(sub(outer[1 % outer.len()], o)).unwrap_or([1.0, 0.0, 0.0]);
            let v = cross(dir, u);
            let flat = |r: &[P3]| -> Vec<P2> { r.iter().map(|q| P2(dot(sub(*q, o), u), dot(sub(*q, o), v))).collect() };
            let ring = ccw(flat(outer));
            let caps: Vec<[P3; 3]> = if rings.len() == 1 {
                let idx = cap(&ring)?;
                let back = |p: P2| add(o, add(mul(u, p.0), mul(v, p.1)));
                idx.iter().map(|[a, b, c]| [back(ring[*a]), back(ring[*b]), back(ring[*c])]).collect()
            } else {
                let holes: Vec<Vec<P2>> = rings[1..].iter().map(|h| ccw(flat(h)).into_iter().rev().collect()).collect();
                let t = crate::triangulation::constrained(&ring, &holes, &[], &[]).map_err(|e| format!("its profile's end: {e}"))?;
                let back = |i: usize| {
                    let p = t.points.get(i).copied().unwrap_or_default();
                    add(o, add(mul(u, p.0), mul(v, p.1)))
                };
                t.triangles.iter().map(|[a, b, c]| [back(*a), back(*b), back(*c)]).collect()
            };
            // Counter-clockwise seen from along the path: the start's turned to face back, the end's facing on.
            tris.extend(caps.into_iter().map(|[a, b, c]| if first { [a, c, b] } else { [a, b, c] }));
        }
    }
    let tris: Vec<[P3; 3]> = tris.into_iter().filter(|[a, b, c]| a != b && b != c && a != c).collect();
    if tris.iter().flatten().flatten().any(|x| !x.is_finite()) {
        return Err("its sweep doesn't work out: a corner of its path folds back on its profile".into());
    }
    Ok(outward(tris))
}

/// Corners a true arc is drawn with per turn, as a circle's.
const PER_TURN: f64 = super::SEGMENTS as f64;

/// The points strictly between `a` and `b` of the arc about `centre` from one to the other, counter-clockwise (or clockwise),
/// its radius going from `a`'s to `b`'s where they differ; an arc from a point back to itself is a whole turn.
pub(crate) fn arc2(a: P2, b: P2, centre: P2, clockwise: bool) -> Vec<P2> {
    let (ra, rb) = (a.dist(centre), b.dist(centre));
    let a0 = (a.1 - centre.1).atan2(a.0 - centre.0);
    let a1 = (b.1 - centre.1).atan2(b.0 - centre.0);
    let tau = std::f64::consts::TAU;
    let whole = a.dist(b) < 1e-6;
    let turn = if whole {
        if clockwise { -tau } else { tau }
    } else if clockwise {
        -(a0 - a1).rem_euclid(tau)
    } else {
        (a1 - a0).rem_euclid(tau)
    };
    let steps = (turn.abs() / tau * PER_TURN).ceil().clamp(1.0, 4.0 * PER_TURN) as usize;
    (1..steps)
        .map(|k| {
            let f = k as f64 / steps as f64;
            let (s, c) = (a0 + turn * f).sin_cos();
            let r = ra + (rb - ra) * f;
            P2(centre.0 + r * c, centre.1 + r * s)
        })
        .collect()
}

/// [`arc2`] in 3D: the arc about `centre`, turning about `normal` (counter-clockwise seen from it, or clockwise).
pub(crate) fn arc3(a: P3, b: P3, centre: P3, normal: P3, clockwise: bool) -> Vec<P3> {
    let Some(n) = unit(normal) else { return Vec::new() };
    let flat = |p: P3| {
        let o = sub(p, centre);
        sub(o, mul(n, dot(o, n)))
    };
    let Some(u) = unit(flat(a)) else { return Vec::new() };
    let v = cross(n, u);
    let to2 = |p: P3| {
        let o = flat(p);
        P2(dot(o, u), dot(o, v))
    };
    let (ha, hb) = (dot(sub(a, centre), n), dot(sub(b, centre), n));
    let pts = arc2(to2(a), to2(b), P2(0.0, 0.0), clockwise);
    let count = pts.len() + 1;
    pts.into_iter()
        .enumerate()
        .map(|(k, q)| {
            let h = ha + (hb - ha) * (k + 1) as f64 / count as f64;
            add(add(centre, add(mul(u, q.0), mul(v, q.1))), mul(n, h))
        })
        .collect()
}

/// `ring` with each of `bends` (the edge from corner `edge` to the next, its centre, clockwise) drawn as its true arc:
/// the arc's points put in after the corner. An edge past the ring's end is left straight.
pub(crate) fn bent2(ring: Vec<P2>, bends: &[(usize, P2, bool)]) -> Vec<P2> {
    let n = ring.len();
    let mut out = Vec::with_capacity(n);
    for (i, p) in ring.iter().enumerate() {
        out.push(*p);
        if let Some((_, c, cw)) = bends.iter().find(|(e, _, _)| *e == i) {
            out.extend(arc2(*p, ring[(i + 1) % n], *c, *cw));
        }
    }
    out
}

/// [`bent2`] in 3D, for a sweep's path (`closed`: its last corner joins its first) and its profiles.
pub(crate) fn bent3(ring: Vec<P3>, bends: &[(usize, P3, P3, bool)], closed: bool) -> Vec<P3> {
    let n = ring.len();
    let mut out = Vec::with_capacity(n);
    for (i, p) in ring.iter().enumerate() {
        out.push(*p);
        if !closed && i + 1 == n {
            break;
        }
        if let Some((_, c, normal, cw)) = bends.iter().find(|(e, ..)| *e == i) {
            out.extend(arc3(*p, ring[(i + 1) % n], *c, *normal, *cw));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(s: f64) -> Vec<P2> {
        vec![P2(0.0, 0.0), P2(s, 0.0), P2(s, s), P2(0.0, s)]
    }

    #[test]
    fn a_holed_profile_a_blend_and_a_sweep_enclose_what_they_should() {
        let hole = vec![P2(25.0, 25.0), P2(75.0, 25.0), P2(75.0, 75.0), P2(25.0, 75.0)];
        let v = volume(&holed(&square(100.0), &[hole], 0.0, 10.0, Plane::Xy).unwrap());
        assert!((v - 7500.0 * 10.0).abs() < 1.0, "{v}");
        // A frustum from 100 square to 50 square, 30 tall: h/3 (A1 + A2 + sqrt(A1 A2)).
        let top: Vec<P2> = square(50.0).into_iter().map(|p| P2(p.0 + 25.0, p.1 + 25.0)).collect();
        let v = volume(&blend(&square(100.0), &top, 0.0, 30.0, Plane::Xy).unwrap());
        assert!((v - 10.0 * (10000.0 + 2500.0 + 5000.0)).abs() < 1.0, "{v}");
        // A 10 x 10 profile along an L of 100 and 100: 2 x 100 x 100 with the mitre in.
        let section = [[0.0, -5.0, -5.0], [0.0, 5.0, -5.0], [0.0, 5.0, 5.0], [0.0, -5.0, 5.0]];
        let v = volume(&along(&[[0.0, 0.0, 0.0], [100.0, 0.0, 0.0], [100.0, 100.0, 0.0]], &section, &[]).unwrap());
        assert!((v - 20000.0).abs() < 1.0, "{v}");
        // A swept blend from 10 square to 20 square over 100: a frustum.
        let end = [[100.0, -10.0, -10.0], [100.0, 10.0, -10.0], [100.0, 10.0, 10.0], [100.0, -10.0, 10.0]];
        let v = volume(&along(&[[0.0, 0.0, 0.0], [100.0, 0.0, 0.0]], &section, &end).unwrap());
        assert!((v - 100.0 / 3.0 * (100.0 + 400.0 + 200.0)).abs() < 1.0, "{v}");
        // A quarter arc of radius 500, a 50 x 50 profile: Pappus, within the chords' share.
        let path = bent3(vec![[500.0, 0.0, 0.0], [0.0, 500.0, 0.0]], &[(0, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0], false)], false);
        let section = [[475.0, 0.0, -25.0], [525.0, 0.0, -25.0], [525.0, 0.0, 25.0], [475.0, 0.0, 25.0]];
        let v = volume(&along(&path, &section, &[]).unwrap());
        let want = 2500.0 * std::f64::consts::FRAC_PI_2 * 500.0;
        assert!((v - want).abs() < want * 0.01, "{v} against {want}, path {path:?}");
        // Blend rings of different counts pair along their lengths.
        let tri = vec![P2(0.0, 0.0), P2(10.0, 0.0), P2(5.0, 8.0)];
        let (a, b) = paired(&square(10.0), &tri);
        assert_eq!(a.len(), b.len());
    }
}
