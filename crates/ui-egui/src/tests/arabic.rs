//! Arabic in an English window: a room's name from an Arabic file, and a check that quotes it. The window has Noto Sans
//! Arabic behind Plex, and egui's layout (vendor/epaint, "BIMCraft's patch") shows right-to-left text in the order it reads
//! while the English around it stays left to right. Before, the names were question marks and a line that began with one
//! was drawn backwards, English and all.

use super::*;

const ROOM: &str = "غرفة نوم";

/// Each glyph of `text` laid out on one row at the window's body size, with where it is drawn, in the text's order.
fn laid_out(h: &mut Harness<'_, BimApp>, text: &str, wrap: f32) -> Vec<Vec<(char, f32, f32)>> {
    let galley = h.ctx.fonts_mut(|f| {
        let mut job = egui::text::LayoutJob::simple(text.to_owned(), egui::FontId::proportional(13.0), egui::Color32::BLACK, wrap);
        job.wrap.max_width = wrap;
        f.layout_job(job)
    });
    galley.rows.iter().map(|r| r.glyphs.iter().map(|g| (g.chr, g.pos.x + r.pos.x, g.max_x() + r.pos.x)).collect()).collect()
}

/// The glyphs of `word`, found at its place in `text`, of a row laid out from `text`.
fn glyphs_of<'a>(row: &'a [(char, f32, f32)], text: &str, word: &str) -> &'a [(char, f32, f32)] {
    let start = text[..text.find(word).unwrap()].chars().count();
    &row[start..start + word.chars().count()]
}

/// Within a pixel: egui puts glyphs on whole pixels.
fn left(g: &[(char, f32, f32)]) -> f32 {
    g.iter().map(|g| g.1).fold(f32::INFINITY, f32::min)
}

fn right(g: &[(char, f32, f32)]) -> f32 {
    g.iter().map(|g| g.2).fold(f32::NEG_INFINITY, f32::max)
}

fn reads_left_to_right(g: &[(char, f32, f32)]) -> bool {
    g.windows(2).all(|w| w[0].1 <= w[1].1)
}

#[test]
fn arabic_names_are_letters_and_not_question_marks() {
    let mut h = app(BOX);
    h.run();
    for family in crate::style::families() {
        let drawn = h.ctx.fonts_mut(|f| f.has_glyphs(&egui::FontId::new(13.0, family.clone()), ROOM));
        assert!(drawn, "{family:?} draws Arabic");
    }
}

#[test]
fn a_line_that_begins_with_an_arabic_name_reads_in_order() {
    let mut h = app(BOX);
    h.run();
    let text = format!("{ROOM} on Ground Floor reaches the hall on only 0.1 m of wall");
    let rows = laid_out(&mut h, &text, f32::INFINITY);
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(row.len(), text.chars().count(), "a glyph for each letter, in the text's order");
    assert_eq!(row.iter().map(|g| g.0).collect::<String>(), text);
    let (first, second) = (glyphs_of(row, &text, "غرفة"), glyphs_of(row, &text, "نوم"));
    // Right to left: the name's first word on the right, its first letter its rightmost.
    assert!(right(second) <= left(first) + 1.0, "the second word is left of the first");
    assert!(first[0].1 > first[first.len() - 1].1, "a word's first letter is its rightmost");
    // The English after it is not reversed: left to right, after the name.
    let english = glyphs_of(row, &text, "on Ground Floor reaches the hall");
    assert!(left(english) >= right(first) - 1.0, "the English follows the name");
    assert!(reads_left_to_right(english), "the English reads left to right");
}

#[test]
fn an_arabic_name_inside_an_english_line_keeps_the_english_in_order() {
    let mut h = app(BOX);
    h.run();
    let text = format!("On Ground Floor, {ROOM} has no outside wall");
    let row = &laid_out(&mut h, &text, f32::INFINITY)[0];
    let before = glyphs_of(row, &text, "On Ground Floor,");
    let name = glyphs_of(row, &text, ROOM);
    let after = glyphs_of(row, &text, " has no outside wall");
    assert!(reads_left_to_right(before) && reads_left_to_right(after));
    assert!(right(before) <= left(name) + 1.0 && right(name) <= left(after) + 1.0, "English, the name, English");
    let (first, second) = (glyphs_of(row, &text, "غرفة"), glyphs_of(row, &text, "نوم"));
    assert!(right(second) <= left(first) + 1.0, "the name's words right to left");
}

#[test]
fn numbers_in_an_arabic_name_read_left_to_right() {
    let mut h = app(BOX);
    h.run();
    let text = "غرفة 12 on Level 1";
    let row = &laid_out(&mut h, text, f32::INFINITY)[0];
    let number = glyphs_of(row, text, "12");
    let word = glyphs_of(row, text, "غرفة");
    assert!(number[0].1 < number[1].1, "12 is not drawn 21");
    assert!(right(number) <= left(word) + 1.0, "the number is on the word's left, as it reads in Arabic");
}

#[test]
fn a_wrapped_line_with_arabic_keeps_every_letter() {
    let mut h = app(BOX);
    h.run();
    let text = format!("On Ground Floor, a space of about 13.1 by 3.9 m between {ROOM} and {ROOM} 2 is enclosed and is no room");
    let rows = laid_out(&mut h, &text, 120.0);
    assert!(rows.len() > 2, "it wraps");
    let letters: String = rows.iter().flatten().map(|g| g.0).collect();
    assert_eq!(letters, text, "every letter once, in the text's order");
    for row in &rows {
        for g in row {
            assert!(g.1 > -0.5 && g.2 < 120.0 + 20.0, "{g:?} is on its row");
        }
    }
}

#[test]
fn english_alone_is_laid_out_as_before() {
    let mut h = app(BOX);
    h.run();
    let text = "Rooms are big enough: halls and corridors lead to the rooms";
    let row = &laid_out(&mut h, text, f32::INFINITY)[0];
    assert!(reads_left_to_right(row));
}
