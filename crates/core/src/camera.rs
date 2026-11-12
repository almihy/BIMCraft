//! What a 3D view keeps of how it looks through its camera: where a new one looks from, the lenses and film a camera
//! takes and the field of view they give, and its Graphic Display Options. The 3D view (`view3d`) draws by them.

/// Revit's default 3D view looks from the south-east, from above.
pub const AZIMUTH: f64 = -45.0;
pub const ELEVATION: f64 = 30.0;
/// How a 3D view draws its edges and shadows, as Revit's Graphic Display Options: the lines where its surfaces meet (in
/// Shaded; Hidden Line always draws them), a silhouette `silhouette` px wide round what stands in front of what is further off
/// or of nothing (0 for none), and ambient shadows, the soft dark where faces meet inside a corner.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DisplayOptions {
    #[serde(default = "shown")]
    pub edges: bool,
    #[serde(default, skip_serializing_if = "is_none")]
    pub silhouette: u32,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub ambient_shadows: bool,
}

impl Default for DisplayOptions {
    fn default() -> DisplayOptions {
        DisplayOptions { edges: true, silhouette: 0, ambient_shadows: false }
    }
}

impl DisplayOptions {
    pub fn is_default(&self) -> bool {
        *self == DisplayOptions::default()
    }
}

fn shown() -> bool {
    true
}

fn is_none(px: &u32) -> bool {
    *px == 0
}

/// The widest silhouette, px.
pub const MAX_SILHOUETTE: u32 = 10;
/// The 35 mm lens that focal length `lens` on `p`'s film, zoomed by its zoom, is: as wide a field of view.
pub fn full_frame(lens: f64, p: &crate::model::Physical) -> f64 {
    lens * p.zoom * FILM / p.film.max(1e-6)
}

/// A camera's horizontal field of view, degrees, through focal length `lens` on `p`'s film at its zoom.
pub fn field_of_view(lens: f64, p: &crate::model::Physical) -> f64 {
    2.0 * (p.film / (2.0 * lens.max(1e-6) * p.zoom.max(1e-6))).atan().to_degrees()
}

/// The field of view a 35 mm lens of `lens` mm sees, degrees across its 36 mm film.
pub fn fov_of_35mm(lens: f64) -> f64 {
    2.0 * (FILM / (2.0 * lens.max(1e-6))).atan().to_degrees()
}

/// The narrowest and widest field of view a camera has, degrees: its 35 mm equivalent's limits ([`MIN_LENS`] to
/// [`MAX_LENS`]), the same on any film.
pub fn fov_limits() -> (f64, f64) {
    (fov_of_35mm(MAX_LENS), fov_of_35mm(MIN_LENS))
}

/// Whether focal length `lens` on `p`'s film at its zoom is a lens a camera has: its 35 mm equivalent ([`full_frame`]) from
/// [`MIN_LENS`] to [`MAX_LENS`], so a small sensor goes as wide as a full frame. What is wrong, else, for a person to read.
pub fn lens_ok(lens: f64, p: &crate::model::Physical) -> Result<(), String> {
    let eq = full_frame(lens, p);
    if lens.is_finite() && lens > 0.0 && (MIN_LENS..=MAX_LENS).contains(&eq) {
        return Ok(());
    }
    let ((narrow, wide), (lo, hi)) = (fov_limits(), (MIN_LENS, MAX_LENS));
    let zoom = if p.zoom == 1.0 { String::new() } else { format!(" at zoom {}", p.zoom) };
    Err(format!(
        "a {lens} mm lens on {} mm film{zoom} sees as a {eq:.1} mm lens does on 35 mm film; a camera's lens is {lo} to {hi} mm as          35 mm film has it (a field of view of {narrow:.1}° to {wide:.1}°)",
        p.film
    ))
}

/// The focal length that gives field of view `fov` degrees across `p`'s film at its zoom.
pub fn lens_for(fov: f64, p: &crate::model::Physical) -> f64 {
    p.film / (2.0 * p.zoom.max(1e-6) * (fov.clamp(0.1, 179.0).to_radians() / 2.0).tan())
}

/// The width of the film a lens's focal length is given for, mm: a 35 mm camera's, whose frame is 36 mm across.
pub const FILM: f64 = 36.0;
/// The lens a 3D view is first seen through in perspective, mm.
pub const LENS: f64 = 35.0;
/// The shortest and longest lenses a perspective view takes, mm.
pub const MIN_LENS: f64 = 10.0;
pub const MAX_LENS: f64 = 600.0;
