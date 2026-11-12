//! Origin anchored background grids, shared by canvas drawing and snapping.

use serde::{Deserialize, Serialize};

use crate::geom::P2;
use crate::model::MAX_COORD;
use crate::pen::Pen;
use crate::prim::View;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GridStyle {
    pub enabled: bool,
    pub color: [u8; 3],
    pub pen: Pen,
}

impl Default for GridStyle {
    fn default() -> Self {
        Self { enabled: false, color: [230, 230, 230], pen: Pen::HAIRLINE }
    }
}

/// A view's canvas grid; lengths stay in millimetres regardless of display units.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BackgroundGrid {
    pub visible: bool,
    pub main: GridStyle,
    pub sub: GridStyle,
    pub spacing: [f64; 2],
    pub divisions: [u16; 2],
    /// Counter-clockwise degrees about the fixed origin (0, 0).
    pub rotation: f64,
}

impl Default for BackgroundGrid {
    fn default() -> Self {
        Self {
            visible: true,
            main: GridStyle::default(),
            sub: GridStyle { color: [245, 245, 245], ..GridStyle::default() },
            spacing: [1000.0; 2],
            divisions: [5; 2],
            rotation: 0.0,
        }
    }
}

pub struct GridLine {
    pub points: [P2; 2],
    pub style: GridStyle,
}

impl BackgroundGrid {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    pub fn check(&self) -> Result<(), String> {
        if self.valid() {
            Ok(())
        } else {
            Err(format!(
                "background grid: spacing X/Y must be finite and between 0.001 and {MAX_COORD} mm; divisions X/Y 1–1000, rotation -360–360 degrees, and pens 0–16"
            ))
        }
    }

    pub fn valid(&self) -> bool {
        self.spacing.iter().all(|n| n.is_finite() && (0.001..=MAX_COORD).contains(n))
            && self.divisions.iter().all(|n| (1..=1000).contains(n))
            && self.rotation.is_finite()
            && (-360.0..=360.0).contains(&self.rotation)
            && self.main.pen.is_valid()
            && self.sub.pen.is_valid()
    }

    pub fn shown(&self) -> bool {
        self.visible && (self.main.enabled || self.sub.enabled) && self.valid()
    }

    fn rotate(&self, p: P2, inverse: bool) -> P2 {
        let (s, c) = (self.rotation.to_radians() * if inverse { -1.0 } else { 1.0 }).sin_cos();
        P2(c * p.0 - s * p.1, s * p.0 + c * p.1)
    }

    /// Nearest intersection of the enabled lattice, also when its display is hidden.
    /// The caller controls whether snapping is active and how it combines with object snaps.
    pub fn nearest(&self, p: P2) -> Option<P2> {
        if !self.valid() || !(self.main.enabled || self.sub.enabled) || !p.0.is_finite() || !p.1.is_finite() {
            return None;
        }
        let p = self.rotate(p, true);
        let step = |axis: usize| self.spacing[axis] / if self.sub.enabled { f64::from(self.divisions[axis]) } else { 1.0 };
        Some(self.rotate(P2((p.0 / step(0)).round() * step(0), (p.1 / step(1)).round() * step(1)), false))
    }

    /// Viewport lines in model coordinates, subgrid first. Dense levels are omitted;
    /// main lines are thinned by whole multiples, retaining the fixed origin at every zoom.
    /// At most 4096 lines, even at the widest supported zoom.
    pub fn lines(&self, view: View) -> Vec<GridLine> {
        if !self.shown() || !view.scale.is_finite() || view.scale <= 0.0 {
            return Vec::new();
        }
        let corners = [(0.0, 0.0), (view.width as f32, 0.0), (0.0, view.height as f32), (view.width as f32, view.height as f32)]
            .map(|p| self.rotate(view.mm(p), true));
        let lo = P2(corners.iter().map(|p| p.0).fold(f64::INFINITY, f64::min), corners.iter().map(|p| p.1).fold(f64::INFINITY, f64::min));
        let hi = P2(
            corners.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max),
            corners.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max),
        );
        let mut lines = Vec::new();
        for (style, sub) in [(self.sub, true), (self.main, false)] {
            if !style.enabled {
                continue;
            }
            for axis in 0..2 {
                let division = if sub { f64::from(self.divisions[axis]) } else { 1.0 };
                let base = self.spacing[axis] / division;
                if sub && base * view.scale < 4.0 {
                    continue;
                }
                let (min, max) = if axis == 0 { (lo.0, hi.0) } else { (lo.1, hi.1) };
                let stride = (4.0 / (base * view.scale)).max((max - min) / (base * 1023.0)).ceil().max(1.0);
                let step = base * stride;
                let first = (min / step).ceil();
                let count = ((max / step).floor() - first + 1.0).clamp(0.0, 1024.0) as usize;
                for n in 0..count {
                    let index = first + n as f64;
                    // Main lines replace the coincident subgrid lines, so their colour and weight win.
                    if sub && self.main.enabled && (index * stride).rem_euclid(division).abs() < 1e-8 {
                        continue;
                    }
                    let at = index * step;
                    let points = if axis == 0 { [P2(at, lo.1), P2(at, hi.1)] } else { [P2(lo.0, at), P2(hi.0, at)] };
                    lines.push(GridLine { points: points.map(|p| self.rotate(p, false)), style });
                }
            }
        }
        lines
    }
}
