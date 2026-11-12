//! glTF 2.0 binary (.glb) export of the 3D scene, for Blender and other viewers.
//!
//! What the 3D view draws is what the file holds: the triangles of the [`Scene`] (the ones the processor's picture and the card's
//! mesh are made from), as the default 3D view shows them, so its phase, its design options and what it hides are followed and a
//! section box is not. One node and one mesh for each element, the node named by the element's id, with its GUID and category in
//! `extras`; a mesh has a primitive for each colour the element is shaded in. A surface becomes a PBR material of its base
//! colour (the scene's sRGB colour, in linear light as glTF takes it), blended where the scene sees through it, and double
//! sided, since the scene draws both sides of everything. Textures, edges, levels and point clouds are not written.
//!
//! BIMCraft is z up in millimetres and glTF y up in metres: a point `(x, y, z)` mm is written at `(x, z, -y)` thousandths, a
//! turn about the east axis and not a mirror, so winding and handedness are kept. A .glb is a 12-byte header, a JSON chunk and a
//! binary chunk, each 4-byte aligned; nothing here needs a glTF library.

use std::collections::{BTreeMap, HashMap};

use serde_json::{Value, json};

use crate::geom::v3::{P3, dot};
use crate::model::{Id, Model};
use crate::prim::Rgb;
use crate::view3d::{Facet, Scene};

/// The most triangles a file holds, which keeps a file (at most 84 bytes a triangle) well under the size a project file may be.
/// The card draws a mesh of any size now, so the limit is the file's own.
pub const MAX_TRIS: usize = 1_600_000;

/// What an export wrote.
pub struct Export {
    /// The .glb file.
    pub bytes: Vec<u8>,
    /// How many elements it holds, a node each.
    pub nodes: usize,
    /// How many triangles it holds. Those with no area are not written.
    pub triangles: usize,
}

/// Whether a scene of `n` triangles can be written, and if not, why.
pub fn check_count(n: usize) -> Result<(), String> {
    if n == 0 {
        return Err(
            "the default 3D view shows nothing to export: draw something, or show again what Visibility/Graphics hides there".into()
        );
    }
    if n > MAX_TRIS {
        return Err(format!(
            "the default 3D view shows {n} triangles, more than a glTF file is given ({MAX_TRIS}); hide some of the model there (Visibility/Graphics, a phase or a design option) and export again"
        ));
    }
    Ok(())
}

/// The scene of `m` that the default 3D view shows, as a .glb.
pub fn export(m: &Model) -> Result<Export, String> {
    let scene = Scene::new(m);
    let look = crate::graphics::Look::of_3d(m);
    check_count(scene.shown(&look))?;
    let mut by_owner: BTreeMap<Id, Vec<Facet>> = BTreeMap::new();
    for f in scene.facets(&look) {
        by_owner.entry(f.owner).or_default().push(f);
    }
    let mut file = File::default();
    for (id, facets) in by_owner {
        file.element(m, id, &facets);
    }
    if file.triangles == 0 {
        return Err("the default 3D view shows only surfaces with no area, nothing to export".into());
    }
    let (nodes, triangles) = (file.nodes.len(), file.triangles);
    Ok(Export { bytes: file.finish()?, nodes, triangles })
}

/// A point in glTF's axes and metres: `(x, y, z)` is written at `(x, z, -y)`, a quarter turn about x; `k` scales it.
fn turned(p: P3, k: f64) -> [f32; 3] {
    // `+ 0.0` makes a negative zero a zero.
    [(p[0] * k + 0.0) as f32, (p[2] * k + 0.0) as f32, (-p[1] * k + 0.0) as f32]
}

/// A colour's channel from sRGB, as glTF's factors are, in linear light.
fn linear(c: u8) -> f64 {
    let c = f64::from(c) / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

/// The triangles of one element in one material.
#[derive(Default)]
struct Primitive {
    positions: Vec<f32>,
    normals: Vec<f32>,
    indices: Vec<u32>,
    /// A vertex's place, by its position and normal: corners that agree are one vertex.
    shared: HashMap<[u32; 6], u32>,
}

impl Primitive {
    fn corner(&mut self, position: [f32; 3], normal: [f32; 3]) {
        let key = [position[0], position[1], position[2], normal[0], normal[1], normal[2]].map(f32::to_bits);
        let at = *self.shared.entry(key).or_insert_with(|| {
            self.positions.extend(position);
            self.normals.extend(normal);
            (self.positions.len() / 3 - 1) as u32
        });
        self.indices.push(at);
    }
}

/// The file as it is put together: the binary chunk's three runs (positions, normals, indices), and what the JSON lists.
#[derive(Default)]
struct File {
    positions: Vec<u8>,
    normals: Vec<u8>,
    indices: Vec<u8>,
    accessors: Vec<Value>,
    materials: Vec<Value>,
    material_of: HashMap<([u8; 3], u8), usize>,
    meshes: Vec<Value>,
    nodes: Vec<Value>,
    triangles: usize,
}

/// The buffer views the runs are: positions, normals, indices.
const POSITIONS: usize = 0;
const NORMALS: usize = 1;
const INDICES: usize = 2;
const FLOAT: u32 = 5126;
const UNSIGNED_INT: u32 = 5125;
const ARRAY_BUFFER: u32 = 34962;
const ELEMENT_ARRAY_BUFFER: u32 = 34963;

impl File {
    /// The material of a surface of `color` and `clearness` percent, made once.
    fn material(&mut self, Rgb(r, g, b): Rgb, clearness: u8) -> usize {
        if let Some(i) = self.material_of.get(&([r, g, b], clearness)) {
            return *i;
        }
        let alpha = 1.0 - f64::from(clearness) / 100.0;
        let mut name = format!("#{r:02x}{g:02x}{b:02x}");
        if clearness > 0 {
            name.push_str(&format!(" {clearness}% clear"));
        }
        let mut material = json!({
            "name": name,
            "pbrMetallicRoughness": {"baseColorFactor": [linear(r), linear(g), linear(b), alpha], "metallicFactor": 0.0, "roughnessFactor": 0.9},
            "doubleSided": true,
        });
        if clearness > 0 {
            material["alphaMode"] = json!("BLEND");
        }
        self.materials.push(material);
        self.material_of.insert(([r, g, b], clearness), self.materials.len() - 1);
        self.materials.len() - 1
    }

    fn accessor(&mut self, view: usize, offset: usize, kind: &str, component: u32, count: usize) -> usize {
        self.accessors.push(json!({"bufferView": view, "byteOffset": offset, "componentType": component, "count": count, "type": kind}));
        self.accessors.len() - 1
    }

    /// Element `id` as a mesh and a node.
    fn element(&mut self, m: &Model, id: Id, facets: &[Facet]) {
        let mut parts: BTreeMap<([u8; 3], u8), Primitive> = BTreeMap::new();
        for f in facets {
            // A triangle with no area, or a corner out of range, draws nothing; and a normal of nothing is not a normal.
            let flat = f.corners.iter().flatten().all(|v| v.is_finite()) && f.normals.iter().all(|n| dot(*n, *n) > 0.5);
            if !flat {
                continue;
            }
            let Rgb(r, g, b) = f.color;
            let part = parts.entry(([r, g, b], f.clearness)).or_default();
            for (p, n) in f.corners.iter().zip(&f.normals) {
                part.corner(turned(*p, 0.001), turned(*n, 1.0));
            }
        }
        let mut primitives = Vec::new();
        for ((rgb, clearness), part) in parts {
            let material = self.material(Rgb(rgb[0], rgb[1], rgb[2]), clearness);
            let (at, normal_at, index_at) = (self.positions.len(), self.normals.len(), self.indices.len());
            self.positions.extend(part.positions.iter().flat_map(|v| v.to_le_bytes()));
            self.normals.extend(part.normals.iter().flat_map(|v| v.to_le_bytes()));
            self.indices.extend(part.indices.iter().flat_map(|v| v.to_le_bytes()));
            let count = part.positions.len() / 3;
            let (position, normal, indices) = (
                self.accessor(POSITIONS, at, "VEC3", FLOAT, count),
                self.accessor(NORMALS, normal_at, "VEC3", FLOAT, count),
                self.accessor(INDICES, index_at, "SCALAR", UNSIGNED_INT, part.indices.len()),
            );
            // The bounds a position accessor must state, of the numbers as they are written.
            let bound = |pick: fn(f32, f32) -> f32, start: f32| -> Vec<f64> {
                (0..3).map(|k| f64::from(part.positions.iter().skip(k).step_by(3).copied().fold(start, pick))).collect()
            };
            if let Some(a) = self.accessors.get_mut(position) {
                a["min"] = json!(bound(f32::min, f32::INFINITY));
                a["max"] = json!(bound(f32::max, f32::NEG_INFINITY));
            }
            self.triangles += part.indices.len() / 3;
            primitives
                .push(json!({"attributes": {"POSITION": position, "NORMAL": normal}, "indices": indices, "material": material, "mode": 4}));
        }
        if primitives.is_empty() {
            return;
        }
        let mut extras = json!({"id": id, "guid": m.guid(id).to_string(), "category": m.elements.get(id).map_or("", |e| e.category())});
        if crate::link::is_linked(id) {
            extras["linked"] = json!(true);
        }
        let name = id.to_string();
        self.meshes.push(json!({"name": name, "primitives": primitives}));
        self.nodes.push(json!({"name": name, "mesh": self.meshes.len() - 1, "extras": extras}));
    }

    /// The .glb: the header, the JSON chunk and the binary chunk.
    fn finish(self) -> Result<Vec<u8>, String> {
        let mut bin = self.positions;
        let (normals_at, indices_at) = (bin.len(), bin.len() + self.normals.len());
        bin.extend(&self.normals);
        bin.extend(&self.indices);
        let view = |start: usize, end: usize, target: u32| {
            let mut v = json!({"buffer": 0, "byteOffset": start, "byteLength": end - start, "target": target});
            if target == ARRAY_BUFFER {
                // Tightly packed, which the stride says outright as several accessors read the one view.
                v["byteStride"] = json!(12);
            }
            v
        };
        let document = json!({
            "asset": {
                "version": "2.0",
                "generator": concat!("BIMCraft ", env!("CARGO_PKG_VERSION")),
                "extras": {"units": "metres", "up": "+Y, BIMCraft's +Z", "source": "the default 3D view"},
            },
            "scene": 0,
            "scenes": [{"nodes": (0..self.nodes.len()).collect::<Vec<_>>()}],
            "nodes": self.nodes,
            "meshes": self.meshes,
            "materials": self.materials,
            "accessors": self.accessors,
            "bufferViews": [
                view(0, normals_at, ARRAY_BUFFER),
                view(normals_at, indices_at, ARRAY_BUFFER),
                view(indices_at, bin.len(), ELEMENT_ARRAY_BUFFER),
            ],
            "buffers": [{"byteLength": bin.len()}],
        });
        let mut json = serde_json::to_vec(&document).map_err(|e| format!("glTF: {e}"))?;
        json.resize(json.len().next_multiple_of(4), b' ');
        bin.resize(bin.len().next_multiple_of(4), 0);
        let total = 12 + 8 + json.len() + 8 + bin.len();
        let too_big = || format!("glTF: {total} bytes, more than a glb holds (4 GiB)");
        let (total32, json32, bin32) = (
            u32::try_from(total).map_err(|_| too_big())?,
            u32::try_from(json.len()).map_err(|_| too_big())?,
            u32::try_from(bin.len()).map_err(|_| too_big())?,
        );
        let mut out = Vec::with_capacity(total);
        out.extend(b"glTF");
        out.extend(2u32.to_le_bytes());
        out.extend(total32.to_le_bytes());
        out.extend(json32.to_le_bytes());
        out.extend(b"JSON");
        out.extend(json);
        out.extend(bin32.to_le_bytes());
        out.extend(b"BIN\0");
        out.extend(bin);
        Ok(out)
    }
}
