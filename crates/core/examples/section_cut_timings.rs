//! How long a section takes to cut from the element solids (`docs/manifold-sections.md`), cold and warm, against what the cut cost
//! before them: a section through the middle of the 20-storey highrise (9,521 elements), the reference tower's plate and the sample
//! house. `cargo run -p bimcraft-core --release --example section_cut_timings`.
use std::time::Instant;

use bimcraft_core::engine::Session;
use bimcraft_core::geom::P2;
use bimcraft_core::model::{Element, Id, Model};
use bimcraft_core::{render, view3d};
use serde_json::json;

/// Every element a line is cut by, as it was before the solids: what `Solid::cuts` made of every piece the scene holds.
fn cut_before(m: &Model, start: P2, end: P2) -> usize {
    let u = (end - start).unit();
    let mut owners: std::collections::BTreeSet<Id> = Default::default();
    for piece in view3d::solids(m) {
        if !piece.cuts(start - u * 1e5, u, 2e5 + start.dist(end)).is_empty() {
            owners.insert(piece.owner());
        }
    }
    owners.len()
}

fn main() -> Result<(), String> {
    for (name, model) in [
        ("highrise, 20 storeys", bimcraft_core::sample::highrise(20)?),
        ("tower, reference plate, one storey", bimcraft_core::sample::tower::tower(1)?),
        ("sample house", {
            let mut s = Session::new();
            s.script(bimcraft_core::sample::SAMPLE)?;
            (*s.model).clone()
        }),
    ] {
        let mut s = Session::new();
        s.reset(model, None);
        let (lo, hi) = s
            .model
            .elements
            .values()
            .filter_map(|e| if let Element::Wall(w) = e { Some((w.start, w.end)) } else { None })
            .fold((P2(f64::MAX, f64::MAX), P2(f64::MIN, f64::MIN)), |(lo, hi), (a, b)| {
                (P2(lo.0.min(a.0).min(b.0), lo.1.min(a.1).min(b.1)), P2(hi.0.max(a.0).max(b.0), hi.1.max(a.1).max(b.1)))
            });
        let mid = (lo + hi) * 0.5;
        let line = [[lo.0 - 1000.0, mid.1], [hi.0 + 1000.0, mid.1]];
        let sec = s.execute("section", &json!({"start": line[0], "end": line[1]}))?["id"].as_u64().unwrap_or_default();
        let m = s.model.clone();
        let started = Instant::now();
        let work = render::SectionWork::new(&m);
        let prepared = started.elapsed();
        let started = Instant::now();
        let rings = render::section_cut_rings_with(&m, &work, sec).ok_or("no section")?;
        let cold = started.elapsed();
        let started = Instant::now();
        let again = render::section_cut_rings_with(&m, &work, sec).ok_or("no section")?;
        let warm = started.elapsed();
        assert_eq!(rings.len(), again.len());
        let elements: std::collections::BTreeSet<Id> = rings.iter().map(|(id, ..)| *id).collect();
        let view = render::section_fit_with(&m, &work, sec, 1600, 900);
        let started = Instant::now();
        let _ = render::SectionOverlay::new(&m, &work, sec, view);
        let overlay_warm = started.elapsed();
        let started = Instant::now();
        let before = cut_before(&m, P2(line[0][0], line[0][1]), P2(line[1][0], line[1][1]));
        let old = started.elapsed();
        let work2 = render::SectionWork::new(&m);
        let started = Instant::now();
        let _ = render::SectionOverlay::new(&m, &work2, sec, view);
        let overlay_cold = started.elapsed();
        println!(
            "{name}: {} elements, work prepared in {prepared:.2?}; the cut: cold {cold:.2?}, warm {warm:.2?} ({} elements cut, {} rings; before the solids {before} elements in {old:.2?}); the whole overlay: cold {overlay_cold:.2?}, with the solids kept {overlay_warm:.2?}",
            m.elements.len(),
            elements.len(),
            rings.len(),
        );
    }
    Ok(())
}
