//! `pat`'s tests that drive the Session (moved out of `pat.rs`).

use crate::fills::{HatchLine, MAX_FAMILIES, Target};
use crate::geom::P2;

use crate::pat::*;

const BRICK: &str = ";%UNITS=MM
;%VERSION=3.0
*Brick 230x75, running bond
;%TYPE=MODEL
0, 0,0, 0,75
90, 0,0, 75,115, 75,-75
;a comment
*Diagonal, 45 degrees
45, 0,0, 0,3
";

#[test]
fn a_pat_file_reads_as_patterns_and_writes_back_the_same() {
    let r = read(BRICK).unwrap_or_default();
    assert!(r.left_out.is_empty(), "{:?}", r.left_out);
    let names: Vec<&str> = r.patterns.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["Brick 230x75", "Diagonal"]);
    let brick = &r.patterns[0].1;
    assert_eq!(brick.target, Target::Model);
    assert_eq!(brick.lines[1], HatchLine { angle: 90.0, origin: P2(0.0, 0.0), shift: 75.0, spacing: 115.0, dashes: vec![75.0, -75.0] });
    assert_eq!(r.patterns[1].1.target, Target::Drafting, "drafting when it doesn't say");
    let again = read(&write(r.patterns.iter().map(|(n, p)| (n, p)))).unwrap_or_default();
    assert_eq!(again, r);
}

#[test]
fn inches_are_millimetres_a_negative_offset_steps_the_other_way_and_what_cant_be_is_left_out() {
    let text = ";%UNITS=INCH
*Lines
0, 0,0, 1,-2, 0.5,-0.25
*Flat
0, 0,0, 0,0
*Words
0, zero, 0, 0, 1
*Empty
*Lines
0, 0,0, 0,1
";
    let r = read(text).unwrap_or_default();
    assert_eq!(r.patterns.len(), 1);
    let l = &r.patterns[0].1.lines[0];
    assert_eq!((l.shift, l.spacing), (-25.4, 50.8));
    assert_eq!(l.dashes, [12.7, -6.35]);
    let out: Vec<&str> = r.left_out.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(out, ["Flat", "Words", "Empty", "Lines"]);
    assert!(r.left_out[3].1.contains("twice"));
    assert!(read(&"x".repeat(MAX_TEXT + 1)).is_err());
}

/// Why each of the patterns in `text` was left out, by name, in the order read.
fn why(text: &str) -> Vec<(String, String)> {
    read(text).unwrap_or_default().left_out
}

#[test]
fn a_file_of_empty_patterns_is_counted_and_only_the_first_few_are_named() {
    // 300,000 of them: every one a `left_out` entry, and the reply a megabyte of names, before it was capped.
    let r = read(&"*a\n".repeat(300_000)).unwrap_or_default();
    assert_eq!((r.patterns.len(), r.left_out_count), (0, 300_000));
    assert!(r.left_out.len() <= MAX_LEFT_OUT + 1, "{} named", r.left_out.len());
    assert!(r.left_out[0].1.contains("no lines"), "{:?}", r.left_out[0]);
}

#[test]
fn reading_stops_after_the_most_patterns_and_counts_the_rest() {
    let text: String = (0..MAX_PATTERNS + 1000).map(|i| format!("*p{i}\n0,0,0,0,10\n")).collect();
    let r = read(&text).unwrap_or_default();
    assert_eq!((r.patterns.len(), r.left_out_count), (MAX_PATTERNS, 1000));
    let last = r.left_out.last().cloned().unwrap_or_default();
    assert!(last.0.contains("1000") && last.1.contains(&MAX_PATTERNS.to_string()), "{last:?}");
    // One pattern bad among the first ones is left out and counted, and the rest still come in.
    let r = read("*bad\n0,0,0,0,0\n*good\n0,0,0,0,10\n").unwrap_or_default();
    assert_eq!((r.patterns.len(), r.left_out_count), (1, 1));
}

#[test]
fn a_line_that_is_not_numbers_is_echoed_short() {
    let r = why(&format!("*a\n{}\n", "x".repeat(10_000)));
    assert!(r[0].1.len() < 200, "{} letters of reason", r[0].1.len());
    // Letters, not bytes: a cut can't fall inside one.
    let r = why(&format!("*a\n{}\n", "é".repeat(10_000)));
    assert!(r[0].1.chars().count() < 200);
}

#[test]
fn numbers_that_are_not_numbers_and_more_than_a_pattern_takes_are_left_out() {
    for bad in ["NaN", "inf", "-inf", "1e999", "0x10", ""] {
        let r = read(&format!("*a\n0,0,0,0,10,{bad}\n*b\n{bad},0,0,0,10\n")).unwrap_or_default();
        assert_eq!((r.patterns.len(), r.left_out_count), (0, 2), "`{bad}`: {:?}", r.left_out);
    }
    let seventeen = format!("*a\n0,0,0,0,10{}\n", ",1,-1".repeat(9));
    assert!(why(&seventeen)[0].1.contains("dashes"), "{:?}", why(&seventeen));
    let sixteen = format!("*a\n0,0,0,0,10{}\n", ",1,-1".repeat(8));
    assert!(why(&sixteen).is_empty());
    let families: String = (0..MAX_FAMILIES + 1).map(|i| format!("{i},0,0,0,10\n")).collect();
    assert!(why(&format!("*a\n{families}"))[0].1.contains("families"));
    assert!(why(&format!("*a\n{}", families.lines().take(MAX_FAMILIES).collect::<Vec<_>>().join("\n"))).is_empty());
}

#[test]
fn a_line_of_a_million_numbers_and_a_pattern_of_a_hundred_thousand_dashes_are_left_out() {
    let long = format!("*a\n0,0,0,0,10{}\n*b\n0,0,0,0,10\n", ",1".repeat(1_000_000));
    let r = read(&long).unwrap_or_default();
    assert_eq!((r.patterns.len(), r.left_out_count), (1, 1));
    assert!(r.left_out[0].1.contains("dashes"), "{:?}", r.left_out[0]);
    let spaces = format!("*a\n0,0,0,0,10,5{}\n", ",-5".repeat(100_000));
    assert!(why(&spaces)[0].1.contains("dashes"));
}

#[test]
fn a_byte_order_mark_comments_and_dots_are_read_and_zero_lengths_and_spaces_alone_are_not() {
    let r = read("\u{feff};%UNITS=MM\n\u{feff}*Bom, with a mark\n0, 0,0, 0,10 ; trailing\n").unwrap_or_default();
    assert_eq!(r.patterns.len(), 1, "{:?}", r.left_out);
    assert_eq!(r.patterns[0].0, "Bom");
    // A dot (a 0) between spaces is a dash; dashes that are all 0, or all spaces, draw nothing, and a 0.05 mm one
    // sums to what a family's lines are apart: none of them is a pattern.
    assert!(why("*dots\n0,0,0,0,10,0,-5\n").is_empty());
    for dashes in ["0", "0,0,0", "-1", "-5,-5", "-0.05", "-1e-9"] {
        let r = read(&format!("*a\n0,0,0,0,10,{dashes}\n")).unwrap_or_default();
        assert_eq!((r.patterns.len(), r.left_out_count), (0, 1), "dashes {dashes}");
    }
}

#[test]
fn a_file_over_the_most_is_refused_unread_and_old_ones_read_as_latin_1() {
    let dir = std::env::temp_dir().join(format!("bimcraft-pat-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let big = dir.join("big.pat");
    std::fs::File::create(&big).and_then(|f| f.set_len(MAX_TEXT as u64 + 1)).unwrap_or_default();
    let e = read_file(&big).unwrap_err();
    assert!(e.contains("too large"), "{e}");
    let old = dir.join("old.pat");
    std::fs::write(&old, b"*Caf\xe9\n0,0,0,0,10\n").unwrap_or_default();
    let r = read_file(&old).unwrap_or_default();
    assert_eq!(r.patterns.first().map(|(n, _)| n.as_str()), Some("Café"));
    assert!(read_file(&dir.join("missing.pat")).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_import_command_replies_with_the_first_few_left_out_and_the_count_of_all() {
    let mut s = crate::engine::Session::new();
    let text = format!("*Brick\n0,0,0,0,10\n{}", "*a\n".repeat(50_000));
    let r = s.execute("import_fill_patterns", &serde_json::json!({"text": text})).unwrap();
    assert_eq!(r["imported"], serde_json::json!(["Brick"]));
    assert_eq!(r["left_out_count"], 50_000);
    assert!(r["left_out"].as_array().map_or(usize::MAX, Vec::len) <= MAX_LEFT_OUT, "{}", r["left_out"].as_array().map_or(0, Vec::len));
    assert!(r.to_string().len() < 50_000, "{} bytes of reply", r.to_string().len());
    // The same names again, kept as the project has them: counted as left out, and listed up to the same most.
    let again: String = (0..300).map(|i| format!("*Brick\n0,0,0,0,{}\n*Tile{i}\n0,0,0,0,10\n", 10 + i)).collect();
    let r = s.execute("import_fill_patterns", &serde_json::json!({"text": again})).unwrap();
    assert_eq!(r["imported"].as_array().map_or(0, Vec::len), 300);
    assert_eq!(r["left_out_count"], 300);
    assert!(r["left_out"].as_array().map_or(usize::MAX, Vec::len) <= MAX_LEFT_OUT);
}

#[test]
fn a_name_longer_than_the_most_is_cut_and_a_comma_or_control_letter_never_reaches_a_file() {
    let r = read(&format!("*{}\n0,0,0,0,10\n", "n".repeat(10_000))).unwrap_or_default();
    assert_eq!(r.patterns[0].0.chars().count(), crate::model::MAX_TEXT);
    let written = write([(&"A,B\tC\u{7}".to_string(), &r.patterns[0].1)]);
    assert!(written.contains("*A B C \n"), "{written:?}");
}
