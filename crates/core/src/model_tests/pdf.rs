//! `pdf`'s tests that drive the Session (moved out of `pdf.rs`).

use crate::geom::P2;

use crate::engine::Session;
use crate::pdf::*;

#[test]
fn the_cross_reference_table_points_at_every_object() {
    let mut s = Session::new();
    s.script(r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true}"#).unwrap();
    let d = crate::render::plan(&s.model, 100000, 400, 300);
    let bytes = pdf(&[(&d, P2(80.0, 60.0)), (&d, P2(80.0, 60.0))]);
    let tail = String::from_utf8_lossy(&bytes[bytes.len() - 40..]).to_string();
    let start: usize = tail.split("startxref\n").nth(1).unwrap().lines().next().unwrap().parse().unwrap();
    let table = String::from_utf8_lossy(&bytes[start..]).to_string();
    let mut lines = table.lines().skip(1);
    let count: usize = lines.next().unwrap().split(' ').nth(1).unwrap().parse().unwrap();
    assert_eq!(count, 1 + 2 + 2 * 2, "the free entry, catalog and pages, and a page and its contents twice");
    for (n, entry) in lines.skip(1).take(count - 1).enumerate() {
        let offset: usize = entry[..10].parse().unwrap();
        let head = format!("{} 0 obj", n + 1);
        assert!(bytes[offset..].starts_with(head.as_bytes()), "object {} isn't at {offset}", n + 1);
    }
    assert!(bytes.ends_with(b"%%EOF\n"));
}
