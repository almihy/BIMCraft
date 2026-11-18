//! `alignment`'s tests that drive the Session (moved out of `alignment.rs`).

use crate::geom::P2;

use crate::alignment::*;
use crate::engine::Session;

/// An alignment says which wall it lines up with, so the window can say why a point snapped.
#[test]
fn an_alignment_names_the_wall_it_lines_up_with() {
    let mut s = Session::new();
    s.script(r#"wall {"points": [[0, 0], [5000, 0]]}"#).unwrap();
    let wall = s.model.elements.iter().find(|(_, e)| e.category() == "wall").map(|(id, _)| *id).unwrap();
    let level = s.model.levels()[0].0;
    let config = AlignmentConfig {
        enabled: true,
        tolerance: 50.0,
        endpoints: true,
        midpoints: true,
        faces: true,
        intersections: true,
        angles: false,
    };
    let service = AlignmentGuideService::from_model(&s.model, level, config);
    let found = service.find_alignments(P2(5005.0, 3000.0), Some(P2(0.0, 3000.0)), 50.0);
    assert!(found.first().is_some_and(|a| a.wall_id == Some(wall)), "{found:?}");
}
