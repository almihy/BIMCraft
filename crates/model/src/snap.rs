//! Snapping: where a point picked near the cursor lands. It snaps to what the plan has there (the
//! ends, middles and centres of what's drawn, where two lines cross, the foot of the perpendicular
//! from the last point picked), to a guide through the last point picked at an angle (from east,
//! and from the line before it), to a wall's centre line, or to the nearest 10 mm. The ruler's
//! numbers and graduations are here too, so every front end reads them alike.

use bimcraft_i18n::ui_msg;

use crate::curves::Curve;
use crate::geom::{self, P2};
use crate::graphics::Look;
use crate::model::{Ceiling, Element, Floor, Id, Model, TOL};
use crate::walls::Axis;

/// How far off a guide, degrees, a direction may be and still snap to it (less where guides lie
/// closer together than that).
const TOLERANCE: f64 = 4.0;
/// Points with nothing to snap to round to this, mm.
const STEP: f64 = 10.0;
/// The most lines near the cursor that are tried against each other.
const MAX_NEAR: usize = 64;
/// The most walls' outlines, floors' and ceilings' edges a sketch is snapped to: the nearest. A wall's outline joins it to
/// the walls it meets, which costs the walls there are.
#[doc(hidden)]
pub const MAX_OUTLINES: usize = 24;
/// How far past its centre line a wall's face may lie and still be taken for the cursor's, mm: a
/// thicker wall isn't snapped to from farther than the reach plus this.
const MAX_HALF_THICKNESS: f64 = 2500.0;
/// How far over or under a plan's cut plane a scan's point may lie and still be snapped to, mm: where the plan cuts what
/// was scanned.
pub const CUT_BAND: f64 = 100.0;

/// What a point snapped to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Endpoint,
    Midpoint,
    Intersection,
    Centre,
    Perpendicular,
    /// A circle's or an ellipse's (or an arc's) point due east, north, west or south of its centre, along its axes.
    Quadrant,
    /// Where a line from the last point picked touches a circle or an arc.
    Tangent,
    /// The nearest point of a wall's centre line, or of a line.
    Nearest,
    /// A point of a scan where the plan's cut plane cuts it.
    Point,
    /// Where an annotation is placed: a note, a dimension, a tag, a spot elevation, a section or elevation mark, a detail
    /// group or a repeating detail.
    Anchor,
    /// A point on an angle guide.
    Angle,
    /// Nothing to snap to: the nearest 10 mm.
    Free,
}

impl Kind {
    /// What it's called, in English.
    pub fn name(self) -> &'static str {
        match self {
            Kind::Endpoint => ui_msg!("Endpoint"),
            Kind::Midpoint => ui_msg!("Midpoint"),
            Kind::Intersection => ui_msg!("Intersection"),
            Kind::Centre => "Centre",
            Kind::Perpendicular => ui_msg!("Perpendicular"),
            Kind::Quadrant => ui_msg!("Quadrant"),
            Kind::Tangent => ui_msg!("Tangent"),
            Kind::Nearest => ui_msg!("Nearest"),
            Kind::Point => "Point Cloud",
            Kind::Anchor => ui_msg!("Annotation Anchor"),
            Kind::Angle => ui_msg!("Angle"),
            Kind::Free => "",
        }
    }
}

/// The snaps in force.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Snaps {
    pub endpoint: bool,
    pub midpoint: bool,
    pub intersection: bool,
    pub centre: bool,
    pub perpendicular: bool,
    pub quadrant: bool,
    pub tangent: bool,
    pub nearest: bool,
    /// The points of a scan where the plan's cut plane cuts it.
    pub cloud: bool,
    /// Where annotations are placed: notes, dimensions, tags, spot elevations, section and elevation marks, detail groups
    /// and repeating details.
    pub annotation: bool,
    /// Degrees between the angles a direction snaps to, from east and from the line before it,
    /// from 1 to 180; anything else for none.
    pub angle: f64,
    /// Whether temporary alignment guides (smart snapping) are active.
    pub alignment_guides: bool,
    /// Alignment snap tolerance in screen pixels (default 8.0 px).
    pub alignment_tolerance: f32,
}

impl Snaps {
    /// No snap at all, and no angle steps.
    pub fn none() -> Snaps {
        Snaps {
            endpoint: false,
            midpoint: false,
            intersection: false,
            centre: false,
            perpendicular: false,
            quadrant: false,
            tangent: false,
            nearest: false,
            cloud: false,
            annotation: false,
            angle: 0.0,
            alignment_guides: false,
            alignment_tolerance: 8.0,
        }
    }

    /// Just `kind` snapped to (a kind with no switch of its own is none), and no angle steps: what
    /// Revit's snap shortcuts give the next point.
    pub fn only(kind: Kind) -> Snaps {
        let none = Snaps::none();
        match kind {
            Kind::Endpoint => Snaps { endpoint: true, ..none },
            Kind::Midpoint => Snaps { midpoint: true, ..none },
            Kind::Intersection => Snaps { intersection: true, ..none },
            Kind::Centre => Snaps { centre: true, ..none },
            Kind::Perpendicular => Snaps { perpendicular: true, ..none },
            Kind::Quadrant => Snaps { quadrant: true, ..none },
            Kind::Tangent => Snaps { tangent: true, ..none },
            Kind::Nearest => Snaps { nearest: true, ..none },
            Kind::Point => Snaps { cloud: true, ..none },
            Kind::Anchor => Snaps { annotation: true, ..none },
            Kind::Angle | Kind::Free => none,
        }
    }
}

impl Default for Snaps {
    fn default() -> Snaps {
        Snaps {
            endpoint: true,
            midpoint: true,
            intersection: true,
            centre: true,
            perpendicular: true,
            quadrant: true,
            tangent: true,
            nearest: true,
            cloud: true,
            annotation: true,
            angle: 15.0,
            alignment_guides: true,
            alignment_tolerance: 8.0,
        }
    }
}

/// How far from the cursor a snap reaches, mm: to a point, and to a line.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reach {
    pub point: f64,
    pub line: f64,
}

/// A line through a point, at an angle, that a pick snapped to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Guide {
    pub from: P2,
    /// Degrees from east, counter-clockwise, in [0, 360).
    pub angle: f64,
    /// With a line before the last point, the angle between that line and this one, degrees.
    pub corner: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Found {
    pub at: P2,
    pub kind: Kind,
    pub guide: Option<Guide>,
    /// The element it came from, by the id Select by ID finds it by: none for a sketch's own lines, the points picked so far,
    /// a scan, a guide's angle or nothing.
    pub source: Option<Id>,
}

/// `a` degrees turned into [0, 360).
fn norm(a: f64) -> f64 {
    let a = a.rem_euclid(360.0);
    if a >= 360.0 { 0.0 } else { a }
}

/// The direction of `d`, degrees from east, counter-clockwise, in [0, 360).
fn bearing(d: P2) -> f64 {
    if d.0 == 0.0 && d.1 == 0.0 { 0.0 } else { norm(d.1.atan2(d.0).to_degrees()) }
}

fn round(p: P2) -> P2 {
    P2((p.0 / STEP).round() * STEP, (p.1 / STEP).round() * STEP)
}

/// `p` to the micron: what a point meant to be exact is, without float noise.
fn micron(p: P2) -> P2 {
    P2((p.0 * 1e3).round() / 1e3, (p.1 * 1e3).round() / 1e3)
}

/// What the plan of a level has to snap to.
/// Each point, line and curve holds the element it comes from, none for a sketch's own lines.
struct Scene<'a> {
    points: Vec<(P2, Kind, Option<Id>)>,
    /// Straight lines: walls' centre lines, and grids.
    lines: Vec<(P2, P2, Option<Id>)>,
    /// The walls, with their thickness and their ids.
    walls: Vec<(Axis, f64, Id)>,
    /// The detail lines near, of any shape.
    curves: Vec<(Curve, Option<Id>)>,
    /// Their chords near, each with its line's place in `curves`: crossed with other lines, never with their own.
    chords: Vec<(P2, P2, usize)>,
    /// Where a scan's points can be had: the model, the plan asked for and what its view shows.
    cloud: Option<(&'a Model, Id, &'a Look)>,
}

impl<'a> Scene<'a> {
    fn segment(&mut self, a: P2, b: P2, of: Option<Id>) {
        self.points.extend([(a, Kind::Endpoint, of), (b, Kind::Endpoint, of), ((a + b) * 0.5, Kind::Midpoint, of)]);
        self.lines.push((a, b, of));
    }

    fn polyline(&mut self, points: &[P2], closed: bool, of: Option<Id>) {
        for w in points.windows(2) {
            self.segment(w[0], w[1], of);
        }
        if closed && let (Some(a), Some(b)) = (points.last(), points.first()) {
            self.segment(*a, *b, of);
        }
    }

    /// One real curve supplies its own points and chords, retaining the curve identity for crossings and tangents.
    fn curve(&mut self, curve: &Curve, p: P2, window: f64, of: Option<Id>) {
        let pts = curve.points();
        let (lo, hi) = pts.iter().fold((P2(f64::INFINITY, f64::INFINITY), P2(f64::NEG_INFINITY, f64::NEG_INFINITY)), |(lo, hi), q| {
            (P2(lo.0.min(q.0), lo.1.min(q.1)), P2(hi.0.max(q.0), hi.1.max(q.1)))
        });
        if p.0 < lo.0 - window || p.0 > hi.0 + window || p.1 < lo.1 - window || p.1 > hi.1 + window {
            return;
        }
        if !curve.is_closed() {
            self.points.extend([(curve.start(), Kind::Endpoint, of), (curve.end(), Kind::Endpoint, of)]);
        }
        self.points.extend(curve.vertices().into_iter().filter(|_| curve.is_poly()).map(|q| (q, Kind::Endpoint, of)));
        // A polyline's middles are its segments' own; another curve's is half way along it.
        if curve.is_poly() {
            self.points.extend(curve.segments().iter().map(|seg| (seg.at(seg.length() / 2.0, 0.0), Kind::Midpoint, of)));
        } else {
            self.points.push((curve.middle(), Kind::Midpoint, of));
        }
        self.points.extend(rounds(curve).into_iter().map(|(q, k)| (q, k, of)));
        let n = self.curves.len();
        self.chords.extend(pts.windows(2).filter(|w| geom::dist_to_segment(p, w[0], w[1]) <= window).map(|w| (w[0], w[1], n)));
        self.curves.push((curve.clone(), of));
    }

    /// What `level`'s plan has within `window` mm of `p`, but for what `look` hides: a cheap look at
    /// each element's extent first, so a plan of thousands of walls costs a few microseconds a wall.
    fn of(m: &'a Model, level: Id, p: P2, window: f64, look: &'a Look, skip: &[Id]) -> Scene<'a> {
        let mut s = Scene {
            points: Vec::new(),
            lines: Vec::new(),
            walls: Vec::new(),
            curves: Vec::new(),
            chords: Vec::new(),
            cloud: Some((m, level, look)),
        };
        let view = level;
        // A plan is cut where its view range says, which a plan view of its own may say otherwise than its level does.
        let plan = m.plan_level(view).is_some();
        let level = m.guid(m.plan_level(view).unwrap_or(view));
        let above = m.level_above(level);
        let cut = m.shown_range(view).cut;
        // Whether the box round `pts` lies within `window` of `p`.
        let near = |pts: &[P2]| {
            let (lo, hi) = pts.iter().fold((P2(f64::INFINITY, f64::INFINITY), P2(f64::NEG_INFINITY, f64::NEG_INFINITY)), |(lo, hi), q| {
                (P2(lo.0.min(q.0), lo.1.min(q.1)), P2(hi.0.max(q.0), hi.1.max(q.1)))
            });
            p.0 >= lo.0 - window && p.0 <= hi.0 + window && p.1 >= lo.1 - window && p.1 <= hi.1 + window
        };
        for (&id, e) in m.elements.iter().filter(|(id, _)| look.shows(**id) && !skip.contains(id)) {
            let of = Some(id);
            let ring = |pts: &[P2]| if near(pts) { pts.iter().map(|q| (*q, Kind::Endpoint, of)).collect::<Vec<_>>() } else { Vec::new() };
            match e {
                // What a view draws of its own, in a section as in a plan.
                Element::FilledRegion(r) if m.annotation_in(e, view) => {
                    for line in &r.sketch {
                        s.curve(&line.curve, p, window, of);
                    }
                }
                Element::DetailLine(l) if m.annotation_in(e, view) => s.curve(&l.curve, p, window, of),
                // A PDF's lines, Revit's Enable Snaps: their ends, middles and crossings, and the lines themselves.
                Element::Image(im) if im.snaps && m.annotation_in(e, view) => {
                    if let Some((cad, ci)) = m.image_types.get(&im.type_name).and_then(|t| im.lines_placed(t, im.view)) {
                        cad_scene(&mut s, &cad, &ci, p, window, look.hidden_pdf_layers(&im.type_name), id);
                    }
                }
                // The rest is a plan's: a section has none of it.
                _ if !plan => {}
                // Annotation's anchors: where it is placed, and the points its leader or dimension line reaches.
                Element::Text(n) if m.annotation_in(e, view) => {
                    s.points.push((n.at, Kind::Anchor, of));
                    if let Some(to) = n.leader {
                        s.points.push((to, Kind::Anchor, of));
                    }
                }
                Element::Spot(sp) if m.annotation_in(e, view) => s.points.push((sp.at, Kind::Anchor, of)),
                Element::Tag(t) if m.annotation_in(e, view) && m.elements.id(t.host).is_some_and(|id| look.shows(id)) => {
                    if let Some(host) = m.anchor(t.host) {
                        s.points.push((host + t.offset, Kind::Anchor, of));
                        if t.leader {
                            s.points.push((host, Kind::Anchor, of));
                        }
                    }
                }
                Element::Dimension(d) if m.annotation_in(e, view) => {
                    s.points.push((d.at, Kind::Anchor, of));
                    let (along, stations) = m.stations(d);
                    for (at, _, _) in stations {
                        s.points.push((d.at + along * at, Kind::Anchor, of));
                    }
                }
                Element::DetailGroup(g) if m.annotation_in(e, view) => s.points.push((g.at, Kind::Anchor, of)),
                Element::RepeatingDetail(r) if m.annotation_in(e, view) => {
                    s.points.extend([(r.start, Kind::Anchor, of), (r.end, Kind::Anchor, of)]);
                    if let Some(places) = r.places() {
                        s.points.extend(places.into_iter().map(|(at, _)| (at, Kind::Anchor, of)).filter(|(at, _, _)| near(&[*at])));
                    }
                }
                // Their marks stand on every plan.
                Element::Section(v) | Element::Elevation(v) => {
                    s.points.extend([(v.start, Kind::Anchor, of), (v.end, Kind::Anchor, of), ((v.start + v.end) * 0.5, Kind::Anchor, of)]);
                }
                // A camera gives its eye and target, never the mark's body, lens, name or sight line.
                Element::View3D(v) => {
                    if let Some(c) = v.camera
                        && let Some(level_id) = m.elements.id(level)
                        && m.camera_level(c.eye[2]) == Some(level_id)
                    {
                        let eye = P2(c.eye[0], c.eye[1]);
                        let target = P2(c.target[0], c.target[1]);
                        s.points.push((eye, Kind::Endpoint, of));
                        if eye.dist(target) > TOL {
                            s.points.push((target, Kind::Endpoint, of));
                        }
                    }
                }
                Element::Wall(w) => {
                    let a = Axis::of(w);
                    // An arc may bulge past its ends: take the box round its circle.
                    let reach = match (a.centre(), a.radius()) {
                        (Some(c), Some(r)) => [c + P2(-r, -r), c + P2(r, r)],
                        _ => [a.start, a.end],
                    };
                    if !near(&reach) || !m.cut_by(w, view) {
                        continue;
                    }
                    s.points.extend([
                        (a.start, Kind::Endpoint, of),
                        (a.end, Kind::Endpoint, of),
                        (a.at(a.length() / 2.0, 0.0), Kind::Midpoint, of),
                    ]);
                    match a.centre() {
                        Some(c) => s.points.push((c, Kind::Centre, of)),
                        None => s.lines.push((a.start, a.end, of)),
                    }
                    s.walls.push((a, m.thickness(w), id));
                }
                Element::Grid(g) if near(&[g.start, g.end]) => {
                    s.points.extend([(g.start, Kind::Endpoint, of), (g.end, Kind::Endpoint, of)]);
                    s.lines.push((g.start, g.end, of));
                }
                Element::Column(c) | Element::StructuralColumn(c) | Element::Footing(c) if c.level == level => {
                    // Only the columns near the cursor are worked out: the box its type can fill, turned any way. One whose type is
                    // gone has no outline and keeps its centre, as it always had.
                    let half = m.column_type(e).map_or(0.0, |t| t.width.hypot(t.depth) / 2.0);
                    if !near(&[c.at - P2(half, half), c.at + P2(half, half)]) {
                        continue;
                    }
                    let outline = m.column_outline(e);
                    s.points.push((c.at, Kind::Centre, of));
                    if m.column_type(e).is_some_and(|t| t.round) {
                        // A round one has no corners: the 24-gon's vertices due east, north, west and south of its centre.
                        s.points.extend(outline.iter().step_by(6).map(|q| (*q, Kind::Quadrant, of)));
                    } else {
                        // The corners of its outline, and the middles of its edges, which a wall can start from or run along.
                        s.points.extend(outline.iter().map(|q| (*q, Kind::Endpoint, of)));
                        s.points
                            .extend(outline.iter().zip(outline.iter().cycle().skip(1)).map(|(a, b)| ((*a + *b) * 0.5, Kind::Midpoint, of)));
                    }
                }
                crate::model::run_arm!(r)
                    if r.level == level && {
                        // Its body may reach `width` past its line: a mitred corner lies there.
                        let h = m.run_geom(e).map_or(0.0, |g| g.width / 2.0);
                        let h = P2(h, h);
                        near(&[r.start - h, r.start + h, r.end - h, r.end + h])
                    } =>
                {
                    s.points.extend([
                        (r.start, Kind::Endpoint, of),
                        (r.end, Kind::Endpoint, of),
                        (r.start + (r.end - r.start) * 0.5, Kind::Midpoint, of),
                    ]);
                    // The corners of its outline as the plan fills it: mitred where it joins, so a click near a
                    // mitre lands on the corner that is there, not in the air where the rectangle was. The axis
                    // keeps its line, which is what a run is picked and turned by.
                    s.points.extend(crate::runs::outline(m, id).into_iter().map(|q| (q, Kind::Endpoint, of)));
                    s.lines.push((r.start, r.end, of));
                }
                Element::Component(c) if c.level == level && near(&[c.at]) => s.points.push((c.at, Kind::Centre, of)),
                Element::Floor(f) if f.level == level => {
                    s.points.extend(ring(&f.boundary));
                    f.openings.iter().for_each(|o| s.points.extend(ring(o)));
                    // Its shape's points, and its split lines to run along.
                    s.points.extend(f.shape.points.iter().map(|x| (x.at, Kind::Endpoint, of)));
                    s.lines.extend(f.shape.lines.iter().map(|l| (l.a, l.b, of)));
                }
                Element::Ceiling(c) if c.level == level => {
                    s.points.extend(ring(&c.boundary));
                    c.openings.iter().for_each(|o| s.points.extend(ring(o)));
                }
                Element::Roof(r) if r.level == level || Some(r.level) == above => {
                    s.points.extend(ring(&r.boundary));
                    r.courtyards.iter().chain(&r.openings).for_each(|o| s.points.extend(ring(o)));
                    // A flat one's shape points, and its split lines to run along, as a floor's.
                    s.points.extend(r.shape.points.iter().map(|x| (x.at, Kind::Endpoint, of)));
                    s.lines.extend(r.shape.lines.iter().map(|l| (l.a, l.b, of)));
                }
                Element::Shaft(sh) if sh.level == level => s.points.extend(ring(&sh.boundary)),
                Element::Mass(ms) if m.mass_cut_by(ms, cut) => s.points.extend(ring(&m.mass_outline(ms, cut))),
                Element::Stair(st) if st.level == level && near(&[st.start]) => s.points.push((st.start, Kind::Endpoint, of)),
                Element::Ramp(r) if r.level == level && near(&[r.start]) => s.points.push((r.start, Kind::Endpoint, of)),
                Element::Railing(r) if r.level == level => s.points.extend(ring(&r.path)),
                Element::CadImport(ci) if ci.level == level => {
                    if let Some(cad) = m.cads.get(&ci.cad) {
                        cad_scene(&mut s, cad, ci, p, window, look.hidden_layers(&ci.cad), id);
                    }
                }
                _ => {}
            }
        }
        s
    }
}

/// What of a CAD import lies within `window` mm of `p`, of the layers it and the view show: its lines' ends and middles, its
/// arcs' and circles' centres, and its straight segments, so a wall can be traced over it. They are the import's, `id`.
/// `in_view` are the layers the view hides of it.
fn cad_scene(
    s: &mut Scene,
    cad: &crate::cad::Cad,
    ci: &crate::model::CadImport,
    p: P2,
    window: f64,
    in_view: Option<&std::collections::BTreeSet<String>>,
    id: Id,
) {
    use crate::dxf::Shape;
    let of = Some(id);
    let place = crate::model::cad_place(ci);
    // The window in the drawing's own units: the box round its corners, turned back.
    let corners = [P2(-1.0, -1.0), P2(1.0, -1.0), P2(1.0, 1.0), P2(-1.0, 1.0)].map(|k| {
        let q = crate::model::cad_unplace(ci, p + k * window);
        [q, q]
    });
    let Some([lo, hi]) = crate::dxf::union(corners) else { return };
    let near = |b: [P2; 2]| b[0].0 <= hi.0 && b[1].0 >= lo.0 && b[0].1 <= hi.1 && b[1].1 >= lo.1;
    for e in &cad.dxf().entities {
        if !cad.shows(e, &ci.hidden) || in_view.is_some_and(|h| h.contains(&e.layer)) || !near(e.shape.bounds()) {
            continue;
        }
        match &e.shape {
            Shape::Circle { c, .. } => s.points.push((place(*c), Kind::Centre, of)),
            Shape::Arc { c, .. } => s.points.push((place(*c), Kind::Centre, of)),
            Shape::Point(q) => s.points.push((place(*q), Kind::Endpoint, of)),
            Shape::Text { .. } => continue,
            _ => {}
        }
        let straight = matches!(e.shape, Shape::Line(..) | Shape::Polyline { .. } | Shape::Fill { .. });
        for (pts, closed) in crate::cad::paths(&e.shape) {
            let close = closed.then(|| pts.last().copied().zip(pts.first().copied())).flatten();
            let segs = pts.windows(2).filter_map(|w| Some((*w.first()?, *w.get(1)?))).chain(close);
            if !straight {
                // A curve: its ends, and its middle point.
                if let (Some(a), Some(b), false) = (pts.first(), pts.last(), closed) {
                    s.points.extend([(place(*a), Kind::Endpoint, of), (place(*b), Kind::Endpoint, of)]);
                    if let Some(mid) = pts.get(pts.len() / 2) {
                        s.points.push((place(*mid), Kind::Midpoint, of));
                    }
                }
                continue;
            }
            for (a, b) in segs {
                if !near(crate::dxf::union([[a, a], [b, b]]).unwrap_or_default()) {
                    continue;
                }
                s.segment(place(a), place(b), of);
            }
        }
    }
}

/// Of the points of the scans `look` shows that lie within [`CUT_BAND`] of `level`'s cut plane, the nearest to `p` seen from
/// above, within `reach`.
fn cloud_point(m: &Model, level: Id, p: P2, reach: f64, look: &Look) -> Option<P2> {
    if m.clouds.is_empty() {
        return None;
    }
    let cut = m.shown_range(level).cut;
    m.elements
        .iter()
        .filter_map(|(&id, e)| match e {
            Element::PointCloud(pc) if look.shows(id) => Some(pc),
            _ => None,
        })
        .filter_map(|pc| {
            let cloud = m.clouds.get(&pc.cloud)?;
            // The cursor in the scan's own terms: from its middle, turned back.
            let base = m.elevation(pc.level) + pc.offset;
            let (sin, cos) = pc.angle.to_radians().sin_cos();
            let d = p - pc.at;
            let local = [d.0 * cos + d.1 * sin, -d.0 * sin + d.1 * cos];
            let q = cloud.nearest(local, reach, (cut - CUT_BAND - base, cut + CUT_BAND - base))?;
            let [x, y, _] = m.cloud_place(pc)(q);
            Some(P2(x, y))
        })
        .min_by(|a, b| a.dist(p).total_cmp(&b.dist(p)))
}

/// A round curve's centre and its quadrants: a circle's or an ellipse's four points along its axes (those a partial one
/// passes), an arc's centre and the quadrants it passes.
pub(crate) fn rounds(c: &Curve) -> Vec<(P2, Kind)> {
    let on = |q: P2| c.nearest(q).1 < 1e-6;
    match c {
        Curve::Circle { centre, radius } => {
            let mut v = vec![(*centre, Kind::Centre)];
            v.extend([P2(1.0, 0.0), P2(0.0, 1.0), P2(-1.0, 0.0), P2(0.0, -1.0)].map(|u| (*centre + u * *radius, Kind::Quadrant)));
            v
        }
        Curve::Ellipse { centre, radii, angle, .. } => {
            let a = angle.to_radians();
            let (u, w) = (P2(a.cos(), a.sin()), P2(-a.sin(), a.cos()));
            let mut v = vec![(*centre, Kind::Centre)];
            let quads = [u * radii[0], w * radii[1], u * -radii[0], w * -radii[1]].map(|d| *centre + d);
            v.extend(quads.into_iter().filter(|q| on(*q)).map(|q| (q, Kind::Quadrant)));
            v
        }
        Curve::Poly { .. } => {
            let mut v = Vec::new();
            for seg in c.segments() {
                if let (Some(o), Some(r)) = (seg.centre(), seg.radius()) {
                    v.push((o, Kind::Centre));
                    for u in [P2(1.0, 0.0), P2(0.0, 1.0), P2(-1.0, 0.0), P2(0.0, -1.0)] {
                        let q = o + u * r;
                        if seg.distance(q, 0.0) < 1e-6 {
                            v.push((q, Kind::Quadrant));
                        }
                    }
                }
            }
            v
        }
        Curve::Spline { .. } | Curve::Fit { .. } => Vec::new(),
    }
}

/// Where lines from `from` touch the circles and arcs of `curves`: the tangent points, each with its curve's element.
fn tangents(curves: &[(Curve, Option<Id>)], from: P2) -> Vec<(P2, Option<Id>)> {
    let mut out = Vec::new();
    for (c, of) in curves {
        let rounds: Vec<(P2, f64)> = match c {
            Curve::Circle { centre, radius } => vec![(*centre, *radius)],
            Curve::Poly { .. } => c.segments().iter().filter_map(|s| Some((s.centre()?, s.radius()?))).collect(),
            _ => Vec::new(),
        };
        for (o, r) in rounds {
            let d = from.dist(o);
            if d <= r {
                continue;
            }
            // The angle at the centre between the line to `from` and the radius to the tangent point.
            let a = (r / d).acos();
            let base = (from.1 - o.1).atan2(from.0 - o.0);
            for t in [base + a, base - a] {
                let q = o + P2(t.cos(), t.sin()) * r;
                if c.nearest(q).1 < 1e-3 {
                    out.push((q, *of));
                }
            }
        }
    }
    out
}

/// Where the segments `lines` cross each other, each with the element of the first of the two: the nearer, as `solve`
/// sorts them.
fn crossings(lines: &[(P2, P2, Option<Id>)]) -> Vec<(P2, Option<Id>)> {
    let on = |q: P2, (a, b): (P2, P2)| geom::dist_to_segment(q, a, b) <= TOL;
    let mut out = Vec::new();
    for (i, &(a, b, of)) in lines.iter().enumerate() {
        for &(c, d, _) in lines.iter().skip(i + 1) {
            if let Some(q) = geom::line_intersection(a, b, c, d)
                && on(q, (a, b))
                && on(q, (c, d))
            {
                out.push((q, of));
            }
        }
    }
    out
}

/// Where the ray from `l` along the unit vector `u` meets the segment `a`→`b`.
fn ray_hit(l: P2, u: P2, a: P2, b: P2) -> Option<P2> {
    let e = b - a;
    let (len, den) = (e.length(), u.cross(e));
    if den.abs() <= 1e-12 * len {
        return None;
    }
    let (s, t) = ((a - l).cross(e) / den, (a - l).cross(u) / den);
    (s > 0.0 && t * len >= -TOL && t * len <= len + TOL).then(|| l + u * s)
}

/// The angles, degrees, a direction from the last of `from` snaps to, every `inc`: from east, and
/// from the line before the last point.
fn rays(from: &[P2], inc: f64) -> Vec<f64> {
    // As many as it takes to come round: the last short of 360° where the step doesn't divide it.
    let n = (360.0 / inc - 1e-9).ceil().min(360.0) as usize;
    let mut out: Vec<f64> = (0..n).map(|k| k as f64 * inc).collect();
    if let [.., prev, last] = from
        && prev.dist(*last) > TOL
    {
        let base = bearing(*last - *prev);
        out.extend((0..n).map(|k| norm(base + k as f64 * inc)));
    }
    out
}

/// The angle at `last` between the line back to `prev` and the line on to `next`, degrees: a
/// straight run on is 180°, a right-angled turn 90°; none while either line has no length.
pub fn corner(prev: P2, last: P2, next: P2) -> Option<f64> {
    if prev.dist(last) <= TOL || last.dist(next) <= TOL {
        return None;
    }
    let turn = norm(bearing(next - last) - bearing(last - prev));
    Some(180.0 - if turn > 180.0 { 360.0 - turn } else { turn })
}

/// With a line before the last of `from`, the corner it makes with one going on in direction `d`.
fn corner_on(from: &[P2], d: P2) -> Option<f64> {
    let [.., prev, last] = from else { return None };
    // A step long enough to count as a line, whatever length `d` is.
    corner(*prev, *last, *last + d.unit() * (2.0 * TOL))
}

/// How points rank where two lie as near: an end before a crossing before a middle before an annotation's anchor before
/// a centre or any other point.
fn rank(k: Kind) -> u8 {
    match k {
        Kind::Endpoint => 0,
        Kind::Intersection => 1,
        Kind::Midpoint => 2,
        Kind::Anchor => 3,
        _ => 4,
    }
}

/// Where a point picked at `p` lands on `level`'s plan: what the plan has there, the foot of the
/// perpendicular from the last of `from` (the points this pick follows), a guide through that
/// point at an angle, a wall's centre line, else the nearest 10 mm. `square` keeps the step from
/// the last of `from` horizontal or vertical.
pub fn snap(m: &Model, level: Id, p: P2, reach: Reach, from: &[P2], square: bool, o: &Snaps) -> Found {
    snap_skipping(m, level, p, reach, from, square, o, &[])
}

/// [`snap`], but the elements `skip` names attract nothing: what is dragged must not snap to itself.
#[allow(clippy::too_many_arguments)]
pub fn snap_skipping(m: &Model, level: Id, p: P2, reach: Reach, from: &[P2], square: bool, o: &Snaps, skip: &[Id]) -> Found {
    let look = Look::of(m, level);
    let scene = Scene::of(m, level, p, reach.point.max(reach.line) + MAX_HALF_THICKNESS, &look, skip);
    solve(scene, p, reach, from, square, o, false)
}

/// A sketch's view and draft geometry. The element being edited is omitted so its old boundary cannot attract a grip.
pub struct Sketch<'a> {
    pub view: Id,
    pub lines: &'a [crate::sketch::SketchLine],
    pub editing: Option<Id>,
}

#[cfg(debug_assertions)]
thread_local! {
    /// The outlines [`in_sketch`] has built on this thread: the tests assert on it, not on time, which varies with the load of
    /// the machine.
    #[doc(hidden)]
    pub static BUILT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Snap a boundary to visible model edges, view-specific detail, and the sketch's own lines, with the same snap switches
/// and guides as placement. All front ends use the same candidates for preview and the point they commit.
pub fn in_sketch(m: &Model, sk: Sketch<'_>, p: P2, reach: Reach, from: &[P2], square: bool, o: &Snaps) -> Found {
    let look = Look::of(m, sk.view);
    let window = reach.point.max(reach.line) + MAX_HALF_THICKNESS;
    let mut scene = Scene::of(m, sk.view, p, window, &look, sk.editing.as_slice());
    // The edges of what the plan builds, as the plan draws it: a section has none of them.
    if m.plan_level(sk.view).is_some() {
        let level = m.guid(m.plan_level(sk.view).unwrap_or(sk.view));
        let edge = |ring: &[P2]| geom::ring_edges(ring).map(|(a, b)| geom::dist_to_segment(p, a, b)).fold(f64::INFINITY, f64::min);
        let edges = |outline: &[P2], holes: &[Vec<P2>]| holes.iter().map(|h| edge(h)).fold(edge(outline), f64::min);
        let mut near: Vec<(f64, Id)> = m
            .elements
            .iter()
            .filter(|(id, _)| look.shows(**id) && sk.editing != Some(**id))
            .filter_map(|(id, e)| match e {
                Element::Wall(w) => {
                    let d = crate::walls::Axis::of(w).distance(p, 0.0);
                    (d <= window && m.cut_by(w, sk.view)).then_some((d, *id))
                }
                // Near an edge of its outline or of any opening: a big slab's hole in its middle is far from its outer edge.
                Element::Floor(f) if f.level == level => Some((edges(&f.boundary, &f.openings), *id)),
                Element::Ceiling(c) if c.level == level => Some((edges(&c.boundary, &c.openings), *id)),
                _ => None,
            })
            .filter(|(d, _)| *d <= window)
            .collect();
        near.sort_by(|a, b| a.0.total_cmp(&b.0));
        near.truncate(MAX_OUTLINES);
        // One index for all of them, so each wall's joins are found among the walls its height meets.
        let index = near.iter().any(|(_, id)| matches!(m.elements.get(*id), Some(Element::Wall(_)))).then(|| crate::walls::Index::new(m));
        for (_, id) in near {
            #[cfg(debug_assertions)]
            BUILT.with(|n| n.set(n.get() + 1));
            match m.elements.get(id) {
                Some(Element::Wall(_)) => scene.polyline(&crate::walls::outline_in(m, id, index.as_ref()), true, Some(id)),
                Some(Element::Floor(Floor { boundary, openings, .. }) | Element::Ceiling(Ceiling { boundary, openings, .. })) => {
                    scene.polyline(boundary, true, Some(id));
                    for hole in openings {
                        scene.polyline(hole, true, Some(id));
                    }
                }
                _ => {}
            }
        }
    }
    for l in sk.lines {
        scene.curve(&l.curve, p, window, None);
    }
    solve(scene, p, reach, from, square, o, true)
}

fn solve(scene: Scene, p: P2, reach: Reach, from: &[P2], square: bool, o: &Snaps, nearest_lines: bool) -> Found {
    let last = from.last().copied();
    let mut p = p;
    // The coordinate a square step pinned to the last point's: y if it runs along x.
    let mut pinned = None;
    if square && let Some(f) = last {
        if (p.0 - f.0).abs() > (p.1 - f.1).abs() {
            (p.1, pinned) = (f.1, Some((true, f.1)));
        } else {
            (p.0, pinned) = (f.0, Some((false, f.0)));
        }
    }
    let by_distance = |a: &P2, b: &P2| a.dist(p).total_cmp(&b.dist(p));
    // The lines that pass near the cursor: what it can cross, or stand square to.
    let mut near: Vec<(P2, P2, Option<Id>)> =
        scene.lines.iter().copied().filter(|(a, b, _)| geom::dist_to_segment(p, *a, *b) <= reach.point).collect();
    near.sort_by(|x, y| geom::dist_to_segment(p, x.0, x.1).total_cmp(&geom::dist_to_segment(p, y.0, y.1)));
    near.truncate(MAX_NEAR);
    // The detail lines' chords near it: crossed with the lines and with other detail lines' chords, and stood square to.
    let mut chords: Vec<(P2, P2, usize)> =
        scene.chords.iter().copied().filter(|(a, b, _)| geom::dist_to_segment(p, *a, *b) <= reach.point).collect();
    chords.sort_by(|x, y| geom::dist_to_segment(p, x.0, x.1).total_cmp(&geom::dist_to_segment(p, y.0, y.1)));
    chords.truncate(MAX_NEAR);

    // What the plan has here: its ends, middles and centres, the points picked so far, crossings.
    let wanted = |k: Kind| match k {
        Kind::Endpoint => o.endpoint,
        Kind::Midpoint => o.midpoint,
        Kind::Centre => o.centre,
        Kind::Quadrant => o.quadrant,
        Kind::Anchor => o.annotation,
        _ => true,
    };
    // A chord is its curve's element's.
    let chord_of = |k: usize| scene.curves.get(k).and_then(|(_, of)| *of);
    let mut points: Vec<(P2, Kind, Option<Id>)> = scene.points.iter().copied().filter(|(_, k, _)| wanted(*k)).collect();
    if o.endpoint {
        points.extend(from.iter().map(|q| (*q, Kind::Endpoint, None)));
    }
    if o.intersection {
        points.extend(crossings(&near).into_iter().map(|(q, of)| (q, Kind::Intersection, of)));
        let on = |q: P2, a: P2, b: P2| geom::dist_to_segment(q, a, b) <= TOL;
        for (i, &(a, b, k)) in chords.iter().enumerate() {
            let others =
                near.iter().map(|&(c, d, _)| (c, d)).chain(chords.iter().skip(i + 1).filter(|x| x.2 != k).map(|&(c, d, _)| (c, d)));
            for (c, d) in others {
                if let Some(q) = geom::line_intersection(a, b, c, d)
                    && on(q, a, b)
                    && on(q, c, d)
                {
                    points.push((q, Kind::Intersection, chord_of(k)));
                }
            }
        }
    }
    near.extend(chords.iter().map(|&(a, b, k)| (a, b, chord_of(k))));
    if o.tangent
        && let Some(l) = last
    {
        points.extend(tangents(&scene.curves, l).into_iter().map(|(q, of)| (micron(q), Kind::Tangent, of)));
    }
    let hit = points
        .into_iter()
        .filter(|(q, _, _)| q.dist(p) <= reach.point)
        .min_by(|a, b| by_distance(&a.0, &b.0).then(rank(a.1).cmp(&rank(b.1))));
    if let Some((at, kind, source)) = hit {
        return Found { at, kind, guide: None, source };
    }
    // Else a scan's point where the cut plane cuts it.
    if o.cloud
        && let Some((m, level, look)) = scene.cloud
        && let Some(at) = cloud_point(m, level, p, reach.point, look)
    {
        return Found { at: micron(at), kind: Kind::Point, guide: None, source: None };
    }

    // The foot of the perpendicular from the last point.
    if o.perpendicular
        && let Some(l) = last
    {
        let foot = near
            .iter()
            .filter_map(|&(a, b, of)| {
                let (len, u) = ((b - a).length(), (b - a).unit());
                let t = (l - a).dot(u);
                (len > 0.0 && (0.0..=len).contains(&t)).then(|| (a + u * t, of))
            })
            .filter(|(f, _)| f.dist(p) <= reach.point && f.dist(l) > STEP)
            .min_by(|x, y| by_distance(&x.0, &y.0));
        if let Some((f, source)) = foot {
            let guide = Guide { from: l, angle: bearing(f - l), corner: corner_on(from, f - l) };
            return Found { at: micron(f), kind: Kind::Perpendicular, guide: Some(guide), source };
        }
    }

    // A guide through the last point at the angle the cursor is nearest.
    if (1.0..=180.0).contains(&o.angle)
        && let Some(l) = last
        && p.dist(l) > 2.0 * STEP
    {
        let d = p - l;
        let tolerance = TOLERANCE.min(0.4 * o.angle);
        let best = rays(from, o.angle)
            .into_iter()
            .map(|a| {
                let u = P2(a.to_radians().cos(), a.to_radians().sin());
                (a, u, u.dot(d), u.cross(d).abs())
            })
            .filter(|(_, _, along, perp)| *along > 0.0 && *perp <= reach.line && (perp / along).atan().to_degrees() <= tolerance)
            .min_by(|x, y| x.3.total_cmp(&y.3));
        if let Some((angle, u, along, _)) = best {
            let guide = Some(Guide { from: l, angle, corner: corner_on(from, u) });
            // Where it crosses a wall or a grid near the cursor.
            let cross = near
                .iter()
                .filter_map(|&(a, b, of)| Some((ray_hit(l, u, a, b)?, of)))
                .filter(|(q, _)| q.dist(p) <= reach.point)
                .min_by(|x, y| by_distance(&x.0, &y.0));
            // Else the grid point that lies on it, if one does, else as long as the nearest 10 mm.
            let grid = round(p);
            let on_grid = u.cross(grid - l).abs() <= 1e-3 && u.dot(grid - l) > 0.0;
            return match cross {
                Some((q, source)) if o.intersection => Found { at: micron(q), kind: Kind::Intersection, guide, source },
                _ if on_grid => Found { at: grid, kind: Kind::Angle, guide, source: None },
                _ => Found { at: micron(l + u * ((along / STEP).round().max(1.0) * STEP)), kind: Kind::Angle, guide, source: None },
            };
        }
    }

    if nearest_lines && o.nearest {
        let nearest = scene
            .lines
            .iter()
            .filter_map(|&(a, b, of)| {
                let d = b - a;
                let length = d.dot(d);
                (length > 0.0).then(|| (a + d * ((p - a).dot(d) / length).clamp(0.0, 1.0), of))
            })
            .chain(scene.curves.iter().map(|(c, of)| {
                let (t, _) = c.nearest(p);
                (c.at(t), *of)
            }))
            .filter(|(q, _)| q.dist(p) <= reach.line)
            .min_by(|x, y| by_distance(&x.0, &y.0));
        if let Some((at, source)) = nearest {
            return Found { at: micron(at), kind: Kind::Nearest, guide: None, source };
        }
    }

    // A wall's centre line, to the nearest 10 mm along it from its start, else the nearest 10 mm.
    if o.nearest {
        let on = scene
            .walls
            .iter()
            .map(|(a, t, id)| (a, a.distance(p, 0.0) - t / 2.0, *id))
            .filter(|(_, d, _)| *d <= reach.line)
            .min_by(|x, y| x.1.total_cmp(&y.1));
        if let Some((a, _, id)) = on {
            let at = micron(a.at((a.project(p).0 / STEP).round() * STEP, 0.0));
            return Found { at, kind: Kind::Nearest, guide: None, source: Some(id) };
        }
        // A line: the point of it nearest the cursor.
        let on = scene
            .curves
            .iter()
            .map(|(c, of)| (c, c.nearest(p), *of))
            .filter(|(_, (_, d), _)| *d <= reach.line)
            .min_by(|x, y| x.1.1.total_cmp(&y.1.1));
        if let Some((c, (t, _), source)) = on {
            return Found { at: micron(c.at(t)), kind: Kind::Nearest, guide: None, source };
        }
    }
    // Rounded, but not off the line a square step keeps to.
    let mut at = round(p);
    match pinned {
        Some((true, y)) => at.1 = y,
        Some((false, x)) => at.0 = x,
        None => {}
    }
    Found { at, kind: Kind::Free, guide: None, source: None }
}

/// A length, its parts and its angle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ruler {
    pub length: f64,
    pub dx: f64,
    pub dy: f64,
    /// Degrees from east, counter-clockwise, in [0, 360).
    pub angle: f64,
}

/// What a ruler laid from `a` to `b` reads.
pub fn ruler(a: P2, b: P2) -> Ruler {
    let d = b - a;
    Ruler { length: d.length(), dx: d.0, dy: d.1, angle: bearing(d) }
}

/// How far apart a ruler's graduations lie, mm, at `px_per_mm`: 1, 2 or 5 times a power of ten,
/// at least 10 px apart.
pub fn graduation(px_per_mm: f64) -> f64 {
    if !(px_per_mm.is_finite() && px_per_mm > 0.0) {
        return 100.0;
    }
    let least = 10.0 / px_per_mm;
    let decade = 10f64.powi(least.log10().floor() as i32);
    [1.0, 2.0, 5.0, 10.0].into_iter().map(|k| k * decade).find(|g| *g >= least * (1.0 - 1e-9)).unwrap_or(10.0 * decade)
}

#[cfg(test)]
#[path = "snap_tests.rs"]
mod tests;
