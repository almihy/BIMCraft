//! Line patterns, line styles and detail lines, as Revit's Manage > Additional Settings and Annotate > Detail Line.
//!
//! A [`LinePattern`] is a sequence of graphic segments in millimetres of paper, so it reads the same at any scale; a [`LineStyle`]
//! is a pen, a colour and a pattern (or none, solid); a [`DetailLine`] is a line drawn in one view, in a style. Both are
//! entries of the project's library: a style names its pattern, a line its style, and a file holds their GUIDs.

use bimcraft_i18n::msg;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::curves::Curve;
use crate::ids::Guid;
use crate::language::Language;
use crate::pen::{MAX_PEN, Pen};

/// A piece of a line pattern. Lengths, spacing, amplitude and text size are millimetres on paper.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase", deny_unknown_fields)]
pub enum Segment {
    Dash(f64),
    Space(f64),
    Dot,
    /// A dot centered in a segment of this length; its diameter is the pen width.
    #[serde(rename = "dot_length")]
    DotLength(f64),
    Offset {
        length: f64,
        count: u8,
        distance: f64,
    },
    /// Frequency is the number of cycles over this segment's full length.
    Wave {
        length: f64,
        frequency: f64,
        amplitude: f64,
    },
    Zigzag {
        length: f64,
        frequency: f64,
        amplitude: f64,
    },
    Text {
        length: f64,
        size: f64,
        text: String,
    },
}

/// A repeating, freely ordered sequence of graphic segments along an unchanged centerline.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LinePattern(pub Vec<Segment>);

/// What a line with no pattern is: not an entry, a name no pattern may take.
pub const SOLID: &str = "Solid";

/// The most segments a pattern has, and the shortest and the longest one, mm on paper.
pub const MAX_SEGMENTS: usize = 32;
pub const MIN_SEGMENT: f64 = 0.05;
pub const MAX_SEGMENT: f64 = 100.0;

/// How long a dot draws, mm on paper: next to nothing, its round ends make it a dot as wide as the line.
const DOT: f64 = 0.01;

/// A pattern's repeat shorter than this, px, draws solid: dashes too small to see, and too many to draw.
const SMALLEST_REPEAT: f32 = 2.0;

impl LinePattern {
    /// Validate lengths and the bounded parameters of each segment in one place.
    pub fn check(&self, name: &str) -> Result<(), String> {
        let who = format!("line pattern `{name}`");
        if !(1..=MAX_SEGMENTS).contains(&self.0.len()) {
            return Err(format!("{who}: a pattern needs 1 to {MAX_SEGMENTS} segments"));
        }
        for (i, s) in self.0.iter().enumerate() {
            let valid = |v: f64| v.is_finite() && (MIN_SEGMENT..=MAX_SEGMENT).contains(&v);
            if !matches!(s, Segment::Dot) && !valid(s.length()) {
                return Err(format!(
                    "{who}: segment {} is {} mm; a segment is {MIN_SEGMENT} to {MAX_SEGMENT} mm on paper",
                    i + 1,
                    s.length()
                ));
            }
            let bad = match s {
                Segment::Offset { count, distance, .. } => !(2..=9).contains(count) || !valid(*distance),
                Segment::Wave { frequency, amplitude, .. } | Segment::Zigzag { frequency, amplitude, .. } => {
                    !(frequency.is_finite() && (0.1..=100.0).contains(frequency) && valid(*amplitude))
                }
                Segment::Text { size, text, .. } => {
                    !valid(*size) || text.trim().is_empty() || text.chars().count() > 256 || text.chars().any(char::is_control)
                }
                _ => false,
            };
            if bad {
                return Err(format!(
                    "{who}: segment {}: offsets need 2–9 lines and positive spacing; waves need 0.1–100 cycles and positive amplitude; text needs 1–256 characters on one line and a size of {MIN_SEGMENT}–{MAX_SEGMENT} mm",
                    i + 1
                ));
            }
        }
        Ok(())
    }

    /// Whether the native dash renderer can draw this unchanged (including legacy dots).
    pub fn is_dash(&self) -> bool {
        !self.0.is_empty()
            && self.0.len().is_multiple_of(2)
            && self.0.iter().enumerate().all(|(i, s)| matches!((i % 2, s), (0, Segment::Dash(_) | Segment::Dot) | (1, Segment::Space(_))))
    }

    /// Its on and off lengths, px, where a mm of paper is `px_per_mm` pixels: none when its repeat is too small to see, so
    /// it draws solid.
    pub fn dash(&self, px_per_mm: f32) -> Option<Vec<f32>> {
        if !self.is_dash() {
            return None;
        }
        let px: Vec<f32> = self
            .0
            .iter()
            .map(|s| match s {
                Segment::Dash(l) | Segment::Space(l) => *l as f32 * px_per_mm,
                _ => DOT as f32 * px_per_mm,
            })
            .collect();
        let repeat: f32 = px.iter().sum();
        (repeat.is_finite() && repeat >= SMALLEST_REPEAT).then_some(px)
    }
}

impl Segment {
    /// The centerline distance reserved for this segment, mm on paper.
    pub fn length(&self) -> f64 {
        match self {
            Self::Dash(l) | Self::Space(l) | Self::DotLength(l) => *l,
            Self::Dot => DOT,
            Self::Offset { length, .. } | Self::Wave { length, .. } | Self::Zigzag { length, .. } | Self::Text { length, .. } => *length,
        }
    }
}

mod pattern;

/// A line style: the pen it draws in, its colour, and its pattern by name (none, solid).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LineStyle {
    pub pen: Pen,
    #[serde(default)]
    pub color: [u8; 3],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
}

impl LineStyle {
    /// An error saying what to fix, unless its pen is one and its pattern is one of `patterns`.
    pub fn check(&self, name: &str, patterns: &BTreeMap<String, LinePattern>) -> Result<(), String> {
        let who = format!("line style `{name}`");
        if !self.pen.is_valid() {
            return Err(format!("{who}: a line weight is a pen, 0 (a hairline) to {MAX_PEN}, not {}", self.pen.0));
        }
        if let Some(p) = &self.pattern
            && !patterns.contains_key(p)
        {
            let there: Vec<&str> = patterns.keys().map(String::as_str).collect();
            return Err(format!("{who}: there's no line pattern `{p}` (there are: {SOLID}, {})", there.join(", ")));
        }
        Ok(())
    }
}

/// The line patterns a project starts with, named in `language`: our own lengths, mm on paper.
pub fn patterns(language: Language) -> BTreeMap<String, LinePattern> {
    use Segment::{Dash as D, Dot, Space as S};
    [
        (msg!("Dash"), vec![D(3.0), S(1.5)]),
        (msg!("Long Dash"), vec![D(8.0), S(2.0)]),
        (msg!("Dot"), vec![Dot, S(1.0)]),
        (msg!("Dash Dot"), vec![D(5.0), S(1.5), Dot, S(1.5)]),
        (msg!("Hidden"), vec![D(1.5), S(1.0)]),
        (msg!("Centre"), vec![D(10.0), S(1.5), D(2.0), S(1.5)]),
    ]
    .into_iter()
    .map(|(n, s)| (language.word(n).to_string(), LinePattern(s)))
    .collect()
}

pub fn default_patterns() -> BTreeMap<String, LinePattern> {
    patterns(Language::English)
}

/// The style a detail line is drawn in unless told another, the first of a project's styles: in English.
pub const THIN: &str = msg!("Thin Lines");

/// The line styles a project starts with, named in `language`, black: a hairline, thin, medium and wide lines in the pens
/// of fine lines, of what's seen and of what's cut, and hidden, centre and overhead lines.
pub fn styles(language: Language) -> BTreeMap<String, LineStyle> {
    let pattern = |n: &str| Some(language.word(n).to_string());
    let style = |pen, pattern| LineStyle { pen, color: [0, 0, 0], pattern };
    [
        (msg!("Hairline"), style(Pen::HAIRLINE, None)),
        (THIN, style(Pen::FINE, None)),
        (msg!("Medium Lines"), style(Pen::PROJECTION, None)),
        (msg!("Wide Lines"), style(Pen::CUT, None)),
        (msg!("Hidden Lines"), style(Pen(2), pattern("Hidden"))),
        (msg!("Centre Lines"), style(Pen(2), pattern("Centre"))),
        (msg!("Overhead Lines"), style(Pen(2), pattern("Dash"))),
    ]
    .into_iter()
    .map(|(n, s)| (language.word(n).to_string(), s))
    .collect()
}

pub fn default_styles() -> BTreeMap<String, LineStyle> {
    styles(Language::English)
}

/// The style a detail line is drawn in unless told another, of `styles` in `language`: Thin Lines by the name the language
/// gives it, else by its English name (the styles of a file from before they were named in a language), else whichever is a
/// thin black solid line, else the first.
pub fn default_style(styles: &BTreeMap<String, LineStyle>, language: Language) -> Option<&String> {
    let thin = LineStyle { pen: Pen::FINE, color: [0, 0, 0], pattern: None };
    [language.word(THIN), THIN]
        .into_iter()
        .find_map(|n| styles.get_key_value(n).map(|(k, _)| k))
        .or_else(|| styles.iter().find(|(_, s)| **s == thin).map(|(k, _)| k))
        .or_else(|| styles.keys().next())
}

/// A detail line: a line drawn in one view (a level's plan, a section, an elevation or a sheet), in the view's own terms
/// (plan mm, along and up, or paper mm), in line style `style`. It is any [`Curve`]: a line, an arc, a polyline through
/// points (round to the first again where closed), a circle, an ellipse or a spline. In a file it's one flat object: its
/// view, its style and its curve's fields.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "RawDetailLine", into = "RawDetailLine")]
pub struct DetailLine {
    pub view: Guid,
    pub curve: Curve,
    pub style: String,
    /// Revit's Insulation: drawn as batt insulation this wide (mm in the view's terms) along its curve, instead of a line.
    pub insulation: Option<f64>,
    /// Revit's Center Mark Visible: a small cross drawn at the centre of each of its arcs, its circle or its ellipse.
    pub centre_mark: bool,
    /// Revit's Revision Cloud: drawn as scallops this long (mm in the view's terms) along its curve, outward round a loop.
    pub cloud: Option<f64>,
    /// A revision cloud's revision, as it's issued: "Rev 2", "B".
    pub revision: Option<String>,
}

crate::curve_struct! {
    /// A detail line as a file holds it.
    RawDetailLine {
        view: Guid,
        style: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        insulation: Option<f64>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        centre_mark: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cloud: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        revision: Option<String>,
    }
}

impl TryFrom<RawDetailLine> for DetailLine {
    type Error = String;
    fn try_from(r: RawDetailLine) -> Result<DetailLine, String> {
        Ok(DetailLine {
            curve: r.fields().curve()?,
            view: r.view,
            style: r.style,
            insulation: r.insulation,
            centre_mark: r.centre_mark,
            cloud: r.cloud,
            revision: r.revision,
        })
    }
}

impl From<DetailLine> for RawDetailLine {
    fn from(l: DetailLine) -> RawDetailLine {
        let mut r = RawDetailLine {
            view: l.view,
            style: l.style,
            insulation: l.insulation,
            centre_mark: l.centre_mark,
            cloud: l.cloud,
            revision: l.revision,
            ..RawDetailLine::default()
        };
        r.set(&l.curve);
        r
    }
}

/// The widest insulation, mm.
pub const MAX_INSULATION: f64 = 10_000.0;

/// How far apart a batt's loops are, for each mm of its width: Revit's Insulation Bulge to Width Ratio of 2.
pub const BULGE: f64 = 0.5;

impl DetailLine {
    /// An error saying what to fix, unless its curve is one (see [`Curve::check`]) and its insulation, if any, a width.
    pub fn check(&self, who: &str) -> Result<(), String> {
        self.curve.check(who)?;
        match self.insulation {
            Some(w) if !(w.is_finite() && w > crate::model::TOL && w <= MAX_INSULATION) => {
                Err(format!("{who}: its insulation is {w} mm wide; it's more than 1 mm and at most {MAX_INSULATION}"))
            }
            _ => Ok(()),
        }?;
        match self.cloud {
            Some(a) if !(a.is_finite() && a > crate::model::TOL && a <= MAX_INSULATION) => {
                Err(format!("{who}: its cloud's arcs are {a} mm long; they're more than 1 mm and at most {MAX_INSULATION}"))
            }
            // Two ways to draw one line: the insulation would win and the cloud go unseen.
            Some(_) if self.insulation.is_some() => Err(format!("{who}: it is insulation or a revision cloud, not both")),
            _ => Ok(()),
        }?;
        match &self.revision {
            Some(r) if r.chars().count() > crate::model::MAX_TEXT => {
                Err(format!("{who}: its revision is longer than {} letters", crate::model::MAX_TEXT))
            }
            _ => Ok(()),
        }
    }

    /// The points it's drawn through: its curve's, batt insulation's loops along it, `width` across, or a revision
    /// cloud's scallops.
    pub fn drawn(&self) -> Vec<crate::geom::P2> {
        match (self.insulation, self.cloud) {
            (Some(w), _) => batt(&self.curve, w),
            (None, Some(a)) => cloud(&self.curve, a),
            (None, None) => self.curve.points(),
        }
    }
}

/// How far each scallop of a cloud turns, degrees.
const SCALLOP: f64 = 150.0;

/// A revision cloud along `c`: arcs about `arc` long end to end, each bulging out (round a loop, away from inside it; along
/// an open line, to its left), as Revit draws one.
pub fn cloud(c: &Curve, arc: f64) -> Vec<crate::geom::P2> {
    let Some(walk) = Walk::of(c) else { return c.points() };
    if arc.is_nan() || arc <= 0.0 {
        return walk.pts;
    }
    let closed = c.is_closed();
    // Round a loop drawn counter-clockwise, outside is to the right: an arc turning left from one point to the next bulges
    // that way.
    let sweep = match closed {
        true if crate::geom::area(&walk.pts) < 0.0 => -SCALLOP,
        true => SCALLOP,
        false => -SCALLOP,
    };
    // Where its way turns sharply (a rectangle's corners) a scallop ends, so none cuts across a corner; a curve with more
    // corners than the cap below allows is scalloped evenly instead.
    let (min, most) = (if closed { 3 } else { 1 }, crate::curves::MAX_DRAWN / SCALLOP_POINTS);
    let mut stops = vec![0.0];
    for (i, w) in walk.pts.windows(3).enumerate() {
        if (w[1] - w[0]).unit().dot((w[2] - w[1]).unit()) < 0.9 {
            stops.push(walk.along[i + 1]);
        }
    }
    stops.push(walk.total);
    let mut cuts = Vec::new();
    for span in stops.windows(2).filter(|s| s[1] - s[0] > 1e-9) {
        let n = scallops(span[1] - span[0], arc, most);
        cuts.extend((0..n).map(|k| span[0] + (span[1] - span[0]) * k as f64 / n as f64));
        // Past the most there is no use listing more: the even scalloping below takes over.
        if cuts.len() > most {
            break;
        }
    }
    cuts.push(walk.total);
    if cuts.len() - 1 < min || cuts.len() - 1 > most {
        let n = ((walk.total / arc).round() as usize).clamp(min, most);
        cuts = (0..=n).map(|k| walk.total * k as f64 / n as f64).collect();
    }
    // Each scallop draws within its share of the points a curve may draw.
    let share = (crate::curves::MAX_DRAWN / (cuts.len() - 1)).max(2);
    let mut out = Vec::new();
    for (k, w) in cuts.windows(2).enumerate() {
        let (a, b) = (walk.at(w[0]).0, walk.at(w[1]).0);
        let mut pts = Curve::arc(a, b, sweep).points();
        if pts.len() > share {
            let last = pts.len() - 1;
            pts = (0..share).map(|i| pts[i * last / (share - 1)]).collect();
        }
        out.extend(pts.into_iter().skip(usize::from(k > 0)));
    }
    out
}

/// How many scallops of about `arc` fit a span of `len`: at least one, and at most `most + 1`, so a hostile length or arc never
/// sizes the list of cuts (a circle 5e8 mm across asked 16.8 GB of them).
pub fn scallops(len: f64, arc: f64, most: usize) -> usize {
    ((len / arc).round().max(1.0) as usize).min(most + 1)
}

/// The points a scallop gets at least, so a cloud of more scallops than the points allow is made of fewer, larger ones.
const SCALLOP_POINTS: usize = 8;

/// A curve's drawn points with how far along it each lies, to find a point and its way at any distance.
struct Walk {
    pts: Vec<crate::geom::P2>,
    along: Vec<f64>,
    total: f64,
}

impl Walk {
    fn of(c: &Curve) -> Option<Walk> {
        let pts = c.points();
        let mut along = vec![0.0];
        for w in pts.windows(2) {
            let last = along.last().copied().unwrap_or(0.0);
            along.push(last + w[0].dist(w[1]));
        }
        let total = along.last().copied().unwrap_or(0.0);
        (total > 0.0 && pts.len() >= 2).then_some(Walk { pts, along, total })
    }

    /// The point `s` along, and the way it runs there.
    fn at(&self, s: f64) -> (crate::geom::P2, crate::geom::P2) {
        let s = s.clamp(0.0, self.total);
        // `along` only grows: the first stretch that reaches `s`.
        let i = self.along.partition_point(|a| *a < s).min(self.along.len() - 1).max(1);
        let (a, b) = (self.pts[i - 1], self.pts[i]);
        let span = (self.along[i] - self.along[i - 1]).max(f64::MIN_POSITIVE);
        let k = ((s - self.along[i - 1]) / span).clamp(0.0, 1.0);
        (a + (b - a) * k, (b - a).unit())
    }
}

/// Batt insulation along `c`, `width` across: a line looping across it again and again, each loop the whole width tall and
/// `BULGE` times it on from the last, as a section through a roll of it is drawn.
pub fn batt(c: &Curve, width: f64) -> Vec<crate::geom::P2> {
    use crate::geom::P2;
    let Some(walk) = Walk::of(c) else { return c.points() };
    let (total, pitch) = (walk.total, width * BULGE);
    if pitch.is_nan() || pitch <= 0.0 {
        return walk.pts;
    }
    let at = |s: f64| -> (P2, P2) { walk.at(s) };
    // A prolate cycloid: across by the cosine, along by the steady run less a sine, running back over the top: a loop.
    let loops = (total / pitch).round().max(1.0);
    let pitch = total / loops;
    let steps = ((loops * 32.0) as usize).min(crate::curves::MAX_DRAWN);
    let back = pitch * 0.6;
    (0..=steps)
        .map(|k| {
            let theta = std::f64::consts::TAU * loops * k as f64 / steps as f64;
            let s = pitch * theta / std::f64::consts::TAU - back * theta.sin();
            let (p, u) = at(s);
            p + u.perp() * (width / 2.0 * theta.cos())
        })
        .collect()
}
