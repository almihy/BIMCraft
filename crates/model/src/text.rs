//! Text notes' lettering: text types (Revit's: a font, a size on paper, bold, italic and underline, a width factor, a
//! colour and a border), the fonts that letter them, the formatting a note may override on parts of itself, and a note
//! laid out line by line and character by character: what its drawing, the desktop's editor (its caret and selection)
//! and a click on it all read.
//!
//! A type's font is any the system has (`system.rs` lists them), our own single-stroke lettering (`font.rs`), or IBM Plex
//! Sans or Mono, under the OFL (`assets/fonts`, which every package carries with its licence). A font the system hasn't
//! is stood in for by its look-alikes (Arial by Liberation Sans, Arimo, Helvetica…), else Plex Sans: so Arial, the
//! default, letters a note on any system, and a project from another machine opens and draws. A character a face has
//! no letter for is lettered from Plex Sans, then Plex Mono, then Noto Sans Arabic (OFL, carried as Plex is), then the
//! system's wide fonts (CJK ones among them), else drawn as a question mark.
//!
//! Right-to-left text (Arabic, Hebrew) is laid out as it reads: a line's runs are ordered by the Unicode Bidirectional
//! Algorithm (its direction that of its first strong letter), and a right-to-left run is shaped by its font (harfrust),
//! so Arabic's letters join. A line with none is laid out character by character, as it always was. Bold and italic are a family's own faces where it has them; else a face is made bold by
//! drawing its outlines and strokes heavier (a variable one by its weight) and italic by slanting it.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

pub mod system;

use crate::language::Language;

/// IBM Plex Sans, its weights from 100 to 700 (OFL).
pub const PLEX_SANS: &[u8] = include_bytes!("../../../assets/fonts/ibm-plex-sans/IBMPlexSans-Variable.ttf");
/// IBM Plex Mono, regular (OFL).
pub const PLEX_MONO: &[u8] = include_bytes!("../../../assets/fonts/ibm-plex-mono/IBMPlexMono-Regular.ttf");

/// Noto Sans Arabic, regular and bold (OFL): Arabic where the font asked for has none, as in a browser, which has no
/// system fonts.
pub const NOTO_ARABIC: &[u8] = include_bytes!("../../../assets/fonts/noto-sans-arabic/NotoSansArabic-Regular.ttf");
pub const NOTO_ARABIC_BOLD: &[u8] = include_bytes!("../../../assets/fonts/noto-sans-arabic/NotoSansArabic-Bold.ttf");

/// The text size a project's first text type has, mm on paper.
pub const DEFAULT_SIZE: f64 = 2.5;
/// Formatting runs one note may have.
pub const MAX_RUNS: usize = 1000;
/// Characters one note may have.
pub const MAX_NOTE: usize = 4000;
/// From one line's top to the next, times the text size.
pub const LINE_PITCH: f64 = 1.6;
/// How far an italic letter leans: across, for each unit up.
const SLANT: f32 = 0.2;
/// Spaces a tab is.
const TAB: usize = 4;

/// The font a new text type has: on Windows and macOS, Arial itself; elsewhere, what stands in for it.
pub const ARIAL: &str = "Arial";
/// Our single-stroke lettering.
pub const LETTERING: &str = "BIMCraft Lettering";
pub const SANS: &str = "IBM Plex Sans";
pub const MONO: &str = "IBM Plex Mono";
/// Stands behind the others for Arabic; not a font a text type names.
pub const ARABIC: &str = "Noto Sans Arabic";
/// The fonts every BIMCraft has.
pub const BUNDLED: [&str; 3] = [SANS, MONO, LETTERING];

/// Where a face's letters come from.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Source {
    Lettering,
    Sans,
    Mono,
    /// Noto Sans Arabic, its bold a file of its own.
    Arabic,
    /// A system font's file, which of its faces, and its family.
    File {
        path: Arc<Path>,
        index: u32,
        family: Arc<str>,
    },
}

/// A face characters are drawn from: where its letters come from, the weight a variable one is set to, and whether its
/// outlines are drawn heavier (a bold it hasn't) or slanted (an italic it hasn't).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Face {
    pub source: Source,
    pub weight: Option<u16>,
    pub heavy: bool,
    pub slant: bool,
}

impl Face {
    /// One of ours, bold and italic as asked: Plex Sans and Noto Sans Arabic by their own weight, the others drawn
    /// heavier; all slanted.
    pub fn bundled(source: Source, bold: bool, italic: bool) -> Face {
        let own = matches!(source, Source::Sans | Source::Arabic);
        Face { source, weight: (bold && own).then_some(700), heavy: bold && !own, slant: italic }
    }

    /// The family that letters it.
    pub fn family(&self) -> &str {
        match &self.source {
            Source::Lettering => LETTERING,
            Source::Sans => SANS,
            Source::Mono => MONO,
            Source::Arabic => ARABIC,
            Source::File { family, .. } => family,
        }
    }
}

/// The face font `font` letters bold or italic characters in, on this system ([`resolve_in`] its fonts), kept.
pub fn resolve(font: &str, bold: bool, italic: bool) -> Face {
    type Kept = Mutex<HashMap<(String, bool, bool), Face>>;
    static KEPT: OnceLock<Kept> = OnceLock::new();
    let kept = KEPT.get_or_init(Default::default);
    let key = (font.to_string(), bold, italic);
    if let Some(f) = kept.lock().ok().and_then(|m| m.get(&key).cloned()) {
        return f;
    }
    let f = resolve_in(system::index(), font, bold, italic);
    if let Ok(mut m) = kept.lock() {
        if m.len() > 1000 {
            m.clear();
        }
        m.insert(key, f.clone());
    }
    f
}

/// The face font `font` letters bold or italic characters in, of the fonts `index` lists: ours by name, else the family,
/// else the first of its stand-ins the system has, else Plex Sans.
pub fn resolve_in(index: &system::Index, font: &str, bold: bool, italic: bool) -> Face {
    let name = font.trim();
    for (n, source) in [(LETTERING, Source::Lettering), (SANS, Source::Sans), (MONO, Source::Mono)] {
        if name.eq_ignore_ascii_case(n) {
            return Face::bundled(source, bold, italic);
        }
    }
    std::iter::once(name)
        .chain(system::substitutes(name).iter().copied())
        .find_map(|n| index.family(n))
        .and_then(|faces| pick(faces, bold, italic))
        .unwrap_or_else(|| Face::bundled(Source::Sans, bold, italic))
}

/// Of a family's faces, the one nearest bold or italic as asked, and what it is made to be the rest.
fn pick(faces: &[system::SystemFace], bold: bool, italic: bool) -> Option<Face> {
    let target: u16 = if bold { 700 } else { 400 };
    let off = |f: &system::SystemFace| {
        // Upright or italic as asked first, then of normal width (not a Narrow listed in the family), then the weight.
        u32::from(f.italic != italic) * 100_000
            + u32::from(f.width.abs_diff(5)) * 1_000
            + if f.variable { 0 } else { u32::from(f.weight.abs_diff(target)) }
    };
    let f = faces.iter().min_by_key(|f| off(f))?;
    Some(Face {
        source: Source::File { path: f.path.clone(), index: f.index, family: Arc::from(f.family.as_str()) },
        weight: (bold && f.variable).then_some(700),
        heavy: bold && !f.variable && f.weight < 600,
        slant: italic && !f.italic,
    })
}

/// Font `name` as a text type names it, if this system letters it: one of ours, a family it has, or one it stands
/// another in for (Arial anywhere).
pub fn known(name: &str) -> Option<String> {
    known_in(system::index(), name)
}

pub fn known_in(index: &system::Index, name: &str) -> Option<String> {
    let name = name.trim();
    if let Some(b) = BUNDLED.iter().find(|b| b.eq_ignore_ascii_case(name)) {
        return Some(b.to_string());
    }
    if let Some(f) = index.family(name).and_then(|f| f.first()) {
        return Some(f.family.clone());
    }
    system::substitutes(name).iter().find(|s| s.eq_ignore_ascii_case(name)).map(|s| s.to_string())
}

/// Every font a text type can name here, by name: Arial, ours and the system's.
pub fn families() -> Vec<String> {
    let mut out: Vec<String> =
        std::iter::once(ARIAL.to_string()).chain(BUNDLED.iter().map(|b| b.to_string())).chain(system::index().names()).collect();
    out.sort_by_key(|n| n.to_lowercase());
    out.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    out
}

/// A text type: the font its notes are lettered in, their capitals' height on paper (mm), whether they are bold, italic
/// and underlined, how much wider than the font draws them, their colour, and the border drawn round them `border_offset`
/// mm on paper out from their text (from which a leader starts too) in pen `border_pen`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextType {
    #[serde(default = "sans")]
    pub font: String,
    #[serde(default = "default_size")]
    pub size: f64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub bold: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub italic: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub underline: bool,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub width_factor: f64,
    #[serde(default)]
    pub color: [u8; 3],
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub border: bool,
    #[serde(default = "default_offset")]
    pub border_offset: f64,
    #[serde(default = "default_pen")]
    pub border_pen: crate::pen::Pen,
}

fn sans() -> String {
    ARIAL.into()
}
fn default_size() -> f64 {
    DEFAULT_SIZE
}
fn one() -> f64 {
    1.0
}
fn is_one(x: &f64) -> bool {
    *x == 1.0
}
fn default_offset() -> f64 {
    1.0
}
fn default_pen() -> crate::pen::Pen {
    crate::pen::Pen::FINE
}

impl Default for TextType {
    fn default() -> TextType {
        TextType {
            font: sans(),
            size: DEFAULT_SIZE,
            bold: false,
            italic: false,
            underline: false,
            width_factor: 1.0,
            color: [0, 0, 0],
            border: false,
            border_offset: default_offset(),
            border_pen: default_pen(),
        }
    }
}

impl TextType {
    /// An error saying what to fix, unless it is one a note can be lettered in.
    pub fn check(&self, name: &str) -> Result<(), String> {
        let who = format!("text type `{name}`");
        // Any name: a font this system hasn't is stood in for, so a project from another opens and draws.
        let n = self.font.trim().chars().count();
        if n == 0 || n > crate::model::MAX_TEXT || self.font.chars().any(char::is_control) {
            return Err(format!("{who}: a font is named by up to {} characters", crate::model::MAX_TEXT));
        }
        if !(self.size > 0.0 && self.size <= 100.0) {
            return Err(format!("{who}: its size, {} mm, isn't above 0 and at most 100", self.size));
        }
        if !(0.1..=10.0).contains(&self.width_factor) {
            return Err(format!("{who}: its width factor, {}, isn't from 0.1 to 10", self.width_factor));
        }
        if !(0.0..=50.0).contains(&self.border_offset) {
            return Err(format!("{who}: its border offset, {} mm, isn't from 0 to 50", self.border_offset));
        }
        Ok(())
    }

    /// How its notes are lettered where they override nothing.
    pub fn style(&self) -> Style {
        Style { bold: self.bold, italic: self.italic, underline: self.underline }
    }
}

/// The text types a project starts with: 2.5, 3.5 and 5 mm in Arial.
pub fn types(_language: Language) -> BTreeMap<String, TextType> {
    [(DEFAULT_SIZE, "2.5mm"), (3.5, "3.5mm"), (5.0, "5mm")]
        .into_iter()
        .map(|(size, name)| (name.to_string(), TextType { size, ..TextType::default() }))
        .collect()
}

/// [`types`] in English: what a file from before text types has.
pub fn default_types() -> BTreeMap<String, TextType> {
    types(Language::English)
}

/// The type a new note gets when none is named: 2.5mm, else the first.
pub fn default_type(types: &BTreeMap<String, TextType>) -> Option<&String> {
    types.get_key_value("2.5mm").map(|(k, _)| k).or_else(|| types.keys().next())
}

/// The name of a text type of `size` mm for a note from before text types: one in Arial of that size and nothing
/// else, or a new one named for it.
pub fn type_for_size(types: &mut BTreeMap<String, TextType>, size: f64) -> String {
    let wanted = TextType { size, ..TextType::default() };
    if let Some((n, _)) = types.iter().find(|(_, t)| **t == wanted) {
        return n.clone();
    }
    let base = format!("{}mm", (size * 100.0).round() / 100.0);
    let mut name = base.clone();
    let mut i = 2;
    while types.contains_key(&name) {
        name = format!("{base} {i}");
        i += 1;
    }
    types.insert(name.clone(), wanted);
    name
}

/// How a character is lettered.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}

/// What a part of a note overrides of its type's lettering: each of bold, italic and underline set either way, or left
/// to the type. A note keeps them when its type changes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Format {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bold: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub italic: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub underline: Option<bool>,
}

impl Format {
    pub fn is_empty(&self) -> bool {
        *self == Format::default()
    }

    /// `base` with what it overrides.
    pub fn over(&self, base: Style) -> Style {
        Style {
            bold: self.bold.unwrap_or(base.bold),
            italic: self.italic.unwrap_or(base.italic),
            underline: self.underline.unwrap_or(base.underline),
        }
    }
}

/// A part of a note's text, characters `from` up to `to` (counted in characters, a line break one), lettered as its
/// format overrides.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Run {
    pub from: usize,
    pub to: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bold: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub italic: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub underline: Option<bool>,
}

impl Run {
    pub fn format(&self) -> Format {
        Format { bold: self.bold, italic: self.italic, underline: self.underline }
    }
}

/// An error saying what to fix, unless `runs` are in order, each of some characters of a text `len` characters long
/// and overriding something, none overlapping.
pub fn check_runs(who: &str, runs: &[Run], len: usize) -> Result<(), String> {
    if runs.len() > MAX_RUNS {
        return Err(format!("{who}: it has {} formatting runs; a note holds up to {MAX_RUNS}", runs.len()));
    }
    let mut end = 0;
    for (i, r) in runs.iter().enumerate() {
        if r.from >= r.to || r.to > len {
            return Err(format!("{who}: run {i} is of characters {} to {}; it needs some of the text's {len}", r.from, r.to));
        }
        if r.from < end {
            return Err(format!("{who}: run {i} starts at {}, inside the run before it; give them in order, apart", r.from));
        }
        if r.format().is_empty() {
            return Err(format!("{who}: run {i} overrides nothing; give it `bold`, `italic` or `underline`"));
        }
        end = r.to;
    }
    Ok(())
}

/// Each of `len` characters' format, from `runs`.
pub fn formats(runs: &[Run], len: usize) -> Vec<Format> {
    let mut out = vec![Format::default(); len];
    for r in runs {
        for f in out.iter_mut().take(r.to.min(len)).skip(r.from) {
            *f = r.format();
        }
    }
    out
}

/// Characters' formats as runs: each stretch of characters that override the same.
pub fn runs(formats: &[Format]) -> Vec<Run> {
    let mut out: Vec<Run> = Vec::new();
    for (i, f) in formats.iter().enumerate() {
        if f.is_empty() {
            continue;
        }
        match out.last_mut() {
            Some(r) if r.to == i && r.format() == *f => r.to = i + 1,
            _ => out.push(Run { from: i, to: i + 1, bold: f.bold, italic: f.italic, underline: f.underline }),
        }
    }
    out
}

/// `runs` of text `old`, kept on what is the same in `new`: the characters before and after what changed keep theirs,
/// and what came in takes the format of the character before it.
pub fn remap(old: &str, new: &str, runs_of_old: &[Run]) -> Vec<Run> {
    let (a, b): (Vec<char>, Vec<char>) = (old.chars().collect(), new.chars().collect());
    let before = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let after = a[before..].iter().rev().zip(b[before..].iter().rev()).take_while(|(x, y)| x == y).count();
    let was = formats(runs_of_old, a.len());
    let typed = before.checked_sub(1).and_then(|i| was.get(i)).copied().unwrap_or_default();
    let mut now = was[..before].to_vec();
    now.extend(std::iter::repeat_n(typed, b.len() - before - after));
    now.extend_from_slice(&was[a.len() - after..]);
    runs(&now)
}

/// Each character of a note in its type `t`: its type's lettering, with what its runs override where it lets them.
pub fn styles(t: &TextType, text: &str, override_format: bool, runs: &[Run]) -> Vec<Style> {
    let len = text.chars().count();
    let base = t.style();
    if override_format { formats(runs, len).iter().map(|f| f.over(base)).collect() } else { vec![base; len] }
}

/// A letter drawn: in units of the font's capital height, x right and y up from its left end on the baseline. Its
/// `fill`'s rings fill by the non-zero rule; its `strokes` are lines `stroke` wide.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Shape {
    pub fill: Vec<Vec<(f32, f32)>>,
    pub strokes: Vec<Vec<(f32, f32)>>,
    pub stroke: f32,
    pub advance: f32,
}

type Cache = Mutex<HashMap<(Face, char), Arc<Shape>>>;

/// How character `c` is drawn in `face`: from the faces after it when it has no letter for it.
pub fn shape(face: &Face, c: char) -> Arc<Shape> {
    static CACHE: OnceLock<Cache> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    let key = (face.clone(), c);
    if let Some(s) = cache.lock().ok().and_then(|m| m.get(&key).cloned()) {
        return s;
    }
    let s = Arc::new(made(face, c));
    if let Ok(mut m) = cache.lock() {
        // A hostile note can't grow it without end.
        if m.len() > 20_000 {
            m.clear();
        }
        m.insert(key, s.clone());
    }
    s
}

fn made(face: &Face, c: char) -> Shape {
    if c == '\t' {
        let space = made(face, ' ');
        return Shape { advance: space.advance * TAB as f32, ..Shape::default() };
    }
    if unseen(c) {
        return Shape::default();
    }
    behind(face).find_map(|f| drawn(&f, c)).unwrap_or_else(|| lettered(face.heavy, '?'))
}

/// Whether `c` only says how text runs or joins (the direction marks and embeddings, the joiners): nothing is drawn of it.
fn unseen(c: char) -> bool {
    matches!(c, '\u{061C}' | '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}')
}

/// `face`, then the faces that letter what it has no letter for, in the order they are asked: Plex Sans, Plex Mono, Noto
/// Sans Arabic, the system's wide fonts.
fn behind(face: &Face) -> impl Iterator<Item = Face> {
    let bold = face.heavy || face.weight.is_some_and(|w| w >= 600);
    let slant = face.slant;
    let ours = [Source::Sans, Source::Mono, Source::Arabic].map(move |s| Face::bundled(s, bold, slant));
    let wide = move || {
        let index = system::index();
        system::FALLBACKS.iter().filter_map(|f| index.family(f)).filter_map(|faces| pick(faces, bold, slant)).collect::<Vec<_>>()
    };
    std::iter::once(face.clone()).chain(ours).chain(std::iter::once_with(wide).flatten())
}

/// A face's file: one of ours, or a system font's as read.
enum Bytes {
    Ours(&'static [u8]),
    File(Arc<Vec<u8>>),
}

impl Bytes {
    fn as_slice(&self) -> &[u8] {
        match self {
            Bytes::Ours(b) => b,
            Bytes::File(f) => f.as_slice(),
        }
    }
}

/// The file `face`'s letters are in and which of its faces; none for our lettering, which is no file.
fn bytes(face: &Face) -> Option<(Bytes, u32)> {
    Some(match &face.source {
        Source::Lettering => return None,
        Source::Sans => (Bytes::Ours(PLEX_SANS), 0),
        Source::Mono => (Bytes::Ours(PLEX_MONO), 0),
        Source::Arabic => (Bytes::Ours(if face.weight.is_some_and(|w| w >= 600) { NOTO_ARABIC_BOLD } else { NOTO_ARABIC }), 0),
        Source::File { path, index, .. } => (Bytes::File(system::load(path)?), *index),
    })
}

/// `face`'s file read, set to its weight.
fn open<'a>(face: &Face, data: &'a [u8], index: u32) -> Option<ttf_parser::Face<'a>> {
    let mut parsed = ttf_parser::Face::parse(data, index).ok()?;
    if let Some(w) = face.weight {
        parsed.set_variation(ttf_parser::Tag::from_bytes(b"wght"), f32::from(w));
    }
    Some(parsed)
}

/// A font's capitals' height, in its units: as it says, else as its H stands (an older font's OS/2 table doesn't say),
/// else half its em.
fn cap(parsed: &ttf_parser::Face) -> f32 {
    let em = f32::from(parsed.units_per_em().max(16));
    let h = || parsed.glyph_index('H').and_then(|g| parsed.glyph_bounding_box(g)).map(|b| b.y_max);
    f32::from(parsed.capital_height().filter(|&h| h > 0).or_else(h).unwrap_or(0)).max(em * 0.5)
}

/// `c` in `face`, if it has a letter for it.
fn drawn(face: &Face, c: char) -> Option<Shape> {
    if face.source == Source::Lettering {
        return (bimcraft_geom::font::has(c) || c.is_whitespace()).then(|| lettered(face.heavy, c));
    }
    let (data, index) = bytes(face)?;
    let parsed = open(face, data.as_slice(), index)?;
    let glyph = parsed.glyph_index(c).filter(|g| g.0 != 0)?;
    Some(outlined(face, &parsed, glyph))
}

/// Glyph `glyph` of `face`'s file, drawn.
fn outlined(face: &Face, parsed: &ttf_parser::Face, glyph: ttf_parser::GlyphId) -> Shape {
    let cap = cap(parsed);
    let advance = f32::from(parsed.glyph_hor_advance(glyph).unwrap_or(0)) / cap;
    let mut out = Outline { k: 1.0 / cap, rings: Vec::new(), at: (0.0, 0.0) };
    parsed.outline_glyph(glyph, &mut out);
    let fill: Vec<Vec<(f32, f32)>> = out.rings.into_iter().filter(|r| r.len() > 2 && r.len() < 20_000).take(1000).collect();
    // A bold the family hasn't: its outline drawn round in a heavy line.
    let strokes = if face.heavy { fill.iter().map(|r| r.iter().chain(r.first()).copied().collect()).collect() } else { Vec::new() };
    let stroke = if face.heavy { HEAVY } else { 0.0 };
    Shape { fill, strokes, stroke, advance: advance + stroke }
}

/// How wide the line is that draws a bold a family hasn't, in capital heights.
const HEAVY: f32 = 0.07;

/// Whether `face` itself has a letter for `c`.
fn has(face: &Face, c: char) -> bool {
    if face.source == Source::Lettering {
        return bimcraft_geom::font::has(c);
    }
    bytes(face).is_some_and(|(data, index)| open(face, data.as_slice(), index).is_some_and(|p| p.glyph_index(c).is_some_and(|g| g.0 != 0)))
}

/// The face that letters `c` in a text of `face`: itself or the first behind it that has the letter, kept.
fn face_of(face: &Face, c: char) -> Option<Face> {
    type Kept = Mutex<HashMap<(Face, char), Option<Face>>>;
    static KEPT: OnceLock<Kept> = OnceLock::new();
    let kept = KEPT.get_or_init(Default::default);
    let key = (face.clone(), c);
    if let Some(f) = kept.lock().ok().and_then(|m| m.get(&key).cloned()) {
        return f;
    }
    let f = behind(face).find(|f| has(f, c));
    if let Ok(mut m) = kept.lock() {
        if m.len() > 20_000 {
            m.clear();
        }
        m.insert(key, f.clone());
    }
    f
}

/// Glyph `id` of `face`'s file drawn, kept: a shaped run's letters, which are its font's glyphs, not characters.
fn glyph(face: &Face, id: u16) -> Arc<Shape> {
    type Kept = Mutex<HashMap<(Face, u16), Arc<Shape>>>;
    static KEPT: OnceLock<Kept> = OnceLock::new();
    let kept = KEPT.get_or_init(Default::default);
    let key = (face.clone(), id);
    if let Some(s) = kept.lock().ok().and_then(|m| m.get(&key).cloned()) {
        return s;
    }
    let made =
        bytes(face).and_then(|(data, index)| open(face, data.as_slice(), index).map(|p| outlined(face, &p, ttf_parser::GlyphId(id))));
    let s = Arc::new(made.unwrap_or_default());
    if let Ok(mut m) = kept.lock() {
        if m.len() > 20_000 {
            m.clear();
        }
        m.insert(key, s.clone());
    }
    s
}

/// A glyph of a shaped run: which of the font's, the first of the run's characters it stands for, how far the next glyph
/// is from it and where it is drawn from its place, in capital heights (y up).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Shaped {
    glyph: u16,
    cluster: usize,
    advance: f32,
    dx: f32,
    dy: f32,
}

/// The most characters of a run that is shaped as one: a longer one is shaped in pieces.
const MAX_SHAPED: usize = 2000;

/// Right-to-left `text` shaped in `face`: its glyphs from left to right as they are read, kept. None when the face is no
/// file a shaper reads.
fn shaped(face: &Face, text: &[char]) -> Option<Arc<[Shaped]>> {
    type Kept = Mutex<HashMap<(Face, String), Option<Arc<[Shaped]>>>>;
    static KEPT: OnceLock<Kept> = OnceLock::new();
    let kept = KEPT.get_or_init(Default::default);
    let key = (face.clone(), text.iter().collect::<String>());
    if let Some(s) = kept.lock().ok().and_then(|m| m.get(&key).cloned()) {
        return s;
    }
    let s = shape_run(face, text);
    if let Ok(mut m) = kept.lock() {
        if m.len() > 20_000 {
            m.clear();
        }
        m.insert(key, s.clone());
    }
    s
}

fn shape_run(face: &Face, text: &[char]) -> Option<Arc<[Shaped]>> {
    let (data, index) = bytes(face)?;
    let cap = cap(&open(face, data.as_slice(), index)?);
    let font = harfrust::FontRef::from_index(data.as_slice(), index).ok()?;
    let plan = harfrust::ShaperData::new(&font);
    // A variable font at its weight; Noto Sans Arabic's bold is a file of its own.
    let weight = face.weight.filter(|_| face.source != Source::Arabic);
    let instance = weight.map(|w| {
        harfrust::ShaperInstance::from_variations(&font, [harfrust::Variation { tag: harfrust::Tag::new(b"wght"), value: f32::from(w) }])
    });
    let shaper = plan.shaper(&font).instance(instance.as_ref()).build();
    let mut buffer = harfrust::UnicodeBuffer::new();
    for (i, c) in text.iter().enumerate() {
        buffer.add(*c, i as u32);
    }
    buffer.guess_segment_properties();
    buffer.set_direction(harfrust::Direction::RightToLeft);
    let out = shaper.shape(buffer, harfrust::ShapeOptions::new());
    let glyphs = out.glyph_infos().iter().zip(out.glyph_positions()).map(|(g, p)| Shaped {
        glyph: u16::try_from(g.glyph_id).unwrap_or(0),
        cluster: g.cluster as usize,
        advance: p.x_advance as f32 / cap,
        dx: p.x_offset as f32 / cap,
        dy: p.y_offset as f32 / cap,
    });
    Some(glyphs.collect())
}

/// `c` in our lettering, its capital 1 high.
fn lettered(bold: bool, c: char) -> Shape {
    let s: String = c.to_string();
    let strokes =
        bimcraft_geom::font::strokes(&s, 0.0, 0.0, 12.0).into_iter().map(|l| l.into_iter().map(|(x, y)| (x / 12.0, -y / 12.0)).collect());
    let n = c.to_uppercase().count() as f32;
    Shape { fill: Vec::new(), strokes: strokes.collect(), stroke: if bold { 0.16 } else { 0.1 }, advance: n * 11.0 / 12.0 }
}

/// A glyph's outline, its curves cut into short lines.
struct Outline {
    k: f32,
    rings: Vec<Vec<(f32, f32)>>,
    at: (f32, f32),
}

impl Outline {
    fn to(&mut self, p: (f32, f32)) {
        self.at = p;
        if let Some(r) = self.rings.last_mut() {
            r.push((p.0 * self.k, p.1 * self.k));
        }
    }
}

impl ttf_parser::OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.rings.push(Vec::new());
        self.to((x, y));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.to((x, y));
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let a = self.at;
        for i in 1..=6 {
            let t = i as f32 / 6.0;
            let u = 1.0 - t;
            self.to((u * u * a.0 + 2.0 * u * t * x1 + t * t * x, u * u * a.1 + 2.0 * u * t * y1 + t * t * y));
        }
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let a = self.at;
        for i in 1..=8 {
            let t = i as f32 / 8.0;
            let u = 1.0 - t;
            let (b0, b1, b2, b3) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
            self.to((b0 * a.0 + b1 * x1 + b2 * x2 + b3 * x, b0 * a.1 + b1 * y1 + b2 * y2 + b3 * y));
        }
    }
    fn close(&mut self) {}
}

/// A character laid out: the `index`-th of its note's text, its left `x` mm from its line's left end, `advance` mm wide.
/// One read right to left (`rtl`) begins at its right. One of a shaped run has its `glyphs`: what its font draws for it
/// and the characters joined to it (none for one drawn with the character before it), in place of its own letter.
#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    pub index: usize,
    pub ch: char,
    pub x: f64,
    pub advance: f64,
    pub style: Style,
    pub rtl: bool,
    pub glyphs: Option<Arc<[Glyph]>>,
}

/// A glyph of a shaped run laid out: its `shape` drawn from `x` mm along its line and `dy` capital heights above the
/// baseline, slanted when its face is made italic.
#[derive(Clone, Debug, PartialEq)]
pub struct Glyph {
    pub shape: Arc<Shape>,
    pub x: f64,
    pub dy: f32,
    pub slant: bool,
}

/// A line of a note laid out: characters `start` up to `end` (its line break not among them), its `top` and `baseline` mm
/// down from the note's top left. Its `chars` are in the text's order, each where it is read: left to right, unless the
/// line has right-to-left text.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub start: usize,
    pub end: usize,
    pub top: f64,
    pub baseline: f64,
    pub width: f64,
    pub chars: Vec<Placed>,
}

/// A note laid out in its type, mm on paper, x right and y down from its first line's top left, not turned.
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    pub font: String,
    pub size: f64,
    pub width_factor: f64,
    pub lines: Vec<Line>,
    /// How wide its widest line is, and how far down its last line's descenders go.
    pub width: f64,
    pub height: f64,
}

/// `text` laid out in type `t`, each character in its `styles` (as [`styles`] gives them).
pub fn layout(t: &TextType, text: &str, styles: &[Style]) -> Layout {
    let (font, size, wf) = (t.font.clone(), t.size, t.width_factor);
    let mut lines = Vec::new();
    let mut index = 0;
    for (i, line) in text.split('\n').enumerate() {
        let top = i as f64 * LINE_PITCH * size;
        let (chars, x) = line_laid(&font, size * wf, line, index, styles);
        index += chars.len();
        lines.push(Line { start: index - chars.len(), end: index, top, baseline: top + size, width: x, chars });
        index += 1;
    }
    let width = lines.iter().map(|l| l.width).fold(0.0, f64::max);
    let height = lines.last().map_or(size, |l| l.baseline + 0.3 * size);
    Layout { font, size, width_factor: wf, lines, width, height }
}

/// Whether `c` is read right to left, or is a number or a mark of such text: its line is ordered and shaped.
fn right_to_left(c: char) -> bool {
    use unicode_bidi::BidiClass::{AL, AN, R, RLE, RLI, RLO};
    c >= '\u{0590}' && matches!(unicode_bidi::bidi_class(c), R | AL | AN | RLE | RLO | RLI)
}

/// A line's characters laid out, the first of them the `first`-th of its text, a capital height `k` mm wide as well as
/// tall; and how wide the line is.
fn line_laid(font: &str, k: f64, line: &str, first: usize, styles: &[Style]) -> (Vec<Placed>, f64) {
    let style = |i: usize| styles.get(first + i).copied().unwrap_or_default();
    let face = |i: usize| resolve(font, style(i).bold, style(i).italic);
    let mut x = 0.0;
    let mut out = Vec::new();
    let single = |out: &mut Vec<Placed>, x: &mut f64, i: usize, ch: char, rtl: bool| {
        let advance = f64::from(shape(&face(i), ch).advance) * k;
        out.push(Placed { index: first + i, ch, x: *x, advance, style: style(i), rtl, glyphs: None });
        *x += advance;
    };
    if !line.chars().any(right_to_left) {
        for (i, ch) in line.chars().enumerate() {
            single(&mut out, &mut x, i, ch, false);
        }
        return (out, x);
    }
    let chars: Vec<char> = line.chars().collect();
    // Which character each byte of the line is in, and one past the last.
    let mut at = vec![chars.len(); line.len() + 1];
    for (i, (b, c)) in line.char_indices().enumerate() {
        at.iter_mut().skip(b).take(c.len_utf8()).for_each(|a| *a = i);
    }
    let info = unicode_bidi::BidiInfo::new(line, None);
    for para in &info.paragraphs {
        let (levels, runs) = info.visual_runs(para, para.range.clone());
        for run in runs {
            let (a, b) = (at.get(run.start).copied().unwrap_or(0), at.get(run.end).copied().unwrap_or(chars.len()));
            if !levels.get(run.start).is_some_and(|l| l.is_rtl()) {
                for (i, &ch) in chars.iter().enumerate().take(b).skip(a) {
                    single(&mut out, &mut x, i, ch, false);
                }
                continue;
            }
            // The face that letters each character: a space's, and that of one no font has, is its neighbour's, so
            // that words are shaped with the spaces between them.
            let mut faces: Vec<Option<Face>> = (a..b)
                .map(|i| chars.get(i).copied().filter(|c| !c.is_whitespace() && !unseen(*c)).and_then(|c| face_of(&face(i), c)))
                .map(|f| f.filter(|f| f.source != Source::Lettering))
                .collect();
            for i in 1..faces.len() {
                if faces[i].is_none() {
                    faces[i] = faces[i - 1].clone();
                }
            }
            for i in (0..faces.len().saturating_sub(1)).rev() {
                if faces[i].is_none() {
                    faces[i] = faces[i + 1].clone();
                }
            }
            // Its stretches of one face and one style, the last first: they are read from the right.
            let mut end = b;
            while end > a {
                let of = |i: usize| (faces.get(i - a).cloned().flatten(), style(i));
                let mut start = end - 1;
                while start > a && of(start - 1) == of(end - 1) && end - start < MAX_SHAPED {
                    start -= 1;
                }
                let run_face = of(start).0;
                let part = chars.get(start..end).unwrap_or_default();
                let glyphs = run_face.as_ref().and_then(|f| shaped(f, part));
                let (Some(f), Some(glyphs)) = (run_face, glyphs) else {
                    for (i, &ch) in chars.iter().enumerate().take(end).skip(start).rev() {
                        single(&mut out, &mut x, i, ch, true);
                    }
                    end = start;
                    continue;
                };
                // Glyphs of one cluster stand for the characters from theirs up to the cluster's before them (on their
                // left, later in the text): drawn as one, its width shared among those characters.
                let last = part.len().saturating_sub(1);
                let mut rest = glyphs.iter().peekable();
                let mut upto = part.len();
                while let Some(head) = rest.peek() {
                    let cluster = head.cluster.min(last);
                    let mut pen = 0.0;
                    let mut drawn = Vec::new();
                    while let Some(g) = rest.next_if(|g| g.cluster.min(last) == cluster) {
                        let heavy = if f.heavy && g.advance > 0.0 { f64::from(HEAVY) } else { 0.0 };
                        drawn.push(Glyph { shape: glyph(&f, g.glyph), x: x + (pen + f64::from(g.dx)) * k, dy: g.dy, slant: f.slant });
                        pen += f64::from(g.advance) + heavy;
                    }
                    let n = upto.saturating_sub(cluster).max(1);
                    let each = pen * k / n as f64;
                    let mut drawn = Some(Arc::<[Glyph]>::from(drawn));
                    for j in 0..n {
                        let i = start + cluster + j;
                        let Some(&ch) = chars.get(i) else { break };
                        out.push(Placed {
                            index: first + i,
                            ch,
                            x: x + each * (n - 1 - j) as f64,
                            advance: each,
                            style: style(i),
                            rtl: true,
                            glyphs: Some(drawn.take().unwrap_or_else(|| Arc::from(Vec::new()))),
                        });
                    }
                    x += pen * k;
                    upto = upto.min(cluster);
                }
                end = start;
            }
        }
    }
    // A character the ordering left out (it should leave none) still has its place in the text.
    for (i, &ch) in chars.iter().enumerate() {
        if !out.iter().any(|p| p.index == first + i) {
            out.push(Placed { index: first + i, ch, x, advance: 0.0, style: style(i), rtl: false, glyphs: None });
        }
    }
    out.sort_by_key(|p| p.index);
    (out, x)
}

/// Stretches `cells` (each from its left to its right) joined where they touch, from the left.
fn joined(mut cells: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    cells.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out: Vec<(f64, f64)> = Vec::new();
    for (a, b) in cells {
        match out.last_mut() {
            Some(last) if a <= last.1 + 1e-9 => last.1 = last.1.max(b),
            _ => out.push((a, b)),
        }
    }
    out
}

impl Layout {
    /// Where a caret before character `index` stands: x, and its top and bottom, mm.
    pub fn caret(&self, index: usize) -> (f64, f64, f64) {
        let line = self.lines.iter().find(|l| index <= l.end).or(self.lines.last());
        let Some(l) = line else { return (0.0, 0.0, self.size) };
        // Before a character is at its left, or at its right when it is read from the right; after the last, its other end.
        let before = |c: &Placed| if c.rtl { c.x + c.advance } else { c.x };
        let after = |c: &Placed| if c.rtl { c.x } else { c.x + c.advance };
        let x = l.chars.iter().find(|c| c.index >= index).map(before).or_else(|| l.chars.last().map(after)).unwrap_or(0.0);
        (x, l.top - 0.2 * self.size, l.baseline + 0.3 * self.size)
    }

    /// The line the caret before character `index` is on.
    pub fn line_of(&self, index: usize) -> usize {
        self.lines.iter().position(|l| index <= l.end).unwrap_or(self.lines.len().saturating_sub(1))
    }

    /// The caret place nearest point `(x, y)` mm: on the line it's level with (the first above, the last below), before
    /// the character whose middle is past it.
    pub fn index_at(&self, (x, y): (f64, f64)) -> usize {
        let pitch = LINE_PITCH * self.size;
        let n = self.lines.len();
        let i = if y.is_finite() { ((y + 0.3 * self.size) / pitch).floor().clamp(0.0, n.saturating_sub(1) as f64) as usize } else { 0 };
        let Some(l) = self.lines.get(i) else { return 0 };
        if !l.chars.iter().any(|c| c.rtl) {
            return l.chars.iter().find(|c| x < c.x + c.advance / 2.0).map_or(l.end, |c| c.index);
        }
        // The character it is on or nearest: before it on the side it is read from, else after it.
        let off = |c: &Placed| (c.x - x).max(x - c.x - c.advance).max(0.0);
        let Some(c) = l.chars.iter().min_by(|a, b| off(a).total_cmp(&off(b))) else { return l.end };
        let left = x < c.x + c.advance / 2.0;
        if left != c.rtl { c.index } else { c.index + 1 }
    }

    /// The boxes characters `from` up to `to` take, line by line: left, top, right, bottom, mm.
    pub fn boxes(&self, from: usize, to: usize) -> Vec<[f64; 4]> {
        let (from, to) = (from.min(to), from.max(to));
        let mut out = Vec::new();
        for l in &self.lines {
            if to <= l.start && !(from == to && from == l.start) || from > l.end {
                continue;
            }
            let mut cells: Vec<(f64, f64)> =
                l.chars.iter().filter(|c| c.index >= from && c.index < to && c.advance > 0.0).map(|c| (c.x, c.x + c.advance)).collect();
            // A line break taken in shows as a sliver past its line's end.
            if to > l.end && from <= l.end {
                cells.push((l.width, l.width + 0.3 * self.size));
            }
            out.extend(joined(cells).into_iter().map(|(a, b)| [a, l.top - 0.2 * self.size, b, l.baseline + 0.3 * self.size]));
        }
        out
    }

    /// The box its text takes, grown `by` mm all round: left, top, right, bottom.
    pub fn frame(&self, by: f64) -> [f64; 4] {
        [-by, -0.2 * self.size - by, self.width + by, self.height + by]
    }

    /// What is drawn of each character, mm (x right, y down from the note's top left): each letter's rings, filled by
    /// the non-zero rule, lines and how wide they are, and the underlines.
    pub fn drawn(&self) -> Drawn {
        let mut out = Drawn::default();
        let (size, wf) = (self.size as f32, self.width_factor as f32);
        for l in &self.lines {
            for c in &l.chars {
                let base = l.baseline as f32;
                let mut put = |s: &Shape, x0: f32, dy: f32, slant: bool| {
                    let slant = if slant { SLANT } else { 0.0 };
                    let at = |&(gx, gy): &(f32, f32)| (x0 + (gx * wf + slant * (gy + dy)) * size, base - (gy + dy) * size);
                    if !s.fill.is_empty() {
                        out.fill.push(s.fill.iter().map(|r| r.iter().map(at).collect()).collect());
                    }
                    out.strokes.extend(s.strokes.iter().map(|r| (r.iter().map(at).collect::<Vec<_>>(), s.stroke * size)));
                };
                match &c.glyphs {
                    Some(glyphs) => glyphs.iter().for_each(|g| put(&g.shape, g.x as f32, g.dy, g.slant)),
                    None => {
                        let face = resolve(&self.font, c.style.bold, c.style.italic);
                        put(&shape(&face, c.ch), c.x as f32, 0.0, face.slant);
                    }
                }
            }
            // An underline under each stretch of underlined characters.
            let under = l.chars.iter().filter(|c| c.style.underline).map(|c| (c.x, c.x + c.advance)).collect();
            out.underlines
                .extend(joined(under).into_iter().map(|(a, b)| ((a, l.baseline + 0.18 * self.size), (b, l.baseline + 0.18 * self.size))));
        }
        out.underline_width = 0.07 * size;
        out
    }
}

/// A label lettered in our lettering, what it has no letter for in the fonts behind it ([`crate::font::label`]): its
/// capitals 1 tall, x from its left end, y down from its baseline. Each of `fill` is a letter's rings, filled by the
/// non-zero rule; each of `strokes` a line and how wide it is.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Label {
    pub width: f32,
    pub fill: Vec<Vec<Vec<(f32, f32)>>>,
    pub strokes: Vec<(Vec<(f32, f32)>, f32)>,
}

/// `text` as a label, on one line, kept.
pub fn label(text: &str) -> Arc<Label> {
    type Kept = Mutex<HashMap<String, Arc<Label>>>;
    static KEPT: OnceLock<Kept> = OnceLock::new();
    let kept = KEPT.get_or_init(Default::default);
    if let Some(l) = kept.lock().ok().and_then(|m| m.get(text).cloned()) {
        return l;
    }
    let t = TextType { font: LETTERING.to_string(), size: 1.0, ..TextType::default() };
    let line = text.replace('\n', " ");
    let laid = layout(&t, &line, &[]);
    let drawn = laid.drawn();
    let up = |(x, y): (f32, f32)| (x, y - 1.0);
    let l = Arc::new(Label {
        width: laid.width as f32,
        fill: drawn.fill.into_iter().map(|rings| rings.into_iter().map(|r| r.into_iter().map(up).collect()).collect()).collect(),
        strokes: drawn.strokes.into_iter().map(|(r, w)| (r.into_iter().map(up).collect(), w)).collect(),
    });
    if let Ok(mut m) = kept.lock() {
        if m.len() > 5_000 {
            m.clear();
        }
        m.insert(text.to_string(), l.clone());
    }
    l
}

/// A layout's lettering, mm: what [`Layout::drawn`] gives.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Drawn {
    pub fill: Vec<Vec<Vec<(f32, f32)>>>,
    pub strokes: Vec<(Vec<(f32, f32)>, f32)>,
    pub underlines: Vec<((f64, f64), (f64, f64))>,
    pub underline_width: f32,
}

/// Triangles that fill `rings` by the non-zero rule (a letter's outline, its holes wound the other way), each corner
/// counter-clockwise on paper as it is given: what a front end that fills no ring with holes draws a letter with.
pub fn triangles(rings: &[Vec<(f32, f32)>]) -> Vec<[(f32, f32); 3]> {
    fill_triangles(rings, crate::prim::Rule::NonZero)
}

/// The most edges [`fill_triangles`] fills: past them, nothing (a fill that big is drawn ring by ring).
pub const MAX_FILL_EDGES: usize = 4_000_000;
/// The most edges one sweep of [`fill_triangles`] takes: a fill of more is cut into bands across, each of about this many.
const BAND_EDGES: usize = 400_000;
/// The most pairs of edges one sweep looks at for where they cross: past it, the rest are taken not to.
const MAX_CROSSING_WORK: usize = 8_000_000;
/// The most edges [`fill_triangles`] steps over strip by strip, all its sweeps together: past it, the rest is left unfilled,
/// so a hostile fill (edges top to bottom, each starting a little lower) can't keep a frame for ever.
const MAX_SWEEP_WORK: usize = 200_000_000;

/// An edge top to bottom (y down), and which way it went.
type Edge = ((f32, f32), (f32, f32), i32);

/// Where edge `e` is across at height `y`.
fn x_at(e: &Edge, y: f32) -> f32 {
    e.0.0 + (e.1.0 - e.0.0) * (y - e.0.1) / (e.1.1 - e.0.1)
}

/// Triangles that fill `rings` by `rule`: the points they wind round (non-zero), or the points inside an odd number of them
/// (even-odd, so a ring inside another is a hole whichever way it turns). Cut into strips at every corner's and crossing's
/// height, swept top to bottom with the edges each strip crosses; each strip's spans inside are two triangles. A fill of more
/// than a sweep takes is cut into bands across first, each band swept with the edges that reach into it cut to it, so its
/// holes stay holes. Empty past [`MAX_FILL_EDGES`].
pub fn fill_triangles(rings: &[Vec<(f32, f32)>], rule: crate::prim::Rule) -> Vec<[(f32, f32); 3]> {
    let mut edges: Vec<Edge> = Vec::new();
    for r in rings {
        for (i, &a) in r.iter().enumerate() {
            let Some(&b) = r.get((i + 1) % r.len()) else { continue };
            if a.1 != b.1 && a.0.is_finite() && a.1.is_finite() && b.0.is_finite() && b.1.is_finite() {
                if a.1 < b.1 { edges.push((a, b, 1)) } else { edges.push((b, a, -1)) }
            }
        }
        if edges.len() > MAX_FILL_EDGES {
            return Vec::new();
        }
    }
    edges.sort_by(|a, b| a.0.1.total_cmp(&b.0.1));
    let mut work = 0;
    let mut out = Vec::new();
    if edges.len() <= BAND_EDGES {
        sweep(&edges, rule, &mut work, &mut out);
        return out;
    }
    // Bands of about BAND_EDGES edges starting in each, edges of the same top in one; an edge from a band above that reaches
    // into one is cut at its top, and each at its bottom.
    let (mut start, mut reaching) = (0, Vec::<usize>::new());
    while start < edges.len() && work <= MAX_SWEEP_WORK {
        let Some(top) = edges.get(start).map(|e| e.0.1) else { break };
        let mut end = (start + BAND_EDGES).min(edges.len());
        while edges.get(end).is_some_and(|e| e.0.1 <= top) {
            end += 1;
        }
        let bottom = edges.get(end).map_or(f32::INFINITY, |e| e.0.1);
        reaching.retain(|&i| edges.get(i).is_some_and(|e| e.1.1 > top));
        let cut = |e: &Edge| {
            let a = if e.0.1 < top { (x_at(e, top), top) } else { e.0 };
            let b = if e.1.1 > bottom { (x_at(e, bottom), bottom) } else { e.1 };
            (a, b, e.2)
        };
        let mut band: Vec<Edge> = reaching.iter().filter_map(|&i| edges.get(i)).map(cut).collect();
        band.extend(edges.get(start..end).unwrap_or_default().iter().map(cut));
        band.retain(|e| e.1.1 > e.0.1);
        band.sort_by(|a, b| a.0.1.total_cmp(&b.0.1));
        work += band.len();
        sweep(&band, rule, &mut work, &mut out);
        reaching.extend((start..end).filter(|&i| edges.get(i).is_some_and(|e| e.1.1 > bottom)));
        start = end;
    }
    out
}

/// [`fill_triangles`]'s sweep of `edges` (sorted by their tops) into `out`, counting the edges it steps over in `work`.
fn sweep(edges: &[Edge], rule: crate::prim::Rule, work: &mut usize, out: &mut Vec<[(f32, f32); 3]>) {
    let mut ys: Vec<f32> = edges.iter().flat_map(|e| [e.0.1, e.1.1]).collect();
    // Where edges cross, between their ends: each edge against those that start before it ends.
    let mut pairs = 0;
    'cross: for (i, e) in edges.iter().enumerate() {
        for f in edges.get(i + 1..).unwrap_or_default() {
            if f.0.1 >= e.1.1 {
                break;
            }
            pairs += 1;
            if pairs > MAX_CROSSING_WORK {
                break 'cross;
            }
            let (lo, hi) = (e.0.1.max(f.0.1), e.1.1.min(f.1.1));
            if hi <= lo {
                continue;
            }
            let (d0, d1) = (x_at(e, lo) - x_at(f, lo), x_at(e, hi) - x_at(f, hi));
            if d0 * d1 < 0.0 {
                ys.push(lo + (hi - lo) * d0 / (d0 - d1));
            }
        }
    }
    ys.sort_by(f32::total_cmp);
    ys.dedup();
    // The edges that reach the strip, and the next to come in.
    let (mut active, mut next): (Vec<usize>, usize) = (Vec::new(), 0);
    let mut cross: Vec<(f32, f32, f32, i32)> = Vec::new();
    for w in ys.windows(2) {
        let [y0, y1] = [w[0], w[1]];
        while edges.get(next).is_some_and(|e| e.0.1 <= y0) {
            active.push(next);
            next += 1;
        }
        *work += active.len();
        if *work > MAX_SWEEP_WORK {
            return;
        }
        active.retain(|&i| edges.get(i).is_some_and(|e| e.1.1 > y0));
        if y1 - y0 <= 1e-6 {
            continue;
        }
        let mid = (y0 + y1) / 2.0;
        cross.clear();
        cross.extend(
            active.iter().filter_map(|&i| edges.get(i)).filter(|e| e.1.1 >= y1).map(|e| (x_at(e, mid), x_at(e, y0), x_at(e, y1), e.2)),
        );
        cross.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut wind = 0;
        for pair in cross.windows(2) {
            let (l, r) = (pair[0], pair[1]);
            wind += l.3;
            let inside = match rule {
                crate::prim::Rule::NonZero => wind != 0,
                crate::prim::Rule::EvenOdd => wind % 2 != 0,
            };
            if inside {
                out.push([(l.1, y0), (l.2, y1), (r.2, y1)]);
                out.push([(l.1, y0), (r.2, y1), (r.1, y0)]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(text: &str) -> Layout {
        let t = TextType::default();
        layout(&t, text, &styles(&t, text, false, &[]))
    }

    #[test]
    fn a_note_is_laid_out_line_by_line_with_every_character_counted() {
        let l = plain("AB\nőЖ");
        assert_eq!(l.lines.len(), 2);
        assert_eq!((l.lines[0].start, l.lines[0].end, l.lines[1].start, l.lines[1].end), (0, 2, 3, 5));
        assert_eq!(l.lines[1].top, LINE_PITCH * DEFAULT_SIZE);
        assert!(l.lines[1].chars.iter().all(|c| c.advance > 0.5), "Hungarian and Cyrillic letters have widths of their own");
        assert_eq!(l.index_at((-5.0, 0.0)), 0);
        assert_eq!(l.index_at((100.0, 0.0)), 2);
        assert_eq!(l.index_at((100.0, 100.0)), 5);
        assert_eq!(l.line_of(3), 1);
        let (x, ..) = l.caret(1);
        assert_eq!(x, l.lines[0].chars[1].x);
    }

    #[test]
    fn plex_letters_have_outlines_with_holes_and_what_no_font_has_is_a_question_mark() {
        let sans = |bold| Face::bundled(Source::Sans, bold, false);
        let o = shape(&sans(false), 'O');
        assert_eq!(o.fill.len(), 2, "an O is an outline round a hole");
        let cap = o.fill.iter().flatten().map(|p| p.1).fold(f32::MIN, f32::max);
        assert!((cap - 1.0).abs() < 0.05, "a capital is the text size high: {cap}");
        assert!(shape(&sans(true), 'l').advance > shape(&sans(false), 'l').advance * 0.99);
        assert!(shape(&sans(true), 'I').fill != shape(&sans(false), 'I').fill, "bold is Plex Sans's own weight");
        // A character of the last private-use plane: no font has it.
        assert_eq!(*shape(&sans(false), '\u{10FFFD}'), lettered(false, '?'));
        // The lettering has no Σ: Plex Sans letters it.
        let lettering = Face::bundled(Source::Lettering, false, false);
        assert_eq!(*shape(&lettering, 'Σ'), *shape(&sans(false), 'Σ'));
        assert!(*shape(&sans(false), 'Σ') != lettered(false, '?'), "Plex Sans has a Σ of its own");
        assert!(shape(&lettering, 'Ő').fill.is_empty());
        assert!(shape(&Face::bundled(Source::Mono, true, false), 'A').stroke > 0.0, "Plex Mono is made bold by a heavier outline");
    }

    #[test]
    fn arial_is_lettered_by_what_stands_in_for_it_and_a_family_by_its_own_bold_and_italic() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.ttf"), PLEX_SANS).unwrap();
        std::fs::write(dir.path().join("b.ttf"), PLEX_MONO).unwrap();
        let mut index = system::scan(&[dir.path().to_path_buf()]);
        // Plex Sans's file listed as Liberation Sans, a Linux system's Arial; Plex Mono's as a family with a bold face.
        let rename = |index: &mut system::Index, from: &str, to: &str| {
            let mut faces = index.families.remove(from).unwrap();
            faces.iter_mut().for_each(|f| f.family = to.into());
            index.families.insert(to.to_lowercase(), faces);
        };
        rename(&mut index, "ibm plex sans", "Liberation Sans");
        rename(&mut index, "ibm plex mono", "Test Mono");
        let regular = index.families["test mono"][0].clone();
        index.families.get_mut("test mono").unwrap().push(system::SystemFace { weight: 700, index: 7, ..regular });
        let arial = resolve_in(&index, "Arial", false, false);
        assert_eq!(arial.family(), "Liberation Sans");
        assert_eq!(shape(&arial, 'A').fill, shape(&Face::bundled(Source::Sans, false, false), 'A').fill);
        // Bold, a variable font's weight; italic, slanted: it has no italic face.
        let both = resolve_in(&index, "arial", true, true);
        assert_eq!((both.weight, both.heavy, both.slant), (Some(700), false, true));
        // A family's own bold face.
        let bold = resolve_in(&index, "Test Mono", true, false);
        assert!(matches!(bold.source, Source::File { index: 7, .. }) && !bold.heavy && bold.weight.is_none());
        assert!(resolve_in(&index, "Test Mono", false, false).source != bold.source);
        // A font no one has, and Arial where nothing stands in: Plex Sans. Ours, by name.
        assert_eq!(resolve_in(&index, "Futura", false, false).source, Source::Sans);
        assert_eq!(resolve_in(&system::Index::default(), "Arial", false, false).source, Source::Sans);
        assert_eq!(resolve_in(&index, "bimcraft LETTERING", true, false), Face::bundled(Source::Lettering, true, false));
        assert_eq!(known_in(&index, "arial").as_deref(), Some("Arial"));
        assert_eq!(known_in(&index, "liberation SANS").as_deref(), Some("Liberation Sans"));
        assert_eq!(known_in(&system::Index::default(), "Futura"), None);
    }

    #[test]
    fn a_font_that_doesnt_say_its_capitals_height_is_measured_by_its_h() {
        // Plex Sans with its OS/2 table made version 1, which has no capital height: as an older font is.
        let mut bytes = PLEX_SANS.to_vec();
        let n = usize::from(u16::from_be_bytes([bytes[4], bytes[5]]));
        let os2 = (0..n).map(|i| 12 + i * 16).find(|&r| &bytes[r..r + 4] == b"OS/2").unwrap();
        let at = u32::from_be_bytes(bytes[os2 + 8..os2 + 12].try_into().unwrap()) as usize;
        bytes[at..at + 2].copy_from_slice(&1u16.to_be_bytes());
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("old.ttf"), &bytes).unwrap();
        let index = system::scan(&[dir.path().to_path_buf()]);
        let face = pick(index.family("IBM Plex Sans").unwrap(), false, false).unwrap();
        assert!(ttf_parser::Face::parse(&bytes, 0).unwrap().capital_height().is_none());
        let h = shape(&face, 'H').fill.iter().flatten().map(|p| p.1).fold(f32::MIN, f32::max);
        assert!((h - 1.0).abs() < 0.02, "its H is the text size high: {h}");
    }

    #[test]
    fn a_family_letters_in_its_face_of_normal_width_before_a_narrow_one_listed_first() {
        // Windows' Arial Narrow calls its family Arial (its typographic family), a face as regular as Arial's own.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.ttf"), PLEX_SANS).unwrap();
        let mut index = system::scan(&[dir.path().to_path_buf()]);
        let mut faces = index.families.remove("ibm plex sans").unwrap();
        faces.iter_mut().for_each(|f| f.family = "Arial".into());
        let normal = faces[0].clone();
        assert_eq!(normal.width, 5, "Plex Sans is of normal width");
        let narrow = system::SystemFace { width: 3, index: 9, ..normal.clone() };
        index.families.insert("arial".into(), vec![narrow.clone(), normal]);
        assert!(matches!(resolve_in(&index, "Arial", false, false).source, Source::File { index: 0, .. }));
        assert!(matches!(resolve_in(&index, "Arial", true, false).source, Source::File { index: 0, .. }));
        // Narrow alone, it's what there is.
        index.families.insert("arial".into(), vec![narrow]);
        assert!(matches!(resolve_in(&index, "Arial", false, false).source, Source::File { index: 9, .. }));
    }

    #[test]
    fn runs_override_where_the_note_lets_them_and_keep_their_place_when_the_text_changes() {
        let t = TextType { bold: true, ..TextType::default() };
        let r = vec![Run { from: 1, to: 3, bold: Some(false), italic: Some(true), underline: None }];
        assert!(styles(&t, "abcd", false, &r).iter().all(|s| s.bold && !s.italic));
        let s = styles(&t, "abcd", true, &r);
        assert_eq!((s[0].bold, s[1].bold, s[1].italic, s[3].bold), (true, false, true, true));
        assert_eq!(runs(&formats(&r, 4)), r);
        // Typed inside the run, it grows; typed before it, it moves along.
        assert_eq!(remap("abcd", "abXcd", &r)[0], Run { to: 4, ..r[0] });
        assert_eq!(remap("abcd", "XXabcd", &r)[0], Run { from: 3, to: 5, ..r[0] });
        assert!(remap("abcd", "ad", &r).is_empty());
        assert!(check_runs("t", &r, 4).is_ok());
        assert!(check_runs("t", &r, 2).is_err());
        assert!(check_runs("t", &[r[0], r[0]], 4).is_err());
        assert!(check_runs("t", &[Run { from: 0, to: 1, bold: None, italic: None, underline: None }], 4).is_err());
    }

    #[test]
    fn underlines_run_under_each_underlined_stretch() {
        let t = TextType::default();
        let r = [Run { from: 0, to: 2, bold: None, italic: None, underline: Some(true) }];
        let l = layout(&t, "ab cd", &styles(&t, "ab cd", true, &r));
        let d = l.drawn();
        assert_eq!(d.underlines.len(), 1);
        assert_eq!(d.underlines[0].1.0, l.lines[0].chars[2].x);
    }

    #[test]
    fn arabic_is_read_from_the_right_its_letters_joined_and_none_a_question_mark() {
        // "house": ba, ya, ta. Each letter stands left of the one before it.
        let l = plain("بيت");
        let c = &l.lines[0].chars;
        assert_eq!(c.len(), 3);
        assert!(c.iter().all(|c| c.rtl && c.advance > 0.0), "{c:?}");
        assert!(c[0].x > c[1].x && c[1].x > c[2].x, "{c:?}");
        assert!((c[0].x + c[0].advance - l.lines[0].width).abs() < 1e-9, "the first letter ends the line on the right");
        // Shaped: the ya between two letters is its middle form, not the letter as it stands alone.
        let face = Face::bundled(Source::Arabic, false, false);
        let alone = shape(&face, 'ي');
        let joined = c[1].glyphs.as_ref().and_then(|g| g.first()).map(|g| g.shape.clone()).expect("a shaped letter");
        assert!(!joined.fill.is_empty() && *joined != *alone, "ya is joined on both sides");
        let d = l.drawn();
        assert!(d.fill.len() >= 3 && d.strokes.is_empty(), "letters of a font, no lettered question mark");
        // In our lettering too, which has no Arabic: Noto's letters, filled.
        let t = TextType { font: LETTERING.into(), ..TextType::default() };
        let d = layout(&t, "بيت", &[]).drawn();
        assert!(d.fill.len() >= 3 && d.strokes.is_empty());
    }

    #[test]
    fn a_line_of_two_directions_is_ordered_as_it_reads() {
        // Latin first: the line runs left to right, the Arabic word after it read from its right.
        let l = plain("A بيت");
        let c = &l.lines[0].chars;
        assert!(!c[0].rtl && c[0].x == 0.0);
        assert!(c[2].x > c[4].x && c[4].x > c[0].x, "{c:?}");
        // Arabic first: the line runs right to left, so what follows the word stands on its left.
        let l = plain("بيت.");
        let c = &l.lines[0].chars;
        assert!(c[3].x < c[2].x && c[2].x < c[0].x, "{c:?}");
        // A number in it keeps its digits left to right.
        let l = plain("بيت 12");
        let c = &l.lines[0].chars;
        assert!(c[4].x < c[5].x && c[5].x < c[2].x, "{c:?}");
        // Every character of the text has its place, once, in the text's order.
        for text in ["A بيت", "بيت.", "بيت 12", "السلام عليكم world 3", "\u{200F}بيت\u{200E}"] {
            let l = plain(text);
            let got: Vec<usize> = l.lines[0].chars.iter().map(|c| c.index).collect();
            assert_eq!(got, (0..text.chars().count()).collect::<Vec<_>>(), "{text}");
            let wide: f64 = l.lines[0].chars.iter().map(|c| c.advance).sum();
            assert!((wide - l.lines[0].width).abs() < 1e-6, "{text}: the letters fill the line");
        }
    }

    #[test]
    fn a_caret_and_a_selection_follow_right_to_left_text() {
        let l = plain("بيت");
        let w = l.lines[0].width;
        // Before the first letter is the line's right end; after the last, its left.
        assert!((l.caret(0).0 - w).abs() < 1e-9 && l.caret(3).0.abs() < 1e-9);
        for i in 0..=3 {
            let (x, top, _) = l.caret(i);
            assert_eq!(l.index_at((x, top + 1.0)), i, "a click where the caret stands puts it back");
        }
        assert_eq!(l.index_at((w + 5.0, 0.0)), 0);
        assert_eq!(l.index_at((-5.0, 0.0)), 3);
        // The first letter alone is the right end of the word; all of it, one box.
        let b = l.boxes(0, 1);
        assert!(b.len() == 1 && (b[0][2] - w).abs() < 1e-9 && b[0][0] > 0.0, "{b:?}");
        let b = l.boxes(0, 3);
        assert!(b.len() == 1 && b[0][0].abs() < 1e-9 && (b[0][2] - w).abs() < 1e-9, "{b:?}");
        // Underlined, one line under the whole word.
        let t = TextType { underline: true, ..TextType::default() };
        let d = layout(&t, "بيت", &styles(&t, "بيت", false, &[])).drawn();
        assert_eq!(d.underlines.len(), 1);
    }

    #[test]
    fn a_label_the_lettering_cannot_letter_is_lettered_behind_it() {
        let l = label("ROOM غرفة");
        assert!(!l.strokes.is_empty() && !l.fill.is_empty(), "Latin in the lettering's lines, Arabic in Noto's letters");
        assert!(l.width > 5.0);
        let (lo, hi) = l.fill.iter().flatten().flatten().fold((f32::MAX, f32::MIN), |(lo, hi), p| (lo.min(p.1), hi.max(p.1)));
        assert!(lo > -2.0 && hi < 1.0, "on its baseline, y down: {lo} {hi}");
    }

    #[test]
    fn a_letter_with_a_hole_fills_round_it() {
        let square = |a: f32, b: f32, ccw: bool| {
            let r = vec![(a, a), (b, a), (b, b), (a, b)];
            if ccw { r } else { r.into_iter().rev().collect() }
        };
        let area = |t: &[[(f32, f32); 3]]| {
            t.iter().map(|[a, b, c]| ((b.0 - a.0) * (c.1 - a.1) - (c.0 - a.0) * (b.1 - a.1)).abs() / 2.0).sum::<f32>()
        };
        let t = triangles(&[square(0.0, 10.0, true), square(3.0, 7.0, false)]);
        assert!((area(&t) - 84.0).abs() < 1e-3, "{}", area(&t));
        // Two outlines wound the same way, overlapping, as a variable font's are: their union.
        let t = triangles(&[square(0.0, 10.0, true), square(5.0, 15.0, true)]);
        assert!((area(&t) - 175.0).abs() < 1e-3, "{}", area(&t));
    }
}
