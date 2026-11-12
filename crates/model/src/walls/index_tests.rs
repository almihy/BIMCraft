//! `walls::index`'s tests that need only the model.

use crate::model::Id;
use crate::walls::index::*;

#[test]
fn a_tiles_search_by_height_leaves_out_no_wall_that_overlaps() {
    let spans = [(0.0, 4000.0), (0.0, 400_000.0), (4000.0, 8000.0), (-3000.0, -1.0), (8000.0, 8000.0), (396_000.0, 400_000.0)];
    let mut tile = Tile::default();
    for (id, (lo, hi)) in spans.iter().enumerate() {
        tile.items.push((*lo, id as Id));
        tile.tallest = tile.tallest.max(hi - lo);
    }
    tile.items.sort_unstable_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    let edges = [-1e12, -3000.0, -1.0, 0.0, 1.0, 3999.0, 4000.0, 8000.0, 396_000.0, 400_000.0, 1e12];
    let queries = edges.iter().flat_map(|lo| edges.iter().map(move |hi| (*lo, *hi)));
    for (lo, hi) in queries.chain([(f64::NEG_INFINITY, f64::INFINITY), (0.0, f64::INFINITY), (f64::NAN, 1.0), (0.0, f64::NAN)]) {
        let found: Vec<Id> = tile.may_overlap(lo, hi).collect();
        for (id, (bottom, top)) in spans.iter().enumerate() {
            if lo < *top && *bottom < hi {
                assert!(found.contains(&(id as Id)), "wall {id} overlaps {lo} to {hi}: {found:?}");
            }
        }
    }
    // With no tall wall, only the storeys near: what makes a tower's tile cheap.
    let storeys = Tile { items: (0..100).map(|i| (f64::from(i) * 4000.0, Id::from(i as u32))).collect(), tallest: 4000.0 };
    assert!(storeys.may_overlap(200_000.0, 204_000.0).count() <= 4);
}
