//! `pat`'s tests that drive the Session (moved out of `pat.rs`).

use crate::fills::{HatchLine, Target};
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
