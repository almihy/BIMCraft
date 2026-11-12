//! What an imported drawing's lines make, for building from it as Revit's Pick Lines does: the entities a pick names, the
//! closed outlines they make when joined where they meet ([`outlines`]), with the faces a line across a shape splits it into
//! and the whole shape they make together, and the lines and arcs a wall can be drawn along one each ([`segments`]).
//! Everything here is in the model's millimetres, placed by [`crate::model::cad_place`]. Area: docs/areas/cad-import.md.

use std::collections::BTreeSet;

use super::Cad;
use crate::dxf::Shape;
use crate::geom::{self, P2};
use crate::model::{CadImport, cad_place};

/// How near two ends are to be joined, and an end to a line to meet it, mm in the model.
pub const SNAP: f64 = 5.0;
/// The most entities one pick may name, and the most segments what it picks may draw: past that, narrow the pick.
pub const MAX_PICKED: usize = 100_000;
pub const MAX_SEGMENTS: usize = 200_000;
/// The most closed outlines worked out at once (their holes and labels are found by comparing them in pairs).
pub const MAX_RINGS: usize = 5_000;
/// The most labels an outline lists.
pub const MAX_LABELS: usize = 20;
/// The most chords an arc or circle is drawn with, one each this many degrees.
const ARC_CHORDS: usize = 72;
const ARC_STEP: f64 = 5.0;

/// Which entities of an import a pick takes: those it names (`entities`, indices into the drawing's entities), on the layers
/// it names, and lying wholly inside `window` (model mm), each only when given. Naming neither entities nor layers takes what
/// the import shows (its hidden layers left out).
#[derive(Clone, Debug, Default)]
pub struct Pick {
    pub entities: Option<BTreeSet<usize>>,
    pub layers: Option<BTreeSet<String>>,
    pub window: Option<[P2; 2]>,
}

/// A piece of the arrangement: its bounded faces (each ring with the entities it came from), its outline, and its entities.
type Piece = (Vec<(Vec<P2>, Vec<usize>)>, Option<Vec<P2>>, BTreeSet<usize>);

/// The entities of a placed drawing a pick takes, by index.
pub fn picked(cad: &Cad, ci: &CadImport, pick: &Pick) -> Vec<usize> {
    let place = cad_place(ci);
    let inside = |p: P2| pick.window.is_none_or(|[lo, hi]| p.0 >= lo.0 && p.0 <= hi.0 && p.1 >= lo.1 && p.1 <= hi.1);
    cad.dxf()
        .entities
        .iter()
        .enumerate()
        .filter(|(n, e)| {
            pick.entities.as_ref().is_none_or(|s| s.contains(n))
                && pick.layers.as_ref().is_none_or(|l| l.contains(&e.layer))
                && (pick.entities.is_some() || pick.layers.is_some() || cad.shows(e, &ci.hidden))
        })
        .filter(|(_, e)| {
            pick.window.is_none() || {
                let pts: Vec<P2> = match &e.shape {
                    Shape::Point(p) | Shape::Text { at: p, .. } => vec![*p],
                    s => super::paths(s).into_iter().flat_map(|(p, _)| p).collect(),
                };
                !pts.is_empty() && pts.into_iter().all(|p| inside(place(p)))
            }
        })
        .map(|(n, _)| n)
        .collect()
}

/// Where an outline came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// Lines, arcs and open polylines joined where they meet.
    Lines,
    /// A closed polyline.
    Polyline,
    Circle,
    /// A ring of a hatch's boundary.
    Hatch,
}

impl Source {
    pub fn name(self) -> &'static str {
        match self {
            Source::Lines => "lines",
            Source::Polyline => "polyline",
            Source::Circle => "circle",
            Source::Hatch => "hatch",
        }
    }
}

/// A closed outline the picked entities make, in the model: its ring counter-clockwise, its holes clockwise.
#[derive(Clone, Debug, PartialEq)]
pub struct Outline {
    pub ring: Vec<P2>,
    pub holes: Vec<Vec<P2>>,
    pub from: Source,
    /// The entities it was made of, by index.
    pub entities: Vec<usize>,
    /// The whole outer shape of a piece that lines across it split into faces (which are outlines too).
    pub whole: bool,
    /// A face of a piece split by lines across it: inside its whole, never one of the outermost.
    pub face: bool,
    /// Inside another outline that isn't a face (a hole in it, or an island in a hole): none, outermost.
    pub depth: usize,
}

impl Outline {
    /// Its area less its holes', mm².
    pub fn area(&self) -> f64 {
        geom::area(&self.ring).abs() - self.holes.iter().map(|h| geom::area(h).abs()).sum::<f64>()
    }

    /// Whether `p` lies inside it and in none of its holes.
    pub fn contains(&self, p: P2) -> bool {
        geom::contains(&self.ring, p) && !self.holes.iter().any(|h| geom::contains(h, p))
    }
}

/// A run of picked lines that closes nothing, and the entities along it.
#[derive(Clone, Debug, PartialEq)]
pub struct Open {
    pub points: Vec<P2>,
    pub entities: Vec<usize>,
}

/// What [`outlines`] finds.
#[derive(Clone, Debug, Default)]
pub struct Found {
    pub outlines: Vec<Outline>,
    pub open: Vec<Open>,
}

/// The points along an arc of the drawing, `sweep` degrees counter-clockwise from `start`, placed: a chord each
/// [`ARC_STEP`] degrees, but none shorter than four snaps, so joining its ends never takes a chord away.
fn arc_points(place: &dyn Fn(P2) -> P2, scale: f64, c: P2, r: f64, start: f64, sweep: f64) -> Vec<P2> {
    let long = (sweep.abs().to_radians() * r * scale / (4.0 * SNAP)).floor();
    let n = (sweep.abs() / ARC_STEP).ceil().min(long).clamp(1.0, ARC_CHORDS as f64) as usize;
    (0..=n)
        .map(|i| {
            let a = (start + sweep * i as f64 / n as f64).to_radians();
            place(c + P2(a.cos(), a.sin()) * r)
        })
        .collect()
}

/// An arc's sweep, degrees counter-clockwise from its start to its end: above 0, up to 360.
pub fn sweep(start: f64, end: f64) -> f64 {
    let s = (end - start).rem_euclid(360.0);
    if s <= 1e-9 { 360.0 } else { s }
}

/// The closed outlines the picked entities make, and the open runs left: lines, arcs and open polylines joined where they
/// meet (within [`SNAP`]), each face of what they enclose and, when lines across a shape split it, the whole shape too; each
/// closed polyline, circle and ring of a hatch as it is. Outlines inside others are their holes. The same ring found twice
/// (a hatch over the lines round it) is one outline, made of both.
pub fn outlines(cad: &Cad, ci: &CadImport, picked: &[usize]) -> Result<Found, String> {
    if picked.len() > MAX_PICKED {
        return Err(format!(
            "the pick takes {} entities, more than {MAX_PICKED}; narrow it with `layers`, `entities` or `box`",
            picked.len()
        ));
    }
    let place = cad_place(ci);
    let ents = &cad.dxf().entities;
    let (mut segs, mut owner): (Vec<(P2, P2)>, Vec<usize>) = (Vec::new(), Vec::new());
    let mut rings: Vec<Outline> = Vec::new();
    let single = |ring: Vec<P2>, from: Source, n: usize| Outline {
        ring,
        holes: Vec::new(),
        from,
        entities: vec![n],
        whole: false,
        face: false,
        depth: 0,
    };
    for &n in picked {
        let Some(e) = ents.get(n) else { continue };
        let run: Vec<P2> = match &e.shape {
            Shape::Line(a, b) => vec![place(*a), place(*b)],
            Shape::Arc { c, r, start, end } => arc_points(&place, ci.scale, *c, *r, *start, sweep(*start, *end)),
            Shape::Polyline { pts, closed: false } => pts.iter().map(|p| place(*p)).collect(),
            Shape::Polyline { pts, closed: true } => {
                rings.push(single(pts.iter().map(|p| place(*p)).collect(), Source::Polyline, n));
                continue;
            }
            Shape::Circle { c, r } => {
                let mut pts = arc_points(&place, ci.scale, *c, *r, 0.0, 360.0);
                pts.pop();
                rings.push(single(pts, Source::Circle, n));
                continue;
            }
            Shape::Fill { rings: rs, .. } => {
                rings.extend(rs.iter().map(|r| single(r.iter().map(|p| place(*p)).collect(), Source::Hatch, n)));
                continue;
            }
            Shape::Point(_) | Shape::Text { .. } => continue,
        };
        for w in run.windows(2) {
            if let [a, b] = w {
                segs.push((*a, *b));
                owner.push(n);
            }
        }
        if segs.len() > MAX_SEGMENTS {
            return Err(format!(
                "the picked entities draw more than {MAX_SEGMENTS} segments; narrow the pick with `layers`, `entities` or `box`"
            ));
        }
    }
    let ents_of = |tags: &[usize]| -> Vec<usize> {
        let mut v: Vec<usize> = tags.iter().filter_map(|t| owner.get(*t).copied()).collect();
        v.sort_unstable();
        v.dedup();
        v
    };
    let arr = geom::arrangement(&segs, SNAP);
    // Each piece's bounded faces, and its outline; one face alone is the piece.
    let mut pieces: std::collections::BTreeMap<usize, Piece> = Default::default();
    for (ring, tags, piece) in arr.faces {
        let a = geom::area(&ring);
        if a.abs() <= SNAP * SNAP {
            continue;
        }
        let slot = pieces.entry(piece).or_default();
        slot.2.extend(tags.iter().copied());
        if a > 0.0 {
            slot.0.push((ring, tags));
        } else {
            // The piece's outline, the widest when a piece's outline is walked in parts.
            if slot.1.as_ref().is_none_or(|r| geom::area(r).abs() < a.abs()) {
                slot.1 = Some(ring.into_iter().rev().collect());
            }
        }
    }
    let mut lines: Vec<Outline> = Vec::new();
    for (faces, outline, tags) in pieces.into_values() {
        let all = ents_of(&tags.into_iter().collect::<Vec<_>>());
        if faces.len() == 1 {
            if let Some((ring, tags)) = faces.into_iter().next() {
                lines.push(Outline { entities: ents_of(&tags), ..single(ring, Source::Lines, 0) });
            }
            continue;
        }
        if let Some(ring) = outline {
            lines.push(Outline { entities: all, whole: true, ..single(ring, Source::Lines, 0) });
        }
        for (ring, tags) in faces {
            lines.push(Outline { entities: ents_of(&tags), face: true, ..single(ring, Source::Lines, 0) });
        }
    }
    lines.extend(rings);
    let mut outlines: Vec<Outline> = Vec::new();
    for mut o in lines {
        o.ring = tidy(o.ring);
        if o.ring.len() < 3 || geom::area(&o.ring).abs() <= SNAP * SNAP {
            continue;
        }
        if geom::area(&o.ring) < 0.0 {
            o.ring.reverse();
        }
        // The same ring found again: one outline, of both.
        if let Some(same) = outlines.iter_mut().find(|k| k.face == o.face && same_ring(&k.ring, &o.ring)) {
            same.entities.extend(o.entities);
            same.entities.sort_unstable();
            same.entities.dedup();
            continue;
        }
        if outlines.len() >= MAX_RINGS {
            return Err(format!(
                "the picked entities make more than {MAX_RINGS} closed outlines; narrow the pick with `layers`, `entities` or `box`"
            ));
        }
        outlines.push(o);
    }
    nest(&mut outlines);
    let open = arr.open.into_iter().map(|(points, tags)| Open { points, entities: ents_of(&tags) }).collect();
    Ok(Found { outlines, open })
}

/// A ring without a point repeating the one before it, or the first at its end.
fn tidy(ring: Vec<P2>) -> Vec<P2> {
    let mut r: Vec<P2> = Vec::with_capacity(ring.len());
    for q in ring {
        if r.last().is_none_or(|l| l.dist(q) > 1e-6) {
            r.push(q);
        }
    }
    while r.len() > 1 && r.first().zip(r.last()).is_some_and(|(a, b)| a.dist(*b) <= 1e-6) {
        r.pop();
    }
    r
}

/// The box round a ring.
fn bbox(ring: &[P2]) -> [P2; 2] {
    crate::dxf::union(ring.iter().map(|p| [*p, *p])).unwrap_or_default()
}

/// Whether each point of `ring` lies inside `other` or on its edge (within [`SNAP`]).
fn within(ring: &[P2], other: &[P2]) -> bool {
    ring.iter().all(|p| geom::contains(other, *p) || geom::ring_edges(other).any(|(a, b)| geom::dist_to_segment(*p, a, b) <= SNAP))
}

/// Whether two rings are the same shape, each point of one on the other's edges and their areas within a snap's width.
fn same_ring(a: &[P2], b: &[P2]) -> bool {
    let (ba, bb) = (bbox(a), bbox(b));
    let near = |x: P2, y: P2| x.dist(y) <= SNAP;
    near(ba[0], bb[0])
        && near(ba[1], bb[1])
        && (geom::area(a).abs() - geom::area(b).abs()).abs() <= SNAP * geom::perimeter(a).max(geom::perimeter(b))
        && on_edges(a, b)
        && on_edges(b, a)
}

fn on_edges(ring: &[P2], other: &[P2]) -> bool {
    ring.iter().all(|p| geom::ring_edges(other).any(|(a, b)| geom::dist_to_segment(*p, a, b) <= SNAP))
}

/// Each outline's holes and depth. The outermost outlines (those not faces) nest: one inside another is its hole, one
/// inside a hole an island, at even depth again. A face's holes are those of its whole that lie inside it.
fn nest(outlines: &mut [Outline]) {
    let boxes: Vec<[P2; 2]> = outlines.iter().map(|o| bbox(&o.ring)).collect();
    let areas: Vec<f64> = outlines.iter().map(|o| geom::area(&o.ring).abs()).collect();
    let holds = |i: usize, j: usize| -> bool {
        // Whether outline i holds outline j.
        let (Some(bi), Some(bj), Some(ai), Some(aj)) = (boxes.get(i), boxes.get(j), areas.get(i), areas.get(j)) else { return false };
        i != j
            && ai > aj
            && bi[0].0 <= bj[0].0 + SNAP
            && bi[0].1 <= bj[0].1 + SNAP
            && bi[1].0 + SNAP >= bj[1].0
            && bi[1].1 + SNAP >= bj[1].1
            && outlines.get(i).zip(outlines.get(j)).is_some_and(|(a, b)| within(&b.ring, &a.ring))
    };
    let outer: Vec<usize> = (0..outlines.len()).filter(|&i| outlines.get(i).is_some_and(|o| !o.face)).collect();
    // Each outermost outline's parent: the smallest of the others that holds it.
    let parent: Vec<Option<usize>> = (0..outlines.len())
        .map(|j| {
            if outlines.get(j).is_none_or(|o| o.face) {
                return None;
            }
            outer
                .iter()
                .copied()
                .filter(|&i| holds(i, j))
                .min_by(|a, b| areas.get(*a).zip(areas.get(*b)).map_or(std::cmp::Ordering::Equal, |(x, y)| x.total_cmp(y)))
        })
        .collect();
    let depth = |mut j: usize| {
        let mut d = 0;
        while let Some(Some(p)) = parent.get(j) {
            d += 1;
            j = *p;
            if d > outlines.len() {
                break;
            }
        }
        d
    };
    let mut depths: Vec<usize> = (0..outlines.len()).map(depth).collect();
    let mut holes: Vec<Vec<Vec<P2>>> = vec![Vec::new(); outlines.len()];
    for (j, p) in parent.iter().enumerate() {
        if let (Some(p), Some(o)) = (p, outlines.get(j))
            && let Some(h) = holes.get_mut(*p)
        {
            h.push(o.ring.iter().rev().copied().collect());
        }
    }
    // A face's holes: its whole's (the outline that is a whole and holds it) that lie inside it.
    for i in 0..outlines.len() {
        if !outlines.get(i).is_some_and(|o| o.face) {
            continue;
        }
        let whole = outer
            .iter()
            .copied()
            .filter(|&w| outlines.get(w).is_some_and(|o| o.whole) && holds(w, i))
            .min_by(|a, b| areas.get(*a).zip(areas.get(*b)).map_or(std::cmp::Ordering::Equal, |(x, y)| x.total_cmp(y)));
        let (Some(w), Some(face)) = (whole, outlines.get(i)) else { continue };
        let mine: Vec<Vec<P2>> = holes.get(w).map(|hs| hs.iter().filter(|h| within(h, &face.ring)).cloned().collect()).unwrap_or_default();
        if let Some(slot) = holes.get_mut(i) {
            *slot = mine;
        }
        // A face is as deep as its whole.
        if let Some(d) = depths.get(w).copied()
            && let Some(slot) = depths.get_mut(i)
        {
            *slot = d;
        }
    }
    for (i, o) in outlines.iter_mut().enumerate() {
        o.depth = depths.get(i).copied().unwrap_or(0);
        o.holes = holes.get(i).cloned().unwrap_or_default();
    }
}

/// The texts of the drawing (on any layer) whose point lies inside each outline and in none of its holes, up to
/// [`MAX_LABELS`] each, in the drawing's order.
pub fn labels(cad: &Cad, ci: &CadImport, outlines: &[Outline]) -> Vec<Vec<String>> {
    let place = cad_place(ci);
    let mut texts: Vec<(P2, usize, &str)> = cad
        .dxf()
        .entities
        .iter()
        .enumerate()
        .filter_map(|(n, e)| match &e.shape {
            Shape::Text { at, text, .. } if !text.trim().is_empty() => Some((place(*at), n, text.as_str())),
            _ => None,
        })
        .collect();
    texts.sort_by(|a, b| a.0.0.total_cmp(&b.0.0));
    outlines
        .iter()
        .map(|o| {
            let [lo, hi] = bbox(&o.ring);
            let from = texts.partition_point(|t| t.0.0 < lo.0);
            let mut found: Vec<(usize, &str)> = texts
                .get(from..)
                .unwrap_or_default()
                .iter()
                .take_while(|t| t.0.0 <= hi.0)
                .filter(|t| t.0.1 >= lo.1 && t.0.1 <= hi.1 && o.contains(t.0))
                .map(|t| (t.1, t.2))
                .collect();
            found.sort_unstable();
            found.into_iter().take(MAX_LABELS).map(|(_, t)| t.to_string()).collect()
        })
        .collect()
}

/// A wall's line: from `start` to `end`, turning `arc` degrees counter-clockwise (0, straight), and the entity it's along.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Segment {
    pub start: P2,
    pub end: P2,
    pub arc: f64,
    pub entity: usize,
}

/// The lines a wall can be drawn along, one each, of the picked entities, placed: a line's, an arc's (an arc of a wall), each
/// segment of a polyline, and a circle's two halves. What draws no line (a text, a point, a hatch) is counted apart.
pub fn segments(cad: &Cad, ci: &CadImport, picked: &[usize]) -> (Vec<Segment>, usize) {
    let place = cad_place(ci);
    let (mut out, mut skipped) = (Vec::new(), 0);
    let at = |c: P2, r: f64, deg: f64| place(c + P2(deg.to_radians().cos(), deg.to_radians().sin()) * r);
    for &n in picked {
        let Some(e) = cad.dxf().entities.get(n) else { continue };
        match &e.shape {
            Shape::Line(a, b) => out.push(Segment { start: place(*a), end: place(*b), arc: 0.0, entity: n }),
            Shape::Arc { c, r, start, end } => {
                let s = sweep(*start, *end);
                if s >= 360.0 {
                    for k in [0.0, 180.0] {
                        out.push(Segment { start: at(*c, *r, start + k), end: at(*c, *r, start + k + 180.0), arc: 180.0, entity: n });
                    }
                } else {
                    out.push(Segment { start: at(*c, *r, *start), end: at(*c, *r, start + s), arc: s, entity: n });
                }
            }
            Shape::Circle { c, r } => {
                for k in [0.0, 180.0] {
                    out.push(Segment { start: at(*c, *r, k), end: at(*c, *r, k + 180.0), arc: 180.0, entity: n });
                }
            }
            Shape::Polyline { pts, closed } => {
                let close = (*closed && pts.len() > 2).then(|| pts.last().copied().zip(pts.first().copied())).flatten();
                let pairs = pts.windows(2).filter_map(|w| Some((*w.first()?, *w.get(1)?))).chain(close);
                out.extend(pairs.map(|(a, b)| Segment { start: place(a), end: place(b), arc: 0.0, entity: n }));
            }
            Shape::Point(_) | Shape::Text { .. } | Shape::Fill { .. } => skipped += 1,
        }
    }
    (out, skipped)
}
