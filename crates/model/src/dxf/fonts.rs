//! A drawing's fonts. The canvas letters a drawing's text itself ([`crate::font`]), not in the drawing's fonts, so a font
//! matters here for one thing: old Arabic SHX fonts (`xarab.shx`, `xarb.shx`) draw an Arabic letter for each Latin key, as
//! the Arabic keyboard places them, and write from the insertion point leftward. A drawing in one stores "Hu]H] Hgtw,G"
//! and shows «اعداد الفصول». [`shown`] is the Arabic such a text shows, read by CADCraft from the font's own glyphs
//! (`cadcraft_fonts::keyboard`: a font is known by its glyph data, not its name), as Arqiflow's drawing viewer and its
//! agent's drawing reader read it.
//!
//! So the font's file must be here: a host gives it at run time ([`add`]; a browser has no fonts folder), having asked
//! which a model's drawings name and lack ([`missing`]). A text is kept as the file holds it, with its style's font
//! (`Shape::Text::font`), and read when it is drawn, so a font given after its drawing was read still reads it.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use super::{Dxf, Shape};

/// How many fonts have been given: what is kept of what texts show is of that many.
static GIVEN: AtomicU64 = AtomicU64::new(0);

/// The most fonts a model's drawings are asked about.
const MAX_ASKED: usize = 1000;

/// A font file a host gives (the firm's, the computer's, a stand-in), by its file name: "shx" or "outline", or why it
/// was refused. Text drawn after it reads by it.
pub fn add(name: &str, bytes: &[u8]) -> Result<&'static str, String> {
    let added = cadcraft_fonts::given::add_font(name, bytes)?;
    GIVEN.fetch_add(1, Ordering::Relaxed);
    Ok(match added {
        cadcraft_fonts::given::Added::Shx => "shx",
        cadcraft_fonts::given::Added::Outline => "outline",
    })
}

/// Whether font `name`, as a text style names it, is one a text could be read by and its file is not here: an SHX font
/// (named so, or with no extension, which AutoCAD takes for one) that has not been given.
fn lacking(name: &str) -> bool {
    let file = name.rsplit(['/', '\\']).next().unwrap_or_default().trim().to_ascii_lowercase();
    let shx = match file.rsplit_once('.') {
        Some((_, ext)) => ext == "shx",
        None => !file.is_empty() && !file.contains(' '),
    };
    shx && cadcraft_fonts::shx::find(name.trim()).is_none()
}

/// Whether `name` has been given, or is no font a text is read by: a host asks between its sources of fonts.
pub fn has(name: &str) -> bool {
    !lacking(name)
}

/// The fonts drawings' texts name that could read them and are not here, each once, as the styles spell them: what a
/// host may find and give with [`add`].
pub fn missing<'a>(drawings: impl IntoIterator<Item = &'a Dxf>) -> Vec<String> {
    let mut seen: Vec<Arc<str>> = Vec::new();
    for d in drawings {
        let shapes = d.entities.iter().map(|e| &e.shape).chain(d.blocks.iter().flat_map(|b| b.entities.iter().map(|e| &e.shape)));
        for shape in shapes {
            if let Shape::Text { font: Some(f), .. } = shape
                && !seen.iter().any(|s| Arc::ptr_eq(s, f) || s.eq_ignore_ascii_case(f))
                && seen.len() < MAX_ASKED
            {
                seen.push(f.clone());
            }
        }
    }
    seen.iter().filter(|f| lacking(f)).map(|f| f.to_string()).collect()
}

/// What is kept of what texts show, of [`GIVEN`] fonts: whether each font is one laid out on the keyboard, and the texts
/// read in those.
#[derive(Default)]
struct Kept {
    given: u64,
    keyed: HashMap<Arc<str>, bool>,
    texts: HashMap<(Arc<str>, String), Option<Arc<str>>>,
}

/// The Arabic `text` shows in font `font` (its text style's), when that is an SHX font, given, that draws Arabic letters
/// for Latin keys, and the text has letters on them; none for any other font or text, which reads as it is written. Such
/// a text is written from its point leftward.
pub fn shown(font: Option<&Arc<str>>, text: &str) -> Option<Arc<str>> {
    let font = font?;
    if !text.chars().any(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    static KEPT: OnceLock<Mutex<Kept>> = OnceLock::new();
    let mut kept = KEPT.get_or_init(Default::default).lock().ok()?;
    let given = GIVEN.load(Ordering::Relaxed);
    if kept.given != given || kept.texts.len() > 50_000 || kept.keyed.len() > 10_000 {
        *kept = Kept { given, ..Kept::default() };
    }
    let keyed = match kept.keyed.get(font) {
        Some(k) => *k,
        None => {
            let k = cadcraft_fonts::keyboard::shown(font, "a").is_some();
            kept.keyed.insert(font.clone(), k);
            k
        }
    };
    if !keyed {
        return None;
    }
    let key = (font.clone(), text.to_string());
    if let Some(s) = kept.texts.get(&key) {
        return s.clone();
    }
    let s = cadcraft_fonts::keyboard::shown(font, text).map(Arc::from);
    kept.texts.insert(key, s.clone());
    s
}

/// `text` as it reads in font `font`: what it [`shown`]s, else itself.
pub fn read<'a>(font: Option<&Arc<str>>, text: &'a str) -> std::borrow::Cow<'a, str> {
    match shown(font, text) {
        Some(s) => std::borrow::Cow::Owned(s.to_string()),
        None => std::borrow::Cow::Borrowed(text),
    }
}

/// Where a text of font `font` aligned `h` lies along its line from its point: a text [`shown`] in a font that writes
/// leftward ends at its point when it is left-aligned, as it does right-aligned.
pub fn aligned(font: Option<&Arc<str>>, text: &str, h: super::Horizontal) -> super::Horizontal {
    match h {
        super::Horizontal::Left if shown(font, text).is_some() => super::Horizontal::Right,
        h => h,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_font_not_given_reads_nothing_and_only_shx_fonts_are_asked_for() {
        let font: Arc<str> = Arc::from("no-such-keyboard-font.shx");
        assert_eq!(shown(Some(&font), "Hgl"), None);
        assert_eq!(shown(None, "Hgl"), None);
        assert!(lacking("no-such-keyboard-font.shx") && lacking("nosuchfont") && lacking(r"C:\Fonts\NoSuch.SHX"));
        assert!(!lacking("arial.ttf") && !lacking("Simplified Arabic") && !lacking(""));
        assert!(has("arial.ttf") && !has("nosuchfont"));
        assert!(add("notes.txt", b"hello").is_err());
    }

    /// A drawing whose text style `ARAB` names `font`, with one text in it and one in a style of no font.
    fn drawing(font: &str) -> Dxf {
        let text = format!(
            "0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nSTYLE\n0\nSTYLE\n2\nArab\n3\n{font}\n0\nENDTAB\n0\nENDSEC\n\
             0\nSECTION\n2\nENTITIES\n\
             0\nTEXT\n8\nN\n7\nARAB\n10\n100\n20\n50\n40\n3\n1\nHu]H] Hgtw,G\n\
             0\nTEXT\n8\nN\n10\n0\n20\n0\n40\n3\n1\nPlain\n\
             0\nMTEXT\n8\nN\n7\narab\n10\n0\n20\n90\n40\n3\n1\nHgl\n0\nENDSEC\n0\nEOF\n"
        );
        super::super::read_text(&text).unwrap()
    }

    fn fonts_of(d: &Dxf) -> Vec<Option<String>> {
        let font = |e: &super::super::Entity| match &e.shape {
            Shape::Text { font, .. } => Some(font.as_ref().map(|f| f.to_string())),
            _ => None,
        };
        d.entities.iter().filter_map(font).collect()
    }

    #[test]
    fn a_text_keeps_its_styles_font_and_a_drawing_says_which_it_lacks() {
        let d = drawing("no-such-keyboard-font-2.shx");
        let font = Some("no-such-keyboard-font-2.shx".to_string());
        assert_eq!(fonts_of(&d), [font.clone(), None, font], "by the style's name, whatever its case; none for a style with no font");
        assert_eq!(missing([&d]), ["no-such-keyboard-font-2.shx"]);
        assert!(missing([&drawing("Arial.ttf")]).is_empty(), "no text is read by a TrueType font");
        // As the file holds it, until the font is given.
        assert!(matches!(&d.entities[0].shape, Shape::Text { text, .. } if text == "Hu]H] Hgtw,G"));
        assert_eq!(
            aligned(Some(&Arc::from("no-such-keyboard-font-2.shx")), "Hgl", super::super::Horizontal::Left),
            super::super::Horizontal::Left
        );
    }

    /// With the real font, which is not ours to carry: `BIMCRAFT_XARAB` names a copy of `xarab.shx` (Arqiflow's package of
    /// stand-ins has one). Without it, nothing is checked.
    #[test]
    fn text_in_an_arabic_font_laid_out_on_the_keyboard_reads_in_arabic_once_the_font_is_given() {
        let Some(bytes) = std::env::var_os("BIMCRAFT_XARAB").and_then(|p| std::fs::read(p).ok()) else { return };
        let d = drawing("XArab-Given.SHX");
        let Shape::Text { text, font, .. } = &d.entities[0].shape else { panic!() };
        assert_eq!(shown(font.as_ref(), text), None, "not before the font is here");
        assert_eq!(missing([&d]), ["XArab-Given.SHX"]);
        assert_eq!(add(r"C:\Fonts\xarab-given.shx", &bytes), Ok("shx"));
        assert!(missing([&d]).is_empty() && has("xarab-given"));
        assert_eq!(shown(font.as_ref(), text).as_deref(), Some("اعداد الفصول"));
        assert_eq!(&*read(font.as_ref(), text), "اعداد الفصول");
        assert_eq!(shown(font.as_ref(), "120"), None, "digits read as they are");
        // Written from its point leftward: left-aligned, it ends there.
        assert_eq!(aligned(font.as_ref(), text, super::super::Horizontal::Left), super::super::Horizontal::Right);
        assert_eq!(aligned(font.as_ref(), text, super::super::Horizontal::Center), super::super::Horizontal::Center);
        // A text in another style is not read by it.
        let Shape::Text { text, font, .. } = &d.entities[1].shape else { panic!() };
        assert_eq!(&*read(font.as_ref(), text), "Plain");
    }
}
