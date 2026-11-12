//! Read-only acceleration for one model snapshot. Plan tiles and heights never decide geometry: the existing overlap
//! and join rules still test every candidate, in the same order (by id). A wall's candidates at one of its ends are the
//! walls whose bodies reach that end's tile and overlap it in height, not every wall of the project; a run's, the runs of
//! any category whose bodies reach the node's tile (its body grown by its half width, so a run ending on another's middle
//! is found there too) and whose heights overlap as [`crate::walls::stacked`] says for two runs.
use std::collections::{BTreeMap, HashMap};

use crate::geom::P2;
use crate::model::{Element, Guid, Id, Model, TOL};

const MAX_REFERENCES: usize = 1_000_000;
/// Plan tiles, mm on a side, and the most tiles one wall (or run) goes in (a longer one is a candidate everywhere).
const TILE: f64 = 6000.0;
const MAX_TILES: i64 = 4096;

/// A tile's elements by how low they begin, and the tallest's height: of a tower's, what a wall can reach is its own
/// storeys, not the 100 stacked in the tile.
#[derive(Default)]
pub struct Tile {
    pub items: Vec<(f64, Id)>,
    pub tallest: f64,
}

impl Tile {
    /// Those that may overlap `lo` to `hi`: more, near the ends, as the caller still asks each. Numbers that aren't finite
    /// leave the search wider, never narrower.
    pub fn may_overlap(&self, lo: f64, hi: f64) -> impl Iterator<Item = Id> + '_ {
        // A wall's top is its bottom and its height, rounded: a margin well past what the rounding moves.
        let floor = lo - self.tallest - 1.0 - 1e-6 * lo.abs();
        let start = self.items.partition_point(|(bottom, _)| *bottom < floor);
        let end = self.items.partition_point(|(bottom, _)| bottom.total_cmp(&hi).is_lt());
        self.items.get(start..end.max(start)).unwrap_or_default().iter().map(|(_, id)| *id)
    }
}

pub struct Index {
    spans: BTreeMap<Id, (f64, f64)>,
    hosts: BTreeMap<Guid, Vec<Id>>,
    /// Each tile's walls: those whose body, grown by a tolerance, reaches into it.
    tiles: HashMap<(i64, i64), Tile>,
    /// Walls in no tile: too long for them, or of no finite size.
    everywhere: Vec<Id>,
    /// The runs' own spans, tiles and the runs in no tile, kept apart from the walls' so that a
    /// wall query never meets a run and a run query never meets a wall.
    run_spans: BTreeMap<Id, (f64, f64)>,
    run_tiles: HashMap<(i64, i64), Tile>,
    run_everywhere: Vec<Id>,
}

impl Index {
    pub fn new(m: &Model) -> Self {
        let mut index = Self {
            spans: BTreeMap::new(),
            hosts: BTreeMap::new(),
            tiles: HashMap::new(),
            everywhere: Vec::new(),
            run_spans: BTreeMap::new(),
            run_tiles: HashMap::new(),
            run_everywhere: Vec::new(),
        };
        let mut tiled = 0;
        for (&id, e) in &m.elements {
            match e {
                Element::Wall(w) => {
                    let (lo, hi) = (m.base_z(w), m.top_z(w));
                    index.spans.insert(id, (lo, hi));
                    match reach(m, w).map(|[x0, y0, x1, y1]| ([tile(x0), tile(y0)], [tile(x1), tile(y1)])) {
                        Some(([a, b], [c, d])) if (c - a + 1).saturating_mul(d - b + 1) <= MAX_TILES && tiled < MAX_REFERENCES => {
                            for x in a..=c {
                                for y in b..=d {
                                    let tile = index.tiles.entry((x, y)).or_default();
                                    tile.items.push((lo, id));
                                    tile.tallest = tile.tallest.max(hi - lo);
                                    tiled += 1;
                                }
                            }
                        }
                        _ => index.everywhere.push(id),
                    }
                }
                crate::model::run_arm!(_) => {
                    let Some(g) = m.run_geom(e) else { continue };
                    let (lo, hi) = g.z_span();
                    index.run_spans.insert(id, (lo, hi));
                    // Its body's box, grown by its half width (and twice the tolerance, as a wall's is), so that a run
                    // ending on its middle reaches that tile.
                    let grow = g.width / 2.0 + 2.0 * TOL;
                    let b = [g.a[0].min(g.b[0]) - grow, g.a[1].min(g.b[1]) - grow, g.a[0].max(g.b[0]) + grow, g.a[1].max(g.b[1]) + grow];
                    match b.iter().all(|v| v.is_finite() && v.abs() < 1e12).then(|| {
                        let [x0, y0, x1, y1] = [tile(b[0]), tile(b[1]), tile(b[2]), tile(b[3])];
                        ([x0, y0], [x1, y1])
                    }) {
                        Some(([a, b], [c, d])) if (c - a + 1).saturating_mul(d - b + 1) <= MAX_TILES && tiled < MAX_REFERENCES => {
                            for x in a..=c {
                                for y in b..=d {
                                    let tile = index.run_tiles.entry((x, y)).or_default();
                                    tile.items.push((lo, id));
                                    tile.tallest = tile.tallest.max(hi - lo);
                                    tiled += 1;
                                }
                            }
                        }
                        _ => index.run_everywhere.push(id),
                    }
                }
                Element::Door(o) | Element::Window(o) => {
                    index.hosts.entry(o.host).or_default().push(id);
                }
                _ => {}
            }
        }
        for tile in index.tiles.values_mut().chain(index.run_tiles.values_mut()) {
            tile.items.sort_unstable_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        }
        index
    }

    /// The walls that may meet wall `id` at `node`, one of its ends: those that overlap it in height and whose bodies reach
    /// the node's tile, by id. Every wall that ends there or that the node lies in is among them.
    pub fn neighbours_at(&self, id: Id, node: P2) -> Vec<Id> {
        let Some(&(lo, hi)) = self.spans.get(&id) else { return Vec::new() };
        let mut ids = self.everywhere.clone();
        if node.0.is_finite() && node.1.is_finite() {
            if let Some(tile) = self.tiles.get(&(tile(node.0), tile(node.1))) {
                ids.extend(tile.may_overlap(lo, hi));
            }
        } else {
            ids = self.spans.keys().copied().collect();
        }
        ids.sort_unstable();
        ids.dedup();
        ids.retain(|id| self.spans.get(id).is_some_and(|&(bottom, top)| lo < top && bottom < hi));
        ids
    }

    pub fn openings(&self, host: Guid) -> &[Id] {
        self.hosts.get(&host).map_or(&[], Vec::as_slice)
    }

    /// The runs that may meet at `node` (a point in plan) at heights `lo` to `hi`: those whose bodies reach the node's
    /// tile and whose heights overlap `lo` to `hi` the way two runs' must ([`crate::walls::stacked`]), by id. Every run
    /// ending there or with the node in its body is among them.
    pub fn runs_near(&self, node: P2, lo: f64, hi: f64) -> Vec<Id> {
        let mut ids = self.run_everywhere.clone();
        if node.0.is_finite() && node.1.is_finite() {
            if let Some(tile) = self.run_tiles.get(&(tile(node.0), tile(node.1))) {
                ids.extend(tile.may_overlap(lo, hi));
            }
        } else {
            ids = self.run_spans.keys().copied().collect();
        }
        ids.sort_unstable();
        ids.dedup();
        ids.retain(|id| self.run_spans.get(id).is_some_and(|&(bottom, top)| lo < top && bottom < hi));
        ids
    }

    /// The runs whose bodies reach the box `[x0, y0, x1, y1]`, grown by `grow` mm, at heights `lo` to `hi`, by
    /// id: what a run staying put asks of the runs a command took, to stretch to the joins they had.
    pub fn runs_near_box(&self, b: [f64; 4], grow: f64, lo: f64, hi: f64) -> Vec<Id> {
        let grown = [b[0] - grow, b[1] - grow, b[2] + grow, b[3] + grow];
        let tiled = (grown.iter().all(|v| v.is_finite() && v.abs() < 1e12)).then(|| {
            let t = [tile(grown[0]), tile(grown[1]), tile(grown[2]), tile(grown[3])];
            ((t[0], t[1]), (t[2], t[3]))
        });
        let mut ids: Vec<Id> = Vec::new();
        match tiled {
            Some(((a, b), (c, d))) if (c - a + 1).saturating_mul(d - b + 1) <= MAX_TILES => {
                ids.extend(self.run_everywhere.iter().copied());
                for x in a..=c {
                    for y in b..=d {
                        if let Some(tile) = self.run_tiles.get(&(x, y)) {
                            ids.extend(tile.may_overlap(lo, hi));
                        }
                    }
                }
            }
            _ => ids.extend(self.run_spans.keys().copied()),
        }
        ids.sort_unstable();
        ids.dedup();
        ids.retain(|id| self.run_spans.get(id).is_some_and(|&(bottom, top)| lo < top && bottom < hi));
        ids
    }
}

/// The plan box a wall's body lies in, grown by twice the tolerance: [x0, y0, x1, y1]. An arc's whole circle's.
pub fn reach(m: &Model, w: &crate::model::Wall) -> Option<[f64; 4]> {
    let axis = super::Axis::of(w);
    let (ext, int) = super::faces(m, w);
    let grow = ext.abs().max(int.abs()) + 2.0 * TOL;
    let [x0, y0, x1, y1] = match axis.arc() {
        Some((c, r, _, _)) => [c.0 - r, c.1 - r, c.0 + r, c.1 + r],
        None => [w.start.0.min(w.end.0), w.start.1.min(w.end.1), w.start.0.max(w.end.0), w.start.1.max(w.end.1)],
    };
    let b = [x0 - grow, y0 - grow, x1 + grow, y1 + grow];
    b.iter().all(|v| v.is_finite() && v.abs() < 1e12).then_some(b)
}

fn tile(v: f64) -> i64 {
    (v / TILE).floor() as i64
}

#[cfg(test)]
#[path = "index_tests.rs"]
mod tests;
