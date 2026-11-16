//! The ground as Revit models it, on the toposolid ([docs/areas/site.md](../../../docs/areas/site.md)): a toposolid's
//! sub-divisions (Revit 2024's Sub-divide), what excavates it (Revit 2024's Excavate, which took over from the Building
//! Pad), a graded region's cut and fill (Revit 2026's Graded Region), and the Survey Point and Project Base Point with the
//! shared coordinates they make. The masterplan's areas, trees and furniture of before are converted to these as a file
//! is read ([`convert`]).
//!
//! What a toposolid is as its excavations and its sunk sub-divisions cut it is worked out from them every time it is drawn
//! ([`Model::toposolid_cut`]), so a sub-division follows its host whatever is done to the host, as Revit's does.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::geom::{self, P2};
use crate::model::{Element, Floor, Guid, Id, MAX_COORD, MAX_POINTS, MAX_TEXT, Model, ShapePoint, TOL};
use crate::site::shapes::poly::{self, Ring};
use crate::walls::Plane;

fn is_zero(x: &f64) -> bool {
    *x == 0.0
}

fn yes() -> bool {
    true
}

fn is_true(x: &bool) -> bool {
    *x
}

/// Revit's Sub-division: a part of a toposolid's top drawn as its own toposolid type (a road's asphalt, a lawn, a pond),
/// raised over its host by `offset` or, negative, sunk into it. It is its host's: it follows its host's surface, goes
/// where its host is moved and is deleted with it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubDivision {
    /// Its type, a toposolid type: one of the project's floor types, its first layer's material what it looks like.
    #[serde(rename = "type")]
    pub type_name: String,
    /// The toposolid it is a part of.
    pub host: Guid,
    /// Its outline, counter-clockwise; it is clipped to its host's.
    pub boundary: Vec<P2>,
    /// What it leaves out, each clockwise.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub openings: Vec<Vec<P2>>,
    /// Revit's Subdivision Height, mm: how far it stands over its host's surface, or (negative) is sunk into it.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub offset: f64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// What it is on the site, as the masterplan called it (`lane/road`, `landscape/lawn`, `water/pond`, `pitch/tennis`…):
    /// what a car park takes it for when it keeps clear of it, and what checks read.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub role: String,
    /// The sketch it was drawn or last edited by, as a floor's.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sketch: Vec<crate::sketch::SketchLine>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, Value>,
}

/// The Survey Point or the Project Base Point, as Revit's (2020.2 and on). Each is where it is in the project (`at`,
/// `elevation`, from the internal origin). The survey point also holds its shared coordinates (`east`, `north`, `height`:
/// where the project is on the survey) and whether it is clipped; the project base point the angle from project north to
/// true north. The fields the other doesn't have stay 0.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BasePoint {
    #[serde(default)]
    pub at: P2,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub elevation: f64,
    /// The survey point's shared coordinates (Revit's E/W, N/S and Elev), mm.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub east: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub north: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub height: f64,
    /// The project base point's Angle to True North, degrees: project north stands that far counter-clockwise from true
    /// north (true north that far clockwise from project north).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub angle: f64,
    /// The survey point's clip: clipped, moving it moves the shared coordinates with it; unclipped, it moves alone.
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub clipped: bool,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, Value>,
}

/// A graded region's existing ground: the toposolid it was copied from, of this model or of the linked model `link`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GradedFrom {
    pub toposolid: Guid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<Guid>,
    /// The existing ground as it was taken, placed in this model (on the graded toposolid's level): what cut and fill is
    /// worked out against, whether or not its source is still there (a link unloaded, a project passed as text).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ground: Option<Box<Floor>>,
}

/// A surface the site is made of: its name (the type's and its material's), its fill on a plan and its line there, its
/// colour in 3D (Arqiflow's palette), and how far a sub-division of it stands over the ground by default, mm.
pub struct Surface {
    pub name: &'static str,
    pub plan: [u8; 3],
    pub line: [u8; 3],
    pub color: [u8; 3],
    pub offset: f64,
}

/// The surfaces a new sub-division's type may be, made in a project when it first needs one.
pub const SURFACES: &[Surface] = &[
    Surface { name: "Asphalt", plan: [164, 170, 178], line: [111, 119, 130], color: [92, 96, 101], offset: 0.0 },
    // A car park's drive aisles (`carpark.rs`): asphalt, a shade apart so they read beside a road.
    Surface { name: "Drive Aisle", plan: [176, 181, 188], line: [111, 119, 130], color: [98, 102, 108], offset: 0.0 },
    Surface { name: "Paving", plan: [235, 230, 221], line: [163, 152, 130], color: [143, 131, 113], offset: 40.0 },
    Surface { name: "Block Paving", plan: [230, 222, 208], line: [163, 152, 130], color: [143, 131, 113], offset: 40.0 },
    Surface { name: "Gravel", plan: [236, 231, 219], line: [170, 160, 140], color: [205, 195, 174], offset: 0.0 },
    Surface { name: "Concrete", plan: [228, 227, 223], line: [150, 150, 146], color: [184, 182, 176], offset: 40.0 },
    Surface { name: "Terrace", plan: [236, 228, 214], line: [163, 140, 106], color: [154, 124, 98], offset: 150.0 },
    Surface { name: "Grass", plan: [214, 235, 198], line: [127, 174, 99], color: [126, 164, 88], offset: 20.0 },
    Surface { name: "Meadow", plan: [228, 238, 196], line: [152, 173, 95], color: [162, 180, 106], offset: 20.0 },
    Surface { name: "Planting", plan: [196, 223, 174], line: [111, 154, 82], color: [91, 135, 67], offset: 20.0 },
    Surface { name: "Water", plan: [207, 230, 243], line: [79, 151, 196], color: [77, 141, 187], offset: -300.0 },
    Surface { name: "Sand", plan: [246, 236, 210], line: [201, 145, 61], color: [220, 199, 154], offset: 40.0 },
    Surface { name: "Sports Surface", plan: [207, 232, 192], line: [79, 143, 58], color: [79, 154, 74], offset: 40.0 },
    Surface { name: "Play Surface", plan: [246, 227, 196], line: [201, 145, 61], color: [201, 145, 61], offset: 40.0 },
];

/// The surface named `name`, if it is one of [`SURFACES`].
pub fn surface(name: &str) -> Option<&'static Surface> {
    SURFACES.iter().find(|s| s.name.eq_ignore_ascii_case(name))
}

/// How thick a sub-division's type is made, mm (a toposolid type's; what shows is its offset).
pub const SURFACE_THICKNESS: f64 = 300.0;
/// How thick a sub-division at the ground or under it is drawn in 3D, mm: a skin on the ground, so it shows.
pub const SKIN: f64 = 20.0;
/// How far outside an excavation's footprint the ground keeps its height, mm: a height field can't hold an upright face,
/// so the cut's face slopes over this, under any drawing's resolution.
pub const CUT_FACE: f64 = 10.0;

/// Type `name` for a sub-division: the project's floor type of that name, or, when it hasn't one, made of the surface of
/// that name (its material made too, in the surface's colours) or of the project's material of that name.
pub fn ensure_type(m: &mut Model, name: &str) {
    if m.types.floor.contains_key(name) {
        return;
    }
    if let Some(s) = surface(name)
        && !m.materials.contains_key(name)
    {
        let material = crate::model::Material {
            color: s.color,
            surface_color: Some(s.plan),
            ..crate::model::Material::default()
        };
        m.materials.insert(name.to_string(), material);
    }
    let layer = crate::model::Layer {
        function: crate::model::Function::Finish,
        material: if m.materials.contains_key(name) { name.to_string() } else { String::new() },
        thickness: SURFACE_THICKNESS,
        ..crate::model::Layer::default()
    };
    m.types.floor.insert(
        name.to_string(),
        crate::model::FloorType { thickness: SURFACE_THICKNESS, layers: vec![layer], identity: Default::default() },
    );
}

// ====================================================================== rings
fn xy(r: &[P2]) -> Ring {
    r.iter().map(|q| poly::of_p2(*q)).collect()
}

fn p2(r: &Ring) -> Vec<P2> {
    r.iter().map(|q| poly::to_p2(*q)).collect()
}

/// Rings as `i_overlay` hands them out (outlines counter-clockwise, holes clockwise) gathered into outlines with their
/// holes.
pub fn grouped(rings: &[Vec<P2>]) -> Vec<(Vec<P2>, Vec<Vec<P2>>)> {
    let mut outers: Vec<(Vec<P2>, Vec<Vec<P2>>)> =
        rings.iter().filter(|r| r.len() >= 3 && geom::area(r) > 0.0).map(|r| (r.clone(), Vec::new())).collect();
    for h in rings.iter().filter(|r| r.len() >= 3 && geom::area(r) < 0.0) {
        let Some(q) = h.first() else { continue };
        // The smallest outline holding it.
        if let Some(o) = outers
            .iter_mut()
            .filter(|(o, _)| geom::contains(o, *q))
            .min_by(|a, b| geom::area(&a.0).total_cmp(&geom::area(&b.0)))
        {
            o.1.push(h.clone());
        }
    }
    outers
}

/// A floor's or a toposolid's outline less its openings, as rings.
fn floor_region(f: &Floor) -> Vec<Ring> {
    let outline = vec![poly::ccw(xy(&f.boundary))];
    if f.openings.is_empty() {
        return outline;
    }
    let holes: Vec<Ring> = f.openings.iter().map(|o| poly::ccw(xy(o))).collect();
    poly::difference(&outline, &holes)
}

// ====================================================================== the toposolid's surface
/// A toposolid's top, to read heights from: its planes, each with its box, and a grid over them that says which faces
/// may lie over each cell (a surveyed ground has thousands of faces, and cut and fill reads it hundreds of thousands of
/// times).
pub struct Surface3 {
    faces: Vec<(Vec<P2>, Plane, [f64; 4])>,
    grid: Grid,
}

/// [`Surface3`]'s grid: its box, its cells' count a side and size, and each cell's faces.
#[derive(Default)]
struct Grid {
    b: [f64; 4],
    n: usize,
    cell: [f64; 2],
    cells: Vec<Vec<u32>>,
}

impl Grid {
    fn of(faces: &[(Vec<P2>, Plane, [f64; 4])]) -> Grid {
        if faces.len() < 16 {
            return Grid::default();
        }
        let b = faces.iter().fold([f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY], |b, (_, _, f)| {
            [b[0].min(f[0]), b[1].min(f[1]), b[2].max(f[2]), b[3].max(f[3])]
        });
        if !b.iter().all(|v| v.is_finite()) {
            return Grid::default();
        }
        let n = ((faces.len() as f64).sqrt().ceil() as usize).clamp(1, 512);
        let cell = [((b[2] - b[0]) / n as f64).max(1e-6), ((b[3] - b[1]) / n as f64).max(1e-6)];
        let mut cells = vec![Vec::new(); n * n];
        let index = |v: f64, lo: f64, size: f64| (((v - lo) / size).floor().max(0.0) as usize).min(n - 1);
        for (k, (_, _, f)) in faces.iter().enumerate() {
            let (i0, i1) = (index(f[0] - TOL, b[0], cell[0]), index(f[2] + TOL, b[0], cell[0]));
            let (j0, j1) = (index(f[1] - TOL, b[1], cell[1]), index(f[3] + TOL, b[1], cell[1]));
            for i in i0..=i1 {
                for j in j0..=j1 {
                    cells[j * n + i].push(k as u32);
                }
            }
        }
        Grid { b, n, cell, cells }
    }

    /// The faces that may lie over `q`, by index; none when it has no cells (every face is looked at then).
    fn near(&self, q: P2) -> Option<&[u32]> {
        if self.n == 0 {
            return None;
        }
        let b = self.b;
        if q.0 < b[0] - TOL || q.0 > b[2] + TOL || q.1 < b[1] - TOL || q.1 > b[3] + TOL {
            return Some(&[]);
        }
        let i = (((q.0 - b[0]) / self.cell[0]).floor().max(0.0) as usize).min(self.n - 1);
        let j = (((q.1 - b[1]) / self.cell[1]).floor().max(0.0) as usize).min(self.n - 1);
        Some(&self.cells[j * self.n + i])
    }
}

impl Surface3 {
    /// The top of floor `f`: [`Model::floor_faces`].
    pub fn of(m: &Model, f: &Floor) -> Surface3 {
        Surface3::new(m.floor_faces(f).into_iter().filter(|(r, _)| r.len() >= 3).collect())
    }

    /// A surface of these faces.
    fn new(faces: Vec<(Vec<P2>, Plane)>) -> Surface3 {
        let faces: Vec<(Vec<P2>, Plane, [f64; 4])> = faces
            .into_iter()
            .map(|(r, plane)| {
                let b = r.iter().fold([f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY], |b, q| {
                    [b[0].min(q.0), b[1].min(q.1), b[2].max(q.0), b[3].max(q.1)]
                });
                (r, plane, b)
            })
            .collect();
        let grid = Grid::of(&faces);
        Surface3 { faces, grid }
    }

    /// Its height at `q`, mm, where it is over `q`.
    pub fn at(&self, q: P2) -> Option<f64> {
        let near = |r: &[P2]| geom::contains(r, q) || geom::ring_edges(r).any(|(a, b)| geom::dist_to_segment(q, a, b) <= TOL);
        let boxed = |b: &[f64; 4]| q.0 >= b[0] - TOL && q.0 <= b[2] + TOL && q.1 >= b[1] - TOL && q.1 <= b[3] + TOL;
        match self.grid.near(q) {
            Some(ks) => ks
                .iter()
                .map(|k| &self.faces[*k as usize])
                .filter(|(_, _, b)| boxed(b))
                .find(|(r, _, _)| near(r))
                .map(|(_, plane, _)| plane.at(q)),
            None => self.faces.iter().filter(|(_, _, b)| boxed(b)).find(|(r, _, _)| near(r)).map(|(_, plane, _)| plane.at(q)),
        }
    }

    /// Its faces.
    pub fn faces(&self) -> impl Iterator<Item = (&Vec<P2>, &Plane)> {
        self.faces.iter().map(|(r, p, _)| (r, p))
    }
}

/// A cut in a toposolid: where (rings, outlines counter-clockwise and holes clockwise) and down to what (`floor`: a flat
/// bottom, mm; or `by`: so far under the ground, a sunk sub-division's).
struct Cut {
    rings: Vec<Ring>,
    floor: Option<f64>,
    by: f64,
}

impl Model {
    /// The toposolid element `f` is, by what it is in the model (the same value, or a copy of it), with its GUID.
    fn toposolid_guid(&self, f: &Floor) -> Option<Guid> {
        self.elements
            .with_guids()
            .find(|(_, _, e)| matches!(e, Element::Toposolid(t) if std::ptr::eq(t, f)))
            .or_else(|| self.elements.with_guids().find(|(_, _, e)| matches!(e, Element::Toposolid(t) if t == f)))
            .map(|(_, g, _)| g)
    }

    /// The sub-divisions of toposolid `host`, by id.
    pub fn subdivisions_of(&self, host: Guid) -> Vec<(Id, &SubDivision)> {
        self.elements
            .iter()
            .filter_map(|(id, e)| match e {
                Element::SubDivision(s) if s.host == host => Some((*id, s)),
                _ => None,
            })
            .collect()
    }

    /// The toposolid a sub-division is a part of.
    pub fn subdivision_host(&self, s: &SubDivision) -> Option<&Floor> {
        match self.elements.get(s.host) {
            Some(Element::Toposolid(f)) => Some(f),
            _ => None,
        }
    }

    /// Where a sub-division is: its outline less its openings, within its host's outline less the host's openings, as
    /// rings (outlines counter-clockwise, holes clockwise). None without a host.
    pub fn subdivision_region(&self, s: &SubDivision) -> Vec<Vec<P2>> {
        let Some(host) = self.subdivision_host(s) else { return Vec::new() };
        let mut own = vec![poly::ccw(xy(&s.boundary))];
        if !s.openings.is_empty() {
            let holes: Vec<Ring> = s.openings.iter().map(|o| poly::ccw(xy(o))).collect();
            own = poly::difference(&own, &holes);
        }
        poly::intersection(&own, &floor_region(host)).iter().map(p2).collect()
    }

    /// A sub-division's area, mm²: its region's.
    pub fn subdivision_area(&self, s: &SubDivision) -> f64 {
        self.subdivision_region(s).iter().map(|r| geom::area(r)).sum::<f64>().max(0.0)
    }

    /// A sub-division's level: its host's.
    pub fn subdivision_level(&self, s: &SubDivision) -> Option<Guid> {
        self.subdivision_host(s).map(|f| f.level)
    }

    /// What cuts toposolid `f` (whose GUID is `own`): the floors, roofs and toposolids that excavate it, each down to its
    /// bottom within its footprint; and its sub-divisions sunk into it, each its offset under the ground.
    fn cuts(&self, f: &Floor, own: Option<Guid>) -> Vec<Cut> {
        let mut out = Vec::new();
        for g in &f.excavated_by {
            let prisms = match self.elements.get(*g) {
                Some(Element::Floor(x)) => self.floor_prisms(x),
                // Another toposolid cuts by its own shape, not as what excavates it cuts it: two toposolids may excavate
                // each other, which worked out through each other's cuts would never end.
                Some(Element::Toposolid(x)) if Some(*g) != own => self.floor_prisms(x),
                Some(Element::Roof(r)) => self.roof_prisms(r),
                _ => continue,
            };
            let bottom = prisms
                .iter()
                .flat_map(|(ring, bottom, _)| ring.iter().map(move |q| bottom.at(*q)))
                .fold(f64::INFINITY, f64::min);
            if !bottom.is_finite() {
                continue;
            }
            let rings: Vec<Ring> = prisms.iter().map(|(r, _, _)| poly::ccw(xy(r))).collect();
            let rings = poly::union(&rings);
            if !rings.is_empty() {
                out.push(Cut { rings, floor: Some(bottom), by: 0.0 });
            }
        }
        if let Some(g) = own {
            for (_, s) in self.subdivisions_of(g) {
                if s.offset < 0.0 {
                    let rings: Vec<Ring> = self.subdivision_region(s).iter().map(|r| xy(r)).collect();
                    if !rings.is_empty() {
                        out.push(Cut { rings, floor: None, by: s.offset });
                    }
                }
            }
        }
        out
    }

    /// Toposolid `f` as what excavates it cuts it (Revit's Excavate) and its sunk sub-divisions sink into it: within each
    /// footprint the ground comes down to the cutter's bottom where it is higher (or follows the ground down a sunk
    /// sub-division's offset); where a cutter's bottom is at or under the toposolid's own, a hole goes through it. Its
    /// shape points carry it: the footprint's corners at the cut's height, and a ring [`CUT_FACE`] outside at the ground's,
    /// so the ground round a cut stays as it was. What nothing cuts is `f` as it is.
    pub fn toposolid_cut(&self, f: &Floor) -> Floor {
        let own = self.toposolid_guid(f);
        let cuts = self.cuts(f, own);
        if cuts.is_empty() {
            return f.clone();
        }
        let top = self.floor_top(f);
        let nominal_bottom = top.at(f.boundary.first().copied().unwrap_or_default()) - self.floor_depth(f);
        let ground = Surface3::of(self, f);
        let mut cut = f.clone();
        // Through: the holes, and the toposolid's outline less them.
        let mut region = floor_region(f);
        let mut deep: Vec<Ring> = Vec::new();
        for c in cuts.iter().filter(|c| c.floor.is_some_and(|b| b <= nominal_bottom + TOL)) {
            deep.extend(c.rings.iter().cloned());
        }
        if !deep.is_empty() {
            region = poly::difference(&region, &deep);
            let groups = grouped(&region.iter().map(p2).collect::<Vec<_>>());
            // The largest piece left is the toposolid; what a cut leaves of it apart is left out (Revit makes it a
            // toposolid of its own).
            let Some((outer, holes)) = groups.into_iter().max_by(|a, b| geom::area(&a.0).total_cmp(&geom::area(&b.0))) else {
                return Floor { openings: vec![f.boundary.clone()], ..f.clone() };
            };
            cut.boundary = outer;
            cut.openings = holes;
        }
        let shallow: Vec<&Cut> = cuts.iter().filter(|c| c.floor.is_none_or(|b| b > nominal_bottom + TOL)).collect();
        if shallow.is_empty() {
            return without_stray_points(corners_on_ground(self, f, &ground, cut));
        }
        // The ground each cut wants at `q`, absolute: its bottom, or so far under the ground.
        let height = |q: P2| ground.at(q).unwrap_or_else(|| top.at(q));
        let wanted = |q: P2| -> Option<f64> {
            shallow
                .iter()
                .filter(|c| poly::in_rings(&c.rings, poly::of_p2(q)) || on_rings(&c.rings, q))
                .map(|c| c.floor.unwrap_or_else(|| height(q) + c.by))
                .fold(None, |a: Option<f64>, z| Some(a.map_or(z, |a| a.min(z))))
        };
        let mut points: Vec<ShapePoint> = Vec::new();
        let put = |points: &mut Vec<ShapePoint>, q: P2, z: f64| {
            if !(q.0.is_finite() && q.1.is_finite() && z.is_finite()) {
                return;
            }
            match points.iter_mut().find(|p| p.at.dist(q) <= 2.0 * TOL) {
                Some(p) => p.z = p.z.min(z),
                None => points.push(ShapePoint { at: q, z, ..Default::default() }),
            }
        };
        // Its own points, lowered where a cut takes the ground down.
        for p in &f.shape.points {
            let z = top.at(p.at) + p.z;
            let z = wanted(p.at).map_or(z, |w| z.min(w));
            put(&mut points, p.at, z - top.at(p.at));
        }
        // Each cut's outline: its corners at the cut's height, a ring just outside at the ground's.
        for c in &shallow {
            for r in &c.rings {
                let outside = poly::grow(std::slice::from_ref(&poly::ccw(r.clone())), CUT_FACE, 4.0);
                for q in outside.iter().flatten().map(|q| poly::to_p2(*q)) {
                    if wanted(q).is_none() {
                        put(&mut points, q, height(q) - top.at(q));
                    }
                }
                for q in r.iter().map(|q| poly::to_p2(*q)) {
                    if let Some(w) = wanted(q) {
                        put(&mut points, q, height(q).min(w) - top.at(q));
                    }
                }
            }
        }
        // The outline's corners inside a cut come down with it.
        for q in cut.boundary.iter().chain(cut.openings.iter().flatten()).copied().collect::<Vec<_>>() {
            if let Some(w) = wanted(q) {
                put(&mut points, q, height(q).min(w) - top.at(q));
            }
        }
        cut.shape.points = points;
        // A split line of its own may run through a cut; the cut's points are what shape it now.
        cut.shape.lines.clear();
        without_stray_points(corners_on_ground(self, f, &ground, cut))
    }
}

/// `cut`'s corners where a cut made them (its outline's and its holes') lifted onto the ground `f` had there, so the ground
/// round a hole keeps its slope: a corner with no shape point stands at the toposolid's level otherwise.
fn corners_on_ground(m: &Model, f: &Floor, ground: &Surface3, mut cut: Floor) -> Floor {
    if f.shape.is_empty() {
        return cut;
    }
    let top = m.floor_top(f);
    let corners: Vec<P2> = cut.boundary.iter().chain(cut.openings.iter().flatten()).copied().collect();
    for q in corners {
        if cut.shape.points.iter().any(|p| p.at.dist(q) <= TOL) {
            continue;
        }
        if let Some(z) = ground.at(q) {
            cut.shape.points.push(ShapePoint { at: q, z: z - top.at(q), ..Default::default() });
        }
    }
    cut
}

/// Whether `q` lies on an edge of `rings`.
fn on_rings(rings: &[Ring], q: P2) -> bool {
    rings.iter().any(|r| {
        let r = p2(r);
        geom::ring_edges(&r).any(|(a, b)| geom::dist_to_segment(q, a, b) <= TOL)
    })
}

/// `f` with no shape point outside its outline or in an opening, none within a millimetre of a corner, so it triangulates.
fn without_stray_points(mut f: Floor) -> Floor {
    let corners: Vec<P2> = f.boundary.iter().chain(f.openings.iter().flatten()).copied().collect();
    let outline = f.boundary.clone();
    let openings = f.openings.clone();
    let mut kept: Vec<ShapePoint> = Vec::new();
    for p in std::mem::take(&mut f.shape.points) {
        let inside = geom::contains(&outline, p.at) || geom::ring_edges(&outline).any(|(a, b)| geom::dist_to_segment(p.at, a, b) <= TOL);
        let in_hole = openings.iter().any(|o| geom::contains(o, p.at));
        let at_corner = corners.iter().any(|c| c.dist(p.at) <= TOL);
        let near_kept = kept.iter().any(|k| k.at.dist(p.at) <= TOL);
        if at_corner {
            // A corner's height is its point's.
            if let Some(c) = corners.iter().find(|c| c.dist(p.at) <= TOL) {
                kept.push(ShapePoint { at: *c, ..p });
            }
        } else if inside && !in_hole && !near_kept {
            kept.push(p);
        }
    }
    kept.dedup_by(|a, b| a.at.dist(b.at) <= TOL);
    f.shape.points = kept;
    f
}

// ====================================================================== a sub-division in 3D and in plan
impl Model {
    /// A sub-division's pieces in 3D: its region cut into its host's planes (as its excavations cut the host), each
    /// piece a ring in plan with its bottom and top planes: from the ground up to `offset` over it, or a [`SKIN`] on it at
    /// 0 or below (a sunk one on the ground it is sunk to).
    pub fn subdivision_pieces(&self, s: &SubDivision) -> Vec<(Vec<P2>, Plane, Plane)> {
        let Some(host) = self.subdivision_host(s) else { return Vec::new() };
        let region: Vec<Ring> = self.subdivision_region(s).iter().map(|r| xy(r)).collect();
        if region.is_empty() {
            return Vec::new();
        }
        let b = region.iter().flatten().fold([f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY], |b, q| {
            [b[0].min(q[0]), b[1].min(q[1]), b[2].max(q[0]), b[3].max(q[1])]
        });
        let ground = Surface3::of(self, &self.toposolid_cut(host));
        let rise = if s.offset > 0.0 { s.offset } else { SKIN };
        let mut out = Vec::new();
        for (face, plane) in ground.faces() {
            let fb = face.iter().fold([f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY], |b, q| {
                [b[0].min(q.0), b[1].min(q.1), b[2].max(q.0), b[3].max(q.1)]
            });
            if fb[0] > b[2] || b[0] > fb[2] || fb[1] > b[3] || b[1] > fb[3] {
                continue;
            }
            let hit: Vec<Vec<P2>> = poly::intersection(&region, &[poly::ccw(xy(face))]).iter().map(p2).collect();
            for (outer, holes) in grouped(&hit) {
                for ring in crate::model::solid(std::slice::from_ref(&outer), &holes) {
                    if ring.len() >= 3 {
                        out.push((ring, *plane, Plane { z: plane.z + rise, g: plane.g }));
                    }
                }
            }
        }
        out
    }

    /// The colours a sub-division draws in: its fill on a plan and its line, and its colour in 3D: its surface's, or its
    /// type's material's.
    pub fn subdivision_colours(&self, s: &SubDivision) -> ([u8; 3], [u8; 3], [u8; 3]) {
        if let Some(sf) = surface(&s.type_name) {
            return (sf.plan, sf.line, sf.color);
        }
        let material = self.type_layers("floor", &s.type_name).first().and_then(|l| self.materials.get(&l.material));
        match material {
            Some(x) => {
                let plan = x.surface_color.unwrap_or_else(|| x.color.map(|c| ((u16::from(c) + 2 * 255) / 3) as u8));
                (plan, x.color.map(|c| (f32::from(c) * 0.7) as u8), x.color)
            }
            None => ([238, 236, 230], [150, 150, 146], [184, 182, 176]),
        }
    }
}

// ====================================================================== cut and fill
/// The most samples cut and fill reads the two grounds at.
pub const MAX_SAMPLES: usize = 1_000_000;

impl Model {
    /// The existing ground a graded region was copied from, as a toposolid (as what excavates it cuts it), the model it is
    /// in, and the move that takes its points into this model (a link's place and turn): none when it isn't there (deleted,
    /// or its link not loaded).
    pub fn graded_source(&self, g: &GradedFrom) -> Option<(Floor, &Model, Box<dyn Fn(P2) -> P2>)> {
        match g.link {
            None => match self.elements.get(g.toposolid) {
                Some(Element::Toposolid(f)) => Some((self.toposolid_cut(f), self, Box::new(|q| q))),
                _ => None,
            },
            Some(l) => {
                let Some(Element::Link(link)) = self.elements.get(l) else { return None };
                let linked: &Model = self.loaded.get(&link.path)?;
                let Some(Element::Toposolid(f)) = linked.elements.get(g.toposolid) else { return None };
                let (at, (sin, cos)) = (link.at, link.angle.to_radians().sin_cos());
                let place = move |q: P2| P2(at.0 + q.0 * cos - q.1 * sin, at.1 + q.0 * sin + q.1 * cos);
                Some((linked.toposolid_cut(f), linked, Box::new(place)))
            }
        }
    }

    /// A graded region's cut and fill, m³ (Revit 2026's): within its outline, projected up, the volume of the existing
    /// ground over the new (cut) and under it (fill), read on a grid over its outline. None for a toposolid that is no
    /// graded region, or whose existing ground isn't there.
    pub fn cut_fill(&self, f: &Floor) -> Option<(f64, f64)> {
        let g = f.graded_from.as_ref()?;
        let (old, source, place): (Floor, &Model, Box<dyn Fn(P2) -> P2>) = match &g.ground {
            Some(ground) => ((**ground).clone(), self, Box::new(|q| q)),
            None => self.graded_source(g)?,
        };
        let old_ring: Vec<P2> = old.boundary.iter().map(|q| place(*q)).collect();
        let old_holes: Vec<Vec<P2>> = old.openings.iter().map(|o| o.iter().map(|q| place(*q)).collect()).collect();
        // The existing ground's faces, placed in this model.
        let old_surface = Surface3::of(source, &old);
        let old_faces = Surface3::new(
            old_surface
                .faces
                .iter()
                .map(|(r, plane, _)| {
                    let ring: Vec<P2> = r.iter().map(|q| place(*q)).collect();
                    // A plane through three of its corners, placed.
                    let lifted: Vec<[f64; 3]> = r.iter().zip(&ring).map(|(q, p)| [p.0, p.1, plane.at(*q)]).collect();
                    let placed = Plane::of(&lifted).unwrap_or(*plane);
                    (ring, placed)
                })
                .collect(),
        );
        let new = self.toposolid_cut(f);
        let new_surface = Surface3::of(self, &new);
        let b = new.boundary.iter().fold([f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY], |b, q| {
            [b[0].min(q.0), b[1].min(q.1), b[2].max(q.0), b[3].max(q.1)]
        });
        let (w, h) = (b[2] - b[0], b[3] - b[1]);
        if !(w > 0.0 && h > 0.0 && w.is_finite() && h.is_finite()) {
            return Some((0.0, 0.0));
        }
        // Half a metre, or finer for a small site (200 samples a side at least), and no more than the most.
        let mut step = 500.0_f64.min(w.max(h) / 200.0).max(1.0);
        while (w / step).ceil() * (h / step).ceil() > MAX_SAMPLES as f64 {
            step *= 1.5;
        }
        let (nx, ny) = ((w / step).ceil() as usize, (h / step).ceil() as usize);
        let (mut cut, mut fill) = (0.0, 0.0);
        let cell = step * step;
        for i in 0..nx {
            for j in 0..ny {
                let q = P2(b[0] + (i as f64 + 0.5) * step, b[1] + (j as f64 + 0.5) * step);
                if !geom::contains(&new.boundary, q) || new.openings.iter().any(|o| geom::contains(o, q)) {
                    continue;
                }
                if !geom::contains(&old_ring, q) || old_holes.iter().any(|o| geom::contains(o, q)) {
                    continue;
                }
                let (Some(zn), Some(zo)) = (new_surface.at(q), old_faces.at(q)) else { continue };
                if zo > zn {
                    cut += (zo - zn) * cell;
                } else {
                    fill += (zn - zo) * cell;
                }
            }
        }
        Some((cut / 1e9, fill / 1e9))
    }
}

// ====================================================================== the base points and shared coordinates
impl Model {
    /// The project's survey point.
    pub fn survey_point(&self) -> Option<(Id, &BasePoint)> {
        self.elements.iter().find_map(|(id, e)| match e {
            Element::SurveyPoint(b) => Some((*id, b)),
            _ => None,
        })
    }

    /// The project's base point.
    pub fn project_base_point(&self) -> Option<(Id, &BasePoint)> {
        self.elements.iter().find_map(|(id, e)| match e {
            Element::ProjectBasePoint(b) => Some((*id, b)),
            _ => None,
        })
    }

    /// The angle from project north to true north, degrees (the project base point's; 0 without one): project north is
    /// that far counter-clockwise from true north.
    pub fn angle_to_true_north(&self) -> f64 {
        self.project_base_point().map_or(0.0, |(_, b)| b.angle)
    }

    /// Where project point `p` at height `z` is in shared coordinates (east, north, elevation), mm: the survey point's
    /// shared coordinates, plus how far `p` is from it turned by the angle to true north.
    pub fn shared(&self, p: P2, z: f64) -> [f64; 3] {
        let sp = self.survey_point().map(|(_, b)| b.clone()).unwrap_or_default();
        let (sin, cos) = self.angle_to_true_north().to_radians().sin_cos();
        let d = p - sp.at;
        [sp.east + d.0 * cos - d.1 * sin, sp.north + d.0 * sin + d.1 * cos, sp.height + (z - sp.elevation)]
    }

    /// The project point at shared coordinates `e`: [`Model::shared`] the other way.
    pub fn from_shared(&self, e: [f64; 3]) -> (P2, f64) {
        let sp = self.survey_point().map(|(_, b)| b.clone()).unwrap_or_default();
        let (sin, cos) = self.angle_to_true_north().to_radians().sin_cos();
        let (dx, dy) = (e[0] - sp.east, e[1] - sp.north);
        (sp.at + P2(dx * cos + dy * sin, -dx * sin + dy * cos), e[2] - sp.height + sp.elevation)
    }

    /// What the project's georeference's numbers are from its points: where the internal origin is in shared coordinates,
    /// and the angle to true north. None when it has neither point.
    pub fn georeference_numbers(&self) -> Option<(f64, f64, f64, f64)> {
        if self.survey_point().is_none() && self.project_base_point().is_none() {
            return None;
        }
        let o = self.shared(P2(0.0, 0.0), 0.0);
        Some((o[0], o[1], o[2], self.angle_to_true_north()))
    }

    /// The height of the ground on `level` at `q`, mm absolute, and the toposolid it is of: the highest toposolid on that
    /// level over `q`, as its excavations cut it.
    pub fn ground_at(&self, level: Guid, q: P2) -> Option<(Id, f64)> {
        self.elements
            .iter()
            .filter_map(|(id, e)| match e {
                Element::Toposolid(f) if f.level == level && geom::contains(&f.boundary, q) => {
                    Surface3::of(self, &self.toposolid_cut(f)).at(q).map(|z| (*id, z))
                }
                _ => None,
            })
            .max_by(|a, b| a.1.total_cmp(&b.1))
    }
}

/// `Model::check`'s rules for a sub-division: its host a toposolid, its type a floor type, a simple outline of up to the
/// model's points, finite, its offset within reach, its texts not too long.
pub fn check_subdivision(m: &Model, who: &str, s: &SubDivision) -> Result<(), String> {
    match m.elements.get(s.host) {
        Some(Element::Toposolid(_)) => {}
        Some(e) => return Err(format!("{who}: its host is a {}; a sub-division is a part of a toposolid", e.category())),
        None => return Err(format!("{who}: its host toposolid isn't in the project")),
    }
    if !m.types.floor.contains_key(&s.type_name) {
        return Err(format!("{who}: no toposolid type `{}`; make one with `sub_division`'s `type` or `type`", s.type_name));
    }
    if !(3..=MAX_POINTS).contains(&s.boundary.len()) {
        return Err(format!("{who}: its outline runs through 3 to {MAX_POINTS} points"));
    }
    let finite = |q: &P2| q.0.is_finite() && q.1.is_finite() && q.0.abs() <= MAX_COORD && q.1.abs() <= MAX_COORD;
    if !s.boundary.iter().chain(s.openings.iter().flatten()).all(finite) {
        return Err(format!("{who}: a point of its outline is not finite or reaches past 1000 km"));
    }
    if s.openings.len() > MAX_POINTS || s.openings.iter().any(|o| o.len() < 3 || o.len() > MAX_POINTS) {
        return Err(format!("{who}: each opening runs through 3 to {MAX_POINTS} points"));
    }
    if geom::area(&s.boundary).abs() < 1.0 {
        return Err(format!("{who}: its outline encloses no area"));
    }
    if !(s.offset.is_finite() && s.offset.abs() <= 1.0e6) {
        return Err(format!("{who}: its offset is a finite height within 1 km, not {}", s.offset));
    }
    for (what, t) in [("name", &s.name), ("role", &s.role)] {
        if t.chars().count() > MAX_TEXT {
            return Err(format!("{who}: its {what} is longer than {MAX_TEXT} characters"));
        }
    }
    crate::sketch::check(who, &s.sketch)
}

/// `Model::check`'s rules for a base point: finite, within reach, one of each kind in a project.
pub fn check_base_point(m: &Model, who: &str, survey: bool, b: &BasePoint) -> Result<(), String> {
    let near = |v: f64| v.is_finite() && v.abs() <= MAX_COORD;
    if !(near(b.at.0) && near(b.at.1) && near(b.elevation)) {
        return Err(format!("{who}: it stands within 1000 km of the origin, finite"));
    }
    if ![b.east, b.north, b.height].iter().all(|v| v.is_finite() && v.abs() <= crate::model::MAX_MAP) {
        return Err(format!("{who}: its shared coordinates are finite, within {} mm", crate::model::MAX_MAP));
    }
    if !(b.angle.is_finite() && b.angle.abs() <= 360.0) {
        return Err(format!("{who}: its angle to true north is degrees, -360 to 360, not {}", b.angle));
    }
    let count = m
        .elements
        .values()
        .filter(|e| if survey { matches!(e, Element::SurveyPoint(_)) } else { matches!(e, Element::ProjectBasePoint(_)) })
        .count();
    if count > 1 {
        return Err(format!("{who}: a project has one {}", if survey { "survey point" } else { "project base point" }));
    }
    Ok(())
}

// ====================================================================== the site families
/// The family categories of the site's components and what they are called: Revit's Planting and Site.
pub const COMPONENT_CATEGORIES: [&str; 2] = ["planting", "sitecomponent"];

/// Family `name` from the engine's site families ([`crate::family::site_library`]) put in the project when it hasn't one
/// of that name, as Revit loads a family. Whether it has it now.
pub fn load_family(m: &mut Model, name: &str) -> bool {
    if m.families.contains_key(name) {
        return true;
    }
    match crate::family::site_library(m.language).remove(name) {
        Some(f) => {
            // Its materials, made in the project's library where it hasn't them.
            for (n, color) in crate::family::site::MATERIALS {
                let n = m.language.word(n).to_string();
                if f.materials().contains(&n.as_str()) && !m.materials.contains_key(&n) {
                    m.materials.insert(n, crate::model::Material { color, ..crate::model::Material::default() });
                }
            }
            m.families.insert(name.to_string(), f);
            true
        }
        None => false,
    }
}

/// Whether component `c` is planting or a site component, which stands on the ground.
pub fn on_the_ground(m: &Model, c: &crate::model::Component) -> bool {
    m.families.get(&c.family).is_some_and(|f| {
        matches!(f.category, crate::family::Category::Planting | crate::family::Category::SiteComponent | crate::family::Category::Parking)
    })
}

// ====================================================================== keeping it
/// What [`keep`] changes after a command: planting and site components' heights on the ground, and the georeference's
/// numbers from the survey and base points.
pub enum Update {
    Offset { id: Id, offset: f64 },
    Georeference(crate::model::Georeference),
}

/// After a command: each planting or site component it moved, or that stands on a toposolid it changed, stands on the
/// ground again; the georeference's numbers follow the survey and base points.
pub fn keep(m: &Model, before: &Model) -> Vec<Update> {
    let mut out = Vec::new();
    let changed: BTreeSet<Id> = match m.elements.changed_since(&before.elements) {
        crate::model::Changed::Ids(ids) => ids.into_iter().collect(),
        crate::model::Changed::All => m.elements.iter().map(|(id, _)| *id).collect(),
    };
    if changed.is_empty() && m.georeference == before.georeference {
        return out;
    }
    let grounds: BTreeSet<Guid> = changed
        .iter()
        .filter_map(|id| match m.elements.get(*id).or_else(|| before.elements.get(*id)) {
            Some(Element::Toposolid(f)) => Some(f.level),
            Some(Element::SubDivision(s)) => m.subdivision_level(s),
            _ => None,
        })
        .collect();
    let excavators_changed = changed.iter().any(|id| matches!(m.elements.get(*id), Some(Element::Floor(_) | Element::Roof(_))))
        && m.elements.values().any(|e| matches!(e, Element::Toposolid(f) if !f.excavated_by.is_empty()));
    for (id, e) in m.elements.iter() {
        let Element::Component(c) = e else { continue };
        if !(changed.contains(id) || grounds.contains(&c.level) || excavators_changed) || !on_the_ground(m, c) {
            continue;
        }
        let Some((_, z)) = m.ground_at(c.level, c.at) else { continue };
        let offset = ((z - m.elevation(c.level)) * 100.0).round() / 100.0;
        if (offset - c.offset).abs() > 0.005 {
            out.push(Update::Offset { id: *id, offset });
        }
    }
    let points_changed =
        changed.iter().any(|id| matches!(m.elements.get(*id).or_else(|| before.elements.get(*id)), Some(Element::SurveyPoint(_) | Element::ProjectBasePoint(_))));
    if (points_changed || m.georeference != before.georeference)
        && let (Some(g), Some((e, n, h, r))) = (&m.georeference, m.georeference_numbers())
    {
        let round = |v: f64| (v * 1000.0).round() / 1000.0;
        let next = crate::model::Georeference { eastings: round(e), northings: round(n), height: round(h), rotation: round(r), ..g.clone() };
        if next != *g {
            out.push(Update::Georeference(next));
        }
    }
    out
}

pub fn apply(m: &mut Model, updates: Vec<Update>) {
    for u in updates {
        match u {
            Update::Offset { id, offset } => {
                if let Some(Element::Component(c)) = m.elements.get_mut(id) {
                    c.offset = offset;
                }
            }
            Update::Georeference(g) => m.georeference = Some(g),
        }
    }
}

// ====================================================================== the conversions of a file from before
/// The masterplan kinds that are sub-divisions now, and what each becomes: its type and its offset, by its use and
/// surface.
fn subdivision_of(kind: &str, use_: &str, surface_: &str) -> Option<(&'static str, f64)> {
    let by_surface = match surface_ {
        "asphalt" => Some("Asphalt"),
        "block_paving" => Some("Block Paving"),
        "gravel" => Some("Gravel"),
        "concrete" => Some("Concrete"),
        "stone" | "resin" => Some("Paving"),
        "grass" | "grass_pavers" => Some("Grass"),
        "meadow" => Some("Meadow"),
        "planting" => Some("Planting"),
        "sand" => Some("Sand"),
        "natural" | "synthetic" | "hard" => Some("Sports Surface"),
        "safety" => Some("Play Surface"),
        _ => None,
    };
    let (default, offset) = match kind {
        "paving" => ("Paving", 40.0),
        "terrace" => ("Terrace", 150.0),
        "landscape" if use_ == "meadow" => ("Meadow", 20.0),
        "landscape" if use_ == "lawn" || use_ == "orchard" => ("Grass", 20.0),
        "landscape" => ("Planting", 20.0),
        "water" => ("Water", -300.0),
        "stream" => ("Water", -200.0),
        "pitch" => ("Sports Surface", 40.0),
        "playground" => ("Play Surface", 40.0),
        "lane" if use_ == "road" => ("Asphalt", 0.0),
        "lane" => ("Paving", 50.0),
        _ => return None,
    };
    Some((by_surface.unwrap_or(default), offset))
}

/// The kinds `site` no longer takes, and what makes each now.
pub const CONVERTED: &[(&str, &str)] = &[
    ("paving", "sub_division"),
    ("terrace", "sub_division"),
    ("landscape", "sub_division"),
    ("water", "sub_division"),
    ("stream", "sub_division"),
    ("pitch", "sub_division"),
    ("playground", "sub_division"),
    ("lane", "road (or sub_division)"),
    ("tree", "planting"),
    ("hedge", "planting (family Hedge)"),
    ("bike_rack", "site_component (family Bike Rack)"),
    ("play_area", "site_component (family Play Equipment)"),
];

/// The toposolid a converted element goes on: the one on `level` whose outline holds `q`, else the lowest of the
/// project's. None when the project has none.
fn host_for(m: &Model, level: Guid, q: P2) -> Option<Guid> {
    let on_level = m.elements.with_guids().find_map(|(_, g, e)| match e {
        Element::Toposolid(f) if f.level == level && geom::contains(&f.boundary, q) => Some(g),
        _ => None,
    });
    on_level.or_else(|| {
        m.elements
            .with_guids()
            .filter_map(|(_, g, e)| match e {
                Element::Toposolid(f) => Some((g, m.elevation(f.level) + f.offset)),
                _ => None,
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(g, _)| g)
    })
}

/// The toposolid on `level` whose outline holds `q` (the highest there, where they overlap).
pub fn toposolid_under(m: &Model, level: Guid, q: P2) -> Option<Guid> {
    m.elements
        .with_guids()
        .filter_map(|(_, g, e)| match e {
            Element::Toposolid(f) if f.level == level && geom::contains(&f.boundary, q) => Some((g, f.offset)),
            _ => None,
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(g, _)| g)
}

/// A flat toposolid named `Ground` under everything in `boxes`, 5 m past it, on `level`: what converted elements go on in
/// a project that has no toposolid (Revit has no sub-division without one).
fn make_ground(m: &mut Model, level: Guid, b: [f64; 4]) -> Guid {
    if !m.types.floor.contains_key("Toposolid") {
        m.types.floor.insert("Toposolid".into(), crate::model::FloorType { thickness: 1000.0, layers: Vec::new(), identity: Default::default() });
    }
    let pad = 5000.0;
    let f = Floor {
        type_name: "Toposolid".into(),
        level,
        boundary: vec![P2(b[0] - pad, b[1] - pad), P2(b[2] + pad, b[1] - pad), P2(b[2] + pad, b[3] + pad), P2(b[0] - pad, b[3] + pad)],
        openings: Vec::new(),
        offset: 0.0,
        slope: 0.0,
        arrow: None,
        structural: false,
        pattern_origin: P2(0.0, 0.0),
        pattern_angle: 0.0,
        sketch: Vec::new(),
        shape: Default::default(),
        contours: 1000.0,
        excavated_by: Vec::new(),
        graded_from: None,
        params: BTreeMap::new(),
    };
    let id = m.insert(Element::Toposolid(f));
    m.guid(id)
}

/// The values a site element keeps of where it came from, under names no project parameter may take: Arqiflow's `source`
/// (site, osm, overture, survey…), `ref` (its id there) and `survey` (the survey it was read from); and a car park's bays'
/// and aisles' link to it (`carpark.rs`: the car park, and where and how it placed them).
pub const KEPT: [&str; 5] = ["source", "ref", "survey", crate::carpark::BY, crate::carpark::PLACED];

/// Whether `name` is one of [`KEPT`].
pub fn reserved(name: &str) -> bool {
    KEPT.contains(&name)
}

/// `Model::check`'s rule for a value of [`KEPT`] on element `e`: text of up to 4096 characters on a sub-division, a
/// toposolid or a component. None for another name.
pub fn check_value(who: &str, k: &str, v: &Value, e: &Element) -> Option<Result<(), String>> {
    if !reserved(k) {
        return None;
    }
    let on = matches!(e, Element::SubDivision(_) | Element::Toposolid(_) | Element::Component(_));
    Some(match v.as_str() {
        _ if !on => Err(format!("{who}: `{k}` is kept by the site's elements (sub-divisions, toposolids, planting)")),
        Some(t) if t.chars().count() <= crate::masterplan::MAX_PROP_TEXT => Ok(()),
        _ => Err(format!("{who}: `{k}` is text of up to {} characters", crate::masterplan::MAX_PROP_TEXT)),
    })
}

/// The project's params a converted element keeps: its own (the phases among them), and where it came from ([`KEPT`]).
fn kept_params(s: &crate::masterplan::Site) -> BTreeMap<String, Value> {
    let mut p = s.params.clone();
    if !s.source.is_empty() {
        p.insert("source".into(), json!(s.source));
    }
    if !s.reference.is_empty() {
        p.insert("ref".into(), json!(s.reference));
    }
    if let Some(t) = s.props.get("survey").and_then(Value::as_str).filter(|t| !t.is_empty()) {
        p.insert("survey".into(), json!(t));
    }
    p
}

fn num(p: &crate::site::Props, k: &str) -> Option<f64> {
    p.get(k).and_then(Value::as_f64).filter(|v| v.is_finite() && v.abs() <= MAX_COORD)
}

fn point(p: &crate::site::Props, k: &str) -> Option<P2> {
    let a = p.get(k)?.as_array()?;
    let (x, y) = (a.first()?.as_f64()?, a.get(1)?.as_f64()?);
    (x.is_finite() && y.is_finite()).then_some(P2(x, y))
}

/// The masterplan's areas, trees and furniture of a file from before (format 25 or older) made what Revit has: each area
/// a sub-division of the toposolid under it (a road its carriageway, pavements and median), each tree, hedge, bike rack and
/// play area a family's component on the ground (docs/site-as-revit-plan.md). Car parks, parking and ramps stay as they
/// are. A project without a toposolid gets one, `Ground`, under what is converted.
pub fn convert(m: &mut Model) {
    let olds: Vec<(Id, Guid, crate::masterplan::Site)> = m
        .elements
        .with_guids()
        .filter_map(|(id, g, e)| match e {
            Element::Site(s) if CONVERTED.iter().any(|(k, _)| *k == s.kind) => Some((id, g, (**s).clone())),
            _ => None,
        })
        .collect();
    if olds.is_empty() {
        return;
    }
    // A ground for all of them where the project has none.
    let has_ground = m.elements.values().any(|e| matches!(e, Element::Toposolid(_)));
    let mut ground: Option<Guid> = None;
    if !has_ground {
        let mut b = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
        for (_, _, s) in &olds {
            for q in s.outline.iter().flat_map(|p| p.outer.iter()).copied().chain(point(&s.props, "at")) {
                if q.0.is_finite() && q.1.is_finite() {
                    b = [b[0].min(q.0), b[1].min(q.1), b[2].max(q.0), b[3].max(q.1)];
                }
            }
        }
        if b[0].is_finite() {
            let level = olds.iter().map(|(_, _, s)| s.level).next().unwrap_or_default();
            ground = Some(make_ground(m, level, b));
        }
    }
    for (id, guid, s) in olds {
        let middle = s
            .outline
            .first()
            .map(|p| geom::centroid(&p.outer))
            .or_else(|| point(&s.props, "at"))
            .unwrap_or_default();
        let host = ground.or_else(|| host_for(m, s.level, middle));
        let made: Vec<Element> = match s.kind.as_str() {
            "tree" | "hedge" | "bike_rack" | "play_area" => component_of(m, &s),
            _ => match host {
                Some(h) => subdivisions_from(m, &s, h),
                None => Vec::new(),
            },
        };
        m.elements.remove(id);
        for (k, e) in made.into_iter().enumerate() {
            // The first takes the old element's id and GUID, so what pointed at it points at it still.
            if k == 0 {
                let _ = m.elements.insert_as(id, guid, e);
            } else {
                m.insert(e);
            }
        }
    }
}

/// A converted area's sub-divisions on toposolid `host`: one of its outline, or a road's carriageway, pavements and median.
pub fn subdivisions_from(m: &mut Model, s: &crate::masterplan::Site, host: Guid) -> Vec<Element> {
    let use_ = s.use_or_default().to_string();
    let Some((type_name, offset)) = subdivision_of(&s.kind, &use_, s.surface()) else { return Vec::new() };
    let mut out = Vec::new();
    let mut make = |m: &mut Model, polys: &[crate::site::Poly], type_name: &str, offset: f64, role: String| {
        ensure_type(m, type_name);
        for p in polys.iter().filter(|p| p.outer.len() >= 3 && geom::area(&p.outer).abs() >= 1.0) {
            let mut boundary = p.outer.clone();
            if geom::area(&boundary) < 0.0 {
                boundary.reverse();
            }
            let openings: Vec<Vec<P2>> = p
                .holes
                .iter()
                .filter(|h| h.len() >= 3)
                .map(|h| {
                    let mut h = h.clone();
                    if geom::area(&h) > 0.0 {
                        h.reverse();
                    }
                    h
                })
                .collect();
            out.push(Element::SubDivision(SubDivision {
                type_name: type_name.to_string(),
                host,
                boundary,
                openings,
                offset,
                name: s.name.clone(),
                role: role.clone(),
                sketch: Vec::new(),
                params: kept_params(s),
            }));
        }
    };
    let part = |role: &str| -> Vec<crate::site::Poly> { s.parts.iter().filter(|p| p.role == role).flat_map(|p| p.polys.clone()).collect() };
    if s.kind == "lane" && use_ == "road" && !part("carriageway").is_empty() {
        make(m, &part("carriageway"), type_name, offset, "lane/road".into());
        make(m, &part("pavement"), "Paving", 150.0, "lane/footpath".into());
        make(m, &part("median"), "Grass", 150.0, "lane/median".into());
    } else {
        make(m, &s.outline, type_name, offset, format!("{}/{}", s.kind, use_));
    }
    out
}

/// A converted tree, hedge, bike rack or play area: a component of the site families on the ground.
fn component_of(m: &mut Model, s: &crate::masterplan::Site) -> Vec<Element> {
    use crate::family::site;
    let level = s.level;
    let place = |m: &Model, family: &str, type_name: &str, at: P2, angle: f64, params: BTreeMap<String, Value>| {
        let ground = m.ground_at(level, at).map_or(0.0, |(_, z)| z - m.elevation(level));
        Element::Component(crate::model::Component {
            family: family.to_string(),
            type_name: type_name.to_string(),
            level,
            at,
            angle,
            offset: (ground * 100.0).round() / 100.0,
            mirrored: false,
            params,
        })
    };
    let mut own = kept_params(s);
    let language = m.language;
    let w = |k: &str| language.word(k).to_string();
    match s.kind.as_str() {
        "tree" => {
            let Some(at) = point(&s.props, "at") else { return Vec::new() };
            let family = w(site::TREE);
            if !load_family(m, &family) {
                return Vec::new();
            }
            if let Some(crown) = num(&s.props, "crown") {
                own.insert(w(site::CROWN), json!(crown));
            }
            if let Some(h) = num(&s.props, "height") {
                own.insert(w(site::HEIGHT), json!(h));
            }
            let t = first_type(m, &family);
            vec![place(m, &family, &t, at, 0.0, own)]
        }
        "bike_rack" => {
            let Some(at) = point(&s.props, "at") else { return Vec::new() };
            let family = w(site::BIKE_RACK);
            if !load_family(m, &family) {
                return Vec::new();
            }
            if let Some(n) = num(&s.props, "spaces") {
                own.insert(w(site::SPACES), json!(n.round().max(1.0)));
            }
            let t = first_type(m, &family);
            vec![place(m, &family, &t, at, num(&s.props, "angle").unwrap_or(0.0), own)]
        }
        "play_area" => {
            let control: Vec<P2> = s.outline.first().map(|p| p.outer.clone()).unwrap_or_default();
            let at = point(&s.props, "at").or_else(|| (control.len() >= 3).then(|| geom::centroid(&control)));
            let Some(at) = at else { return Vec::new() };
            let family = w(site::PLAY);
            if !load_family(m, &family) {
                return Vec::new();
            }
            let radius = num(&s.props, "radius").or_else(|| (control.len() >= 3).then(|| (geom::area(&control).abs() / std::f64::consts::PI).sqrt()));
            if let Some(r) = radius.filter(|r| *r > 0.0) {
                own.insert(w(site::RADIUS), json!((r * 100.0).round() / 100.0));
            }
            let t = first_type(m, &family);
            vec![place(m, &family, &t, at, 0.0, own)]
        }
        "hedge" => {
            let path: Vec<P2> = s
                .props
                .get("path")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|q| {
                            let q = q.as_array()?;
                            Some(P2(q.first()?.as_f64()?, q.get(1)?.as_f64()?))
                        })
                        .filter(|q| q.0.is_finite() && q.1.is_finite())
                        .collect()
                })
                .unwrap_or_default();
            let family = w(site::HEDGE);
            if path.len() < 2 || !load_family(m, &family) {
                return Vec::new();
            }
            let t = first_type(m, &family);
            let mut out = Vec::new();
            for pair in path.windows(2) {
                let (a, b) = (pair[0], pair[1]);
                let len = a.dist(b);
                if len < 1.0 {
                    continue;
                }
                let mut params = own.clone();
                params.insert(w(site::LENGTH), json!((len * 100.0).round() / 100.0));
                if let Some(width) = num(&s.props, "width") {
                    params.insert(w(site::WIDTH), json!(width));
                }
                if let Some(h) = num(&s.props, "height") {
                    params.insert(w(site::HEIGHT), json!(h));
                }
                let angle = (b.1 - a.1).atan2(b.0 - a.0).to_degrees();
                out.push(place(m, &family, &t, P2((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0), (angle * 1e6).round() / 1e6, params));
            }
            out
        }
        _ => Vec::new(),
    }
}

/// The first type of family `name`.
fn first_type(m: &Model, name: &str) -> String {
    m.families.get(name).and_then(|f| f.types.keys().next().cloned()).unwrap_or_default()
}

/// A building pad of a file from before, as its JSON holds it: made a floor of its type, outline and level, its top at its
/// offset, which excavates the toposolids on its level it lies in. Called on the file's JSON before it is read, as the
/// category is gone: the element's id keys are returned, to excavate with once read ([`excavate_with_pads`]).
pub fn pads_to_floors(v: &mut Value) -> Vec<String> {
    let mut out = Vec::new();
    let Some(Value::Object(es)) = v.get_mut("elements") else { return out };
    for (id, e) in es.iter_mut() {
        let Some(o) = e.as_object_mut() else { continue };
        if o.get("category").and_then(Value::as_str) != Some("buildingpad") {
            continue;
        }
        o.insert("category".into(), json!("floor"));
        // A pad's fields are a floor's, all but none: type, level, boundary, offset, params.
        out.push(id.clone());
    }
    out
}

/// The floors that were building pads (by their keys in the file) excavating each toposolid on their level they lie in,
/// as the pad cut it.
pub fn excavate_with_pads(m: &mut Model, keys: &[String]) {
    let pads: Vec<(Guid, Floor)> = keys
        .iter()
        .filter_map(|k| k.parse::<Id>().ok())
        .filter_map(|id| match m.elements.get(id) {
            Some(Element::Floor(f)) => Some((m.guid(id), f.clone())),
            _ => None,
        })
        .collect();
    for (g, pad) in pads {
        for e in m.elements.values_mut() {
            let Element::Toposolid(t) = e else { continue };
            let inside = t.level == pad.level
                && pad.boundary.iter().all(|q| geom::contains(&t.boundary, *q))
                && !t.boundary.iter().any(|q| geom::contains(&pad.boundary, *q));
            if inside && !t.excavated_by.contains(&g) {
                t.excavated_by.push(g);
            }
        }
    }
}

/// A georeference's numbers of a file from before made the survey point (at the internal origin, its shared coordinates
/// the map point of the origin) and the project base point (its angle to true north the map's rotation), when the
/// project has neither: the points are where they are kept from then on, the georeference holding the map's name.
pub fn points_from_georeference(m: &mut Model) {
    let Some(g) = m.georeference.clone() else { return };
    if m.survey_point().is_some() || m.project_base_point().is_some() {
        return;
    }
    m.insert(Element::SurveyPoint(BasePoint { east: g.eastings, north: g.northings, height: g.height, clipped: true, ..BasePoint::default() }));
    m.insert(Element::ProjectBasePoint(BasePoint { angle: g.rotation, clipped: true, ..BasePoint::default() }));
}

// ====================================================================== the property line's table
/// A survey bearing as an azimuth, degrees clockwise from north: `N 45°30'00" E`, `N45-30-00E`, `S 12.5 W`, or a number.
pub fn bearing(v: &Value) -> Option<f64> {
    if let Some(a) = v.as_f64() {
        return a.is_finite().then(|| a.rem_euclid(360.0));
    }
    let t = v.as_str()?.trim().to_ascii_uppercase();
    if t.chars().count() > 64 {
        return None;
    }
    let (first, rest) = (t.chars().next()?, t.get(1..)?);
    let last = rest.trim_end().chars().last()?;
    let middle = rest.trim_end().strip_suffix(last)?.trim();
    if !matches!(first, 'N' | 'S') || !matches!(last, 'E' | 'W') {
        return None;
    }
    // Degrees, minutes and seconds by any marks between them.
    let nums: Vec<f64> = middle
        .split(|ch: char| !(ch.is_ascii_digit() || ch == '.'))
        .filter(|x| !x.is_empty())
        .map(str::parse::<f64>)
        .collect::<Result<_, _>>()
        .ok()?;
    let deg = match nums.as_slice() {
        [d] => *d,
        [d, m] => d + m / 60.0,
        [d, m, s] => d + m / 60.0 + s / 3600.0,
        _ => return None,
    };
    if !(0.0..=90.0).contains(&deg) {
        return None;
    }
    Some(match (first, last) {
        ('N', 'E') => deg,
        ('S', 'E') => 180.0 - deg,
        ('S', 'W') => 180.0 + deg,
        _ => (360.0 - deg) % 360.0,
    })
}

/// An azimuth as a survey bearing, `N 45°30'00" E`.
pub fn bearing_text(az: f64) -> String {
    let az = az.rem_euclid(360.0);
    let (a, b, d) = if az <= 90.0 {
        ('N', 'E', az)
    } else if az <= 180.0 {
        ('S', 'E', 180.0 - az)
    } else if az <= 270.0 {
        ('S', 'W', az - 180.0)
    } else {
        ('N', 'W', 360.0 - az)
    };
    let total = (d * 3600.0).round() as i64;
    format!("{a} {}°{:02}'{:02}\" {b}", total / 3600, (total / 60) % 60, total % 60)
}

/// A property line's segments as Revit's Edit Table shows them: distance (an arc's chord), bearing, line or arc, radius,
/// side.
pub fn table_of(m: &Model, pl: &crate::masterplan::PropertyLine) -> Vec<Value> {
    let theta = m.angle_to_true_north().to_radians();
    let n = pl.points.len();
    (0..n)
        .map(|i| {
            let (a, b) = (pl.points[i], pl.points[(i + 1) % n]);
            let d = P2(b[0] - a[0], b[1] - a[1]);
            let len = (d.0 * d.0 + d.1 * d.1).sqrt();
            let (sx, sy) = (d.0 * theta.cos() - d.1 * theta.sin(), d.0 * theta.sin() + d.1 * theta.cos());
            let az = sx.atan2(sy).to_degrees().rem_euclid(360.0);
            let r = |v: f64| (v * 100.0).round() / 100.0;
            let mut row = json!({"distance": r(len), "bearing": bearing_text(az), "azimuth": (az * 1e6).round() / 1e6, "type": "line"});
            if a[2].abs() > 1e-12 {
                let theta4 = 4.0 * a[2].abs().atan();
                let radius = len / (2.0 * (theta4 / 2.0).sin()).max(1e-12);
                row["type"] = json!("arc");
                row["radius"] = json!(r(radius));
                row["side"] = json!(if a[2] > 0.0 { "L" } else { "R" });
            }
            row
        })
        .collect()
}

