//! Solids cut from solids, on Manifold (`manifold-csg`, bindings to Manifold 3.5.5). The trial of 14 November 2026
//! (docs/manifold-trial.md): this module will be the one doorway to Manifold (`docs/superpowers/plans/2026-11-14-manifold-core.md`
//! in Arqiflow, step 1); for now it holds only the trial function, which proves that the library builds and runs on the
//! desktop and in the browser (wasm32-unknown-unknown).

use manifold_csg::Manifold;

/// What [`solid_trial`] makes: a box minus a cylinder.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SolidTrial {
    /// The solid's volume.
    pub volume: f64,
    /// Its triangles.
    pub triangles: usize,
}

/// A 100 x 100 x 100 box, centred on the origin, minus a cylinder through it (radius 25, 120 long, 64 sides): the volume
/// and the triangle count of what is left. Volume 1,000,000 - 100 x (64-gon of radius 25 = 1,960.34) = 803,966.
pub fn solid_trial() -> SolidTrial {
    let block = Manifold::cube(100.0, 100.0, 100.0, true);
    let hole = Manifold::cylinder(120.0, 25.0, 25.0, 64, true);
    let cut = block.difference(&hole);
    SolidTrial { volume: cut.volume(), triangles: cut.num_tri() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solid_trial_box_minus_cylinder() {
        let t = solid_trial();
        // The 64-gon's area: n / 2 x r^2 x sin(2 pi / n).
        let ring = 32.0 * 25.0 * 25.0 * (std::f64::consts::TAU / 64.0).sin();
        let want = 1_000_000.0 - 100.0 * ring;
        assert!((t.volume - want).abs() < 1e-6, "volume {} against {want}", t.volume);
        // Each face of the box, with the hole in two of them, and 64 quads for the wall of the hole: more than a box's 12.
        assert!(t.triangles > 100, "{} triangles", t.triangles);
    }
}
