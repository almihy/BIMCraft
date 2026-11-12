//! Fill patterns, filled region types and filled regions, as Revit's Manage > Additional Settings > Fill Patterns and
//! Annotate > Region.
//!
//! A [`FillPattern`] is families of parallel lines, as a PAT file has them: each family at an angle through an origin, its
//! lines `spacing` apart and each `shift` along from the one before, dashed or not; a pattern with no lines is solid. A
//! drafting pattern's lengths are mm on paper, so it reads the same at any scale; a model pattern's are mm in the building
//! (a brick's course, a tile). A [`FilledRegionType`] is a foreground and a background pattern, each in its colour, over
//! the paper when it masks; a [`FilledRegion`] is a sketch's outlines (`crate::sketch`) in one view, filled as its type
//! says, its boundary in a line style or unseen. Patterns and types are entries of the project's library; a type names its
//! patterns, a region its type and its line style.

use bimcraft_i18n::msg;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::geom::{self, P2};
use crate::ids::Guid;
use crate::language::Language;
use crate::sketch::SketchLine;

pub mod make;

/// What a pattern's lengths are measured in: mm on paper (drafting), or mm in the building (model).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Target {
    #[default]
    Drafting,
    Model,
}

/// A family of parallel lines in a pattern: at `angle` degrees through `origin`, each line `spacing` from the last across
/// it and `shift` along it; dashed by `dashes` (a dash's length, a space's as a negative one, 0 a dot), or whole.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HatchLine {
    pub angle: f64,
    #[serde(default, skip_serializing_if = "is_origin")]
    pub origin: P2,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub shift: f64,
    pub spacing: f64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dashes: Vec<f64>,
}

fn is_origin(p: &P2) -> bool {
    *p == P2(0.0, 0.0)
}

fn is_zero(x: &f64) -> bool {
    *x == 0.0
}

impl HatchLine {
    /// Whole lines at `angle`, `spacing` apart.
    pub fn simple(angle: f64, spacing: f64) -> HatchLine {
        HatchLine { angle, origin: P2(0.0, 0.0), shift: 0.0, spacing, dashes: Vec::new() }
    }
}

/// A fill pattern: its lines, in drafting or model lengths; none, it fills solid. One made of lines drawn in a view
/// ([`make`]) remembers that view's scale, `drawn`: drafting, it is the drawing on paper at that scale, model, the drawing
/// itself, so turning it from one to the other rescales it; and the desktop offers to redefine it from lines again.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FillPattern {
    #[serde(default)]
    pub target: Target,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lines: Vec<HatchLine>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drawn: Option<f64>,
}

/// The most line families a pattern has, and dashes a family has.
pub const MAX_FAMILIES: usize = 256;
pub const MAX_DASHES: usize = 16;
/// The closest a family's lines may be, and the furthest apart, in the pattern's lengths; and the longest dash.
pub const MIN_SPACING: f64 = 0.05;
pub const MAX_SPACING: f64 = 100_000.0;

impl FillPattern {
    pub fn is_solid(&self) -> bool {
        self.lines.is_empty()
    }

    /// Whether it is lines alone, whole (no dashes) and not shifted along: what the Fill Patterns dialog's Simple Lines
    /// makes and its Edit Lines edits, an angle, an origin and a spacing for each family. A shift only staggers dashes.
    pub fn whole_lines(&self) -> bool {
        !self.lines.is_empty() && self.lines.iter().all(|l| l.dashes.is_empty() && l.shift == 0.0)
    }

    /// The pattern as a `target` one: one made of lines drawn is rescaled (drafting, the drawing on paper at the scale it was
    /// drawn at; model, the drawing), any other only told so.
    pub fn retargeted(&self, target: Target) -> FillPattern {
        let k = match (self.drawn, self.target, target) {
            (Some(k), Target::Model, Target::Drafting) => 1.0 / k,
            (Some(k), Target::Drafting, Target::Model) => k,
            _ => 1.0,
        };
        FillPattern { target, ..self.scaled(k) }
    }

    /// The same pattern `k` times as big: its origins, shifts, spacings and dashes.
    pub fn scaled(&self, k: f64) -> FillPattern {
        let lines = (self.lines.iter())
            .map(|l| HatchLine {
                angle: l.angle,
                origin: l.origin * k,
                shift: l.shift * k,
                spacing: l.spacing * k,
                dashes: l.dashes.iter().map(|d| d * k).collect(),
            })
            .collect();
        FillPattern { target: self.target, lines, drawn: self.drawn }
    }

    /// Whether it's one family or two square to each other, all whole and through the origin: what the Fill Patterns
    /// dialog's simple pattern edits (an angle, a spacing, crosshatched or not).
    pub fn simple(&self) -> Option<(f64, f64, bool)> {
        let plain = |l: &HatchLine| l.dashes.is_empty() && l.shift == 0.0 && l.origin == P2(0.0, 0.0);
        match self.lines.as_slice() {
            [a] if plain(a) => Some((a.angle, a.spacing, false)),
            [a, b] if plain(a) && plain(b) && a.spacing == b.spacing && ((b.angle - a.angle).rem_euclid(180.0) - 90.0).abs() < 1e-9 => {
                Some((a.angle, a.spacing, true))
            }
            _ => None,
        }
    }

    /// An error saying what to fix, unless its families are few enough and each is a family of lines.
    pub fn check(&self, name: &str) -> Result<(), String> {
        let who = format!("fill pattern `{name}`");
        if self.drawn.is_some_and(|k| !(k.is_finite() && k > 0.0 && k <= MAX_SPACING)) {
            return Err(format!("{who}: the scale it was drawn at is a number above 0, at most {MAX_SPACING}"));
        }
        if self.lines.len() > MAX_FAMILIES {
            return Err(format!("{who}: {} line families, more than {MAX_FAMILIES}", self.lines.len()));
        }
        for (i, l) in self.lines.iter().enumerate() {
            let fam = format!("{who} line {}", i + 1);
            if !(l.angle.is_finite() && l.origin.is_finite() && l.shift.is_finite() && l.shift.abs() <= MAX_SPACING) {
                return Err(format!("{fam}: its angle, origin and shift are numbers, the shift within {MAX_SPACING}"));
            }
            if l.origin.0.abs() > MAX_SPACING || l.origin.1.abs() > MAX_SPACING {
                return Err(format!("{fam}: its origin is within {MAX_SPACING} of the pattern's"));
            }
            if !(l.spacing.is_finite() && (MIN_SPACING..=MAX_SPACING).contains(&l.spacing)) {
                return Err(format!("{fam}: its lines are {MIN_SPACING} to {MAX_SPACING} apart, not {}", l.spacing));
            }
            if l.dashes.len() > MAX_DASHES {
                return Err(format!("{fam}: {} dashes and spaces, more than {MAX_DASHES}", l.dashes.len()));
            }
            if l.dashes.iter().any(|x| !x.is_finite() || x.abs() > MAX_SPACING) {
                return Err(format!("{fam}: a dash or a space is a length within {MAX_SPACING}"));
            }
            // Something drawn (a dash, or a dot: 0), a space between, and a pattern that repeats no closer than lines may lie.
            if !l.dashes.is_empty()
                && (l.dashes.iter().map(|x| x.abs()).sum::<f64>() < MIN_SPACING
                    || !l.dashes.iter().any(|x| *x < 0.0)
                    || !l.dashes.iter().any(|x| *x >= 0.0))
            {
                return Err(format!(
                    "{fam}: dashes need a dash (0 is a dot) and a space (a negative length) among them, and {MIN_SPACING} or more in all"
                ));
            }
        }
        Ok(())
    }
}

/// How short a dot draws, in the pattern's lengths.
const DOT: f64 = 0.15;

/// As [`strokes`], the pattern started at `origin` and turned `angle` degrees counter-clockwise.
pub fn strokes_placed(
    p: &FillPattern,
    rings: &[Vec<P2>],
    k: f64,
    window: (P2, P2),
    max: usize,
    (origin, angle): (P2, f64),
) -> Option<Vec<[P2; 2]>> {
    if at_origin(&origin) && angle == 0.0 {
        return strokes(p, rings, k, window, max);
    }
    let (sin, cos) = angle.to_radians().sin_cos();
    let into = |q: P2| {
        let d = q - origin;
        P2(d.0 * cos + d.1 * sin, d.1 * cos - d.0 * sin)
    };
    let back = |q: P2| origin + P2(q.0 * cos - q.1 * sin, q.0 * sin + q.1 * cos);
    let rings: Vec<Vec<P2>> = rings.iter().map(|r| r.iter().map(|q| into(*q)).collect()).collect();
    let (lo, hi) = window;
    let corners = [lo, P2(hi.0, lo.1), hi, P2(lo.0, hi.1)].map(into);
    let fold = |f: fn(f64, f64) -> f64, start: f64, of: fn(&P2) -> f64| corners.iter().map(of).fold(start, f);
    let w = (
        P2(fold(f64::min, f64::INFINITY, |q| q.0), fold(f64::min, f64::INFINITY, |q| q.1)),
        P2(fold(f64::max, f64::NEG_INFINITY, |q| q.0), fold(f64::max, f64::NEG_INFINITY, |q| q.1)),
    );
    Some(strokes(p, &rings, k, w, max)?.into_iter().map(|[a, b]| [back(a), back(b)]).collect())
}

/// The pattern's strokes over the area inside `rings` (outlines and holes, either way round: inside is inside an odd number
/// of them), each as a straight piece in model mm, `k` model mm to each of the pattern's (the view's scale for a drafting
/// pattern, 1 for a model one); within `window` (its lowest and its highest corner), and no more than `max` of them. None
/// when it would take more, too dense to draw.
pub fn strokes(p: &FillPattern, rings: &[Vec<P2>], k: f64, window: (P2, P2), max: usize) -> Option<Vec<[P2; 2]>> {
    let mut out = Vec::new();
    let pts = rings.iter().flatten();
    let (mut lo, mut hi) = (P2(f64::INFINITY, f64::INFINITY), P2(f64::NEG_INFINITY, f64::NEG_INFINITY));
    for q in pts {
        (lo, hi) = (P2(lo.0.min(q.0), lo.1.min(q.1)), P2(hi.0.max(q.0), hi.1.max(q.1)));
    }
    let (lo, hi) = (P2(lo.0.max(window.0.0), lo.1.max(window.0.1)), P2(hi.0.min(window.1.0), hi.1.min(window.1.1)));
    if !(lo.0 < hi.0 && lo.1 < hi.1 && k.is_finite() && k > 0.0) {
        return Some(out);
    }
    let corners = [lo, P2(hi.0, lo.1), hi, P2(lo.0, hi.1)];
    let edges = rings.iter().map(Vec::len).sum::<usize>().max(1);
    let mut work = 0usize;
    for l in &p.lines {
        let (sin, cos) = l.angle.to_radians().sin_cos();
        let (u, n) = (P2(cos, sin), P2(-sin, cos));
        let (origin, step, shift) = (l.origin * k, l.spacing * k, l.shift * k);
        // The lines that cross the box: their numbers across it.
        let across = corners.iter().map(|c| (*c - origin).dot(n));
        let (c0, c1) = across.fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), x| (a.min(x), b.max(x)));
        let (first, last) = ((c0 / step).ceil(), (c1 / step).floor());
        if !(first.is_finite() && last.is_finite()) || last - first > max as f64 {
            return None;
        }
        // Each line is tried against every edge, and each dash stepped over: too much to ask of a drawing, when it comes to it.
        work = work.saturating_add(((last - first + 1.0).max(0.0) as usize).saturating_mul(edges));
        if work > MAX_WORK {
            return None;
        }
        let dashes: Vec<f64> = l.dashes.iter().map(|x| x * k).collect();
        let repeat: f64 = dashes.iter().map(|x| x.abs()).sum();
        if !dashes.is_empty() && !(repeat.is_finite() && repeat > 0.0) {
            continue;
        }
        let mut j = first;
        while j <= last {
            let base = origin + n * (j * step) + u * (j * shift);
            // Only the stretch of the line over the box is wanted: what lies past it is never drawn.
            let Some((w0, w1)) = reach(base, u, lo, hi) else {
                j += 1.0;
                continue;
            };
            for (t0, t1) in spans(rings, base, u, n).into_iter().map(|(a, b)| (a.max(w0), b.min(w1))).filter(|(a, b)| b > a) {
                if dashes.is_empty() {
                    out.push([base + u * t0, base + u * t1]);
                } else {
                    // Each repeat of the dashes from the base along the line, where it's inside.
                    let mut r = (t0 / repeat).floor();
                    while r * repeat <= t1 {
                        let mut s = r * repeat;
                        for d in &dashes {
                            let len = d.abs();
                            if *d >= 0.0 {
                                let (a, b) = (s.max(t0), (s + if *d == 0.0 { DOT * k } else { len }).min(t1));
                                if b > a {
                                    out.push([base + u * a, base + u * b]);
                                }
                            }
                            s += len;
                        }
                        r += 1.0;
                        #[cfg(debug_assertions)]
                        done(dashes.len());
                        work = work.saturating_add(dashes.len());
                        if out.len() > max || work > MAX_WORK {
                            return None;
                        }
                    }
                }
                if out.len() > max {
                    return None;
                }
            }
            j += 1.0;
        }
    }
    Some(out)
}

/// The most work (an edge tried against a line, a dash stepped over) one pattern may ask of a drawing, however dense it is.
pub const MAX_WORK: usize = 5_000_000;

#[cfg(debug_assertions)]
thread_local! {
    /// What [`strokes`] has done on this thread: the edges tried against a line, the dashes stepped over. The tests assert on it,
    /// not on time, which varies with the load of the machine.
    pub static DONE: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(debug_assertions)]
fn done(n: usize) {
    DONE.with(|d| d.set(d.get().saturating_add(n)));
}

/// The stretch of the line through `base` along unit `u` that lies in the box from `lo` to `hi`, as distances from `base`.
fn reach(base: P2, u: P2, lo: P2, hi: P2) -> Option<(f64, f64)> {
    let mut t = (f64::NEG_INFINITY, f64::INFINITY);
    for (b, d, a, z) in [(base.0, u.0, lo.0, hi.0), (base.1, u.1, lo.1, hi.1)] {
        if d.abs() < 1e-12 {
            if b < a || b > z {
                return None;
            }
        } else {
            let (x, y) = ((a - b) / d, (z - b) / d);
            t = (t.0.max(x.min(y)), t.1.min(x.max(y)));
        }
    }
    (t.0 < t.1).then_some(t)
}

/// Where the line through `base` along unit `u` (`n` square to it) is inside `rings`, as runs along it from `base`: it
/// crosses a ring's edge where the edge's ends lie on either side, and between each two crossings it's in or out by turns.
fn spans(rings: &[Vec<P2>], base: P2, u: P2, n: P2) -> Vec<(f64, f64)> {
    #[cfg(debug_assertions)]
    done(rings.iter().map(Vec::len).sum());
    let mut ts: Vec<f64> = Vec::new();
    for (a, b) in rings.iter().flat_map(|r| geom::ring_edges(r)) {
        let (sa, sb) = ((a - base).dot(n), (b - base).dot(n));
        if (sa > 0.0) != (sb > 0.0) {
            let x = a + (b - a) * (sa / (sa - sb));
            ts.push((x - base).dot(u));
        }
    }
    ts.sort_by(f64::total_cmp);
    ts.as_chunks::<2>().0.iter().map(|&[a, b]| (a, b)).filter(|(a, b)| b > a).collect()
}

/// A filled region type: its foreground and background patterns (none, nothing) and their colours, and whether it masks
/// what's under it, the paper showing through around its patterns.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FilledRegionType {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub foreground: Option<String>,
    #[serde(default)]
    pub foreground_color: [u8; 3],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
    #[serde(default)]
    pub background_color: [u8; 3],
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub masking: bool,
}

impl FilledRegionType {
    /// An error saying what to fix, unless the patterns it names are `patterns`'.
    pub fn check(&self, name: &str, patterns: &BTreeMap<String, FillPattern>) -> Result<(), String> {
        for p in [&self.foreground, &self.background].into_iter().flatten() {
            if !patterns.contains_key(p) {
                let there: Vec<&str> = patterns.keys().map(String::as_str).collect();
                return Err(format!("filled region type `{name}`: there's no fill pattern `{p}` (there are: {})", there.join(", ")));
            }
        }
        Ok(())
    }
}

/// A filled region: the outlines its sketch closes into (holes and islands as a sketch nests them), in one view (a plan,
/// a section, an elevation or a sheet, in its own terms), filled as type `type_name` says, its boundary drawn in line style
/// `line_style` or, none, unseen.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FilledRegion {
    pub view: Guid,
    #[serde(rename = "type")]
    pub type_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_style: Option<String>,
    pub sketch: Vec<SketchLine>,
    /// Where its patterns start, and how far they're turned, degrees: Revit's aligned and rotated model pattern.
    #[serde(default, skip_serializing_if = "at_origin")]
    pub pattern_origin: P2,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub pattern_angle: f64,
}

pub fn at_origin(p: &P2) -> bool {
    *p == P2(0.0, 0.0)
}

/// The fill pattern every project has, which fills solid: in English.
pub const SOLID_FILL: &str = msg!("Solid fill");
/// The filled region type a region takes unless told another, and the one Masking Region takes: in English.
pub const SOLID_BLACK: &str = msg!("Solid Black");
pub const MASKING: &str = msg!("Masking");

/// The fill patterns a project starts with, named in `language`: our own, drafting ones mm on paper and model ones mm in the
/// building.
pub fn patterns(language: Language) -> BTreeMap<String, FillPattern> {
    let drafting = |lines: Vec<HatchLine>| FillPattern { target: Target::Drafting, lines, drawn: None };
    let model = |lines: Vec<HatchLine>| FillPattern { target: Target::Model, lines, drawn: None };
    let s = HatchLine::simple;
    [
        (SOLID_FILL, drafting(Vec::new())),
        (msg!("Diagonal Up"), drafting(vec![s(45.0, 1.5)])),
        (msg!("Diagonal Down"), drafting(vec![s(135.0, 1.5)])),
        (msg!("Diagonal Crosshatch"), drafting(vec![s(45.0, 2.0), s(135.0, 2.0)])),
        (msg!("Crosshatch"), drafting(vec![s(0.0, 2.0), s(90.0, 2.0)])),
        (msg!("Horizontal"), drafting(vec![s(0.0, 1.5)])),
        (msg!("Vertical"), drafting(vec![s(90.0, 1.5)])),
        // A dot every mm, each row half a step along from the last.
        (msg!("Sand"), drafting(vec![HatchLine { shift: 0.5, dashes: vec![0.0, -1.0], ..s(0.0, 1.0) }])),
        // Courses 75 mm high; joints every 225 mm, half a brick along from one course to the next.
        (
            msg!("Brick 225 x 75"),
            model(vec![
                s(0.0, 75.0),
                HatchLine { angle: 90.0, origin: P2(0.0, 0.0), shift: 75.0, spacing: 112.5, dashes: vec![75.0, -75.0] },
            ]),
        ),
        (msg!("Tile 300"), model(vec![s(0.0, 300.0), s(90.0, 300.0)])),
    ]
    .into_iter()
    .map(|(n, p)| (language.word(n).to_string(), p))
    .collect()
}

pub fn default_patterns() -> BTreeMap<String, FillPattern> {
    patterns(Language::English)
}

/// The filled region types a project starts with, named in `language`.
pub fn region_types(language: Language) -> BTreeMap<String, FilledRegionType> {
    let fg = |p: &str| FilledRegionType { foreground: Some(language.word(p).to_string()), ..Default::default() };
    [
        (SOLID_BLACK, fg(SOLID_FILL)),
        ("Diagonal Up", fg("Diagonal Up")),
        ("Crosshatch", fg("Crosshatch")),
        ("Sand", fg("Sand")),
        ("Brick", fg("Brick 225 x 75")),
        (MASKING, FilledRegionType { masking: true, ..Default::default() }),
    ]
    .into_iter()
    .map(|(n, t)| (language.word(n).to_string(), t))
    .collect()
}

pub fn default_region_types() -> BTreeMap<String, FilledRegionType> {
    region_types(Language::English)
}

/// The type a filled region takes unless told another, of `types` in `language`: Solid Black by the name the language gives it,
/// else by its English name (the types of a file from before they were named in a language), else the first.
pub fn default_region_type(types: &BTreeMap<String, FilledRegionType>, language: Language) -> Option<&String> {
    [language.word(SOLID_BLACK), SOLID_BLACK]
        .into_iter()
        .find_map(|n| types.get_key_value(n).map(|(k, _)| k))
        .or_else(|| types.keys().next())
}

#[cfg(test)]
#[path = "fills_tests.rs"]
mod tests;
