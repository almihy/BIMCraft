//! What a drawing is made of: colours, primitives (fills, lines, pictures) in pixels, the part of its element each draws,
//! and the view that turns model millimetres into those pixels.

use crate::geom::P2;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgb(pub u8, pub u8, pub u8);

pub const PAPER: Rgb = Rgb(255, 255, 255);
/// Wall poché, the grey of cut walls.
pub const POCHE: Rgb = Rgb(168, 168, 168);
/// A cut compound wall's layers that aren't its structure: lighter.
pub const LIGHT_POCHE: Rgb = Rgb(212, 212, 212);

/// What a primitive draws of its element, for the view's overrides: lines and fills in projection
/// (what lies below the cut plane, beyond a section's line, annotation) or where the view cuts
/// it; lettering, which takes a line colour and no weight; and hatching, which takes neither.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Part {
    #[default]
    Projection,
    Cut,
    Text,
    Pattern,
    /// Stroke-like pattern graphics: filled dots and font outlines take line colour overrides.
    LinePattern,
    /// A raster image or a PDF page placed in the view: it picks where nothing drawn near is picked instead.
    Picture,
}

impl Part {
    /// Whether this part's lines turn their corners mitred, square as a cut wall's corner: all but lettering, whose strokes
    /// turn round, so a V's point doesn't stick out.
    pub fn mitred(self) -> bool {
        self != Part::Text
    }
}

#[derive(Clone)]
pub enum Prim {
    Fill {
        rings: Vec<Vec<(f32, f32)>>,
        color: Rgb,
    },
    Line {
        pts: Vec<(f32, f32)>,
        width: f32,
        color: Rgb,
        /// Its on and off lengths, px, again and again; none, solid.
        dash: Option<Vec<f32>>,
        closed: bool,
    },
    /// A picture, its top left corner `at`, a pixel to a pixel: a section's on a sheet. `ink` when all of it is one colour
    /// the view gives (a scan the view fills), not colours of its own: a dark canvas shows it as it is rather than dimmed.
    Image {
        at: (f32, f32),
        width: u32,
        height: u32,
        rgba: Vec<u8>,
        ink: bool,
    },
}

impl Prim {
    /// Its points, px: a fill's rings', a line's, a picture's corners.
    pub fn points(&self) -> Vec<(f32, f32)> {
        match self {
            Prim::Fill { rings, .. } => rings.iter().flatten().copied().collect(),
            Prim::Line { pts, .. } => pts.clone(),
            Prim::Image { at, width, height, .. } => Prim::corners(*at, *width, *height),
        }
    }

    /// The same, `dx` px right and `dy` down.
    pub fn moved(self, dx: f32, dy: f32) -> Prim {
        let mv = |pts: Vec<(f32, f32)>| pts.into_iter().map(|(x, y)| (x + dx, y + dy)).collect();
        match self {
            Prim::Fill { rings, color } => Prim::Fill { rings: rings.into_iter().map(mv).collect(), color },
            Prim::Line { pts, width, color, dash, closed } => Prim::Line { pts: mv(pts), width, color, dash, closed },
            Prim::Image { at, width, height, rgba, ink } => Prim::Image { at: (at.0 + dx, at.1 + dy), width, height, rgba, ink },
        }
    }

    /// An image's corners, clockwise from its top left.
    pub fn corners(at: (f32, f32), width: u32, height: u32) -> Vec<(f32, f32)> {
        let (w, h) = (width as f32, height as f32);
        vec![at, (at.0 + w, at.1), (at.0 + w, at.1 + h), (at.0, at.1 + h)]
    }
}

/// Model millimetres (y up) to image pixels (y down). In a section or elevation, the first
/// coordinate runs along its line from its start, the second up.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    /// Pixels per millimetre.
    pub scale: f64,
    pub cx: f64,
    pub cy: f64,
    pub width: u32,
    pub height: u32,
}

impl View {
    pub fn px(&self, p: P2) -> (f32, f32) {
        (
            (f64::from(self.width) / 2.0 + (p.0 - self.cx) * self.scale) as f32,
            (f64::from(self.height) / 2.0 - (p.1 - self.cy) * self.scale) as f32,
        )
    }

    /// The model point at a pixel: the inverse of [`View::px`].
    pub fn mm(&self, (x, y): (f32, f32)) -> P2 {
        P2(
            self.cx + (f64::from(x) - f64::from(self.width) / 2.0) / self.scale,
            self.cy - (f64::from(y) - f64::from(self.height) / 2.0) / self.scale,
        )
    }
}
