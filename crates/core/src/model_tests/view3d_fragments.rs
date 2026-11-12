//! `view3d::fragments`'s tests that drive the Session (moved out of `view3d/fragments.rs`).

use crate::view3d::*;
use std::sync::Arc;

use crate::engine::Session;
use serde_json::json;

#[test]
fn paint_and_texture_ordinals_refresh_without_rebuilding_retained_run_geometry() {
    let mut s = Session::new();
    s.execute("material", &json!({"name":"Probe", "texture":{"kind":"brick"}})).unwrap();
    let id = s.execute("pipe", &json!({"points":[[0,0],[3000,1000]]})).unwrap()["ids"][0].as_u64().unwrap();
    let name = s.model.elements.get(id).unwrap().run().unwrap().type_name.clone();
    s.model_mut().types.runs.get_mut("pipe").unwrap().get_mut(&name).unwrap().material = "Probe".into();
    let mut cache = SectionCache::default();
    cache.work(&s.model);
    let before = Arc::clone(&cache.entries[&id].fragment);
    let mut material = s.model.materials["Probe"].clone();
    material.color = [12, 34, 56];
    material.transparency = 40;
    s.model_mut().materials.insert("Probe".into(), material.clone());
    s.model_mut().materials.insert("AAA earlier texture".into(), material);
    cache.work(&s.model);
    assert_eq!(cache.stats().built, 1);
    assert!(Arc::ptr_eq(&before, &cache.entries[&id].fragment));
    let m = &s.model;
    let e = m.elements.get(id).unwrap();
    let mut retained = Vec::new();
    for solid in cache.entries[&id].fragment.placed(m, e) {
        solid.painted().tris(&mut retained);
    }
    let mut fresh = Vec::new();
    for solid in solids_in_with(m, &[(id, e)], None, &mut crate::component_geometry::Components::new(m)) {
        solid.tris(&mut fresh);
    }
    assert_eq!(retained, fresh, "geometry, colour, transparency and shifted texture ids");
}

#[test]
fn eviction_releases_cache_geometry_after_the_last_completed_work_is_dropped() {
    let mut s = Session::new();
    let id = s.execute("column", &json!({"at":[0,0]})).unwrap()["id"].as_u64().unwrap();
    let mut cache = SectionCache::default();
    let work = cache.work(&s.model);
    let geometry = Arc::downgrade(&cache.entries[&id].fragment.solids[0]);
    cache.set_budget(0);
    assert_eq!(cache.stats().entries, 0);
    assert!(geometry.upgrade().is_some(), "completed work keeps its geometry valid");
    drop(work);
    assert!(geometry.upgrade().is_none(), "evicted geometry must not survive the last work owner");
}
