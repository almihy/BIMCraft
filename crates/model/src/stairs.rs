//! Stairs and ramps as runs and landings, and the railings along them: where each run goes and how
//! high, the landings between runs, the walk line up the middle, and the line a railing follows,
//! in from a side.

use crate::geom::{self, P2};
use crate::model::{Element, Model, RAILING_INSET, Railing, Ramp, Side, Stair, heading};
use crate::walls::Plane;

/// A run: from the middle of its foot, `start`, along unit `u`, `length` long, from height `z.0`
/// up to `z.1`. A stair's has `risers` risers, its first at its start and its last at its end.
///
/// Its ends are square to it, but where a ramp's path bends without a landing (a corner, or a
/// curve cut in chords): there each end lies along `across` (unit, to the run's left), mitred
/// with the next run's, and `joined` says it goes on from the run before, with no line across.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Run {
    pub start: P2,
    pub u: P2,
    pub length: f64,
    pub z: (f64, f64),
    pub risers: u32,
    pub across: (P2, P2),
    pub joined: bool,
}

impl Run {
    /// A straight run with square ends.
    pub fn new(start: P2, u: P2, length: f64, z: (f64, f64), risers: u32) -> Run {
        Run { start, u, length, z, risers, across: (u.perp(), u.perp()), joined: false }
    }

    pub fn end(&self) -> P2 {
        self.start + self.u * self.length
    }

    /// Its outline, `width` wide, counter-clockwise: its sides, and its ends along `across`.
    pub fn ring(&self, width: f64) -> Vec<P2> {
        let n = self.u.perp();
        // Along a cross line from the middle to where it meets the side `width / 2` from the middle.
        let reach = |a: P2| a * (width / 2.0 / a.dot(n).max(0.2));
        let (a, b) = (reach(self.across.0), reach(self.across.1));
        vec![self.start - a, self.end() - b, self.end() + b, self.start + a]
    }

    /// Its height `along` its length (0 at its start).
    pub fn z_at(&self, along: f64) -> f64 {
        if self.length <= 1e-9 { self.z.1 } else { self.z.0 + (self.z.1 - self.z.0) * (along / self.length).clamp(0.0, 1.0) }
    }
}

/// A landing: its outline, counter-clockwise, and its height.
#[derive(Clone, Debug, PartialEq)]
pub struct Pad {
    pub ring: Vec<P2>,
    pub z: f64,
}

/// A stair's or a ramp's runs and landings, how wide they are, and its walk line: up the middle of
/// each run, from its foot to its top, and across each landing, with the height of each point.
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    pub runs: Vec<Run>,
    pub pads: Vec<Pad>,
    pub walk: Vec<(P2, f64)>,
    pub width: f64,
}

/// Runs of `(length, heights, risers)` from `start` toward `angle` degrees, `width` wide, and a
/// landing after each but the last, as deep as they're wide, the next run going on as `turns` say:
/// straight on (0), left or right (90, -90), or back beside it, to its left or right (180, -180).
fn chain(start: P2, angle: f64, width: f64, runs: Vec<(f64, (f64, f64), u32)>, turns: &[f64]) -> Layout {
    let (mut at, mut u) = (start, heading(angle));
    let mut out = Layout { runs: Vec::new(), pads: Vec::new(), walk: Vec::new(), width };
    for (i, (length, z, risers)) in runs.into_iter().enumerate() {
        let run = Run::new(at, u, length, z, risers);
        out.walk.extend([(run.start, z.0), (run.end(), z.1)]);
        out.runs.push(run);
        let Some(&turn) = turns.get(i) else { break };
        let (e, n, w) = (run.end(), u.perp(), width);
        // Across the landing from the run's right edge to its left, and how far it reaches past the
        // run: to the next run's far edge, beside it when it turns back.
        let side = n * if turn > 0.0 { 1.0 } else { -1.0 };
        let (ring, via, next, dir) = if turn == 0.0 {
            (vec![e - n * (w / 2.0), e - n * (w / 2.0) + u * w, e + n * (w / 2.0) + u * w, e + n * (w / 2.0)], vec![], e + u * w, u)
        } else if turn.abs() == 90.0 {
            let ring = vec![e - n * (w / 2.0), e - n * (w / 2.0) + u * w, e + n * (w / 2.0) + u * w, e + n * (w / 2.0)];
            (ring, vec![e + u * (w / 2.0)], e + u * (w / 2.0) + side * (w / 2.0), side)
        } else {
            let ring = vec![e - side * (w / 2.0), e - side * (w / 2.0) + u * w, e + side * (1.5 * w) + u * w, e + side * (1.5 * w)];
            (ring, vec![e + u * (w / 2.0), e + u * (w / 2.0) + side * w], e + side * w, u * -1.0)
        };
        out.pads.push(Pad { ring: ccw(ring), z: z.1 });
        out.walk.extend(via.into_iter().map(|p| (p, z.1)));
        (at, u) = (next, dir);
    }
    out
}

fn ccw(ring: Vec<P2>) -> Vec<P2> {
    if geom::area(&ring) < 0.0 { ring.into_iter().rev().collect() } else { ring }
}

/// A stair's runs: its risers split at its landings, each run a tread shorter than its risers.
pub fn stair(m: &Model, st: &Stair) -> Layout {
    let (n, each, _) = m.flight(st);
    let base = m.elevation(st.level);
    let mut breaks: Vec<u32> = vec![0];
    breaks.extend(st.landings.iter().map(|l| (l.at.max(0.0).min(f64::from(n))) as u32));
    breaks.push(n);
    let runs = breaks
        .windows(2)
        .filter_map(|w| match *w {
            [a, b] => Some((
                f64::from(b.saturating_sub(a).saturating_sub(1)) * st.tread,
                (base + f64::from(a) * each, base + f64::from(b) * each),
                b.saturating_sub(a),
            )),
            _ => None,
        })
        .collect();
    let turns: Vec<f64> = st.landings.iter().map(|l| l.turn).collect();
    chain(st.start, st.angle, st.width, runs, &turns)
}

/// A ramp's runs: along its path when it has one ([`along_path`]); else its rise split at its
/// landings' heights, each run as long as its own slope (or the ramp's) makes it.
pub fn ramp(m: &Model, r: &Ramp) -> Layout {
    let (base, top) = m.span(r.level, 0.0, r.top_level, r.height);
    if r.path.len() >= 2 {
        return along_path(r, base, top);
    }
    let slope = |i: usize| {
        let s = r.slopes.get(i).copied().filter(|s| s.is_finite() && *s > 0.0).unwrap_or(r.slope);
        if s > 0.0 { s / 100.0 } else { 1.0 }
    };
    let mut ups: Vec<f64> = vec![0.0];
    ups.extend(r.landings.iter().map(|l| l.at));
    ups.push(top - base);
    let runs = ups
        .windows(2)
        .enumerate()
        .filter_map(|(i, w)| match *w {
            [a, b] => Some(((b - a) / slope(i), (base + a, base + b), 0)),
            _ => None,
        })
        .collect();
    let turns: Vec<f64> = r.landings.iter().map(|l| l.turn).collect();
    chain(r.start, r.angle, r.width, runs, &turns)
}

/// Each piece of a ramp's path (a line, or an arc of it) as points, the first point left out.
pub fn path_pieces(path: &[[f64; 3]]) -> Vec<Vec<P2>> {
    path.windows(2)
        .map(|w| {
            crate::site::shapes::arc_points([w[0][0], w[0][1]], [w[1][0], w[1][1]], w[0][2], crate::site::shapes::ARC_TOLERANCE)
                .into_iter()
                .map(|q| P2(q[0], q[1]))
                .collect()
        })
        .collect()
}

/// A ramp's path's pieces' lengths (an arc's by its chords) and slopes (percent; 0 a landing).
pub fn path_slopes(r: &Ramp) -> Vec<(f64, f64)> {
    let mut at = r.path.first().map_or(P2(0.0, 0.0), |p| P2(p[0], p[1]));
    path_pieces(&r.path)
        .into_iter()
        .enumerate()
        .map(|(i, pts)| {
            let mut len = 0.0;
            for q in pts {
                len += at.dist(q);
                at = q;
            }
            (len, r.slopes.get(i).copied().filter(|s| s.is_finite() && *s >= 0.0).unwrap_or(r.slope))
        })
        .collect()
}

/// A ramp along its path (Revit's ramp by sketch): a run for each line of it and each chord of its
/// arcs, its rise shared out among its pieces by their lengths times their slopes, so it reaches its
/// top at the path's end; a piece of slope 0 is flat, a landing. Where two runs meet without a
/// change of slope their ends are mitred and the second is `joined` to the first.
fn along_path(r: &Ramp, base: f64, top: f64) -> Layout {
    let pieces = path_pieces(&r.path);
    let measured = path_slopes(r);
    let weight: f64 = measured.iter().map(|(l, s)| l * s).sum();
    let rise = top - base;
    let mut points: Vec<(P2, usize)> = vec![(P2(r.path[0][0], r.path[0][1]), 0)];
    for (i, pts) in pieces.iter().enumerate() {
        points.extend(pts.iter().map(|q| (*q, i)));
    }
    points.dedup_by(|b, a| a.0.dist(b.0) < 1e-6);
    let mut out = Layout { runs: Vec::new(), pads: Vec::new(), walk: Vec::new(), width: r.width };
    let mut z = base;
    for w in points.windows(2) {
        let [(a, _), (b, piece)] = [w[0], w[1]];
        let length = a.dist(b);
        let (plen, pslope) = measured.get(piece).copied().unwrap_or((0.0, 0.0));
        let up = if weight > 1e-9 && plen > 1e-9 {
            rise * (plen * pslope / weight) * (length / plen)
        } else if weight <= 1e-9 {
            rise * length / measured.iter().map(|x| x.0).sum::<f64>().max(1e-9)
        } else {
            0.0
        };
        let u = (b - a).unit();
        out.runs.push(Run::new(a, u, length, (z, z + up), 0));
        z += up;
    }
    // The last run ends at the top exactly, whatever the sums came to.
    if let Some(last) = out.runs.last_mut() {
        last.z.1 = top;
    }
    // Mitred where they meet: along the bisector of the two runs' left sides; a run going on at the
    // same slope is joined to the one before (no line across).
    for i in 1..out.runs.len() {
        let (p, q) = (out.runs[i - 1], out.runs[i]);
        let bis = p.u.perp() + q.u.perp();
        let a = if bis.dot(bis) > 1e-12 { bis.unit() } else { q.u.perp() };
        out.runs[i - 1].across.1 = a;
        out.runs[i].across.0 = a;
        let grade = |run: &Run| if run.length > 1e-9 { (run.z.1 - run.z.0) / run.length } else { 0.0 };
        out.runs[i].joined = (grade(&p) - grade(&q)).abs() < 1e-6;
    }
    out.walk = out.runs.iter().map(|run| (run.start, run.z.0)).collect();
    if let Some(last) = out.runs.last() {
        out.walk.push((last.end(), last.z.1));
    }
    out
}

/// Element `e`'s layout, a stair's or a ramp's.
pub fn layout(m: &Model, e: &Element) -> Option<Layout> {
    match e {
        Element::Stair(st) => Some(stair(m, st)),
        Element::Ramp(r) => Some(ramp(m, r)),
        _ => None,
    }
}

/// The line a railing follows, in plan, with the height of what it runs along at each point: on a
/// stair or a ramp, its walk line moved out to the side, [`RAILING_INSET`] in from its edge; else
/// its path, on its level and `base_offset` above it.
pub fn railing(m: &Model, rl: &Railing) -> Vec<(P2, f64)> {
    let host = rl.host.and_then(|h| m.elements.get(h)).and_then(|e| layout(m, e));
    match (host, rl.side) {
        (Some(l), Some(side)) => {
            let d = (l.width / 2.0 - RAILING_INSET).max(0.0) * if side == Side::Left { 1.0 } else { -1.0 };
            offset(&l.walk, d)
        }
        _ => {
            let z = m.elevation(rl.level) + rl.base_offset;
            rl.path.iter().map(|p| (*p, z)).collect()
        }
    }
}

/// A railing's solids, each a ring in plan between two planes: its rail along each stretch of its
/// line, 50 mm square, its type's height over the line; and its balusters, 20 mm square and its
/// type's `spacing` apart, from the line up under the rail.
pub fn railing_parts(m: &Model, rl: &Railing) -> Vec<(Vec<P2>, Plane, Plane)> {
    let Some(t) = m.types.railing.get(&rl.type_name) else { return Vec::new() };
    let mut out = Vec::new();
    // How far along the next stretch the next baluster stands.
    let mut next = 0.0;
    for w in railing(m, rl).windows(2) {
        let &[(p, zp), (q, zq)] = w else { continue };
        let len = p.dist(q);
        if len < 1e-9 {
            continue;
        }
        let u = (q - p) * (1.0 / len);
        let (n, g) = (u.perp() * 25.0, u * ((zq - zp) / len));
        let top = Plane { z: zp + t.height - g.dot(p), g };
        out.push((vec![p - n, q - n, q + n, p + n], Plane { z: top.z - 50.0, g }, top));
        if t.spacing > 0.0 {
            while next <= len {
                let (at, z) = (p + u * next, zp + (zq - zp) * next / len);
                let (a, b) = (u * 10.0, u.perp() * 10.0);
                out.push((vec![at - a - b, at + a - b, at + a + b, at - a + b], Plane::flat(z), Plane::flat(z + t.height - 50.0)));
                next += t.spacing;
            }
            next -= len;
        }
    }
    out
}

/// `line` moved `d` to its left, its corners mitred, each point keeping its height.
fn offset(line: &[(P2, f64)], d: f64) -> Vec<(P2, f64)> {
    let mut pts: Vec<(P2, f64)> = line.to_vec();
    pts.dedup_by(|b, a| a.0.dist(b.0) < 1e-9);
    let dirs: Vec<P2> = pts.windows(2).filter_map(|w| Some((w.get(1)?.0 - w.first()?.0).unit())).collect();
    pts.iter()
        .enumerate()
        .map(|(i, &(p, z))| {
            let before = i.checked_sub(1).and_then(|j| dirs.get(j)).copied();
            let after = dirs.get(i).copied();
            let shift = match (before, after) {
                (Some(a), Some(b)) if a.dot(b) > -1.0 + 1e-9 => (a.perp() + b.perp()) * (d / (1.0 + a.dot(b))),
                (Some(a), _) | (None, Some(a)) => a.perp() * d,
                (None, None) => P2(0.0, 0.0),
            };
            (p + shift, z)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_moved_aside_keeps_its_corners_square() {
        let line = [(P2(0.0, 0.0), 0.0), (P2(0.0, 1000.0), 500.0), (P2(1000.0, 1000.0), 500.0)];
        // Going north, then east: to the left (west, then north), the corner moves out diagonally.
        let left = offset(&line, 100.0);
        assert_eq!(left.iter().map(|(p, _)| *p).collect::<Vec<_>>(), [P2(-100.0, 0.0), P2(-100.0, 1100.0), P2(1000.0, 1100.0)]);
        assert_eq!(left.iter().map(|(_, z)| *z).collect::<Vec<_>>(), [0.0, 500.0, 500.0]);
        let right = offset(&line, -100.0);
        assert_eq!(right[1].0, P2(100.0, 900.0), "the inside corner moves in");
    }

    #[test]
    fn landings_turn_the_next_run_square_or_back() {
        let runs = vec![(1000.0, (0.0, 500.0), 0), (1000.0, (500.0, 1000.0), 0)];
        let near = |a: P2, b: P2| a.dist(b) < 1e-9;
        let l = chain(P2(0.0, 0.0), 90.0, 1000.0, runs.clone(), &[-90.0]);
        // North, then a metre square landing, then east from its middle.
        assert!(near(l.runs[1].start, P2(500.0, 1500.0)) && near(l.runs[1].u, P2(1.0, 0.0)), "{:?}", l.runs[1]);
        assert!((geom::area(&l.pads[0].ring) - 1e6).abs() < 1e-6);
        let back = chain(P2(0.0, 0.0), 90.0, 1000.0, runs.clone(), &[180.0]);
        assert!(near(back.runs[1].start, P2(-1000.0, 1000.0)) && near(back.runs[1].u, P2(0.0, -1.0)), "{:?}", back.runs[1]);
        assert!((geom::area(&back.pads[0].ring) - 2e6).abs() < 1e-6, "across both runs");
        let on = chain(P2(0.0, 0.0), 90.0, 1000.0, runs, &[0.0]);
        assert!(near(on.runs[1].start, P2(0.0, 2000.0)));
        assert_eq!(on.walk.len(), 4);
    }
}
