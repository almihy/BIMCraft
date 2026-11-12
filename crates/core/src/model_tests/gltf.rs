//! `gltf`'s tests that drive the Session (moved out of `gltf.rs`).

use crate::engine::Session;
use crate::gltf::*;
use serde_json::{Value, json};

/// A .glb taken apart: its JSON, and its binary chunk. Fails the test where the framing is wrong.
fn parts(bytes: &[u8]) -> (Value, Vec<u8>) {
    let word = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
    assert_eq!(&bytes[..4], b"glTF");
    assert_eq!(word(4), 2, "version");
    assert_eq!(word(8), bytes.len(), "the header's length is the file's");
    let (json_len, json_type) = (word(12), &bytes[16..20]);
    assert_eq!(json_type, b"JSON");
    assert_eq!(json_len % 4, 0, "the JSON chunk is 4-byte aligned");
    let json = &bytes[20..20 + json_len];
    assert!(json.ends_with(b"}") || json.ends_with(b" "), "padded with spaces");
    let bin_at = 20 + json_len;
    let (bin_len, bin_type) = (word(bin_at), &bytes[bin_at + 4..bin_at + 8]);
    assert_eq!(bin_type, b"BIN\0");
    assert_eq!(bin_len % 4, 0, "the binary chunk is 4-byte aligned");
    assert_eq!(bin_at + 8 + bin_len, bytes.len(), "two chunks and nothing after");
    (serde_json::from_slice(json).unwrap(), bytes[bin_at + 8..].to_vec())
}

/// Mass `boundary` and `height` in a fresh project: a straight prism of 12 triangles.
fn boxes(sizes: &[(f64, f64, f64)]) -> (Session, Vec<u64>) {
    let mut s = Session::new();
    let ids = sizes
        .iter()
        .enumerate()
        .map(|(k, (w, d, h))| {
            let x = k as f64 * 10_000.0;
            let boundary = json!([[x, 0.0], [x + w, 0.0], [x + w, *d], [x, *d]]);
            s.execute("mass", &json!({"boundary": boundary, "height": h})).unwrap()["id"].as_u64().unwrap()
        })
        .collect();
    (s, ids)
}

fn floats(bin: &[u8], json: &Value, accessor: u64) -> Vec<f32> {
    let a = &json["accessors"][accessor as usize];
    let view = &json["bufferViews"][a["bufferView"].as_u64().unwrap() as usize];
    let at = view["byteOffset"].as_u64().unwrap_or(0) as usize + a["byteOffset"].as_u64().unwrap_or(0) as usize;
    let n = a["count"].as_u64().unwrap() as usize * if a["type"] == "VEC3" { 3 } else { 1 };
    bin[at..at + 4 * n].chunks(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect()
}

fn indices(bin: &[u8], json: &Value, accessor: u64) -> Vec<u32> {
    let a = &json["accessors"][accessor as usize];
    assert_eq!(a["componentType"], 5125, "unsigned int");
    let view = &json["bufferViews"][a["bufferView"].as_u64().unwrap() as usize];
    let at = view["byteOffset"].as_u64().unwrap_or(0) as usize + a["byteOffset"].as_u64().unwrap_or(0) as usize;
    let n = a["count"].as_u64().unwrap() as usize;
    bin[at..at + 4 * n].chunks(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect()
}

#[test]
fn a_glb_has_its_header_and_two_aligned_chunks() {
    let (s, _) = boxes(&[(4000.0, 2000.0, 3000.0)]);
    let out = export(&s.model).unwrap();
    let (json, bin) = parts(&out.bytes);
    assert_eq!(json["asset"]["version"], "2.0");
    assert_eq!(json["buffers"][0]["byteLength"].as_u64().unwrap() as usize, bin.len(), "no padding in the buffer's own length");
}

#[test]
fn each_element_is_a_node_named_by_its_id_with_its_guid_and_category() {
    let (s, ids) = boxes(&[(4000.0, 2000.0, 3000.0), (1000.0, 1000.0, 1000.0)]);
    let out = export(&s.model).unwrap();
    let (json, bin) = parts(&out.bytes);
    let nodes = json["nodes"].as_array().unwrap();
    assert_eq!(nodes.len(), 2);
    assert_eq!(json["meshes"].as_array().unwrap().len(), 2);
    assert_eq!(json["scenes"][0]["nodes"], json!([0, 1]));
    for (node, id) in nodes.iter().zip(&ids) {
        assert_eq!(node["name"], id.to_string());
        assert_eq!(node["extras"]["id"], *id);
        assert_eq!(node["extras"]["guid"], s.model.guid(*id).to_string());
        assert_eq!(node["extras"]["category"], "mass");
    }
    // 12 triangles a box, and the file counts what it holds.
    let held: usize = json["meshes"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|m| m["primitives"].as_array().unwrap())
        .map(|p| indices(&bin, &json, p["indices"].as_u64().unwrap()).len() / 3)
        .sum();
    assert_eq!((held, out.triangles, out.nodes), (24, 24, 2));
}

#[test]
fn a_known_box_comes_out_in_metres_with_y_up_and_turned_not_mirrored() {
    // 4 m east, 2 m north, 3 m up, from the origin.
    let (s, _) = boxes(&[(4000.0, 2000.0, 3000.0)]);
    let (json, bin) = parts(&export(&s.model).unwrap().bytes);
    let p = &json["meshes"][0]["primitives"][0];
    let at = floats(&bin, &json, p["attributes"]["POSITION"].as_u64().unwrap());
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    for v in at.chunks(3) {
        for k in 0..3 {
            lo[k] = lo[k].min(v[k]);
            hi[k] = hi[k].max(v[k]);
        }
    }
    // East stays x; up is y; north, away from the viewer who looks down -z, is -z.
    assert_eq!((lo, hi), ([0.0, 0.0, -2.0], [4.0, 3.0, 0.0]));
    let a = &json["accessors"][p["attributes"]["POSITION"].as_u64().unwrap() as usize];
    assert_eq!((&a["min"], &a["max"]), (&json!([0.0, 0.0, -2.0]), &json!([4.0, 3.0, 0.0])));
    // A rotation keeps the winding: the triangles still enclose a positive volume, 4 x 2 x 3 m.
    let idx = indices(&bin, &json, p["indices"].as_u64().unwrap());
    let v = |i: u32| -> [f64; 3] { std::array::from_fn(|k| f64::from(at[3 * i as usize + k])) };
    let volume: f64 = idx
        .chunks(3)
        .map(|t| {
            let (a, b, c) = (v(t[0]), v(t[1]), v(t[2]));
            (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0]) + a[2] * (b[0] * c[1] - b[1] * c[0])) / 6.0
        })
        .sum();
    assert!((volume - 24.0).abs() < 1e-4, "{volume}");
    // Normals are unit and point out of the box: up on its top.
    let normals = floats(&bin, &json, p["attributes"]["NORMAL"].as_u64().unwrap());
    assert!(normals.chunks(3).all(|n| (n.iter().map(|x| x * x).sum::<f32>().sqrt() - 1.0).abs() < 1e-5));
    assert!(normals.chunks(3).any(|n| n == [0.0, 1.0, 0.0]), "a face looks up");
}

#[test]
fn a_surface_takes_its_colour_in_linear_light_and_what_the_view_sees_through_blends() {
    let (mut s, ids) = boxes(&[(1000.0, 1000.0, 1000.0), (1000.0, 1000.0, 1000.0)]);
    // A mass is seen through by default (60%); the second's view override makes it 80%.
    s.execute("graphics", &json!({"view": "{3D}", "elements": {ids[1].to_string(): {"transparency": 80}}})).unwrap();
    let (json, _) = parts(&export(&s.model).unwrap().bytes);
    let material = |node: usize| {
        let m = json["meshes"][node]["primitives"][0]["material"].as_u64().unwrap() as usize;
        json["materials"][m].clone()
    };
    let factor = |m: &Value| m["pbrMetallicRoughness"]["baseColorFactor"].as_array().unwrap().clone();
    let (first, second) = (factor(&material(0)), factor(&material(1)));
    // The mass is (158, 190, 224) in the scene, an sRGB colour; the file holds it linear.
    let back = |c: &Value| (c.as_f64().unwrap().powf(1.0 / 2.4) * 1.055 - 0.055) * 255.0;
    for (c, want) in first.iter().zip([158.0, 190.0, 224.0]) {
        assert!((back(c) - want).abs() < 0.6, "{first:?}");
    }
    assert!((first[3].as_f64().unwrap() - 0.4).abs() < 1e-9 && (second[3].as_f64().unwrap() - 0.2).abs() < 1e-9);
    assert!([material(0), material(1)].iter().all(|m| m["alphaMode"] == "BLEND"));
    // The scene draws both sides of every surface, so the file does.
    assert!(json["materials"].as_array().unwrap().iter().all(|m| m["doubleSided"] == true));
}

#[test]
fn what_the_view_does_not_see_through_stays_opaque() {
    let mut s = Session::new();
    s.execute("floor", &json!({"boundary": [[0, 0], [4000, 0], [4000, 3000], [0, 3000]]})).unwrap();
    let (json, _) = parts(&export(&s.model).unwrap().bytes);
    let materials = json["materials"].as_array().unwrap();
    assert!(!materials.is_empty());
    for m in materials {
        assert_eq!(m["pbrMetallicRoughness"]["baseColorFactor"][3], 1.0, "{m}");
        assert_eq!(m.get("alphaMode"), None, "opaque is glTF's default");
    }
}

#[test]
fn what_the_default_3d_view_hides_is_left_out() {
    let (mut s, ids) = boxes(&[(1000.0, 1000.0, 1000.0), (2000.0, 1000.0, 1000.0)]);
    s.execute("graphics", &json!({"view": "{3D}", "hide_elements": [ids[0]]})).unwrap();
    let out = export(&s.model).unwrap();
    let (json, _) = parts(&out.bytes);
    assert_eq!(json["nodes"].as_array().unwrap().len(), 1);
    assert_eq!(json["nodes"][0]["name"], ids[1].to_string());
    assert_eq!(out.nodes, 1);
    // Everything hidden is nothing to write, and the error says so.
    s.execute("graphics", &json!({"view": "{3D}", "hide_elements": [ids[0], ids[1]]})).unwrap();
    let e = export(&s.model).err().unwrap();
    assert!(e.contains("nothing"), "{e}");
}

#[test]
fn the_default_3d_views_phase_and_design_options_decide_what_is_written() {
    let named = |s: &Session| -> Vec<String> {
        let (json, _) = parts(&export(&s.model).unwrap().bytes);
        json["nodes"].as_array().unwrap().iter().map(|n| n["name"].as_str().unwrap().to_string()).collect()
    };
    // Two masses in the options of a set: the primary's shows until the view chooses the other.
    let (mut s, ids) = boxes(&[(1000.0, 1000.0, 1000.0), (1000.0, 1000.0, 1000.0)]);
    s.execute("option", &json!({"set": "Facade", "options": ["Brick", "Glass"]})).unwrap();
    s.execute("modify", &json!({"ids": [ids[0]], "params": {"Design Option": "Facade: Brick"}})).unwrap();
    s.execute("modify", &json!({"ids": [ids[1]], "params": {"Design Option": "Facade: Glass"}})).unwrap();
    assert_eq!(named(&s), [ids[0].to_string()]);
    s.execute("graphics", &json!({"view": "{3D}", "options": {"Facade": "Glass"}})).unwrap();
    assert_eq!(named(&s), [ids[1].to_string()]);
    // A mass made in the second phase is not in a 3D view of the first.
    let mut s = Session::new();
    let first = s.execute("mass", &json!({"boundary": [[0, 0], [1000, 0], [1000, 1000], [0, 1000]], "height": 1000})).unwrap()["id"]
        .as_u64()
        .unwrap();
    s.execute("phase", &json!({"now": "New Construction"})).unwrap();
    let later = s.execute("mass", &json!({"boundary": [[3000, 0], [4000, 0], [4000, 1000], [3000, 1000]], "height": 1000})).unwrap()["id"]
        .as_u64()
        .unwrap();
    assert_eq!(named(&s), [first.to_string(), later.to_string()]);
    s.execute("graphics", &json!({"view": "{3D}", "phase": "Existing"})).unwrap();
    assert_eq!(named(&s), [first.to_string()]);
}

#[test]
fn the_sample_house_writes_a_node_for_each_element_the_scene_draws() {
    let mut s = Session::new();
    s.script(crate::sample::SAMPLE).unwrap();
    let out = export(&s.model).unwrap();
    let (json, bin) = parts(&out.bytes);
    let scene = crate::view3d::Scene::new(&s.model);
    let look = crate::graphics::Look::of_3d(&s.model);
    let owners: std::collections::BTreeSet<u64> = scene.facets(&look).map(|f| f.owner).collect();
    let named: std::collections::BTreeSet<u64> =
        json["nodes"].as_array().unwrap().iter().map(|n| n["name"].as_str().unwrap().parse().unwrap()).collect();
    assert_eq!(named, owners);
    assert!(out.triangles > 100 && out.triangles <= scene.shown(&look), "{}", out.triangles);
    // Every index names a vertex of its own primitive.
    for p in json["meshes"].as_array().unwrap().iter().flat_map(|m| m["primitives"].as_array().unwrap()) {
        let count = json["accessors"][p["attributes"]["POSITION"].as_u64().unwrap() as usize]["count"].as_u64().unwrap() as u32;
        assert!(indices(&bin, &json, p["indices"].as_u64().unwrap()).iter().all(|i| *i < count));
    }
}

#[test]
fn saving_as_glb_writes_the_file_through_the_command() {
    let (mut s, _) = boxes(&[(4000.0, 2000.0, 3000.0)]);
    let dir = std::env::temp_dir().join(format!("bimcraft-gltf-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("scene.GLB");
    let r = s.execute("save", &json!({"path": path})).unwrap();
    assert_eq!((r["format"].as_str(), r["triangles"].as_u64(), r["elements"].as_u64()), (Some("glTF 2.0"), Some(12), Some(1)));
    let bytes = std::fs::read(&path).unwrap();
    parts(&bytes);
    assert_eq!(s.path, None, "an export is not the project's file");
    std::fs::remove_dir_all(&dir).unwrap();
}
