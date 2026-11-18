//! `gltf`'s tests that need only the model.

use crate::gltf::*;

#[test]
fn a_scene_past_the_triangle_cap_is_refused_with_how_many_there_are() {
    let e = check_count(MAX_TRIS + 1).unwrap_err();
    assert!(e.contains(&(MAX_TRIS + 1).to_string()) && e.contains(&MAX_TRIS.to_string()), "{e}");
    assert!(check_count(MAX_TRIS).is_ok());
    assert!(check_count(0).unwrap_err().contains("nothing"));
}
