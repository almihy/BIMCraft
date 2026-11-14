//! Triangle soup in: welding its corners and finding out whether it is a closed solid, before Manifold sees it.

use std::collections::HashMap;

use super::{SolidError, tol};
use crate::geom::v3::P3;

/// How many edges of an indexed triangle mesh are open (used by one triangle only: no triangle uses it the other way) and how
/// many are non-manifold (used more than once in one direction).
pub(super) fn edge_faults(tris: &[[u32; 3]]) -> (usize, usize) {
    let mut directed: HashMap<(u32, u32), u32> = HashMap::with_capacity(tris.len() * 3);
    for t in tris {
        for k in 0..3 {
            *directed.entry((t[k], t[(k + 1) % 3])).or_default() += 1;
        }
    }
    let mut open = 0;
    let mut non_manifold = 0;
    for (&(a, b), &n) in &directed {
        if n > 1 {
            non_manifold += 1;
        }
        if !directed.contains_key(&(b, a)) {
            open += 1;
        }
    }
    (open, non_manifold)
}

/// Welded corners and triangles of a soup that is a closed solid, wound outwards; the refusal, with its reason, when it is
/// not one.
pub(super) fn weld_closed(soup: &[[P3; 3]]) -> Result<(Vec<P3>, Vec<[u32; 3]>), SolidError> {
    if soup.is_empty() {
        return Err(SolidError::Degenerate("the mesh has no triangles".into()));
    }
    if soup.iter().flatten().flatten().any(|c| !c.is_finite() || c.abs() > crate::geom::MAX_COORD) {
        return Err(SolidError::Degenerate("a corner of the mesh is not a number or is beyond 1000 km".into()));
    }
    // A grid of cells as wide as the tolerance: a corner meets, among the 27 cells round it, the corners it is welded to.
    let cell = |p: &P3| p.map(|c| (c / tol::WELD).floor() as i64);
    let mut verts: Vec<P3> = Vec::new();
    let mut grid: HashMap<[i64; 3], Vec<u32>> = HashMap::new();
    let mut weld = |p: &P3| -> u32 {
        let c = cell(p);
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    let Some(near) = grid.get(&[c[0] + dx, c[1] + dy, c[2] + dz]) else { continue };
                    for &i in near {
                        let q = verts[i as usize];
                        if (q[0] - p[0]).hypot(q[1] - p[1]).hypot(q[2] - p[2]) <= tol::WELD {
                            return i;
                        }
                    }
                }
            }
        }
        let i = verts.len() as u32;
        verts.push(*p);
        grid.entry(c).or_default().push(i);
        i
    };
    let mut tris: Vec<[u32; 3]> = Vec::with_capacity(soup.len());
    for t in soup {
        let w = [weld(&t[0]), weld(&t[1]), weld(&t[2])];
        if w[0] != w[1] && w[1] != w[2] && w[0] != w[2] {
            tris.push(w);
        }
    }
    if tris.is_empty() {
        return Err(SolidError::Degenerate("every triangle of the mesh has a repeated corner".into()));
    }
    let (open_edges, non_manifold_edges) = edge_faults(&tris);
    if open_edges > 0 || non_manifold_edges > 0 {
        return Err(SolidError::NotClosed { open_edges, non_manifold_edges, triangles: tris.len() });
    }
    // Closed and consistently turned: inside out when its volume is negative.
    let volume: f64 = tris
        .iter()
        .map(|t| {
            let [a, b, c] = t.map(|i| verts[i as usize]);
            (a[0] * (b[1] * c[2] - b[2] * c[1]) + a[1] * (b[2] * c[0] - b[0] * c[2]) + a[2] * (b[0] * c[1] - b[1] * c[0])) / 6.0
        })
        .sum();
    if volume < 0.0 {
        for t in &mut tris {
            t.swap(1, 2);
        }
    }
    Ok((verts, tris))
}
