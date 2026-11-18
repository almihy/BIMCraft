//! The lettering of the canvas's labels (a room's name, a level's, a tag, a drawing's text): our single-stroke lettering
//! ([`bimcraft_geom::font`]) where it has every letter of a text, which is nearly always; else the text is lettered as a
//! text note in the lettering is ([`crate::text::label`]): what the lettering has no letter for in the fonts behind it,
//! and right-to-left text (Arabic, Hebrew) in the order it reads, its letters joined.

use std::sync::Arc;

pub use bimcraft_geom::font::has;

use crate::text::Label;

/// Whether the lettering alone letters `text` as it should be read.
fn plain(text: &str) -> bool {
    text.chars().all(|c| c.is_ascii() || c.is_whitespace() || has(c))
}

/// `text` as the fonts behind the lettering letter it, when the lettering alone does not ([`Label`]): what
/// [`strokes`] draws of it, and its letters' fills, which lines cannot give.
pub fn label(text: &str) -> Option<Arc<Label>> {
    (!plain(text)).then(|| crate::text::label(text))
}

/// How wide `text` is at `height` px.
pub fn width(text: &str, height: f32) -> f32 {
    match label(text) {
        Some(l) => l.width * height,
        None => bimcraft_geom::font::width(text, height),
    }
}

/// Polylines drawing `text` with capitals `height` px tall, from `(x, y)` on the baseline, y down. A letter the lettering
/// has not is its outline: who can fill draws [`label`]'s.
pub fn strokes(text: &str, x: f32, y: f32, height: f32) -> Vec<Vec<(f32, f32)>> {
    let Some(l) = label(text) else { return bimcraft_geom::font::strokes(text, x, y, height) };
    let at = |&(a, b): &(f32, f32)| (x + a * height, y + b * height);
    let lines = l.strokes.iter().map(|(line, _)| line.iter().map(at).collect());
    let outlines = l.fill.iter().flatten().map(|ring| ring.iter().chain(ring.first()).map(at).collect());
    lines.chain(outlines).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_the_lettering_has_is_lettered_as_ever_and_arabic_is_as_wide_as_it_is_drawn() {
        assert!(label("Living 12").is_none() && label("ÉTAGE").is_none());
        assert_eq!(width("Living", 10.0), bimcraft_geom::font::width("Living", 10.0));
        assert_eq!(strokes("Living", 1.0, 2.0, 10.0), bimcraft_geom::font::strokes("Living", 1.0, 2.0, 10.0));
        let l = label("غرفة").expect("the lettering has no Arabic");
        assert!(!l.fill.is_empty() && l.width > 0.5);
        let drawn = strokes("غرفة", 0.0, 0.0, 10.0);
        let right = drawn.iter().flatten().map(|p| p.0).fold(f32::MIN, f32::max);
        assert!(right <= width("غرفة", 10.0) + 1.0 && right > width("غرفة", 10.0) * 0.7, "{right}");
    }
}
