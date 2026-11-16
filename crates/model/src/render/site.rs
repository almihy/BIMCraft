//! The site and masterplan on a plan (docs/areas/site.md), as Arqiflow's editor draws it (draw-site.ts, draw-roads.ts):
//! each area filled in its surface's colour and outlined (lawns green, paving buff, water blue), a road its carriageway,
//! pavements, median, kerbs and markings, a tree its crown, a car park its aisles, stalls and columns, a parking band its
//! bays; existing context halftone, what comes down and what others will build dashed and faded. Building pads and
//! property lines (dash-dot) too. Each primitive is its element's, so the plan picks it.

use super::{Drawing, Part, Prim, Rgb};
use crate::geom::{self, P2};
use crate::ground::{BasePoint, SubDivision};
use crate::masterplan::{self, PropertyLine, Site};
use crate::model::{Guid, Id, Model};
use crate::site::Poly;
use crate::styles::BY_CATEGORY;

/// What its phase does to an element's look: its colours toned, its outlines dashed.
#[derive(Clone, Copy)]
struct Tone {
    /// How far to the paper its colours go (0 as they are).
    fade: f32,
    dashed: bool,
}

impl Tone {
    fn of(s: &Site) -> Tone {
        match s.phase.as_str() {
            "existing" => Tone { fade: 0.5, dashed: false },
            "demolished" => Tone { fade: 0.55, dashed: true },
            "planned" => Tone { fade: 0.55, dashed: true },
            _ => Tone { fade: 0.0, dashed: false },
        }
    }

    fn c(&self, c: Rgb) -> Rgb {
        let k = self.fade.clamp(0.0, 1.0);
        let to = |v: u8| (f32::from(v) + (255.0 - f32::from(v)) * k).round() as u8;
        Rgb(to(c.0), to(c.1), to(c.2))
    }

    fn dash(&self) -> Option<Vec<f32>> {
        self.dashed.then(|| vec![8.0, 4.0])
    }
}

/// A polygon filled, its holes left open (the rasterizer fills by winding: its outline one way, its holes the other).
fn fill_poly(d: &mut Drawing, p: &Poly, color: Rgb) {
    if p.outer.len() < 3 {
        return;
    }
    let turned = |r: &[P2], ccw: bool| -> Vec<(f32, f32)> {
        let mut r = r.to_vec();
        if (geom::area(&r) > 0.0) != ccw {
            r.reverse();
        }
        r.iter().map(|q| d.view.px(*q)).collect()
    };
    let mut rings = vec![turned(&p.outer, true)];
    rings.extend(p.holes.iter().filter(|h| h.len() >= 3).map(|h| turned(h, false)));
    d.push(Prim::fill(rings, color));
}

fn fill_all(d: &mut Drawing, polys: &[Poly], color: Rgb) {
    for p in polys {
        fill_poly(d, p, color);
    }
}

fn outline_all(d: &mut Drawing, polys: &[Poly], color: Rgb, dash: &Option<Vec<f32>>) {
    for p in polys {
        for r in std::iter::once(&p.outer).chain(&p.holes) {
            let px = r.iter().map(|q| d.view.px(*q)).collect();
            d.stroke(px, BY_CATEGORY, color, dash.clone(), true);
        }
    }
}

fn lines(d: &mut Drawing, ls: &[Vec<P2>], color: Rgb, dash: Option<Vec<f32>>) {
    for l in ls.iter().filter(|l| l.len() >= 2) {
        let px = l.iter().map(|q| d.view.px(*q)).collect();
        d.stroke(px, BY_CATEGORY, color, dash.clone(), false);
    }
}

fn part<'a>(s: &'a Site, role: &str) -> impl Iterator<Item = &'a crate::site::shapes::Part> {
    let role = role.to_string();
    s.parts.iter().filter(move |p| p.role == role)
}

fn polys(s: &Site, role: &str) -> Vec<Poly> {
    part(s, role).flat_map(|p| p.polys.iter().cloned()).collect()
}

fn part_lines(s: &Site, role: &str) -> Vec<Vec<P2>> {
    part(s, role).flat_map(|p| p.lines.iter().cloned()).collect()
}

/// A dot where a point stands (a tree's trunk), `r` px across.
fn dot(d: &mut Drawing, at: P2, r: f32, color: Rgb) {
    let (x, y) = d.view.px(at);
    let ring: Vec<(f32, f32)> = (0..12)
        .map(|k| {
            let t = std::f32::consts::TAU * k as f32 / 12.0;
            (x + r * t.cos(), y + r * t.sin())
        })
        .collect();
    d.push(Prim::fill(vec![ring], color));
}

/// An arrow along a drive aisle's length through its middle, its heads at both ends (two-way) or the end it runs to.
fn aisle_arrow(d: &mut Drawing, ring: &[P2], flow: Option<P2>, color: Rgb) {
    if ring.len() < 3 {
        return;
    }
    let c = geom::centroid(ring);
    // Its long way: along its longest side.
    let n = ring.len();
    let (mut u, mut long) = (P2(1.0, 0.0), 0.0);
    for i in 0..n {
        let e = ring[(i + 1) % n] - ring[i];
        let len = e.dot(e).sqrt();
        if len > long {
            (u, long) = (e * (1.0 / len), len);
        }
    }
    // One way: along the way it runs (a short cross aisle runs across its long side), as long as it is that way.
    if let Some(f) = flow
        && f.dot(f) > 1e-12
    {
        u = f * (1.0 / f.dot(f).sqrt());
        let (lo, hi) = ring.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), q| (lo.min(q.dot(u)), hi.max(q.dot(u))));
        long = hi - lo;
    }
    let half = long * 0.35;
    let (a, b) = (c - u * half, c + u * half);
    let pts = vec![d.view.px(a), d.view.px(b)];
    d.stroke(pts, BY_CATEGORY, color, None, false);
    super::arrow(d, d.view.px(a), d.view.px(b));
    if flow.is_none() {
        super::arrow(d, d.view.px(b), d.view.px(a));
    }
}

/// Site element `id`, `s`, on the plan.
pub(super) fn site(d: &mut Drawing, id: Id, s: &Site) {
    d.own(Some(id));
    d.part(Part::Projection);
    let tone = Tone::of(s);
    let dash = tone.dash();
    let use_ = s.use_or_default();
    let survey = s.props.get("survey").is_some_and(|v| v.as_str().is_some_and(|t| !t.is_empty()));
    let (fill, stroke) = masterplan::plan_colours(&s.kind, use_, survey);
    let (fill, stroke) = (tone.c(fill), tone.c(stroke));
    match s.kind.as_str() {
        "lane" if use_ == "road" => {
            let c = masterplan::road_colours(s.context()).map(|c| tone.c(c));
            fill_all(d, &polys(s, "pavement"), c[1]);
            fill_all(d, &polys(s, "carriageway"), c[0]);
            fill_all(d, &polys(s, "median"), c[4]);
            fill_all(d, &polys(s, "paint"), c[5]);
            lines(d, &part_lines(s, "pavement_edge"), c[3], dash.clone());
            lines(d, &part_lines(s, "kerb"), c[2], dash.clone());
            lines(d, &part_lines(s, "marking"), c[5], Some(vec![6.0, 6.0]));
            lines(d, &part_lines(s, "centre_line"), c[5], Some(vec![10.0, 6.0]));
            lines(d, &part_lines(s, "double_centre_line"), c[5], None);
            // Where its parts don't reach (a road whose parts weren't worked out), its outline.
            if s.parts.iter().all(|p| p.role == "path") {
                fill_all(d, &s.outline, c[0]);
            }
        }
        // Its area; its bays and its aisles at ground level are elements of their own. Below ground, its aisles are the
        // floor: an arrow along each, both ways or the way it runs (Arqiflow's aisle arrows, as the car park's annotation).
        "car_park" => {
            outline_all(d, &s.outline, stroke, &Some(vec![8.0, 4.0]));
            let aisles = s.props.get("result").and_then(|r| r.get("aisles")).and_then(serde_json::Value::as_array);
            for a in aisles.into_iter().flatten() {
                let ring: Vec<P2> = a
                    .get("ring")
                    .and_then(serde_json::Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|q| Some(P2(q.get(0)?.as_f64()?, q.get(1)?.as_f64()?)))
                    .collect();
                let flow = a.get("flow").and_then(|f| Some(P2(f.get(0)?.as_f64()?, f.get(1)?.as_f64()?)));
                aisle_arrow(d, &ring, flow, stroke);
            }
            let _ = fill;
        }
        "parking_band" => {
            let (af, _) = masterplan::plan_colours("aisle", "", false);
            fill_all(d, &polys(s, "aisle"), tone.c(af));
            for role in ["stall", "accessible_stall"] {
                let (f, st) = masterplan::plan_colours(role, "", false);
                let ps = polys(s, role);
                fill_all(d, &ps, tone.c(f));
                outline_all(d, &ps, tone.c(st), &dash);
            }
        }
        "parking_stall" => {
            let role = match use_ {
                "accessible" => "accessible_stall",
                "ev" => "ev_stall",
                _ => "stall",
            };
            let (f, st) = masterplan::plan_colours(role, "", false);
            fill_all(d, &s.outline, tone.c(f));
            outline_all(d, &s.outline, tone.c(st), &dash);
        }
        "tree" => {
            fill_all(d, &polys(s, "crown"), fill);
            outline_all(d, &polys(s, "crown"), stroke, &dash);
            for at in part_lines(s, "point").iter().filter_map(|l| l.first()) {
                dot(d, *at, 2.0, stroke);
            }
        }
        "pitch" => {
            fill_all(d, &polys(s, "run_off"), tone.c(Rgb(222, 238, 212)));
            fill_all(d, &s.outline, fill);
            outline_all(d, &s.outline, stroke, &dash);
            lines(d, &part_lines(s, "marking"), Rgb(255, 255, 255), None);
        }
        "water" => {
            fill_all(d, &s.outline, fill);
            let (bank, _) = masterplan::plan_colours("landscape", "planting", false);
            fill_all(d, &polys(s, "bank"), tone.c(bank));
            outline_all(d, &s.outline, stroke, &dash);
        }
        "ramp" => {
            fill_all(d, &s.outline, fill);
            outline_all(d, &polys(s, "open"), stroke, &dash);
            outline_all(d, &polys(s, "covered"), stroke, &Some(vec![6.0, 4.0]));
        }
        "bike_rack" => {
            fill_all(d, &s.outline, fill);
            outline_all(d, &s.outline, stroke, &dash);
        }
        _ => {
            // A lane's own surface where it has one; else its kind's colour.
            let fill = match (s.kind.as_str(), masterplan::surface_colour(s.surface())) {
                ("lane", Some(_)) if s.surface() == "asphalt" => tone.c(masterplan::road_colours(false)[0]),
                _ => fill,
            };
            fill_all(d, &s.outline, fill);
            outline_all(d, &s.outline, stroke, &dash);
        }
    }
    // What comes down is crossed through, as Revit draws Demolished.
    if s.phase == "demolished"
        && let Some(b) = bounds(&s.outline)
    {
        lines(d, &[vec![b.0, b.1], vec![P2(b.0.0, b.1.1), P2(b.1.0, b.0.1)]], stroke, Some(vec![8.0, 4.0]));
    }
    d.own(None);
}

fn bounds(polys: &[Poly]) -> Option<(P2, P2)> {
    let mut it = polys.iter().flat_map(|p| p.outer.iter().copied());
    let first = it.next()?;
    Some(it.fold((first, first), |(lo, hi), q| (P2(lo.0.min(q.0), lo.1.min(q.1)), P2(hi.0.max(q.0), hi.1.max(q.1)))))
}

/// A property line: dash-dot, as Revit draws one; and, inside it, the buildable line its setbacks draw, thin and dashed.
pub(super) fn property_line(d: &mut Drawing, id: Id, p: &PropertyLine) {
    d.own(Some(id));
    d.part(Part::Projection);
    let px = p.ring().iter().map(|q| d.view.px(*q)).collect();
    d.stroke(px, BY_CATEGORY, Rgb(25, 25, 25), Some(vec![14.0, 3.0, 2.0, 3.0]), true);
    if p.edges.iter().any(|e| e.setback > 0.0) {
        for ring in p.buildable() {
            let px = ring.iter().map(|q| d.view.px(*q)).collect();
            d.stroke(px, BY_CATEGORY, Rgb(200, 60, 40), Some(vec![6.0, 4.0]), true);
        }
    }
    d.own(None);
}

/// A sub-division: its region filled in its surface's colour and outlined, over its host's fill.
pub(super) fn subdivision(d: &mut Drawing, m: &Model, id: Id, s: &SubDivision) {
    let region = m.subdivision_region(s);
    if region.is_empty() {
        return;
    }
    let (fill, line, _) = m.subdivision_colours(s);
    d.own(Some(id));
    d.part(Part::Projection);
    let rings: Vec<Vec<(f32, f32)>> = region.iter().map(|r| r.iter().map(|q| d.view.px(*q)).collect()).collect();
    d.push(Prim::fill(rings, Rgb(fill[0], fill[1], fill[2])));
    for r in &region {
        let px = r.iter().map(|q| d.view.px(*q)).collect();
        d.stroke(px, BY_CATEGORY, Rgb(line[0], line[1], line[2]), None, true);
    }
    d.own(None);
}

/// The survey point (a triangle in a circle, a clip beside it when clipped) or the project base point (a circle with a
/// cross), as Revit draws them, a fixed size on the paper.
pub(super) fn base_point(d: &mut Drawing, id: Id, b: &BasePoint, survey: bool) {
    d.own(Some(id));
    d.part(Part::Projection);
    let (x, y) = d.view.px(b.at);
    let r = 9.0_f32;
    let circle: Vec<(f32, f32)> = (0..24)
        .map(|k| {
            let t = std::f32::consts::TAU * k as f32 / 24.0;
            (x + r * t.cos(), y + r * t.sin())
        })
        .collect();
    let ink = Rgb(40, 40, 40);
    d.stroke(circle, BY_CATEGORY, ink, None, true);
    if survey {
        d.stroke(vec![(x, y - r), (x + r * 0.866, y + r * 0.5), (x - r * 0.866, y + r * 0.5)], BY_CATEGORY, ink, None, true);
        if b.clipped {
            d.stroke(vec![(x + r + 3.0, y - r), (x + r + 9.0, y - r), (x + r + 9.0, y - r + 6.0)], BY_CATEGORY, ink, None, false);
        }
    } else {
        d.stroke(vec![(x - r, y), (x + r, y)], BY_CATEGORY, ink, None, false);
        d.stroke(vec![(x, y - r), (x, y + r)], BY_CATEGORY, ink, None, false);
    }
    d.own(None);
}

/// Every sub-division, site element, property line and base point of `elements` on `level` the view shows: the
/// sub-divisions on the ground first, then the site's areas (by layer, then in the order they were made), then the
/// property lines and the base points over them.
pub(super) fn draw(d: &mut Drawing, m: &Model, level: Guid, elements: &[(Id, &crate::model::Element)], shown: &dyn Fn(Id) -> bool) {
    use crate::model::Element;
    for (id, e) in elements.iter().filter(|(id, _)| shown(*id)) {
        if let Element::SubDivision(sd) = e
            && m.subdivision_level(sd) == Some(level)
        {
            subdivision(d, m, *id, sd);
        }
    }
    // Ground first, then what lies on it: lawns, paving and water; hedges; lanes; roads; parking; trees over it all.
    let rank = |s: &Site| match s.kind.as_str() {
        "hedge" => 1,
        "lane" if s.use_or_default() == "road" => 3,
        "lane" => 2,
        "parking_band" | "car_park" | "parking_stall" | "ramp" | "bike_rack" => 4,
        "tree" => 5,
        _ => 0,
    };
    let mut sites: Vec<(u8, Id, &Site)> = elements
        .iter()
        .filter(|(id, _)| shown(*id))
        .filter_map(|(id, e)| match e {
            Element::Site(s) if s.level == level => Some((rank(s), *id, &**s)),
            _ => None,
        })
        .collect();
    sites.sort_by_key(|(r, id, _)| (*r, *id));
    for (_, id, s) in sites {
        site(d, id, s);
    }
    for (id, e) in elements.iter().filter(|(id, _)| shown(*id)) {
        match e {
            Element::PropertyLine(p) if p.level == level => property_line(d, *id, p),
            Element::SurveyPoint(b) => base_point(d, *id, b, true),
            Element::ProjectBasePoint(b) => base_point(d, *id, b, false),
            _ => {}
        }
    }
}
