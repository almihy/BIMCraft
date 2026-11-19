//! A drawing's fonts. A DXF's or a DWG's text is lettered in its text style's font by CADCraft's own text code
//! (`cadcraft_fonts`, which the DWG reader builds already): an SHX font's shapes, a TrueType font's outlines, Arabic
//! joined and read from the right, the characters a font lacks from its fallbacks. So a drawing reads on the canvas as it
//! does in Arqiflow's drawing viewer, which is that code too, and in the program that made it; an old Arabic SHX font that
//! draws an Arabic letter for each Latin key (`xarab.shx`: a drawing stores "Hu]H] Hgtw,G" and shows «اعداد الفصول»,
//! written from its point leftward) draws so with no more said. [`lettered`] is a line of text in its font.
//!
//! The font's file must be here for that: a host gives it at run time ([`add`]; a browser has no fonts folder), having
//! asked which a model's drawings name and lack ([`missing`]). A font not here is stood in for by CADCraft's stroke font,
//! as in the viewer. A text is kept as the file holds it, with its style's font (`Shape::Text::font`), and lettered
//! when it is drawn, so a font given after its drawing was read still letters it.
//!
//! Lettering gives shapes, not words. What a text in a keyboard-laid Arabic font says, for who reads it as words (the
//! agent: `cad_read`, `cad_outlines`' labels), is [`shown`], by CADCraft's table for the font
//! (`cadcraft_fonts::keyboard`, which knows a font by its glyph data and has the xarab family's).
//!
//! A text with no font (a PDF page's, which has no text styles) is lettered in ours ([`crate::font`]).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use super::{Dxf, Font, Shape};

/// How many fonts have been given: what is kept of how texts are lettered and read is of that many.
static GIVEN: AtomicU64 = AtomicU64::new(0);

/// The most fonts a model's drawings are asked about.
const MAX_ASKED: usize = 1000;
/// The most lines of text kept lettered.
const MAX_KEPT: usize = 50_000;

/// A font file a host gives (the firm's, the computer's, a stand-in), by its file name: "shx" or "outline", or why it
/// was refused. Text drawn after it is lettered by it.
pub fn add(name: &str, bytes: &[u8]) -> Result<&'static str, String> {
    let added = cadcraft_fonts::given::add_font(name, bytes)?;
    GIVEN.fetch_add(1, Ordering::Relaxed);
    Ok(match added {
        cadcraft_fonts::given::Added::Shx => "shx",
        cadcraft_fonts::given::Added::Outline => "outline",
    })
}

/// Whether font `name`, as a text style names it, is here (given, on this system, or the stroke font, which an empty
/// name is): a host asks between its sources of fonts.
pub fn has(name: &str) -> bool {
    cadcraft_fonts::given::found(name)
}

/// The fonts (and big fonts) drawings' texts name that are not here, each once, as the styles spell them: what a host
/// may find and give with [`add`].
pub fn missing<'a>(drawings: impl IntoIterator<Item = &'a Dxf>) -> Vec<String> {
    let mut seen: Vec<Arc<Font>> = Vec::new();
    for d in drawings {
        let shapes = d.entities.iter().map(|e| &e.shape).chain(d.blocks.iter().flat_map(|b| b.entities.iter().map(|e| &e.shape)));
        for shape in shapes {
            if let Shape::Text { font: Some(f), .. } = shape
                && !seen.iter().any(|s| Arc::ptr_eq(s, f) || (s.name.eq_ignore_ascii_case(&f.name) && s.big.eq_ignore_ascii_case(&f.big)))
                && seen.len() < MAX_ASKED
            {
                seen.push(f.clone());
            }
        }
    }
    cadcraft_fonts::given::missing(seen.iter().map(|f| (f.name.as_str(), f.big.as_str())))
}

/// A line of text lettered in its font, its capitals 1 tall: x along its baseline from where its pen starts, y up from
/// the baseline. `strokes` are lines (an SHX font's, the stroke font's, an underline); each of `fills` is a letter's
/// rings, filled by even-odd (a TrueType font's). It takes `width` from `start`, which is 0, or less for a font that
/// writes leftward from its point (an old Arabic SHX font).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Lettered {
    pub strokes: Vec<Vec<(f32, f32)>>,
    pub fills: Vec<Vec<Vec<(f32, f32)>>>,
    pub start: f32,
    pub width: f32,
}

/// What is kept, of [`GIVEN`] fonts: each font found, the lines lettered, and what is read of texts in keyboard-laid
/// fonts. A font is known by where it is held, and held here while it is, so that place is no other font's.
#[derive(Default)]
struct Kept {
    given: u64,
    faces: HashMap<usize, (Arc<Font>, cadcraft_fonts::TextFont)>,
    lines: HashMap<(usize, u64, String), Arc<Lettered>>,
    keyed: HashMap<String, bool>,
    texts: HashMap<(String, String), Option<Arc<str>>>,
}

/// What is kept, emptied when a font was given since, or when it holds too much.
fn kept() -> Option<std::sync::MutexGuard<'static, Kept>> {
    static KEPT: OnceLock<Mutex<Kept>> = OnceLock::new();
    let mut kept = KEPT.get_or_init(Default::default).lock().ok()?;
    let given = GIVEN.load(Ordering::Relaxed);
    if kept.given != given || kept.lines.len() > MAX_KEPT || kept.texts.len() > MAX_KEPT || kept.faces.len() > MAX_ASKED {
        *kept = Kept { given, ..Kept::default() };
    }
    Some(kept)
}

/// `text` (one line, as the file holds it) lettered in `font`, its letters `wide` times as wide as the font draws them:
/// kept, for a text is drawn at every frame.
pub fn lettered(font: &Arc<Font>, text: &str, wide: f64) -> Arc<Lettered> {
    let wide = if wide.is_finite() && wide > 0.0 { wide } else { 1.0 };
    let key = (Arc::as_ptr(font) as usize, wide.to_bits(), text.to_string());
    let face = {
        let Some(mut kept) = kept() else { return Arc::default() };
        if let Some(l) = kept.lines.get(&key) {
            return l.clone();
        }
        let found = || cadcraft_fonts::TextFont::resolve_style_in(&font.name, &font.big, font.vertical, &font.code_page);
        kept.faces.entry(key.0).or_insert_with(|| (font.clone(), found())).1.clone()
    };
    // Shaped with nothing held: a long line in a big font takes a while, and another view may letter meanwhile.
    let oblique = if font.oblique.is_finite() { font.oblique } else { 0.0 };
    let shaped = cadcraft_fonts::shape_line_dir(&face, text, 1.0, wide, oblique, Some(false));
    let ok = |line: &Vec<(f32, f32)>| line.iter().all(|p| p.0.is_finite() && p.1.is_finite());
    let lettered = Arc::new(Lettered {
        strokes: shaped
            .strokes
            .iter()
            .map(|l| l.iter().map(|p| (p.x as f32, p.y as f32)).collect::<Vec<_>>())
            .filter(|l| l.len() > 1 && ok(l))
            .collect(),
        fills: shaped
            .glyphs
            .iter()
            .map(|g| {
                g.iter()
                    .map(|r| r.iter().map(|p| (p.x as f32, p.y as f32)).collect::<Vec<_>>())
                    .filter(|r| r.len() > 2 && ok(r))
                    .collect::<Vec<_>>()
            })
            .filter(|g| !g.is_empty())
            .collect(),
        start: if shaped.start.is_finite() { shaped.start as f32 } else { 0.0 },
        width: if shaped.width.is_finite() { shaped.width.max(0.0) as f32 } else { 0.0 },
    });
    if let Some(mut kept) = kept() {
        kept.faces.entry(key.0).or_insert_with(|| (font.clone(), face));
        kept.lines.insert(key, lettered.clone());
    }
    lettered
}

/// The Arabic `text` says in font `font` (its text style's), when that is an SHX font, here, that draws Arabic letters
/// for Latin keys, and the text has letters on them; none for any other font or text, which says what is written.
pub fn shown(font: Option<&Arc<Font>>, text: &str) -> Option<Arc<str>> {
    let name = font?.name.as_str();
    if !text.chars().any(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    let mut kept = kept()?;
    let keyed = match kept.keyed.get(name) {
        Some(k) => *k,
        None => {
            let k = cadcraft_fonts::keyboard::shown(name, "a").is_some();
            kept.keyed.insert(name.to_string(), k);
            k
        }
    };
    if !keyed {
        return None;
    }
    let key = (name.to_string(), text.to_string());
    if let Some(s) = kept.texts.get(&key) {
        return s.clone();
    }
    let s = cadcraft_fonts::keyboard::shown(name, text).map(Arc::from);
    kept.texts.insert(key, s.clone());
    s
}

/// `text` as it reads in font `font`: what it [`shown`]s, else itself.
pub fn read<'a>(font: Option<&Arc<Font>>, text: &'a str) -> std::borrow::Cow<'a, str> {
    match shown(font, text) {
        Some(s) => std::borrow::Cow::Owned(s.to_string()),
        None => std::borrow::Cow::Borrowed(text),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn font(name: &str) -> Arc<Font> {
        Arc::new(Font { name: name.into(), ..Font::default() })
    }

    #[test]
    fn a_font_not_here_reads_nothing_and_is_stood_in_for_by_the_stroke_font() {
        let f = font("no-such-keyboard-font.shx");
        assert_eq!(shown(Some(&f), "Hgl"), None);
        assert_eq!(shown(None, "Hgl"), None);
        assert!(!has("no-such-keyboard-font.shx") && has(""));
        assert!(add("notes.txt", b"hello").is_err());
        let l = lettered(&f, "Room 12", 1.0);
        assert!(!l.strokes.is_empty() && l.fills.is_empty() && l.start == 0.0 && l.width > 3.0, "{l:?}");
        assert!(Arc::ptr_eq(&l, &lettered(&f, "Room 12", 1.0)), "kept");
        // Half as wide, half the width; nothing of nothing.
        let narrow = lettered(&f, "Room 12", 0.5);
        assert!((narrow.width - l.width / 2.0).abs() < 0.05, "{} {}", narrow.width, l.width);
        assert!(lettered(&f, "", 1.0).strokes.is_empty());
    }

    /// A drawing whose text style `ARAB` names `font`, with one text in it, one in a style the file has not, and a
    /// multi-line text in it.
    fn drawing(font: &str) -> Dxf {
        let text = format!(
            "0\nSECTION\n2\nHEADER\n9\n$DWGCODEPAGE\n3\nANSI_1256\n0\nENDSEC\n\
             0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nSTYLE\n0\nSTYLE\n2\nArab\n3\n{font}\n4\nbig.shx\n50\n15\n0\nENDTAB\n0\nENDSEC\n\
             0\nSECTION\n2\nENTITIES\n\
             0\nTEXT\n8\nN\n7\nARAB\n10\n100\n20\n50\n40\n3\n1\nHu]H] Hgtw,G\n\
             0\nTEXT\n8\nN\n10\n0\n20\n0\n40\n3\n1\nPlain\n\
             0\nMTEXT\n8\nN\n7\narab\n10\n0\n20\n90\n40\n3\n1\nHgl\n0\nENDSEC\n0\nEOF\n"
        );
        super::super::read_text(&text).unwrap()
    }

    fn fonts_of(d: &Dxf) -> Vec<Arc<Font>> {
        let font = |e: &super::super::Entity| match &e.shape {
            Shape::Text { font, .. } => font.clone(),
            _ => None,
        };
        d.entities.iter().filter_map(font).collect()
    }

    #[test]
    fn a_text_keeps_its_styles_font_and_a_drawing_says_which_it_lacks() {
        let d = drawing("no-such-keyboard-font-2.shx");
        let fonts = fonts_of(&d);
        assert_eq!(fonts.len(), 3, "every text of a drawing has a font: its style's, or the stroke font");
        let styled = Font {
            name: "no-such-keyboard-font-2.shx".into(),
            big: "big.shx".into(),
            oblique: 15_f64.to_radians(),
            vertical: false,
            code_page: "ANSI_1256".into(),
        };
        assert_eq!(*fonts[0], styled, "by the style's name, whatever its case");
        assert!(Arc::ptr_eq(&fonts[0], &fonts[2]), "one font for a style's texts");
        assert_eq!(*fonts[1], Font { code_page: "ANSI_1256".into(), ..Font::default() });
        assert_eq!(missing([&d]), ["no-such-keyboard-font-2.shx", "big.shx"]);
        // As the file holds it.
        assert!(matches!(&d.entities[0].shape, Shape::Text { text, .. } if text == "Hu]H] Hgtw,G"));
    }

    /// With the real font, which is not ours to carry: `BIMCRAFT_XARAB` names a copy of `xarab.shx` (Arqiflow's package of
    /// stand-ins has one). Without it, nothing is checked.
    #[test]
    fn text_in_an_arabic_font_laid_out_on_the_keyboard_is_written_leftward_and_reads_in_arabic_once_the_font_is_given() {
        let Some(bytes) = std::env::var_os("BIMCRAFT_XARAB").and_then(|p| std::fs::read(p).ok()) else { return };
        let f = font("XArab-Given.SHX");
        let text = "Hu]H] Hgtw,G";
        assert_eq!(shown(Some(&f), text), None, "not before the font is here");
        let before = lettered(&f, text, 1.0);
        assert!(before.start == 0.0, "the stroke font stands in, writing rightward");
        assert_eq!(add(r"C:\Fonts\xarab-given.shx", &bytes), Ok("shx"));
        assert!(has("xarab-given"));
        // Lettered again, by the font: from its point leftward.
        let l = lettered(&f, text, 1.0);
        assert!(*l != *before && !l.strokes.is_empty(), "the font's own shapes");
        assert!(l.start < -1.0 && l.width > 1.0 && (l.start + l.width).abs() < 1.0, "{} {}", l.start, l.width);
        let left = l.strokes.iter().flatten().map(|p| p.0).fold(f32::MAX, f32::min);
        assert!(left < -1.0, "its ink lies left of its point: {left}");
        // And what it says, for who reads words.
        assert_eq!(shown(Some(&f), text).as_deref(), Some("اعداد الفصول"));
        assert_eq!(&*read(Some(&f), text), "اعداد الفصول");
        assert_eq!(shown(Some(&f), "120"), None, "digits read as they are");
        assert_eq!(&*read(Some(&font("another.shx")), "Plain"), "Plain");
    }
}
