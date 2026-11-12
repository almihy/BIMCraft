use super::testing::*;
use crate::engine::Session;
use crate::geom::P2;
use crate::model::Model;
use crate::prim::Rgb;
use crate::render;
use serde_json::json;

#[test]
fn fill_overrides_draw_two_layers_in_cut_and_projection_and_keep_library_references() {
    let mut s = Session::new();
    house(&mut s);
    run(&mut s, "floor", json!({"boundary": [[1000,1000],[7000,1000],[7000,5000],[1000,5000]]}));
    let style = json!({
        "foreground": {"pattern": "Horizontal", "color": [220,0,0]},
        "background": {"pattern": "Solid fill", "color": [30,180,50]}
    });
    run(
        &mut s,
        "graphics",
        json!({"view": "Level 1", "categories": {
            "wall": {"cut": style}, "floor": {"projection": style}
        }}),
    );
    let d = plan(&s, 100000);
    assert_eq!(pixel(&d, P2(4000.0, 20.0)), Rgb(30, 180, 50));
    let pm = tiny_skia::Pixmap::decode_png(&render::png(&d).unwrap()).unwrap();
    assert!(painted(&pm, |r, g, b| r > 180 && g < 60 && b < 60) > 200);
    assert_eq!(pixel(&d, P2(4000.0, 3020.0)), Rgb(30, 180, 50));
    run(&mut s, "fill_pattern", json!({"name": "Horizontal", "rename": "Courses"}));
    assert!(s.model.to_json().unwrap().contains("Courses"));
    let saved = s.model.to_json().unwrap();
    let (back, left) = Model::read(&saved).unwrap();
    assert!(left.is_empty(), "{left:?}");
    assert_eq!(back, *s.model);
    let before = s.model.clone();
    assert!(s.execute("fill_pattern", &json!({"name":"Courses", "delete":true})).is_err());
    assert_eq!(s.model, before);
    assert!(s.execute("graphics", &json!({"categories":{"wall":{"cut":{"foreground":{"pattern":"missing"}}}}})).is_err());
}

#[test]
fn fill_overrides_resolve_each_layer_through_templates_filters_and_elements_and_undo() {
    let mut s = Session::new();
    let wall = house(&mut s)[0];
    run(&mut s, "filter", json!({"name":"Walls", "categories":["wall"]}));
    run(
        &mut s,
        "graphics",
        json!({"categories":{"wall":{"cut":{
        "foreground":{"pattern":"Horizontal","color":[200,0,0]},
        "background":{"pattern":"Solid fill","color":[0,200,0]}
    }}}, "filters":[{"filter":"Walls","cut":{"foreground":{"pattern":"Vertical","color":[0,0,200]}}}]}),
    );
    run(&mut s, "template", json!({"name":"Pattern view", "from":100000, "controls":["categories","filters"]}));
    run(&mut s, "graphics", json!({"reset":true,"template":"Pattern view"}));
    let inherited = render::png(&plan(&s, 100000)).unwrap();
    run(&mut s, "graphics", json!({"elements":{wall.to_string():{"cut":{"background":{"pattern":"Solid fill","color":[220,220,0]}}}}}));
    let d = plan(&s, 100000);
    let pm = tiny_skia::Pixmap::decode_png(&render::png(&d).unwrap()).unwrap();
    assert!(painted(&pm, |r, g, b| r > 180 && g > 180 && b < 40) > 100, "the element's background");
    assert!(painted(&pm, |r, g, b| r < 40 && g < 40 && b > 160) > 100, "the filter's foreground");
    assert_eq!(painted(&pm, |r, g, b| r > 160 && g < 40 && b < 40), 0, "the category's foreground is replaced");
    run(&mut s, "undo", json!({}));
    assert_eq!(render::png(&plan(&s, 100000)).unwrap(), inherited);
    assert!(
        s.execute("graphics", &json!({"categories":{"wall":{"cut":{"background":{"pattern":null}}}}})).is_err(),
        "the template controls categories"
    );
    run(&mut s, "fill_pattern", json!({"name":"Vertical","rename":"Upright"}));
    assert_eq!(
        s.model.view_templates["Pattern view"].graphics.filters[0].over.cut.foreground.as_ref().unwrap().pattern.as_deref(),
        Some("Upright")
    );
    assert!(s.execute("fill_pattern", &json!({"name":"Upright","delete":true})).is_err());
}

#[test]
fn fill_overrides_render_section_cuts_and_regions_with_holes_without_changing_the_type() {
    let mut s = Session::new();
    house(&mut s);
    let section = run(&mut s, "section", json!({"start":[-1000,3000],"end":[9000,3000]}))["id"].as_u64().unwrap();
    run(
        &mut s,
        "graphics",
        json!({"view":section,"categories":{"wall":{"cut":{
            "foreground":{"pattern":"Diagonal Up","color":[220,0,0]},
            "background":{"pattern":"Solid fill","color":[20,180,40]}
        }}}}),
    );
    let view = render::section_fit(&s.model, section, 1000, 800);
    let d = render::section(&s.model, section, view);
    let pm = tiny_skia::Pixmap::decode_png(&render::png(&d).unwrap()).unwrap();
    // At this zoom a fine red stroke is thinner than a pixel and blends with the green background.
    assert!(painted(&pm, |r, g, b| r > 100 && r > g && b < 80) > 20);
    assert!(painted(&pm, |r, g, b| r < 60 && g > 140 && b < 80) > 100);
    run(
        &mut s,
        "filled_region",
        json!({"type":"Solid Black", "sketch":[
            [[1000,1000],[7000,1000]],[[7000,1000],[7000,5000]],[[7000,5000],[1000,5000]],[[1000,5000],[1000,1000]],
            [[3000,2000],[5000,2000]],[[5000,2000],[5000,4000]],[[5000,4000],[3000,4000]],[[3000,4000],[3000,2000]]
        ]}),
    );
    let types = s.model.region_types.clone();
    run(
        &mut s,
        "graphics",
        json!({"categories":{"region":{"projection":{
            "foreground":{"pattern":null}, "background":{"pattern":"Solid fill","color":[20,180,40]}
        }}}}),
    );
    let d = plan(&s, 100000);
    // Sample away from the section marker, which is annotation drawn over the region at y = 3000.
    assert_eq!(pixel(&d, P2(2000.0, 2800.0)), Rgb(20, 180, 40));
    assert_eq!(pixel(&d, P2(4000.0, 2800.0)), PAPER, "the hole stays open");
    assert_eq!(s.model.region_types, types);
    if let Ok(path) = std::env::var("BIMCRAFT_VG_PLAN_PREVIEW") {
        std::fs::write(path, render::png(&d).unwrap()).unwrap();
    }
}

#[test]
fn fill_overrides_none_suppresses_a_pattern_and_clear_restores_the_original_drawing() {
    let mut s = Session::new();
    house(&mut s);
    let before = render::png(&plan(&s, 100000)).unwrap();
    run(
        &mut s,
        "graphics",
        json!({"categories":{"wall":{"cut":{
            "foreground":{"pattern":null}, "background":{"pattern":null}
        }}}}),
    );
    assert_eq!(pixel(&plan(&s, 100000), P2(4000.0, 20.0)), PAPER);
    run(&mut s, "graphics", json!({"categories":{"wall":{"cut":{"foreground":null,"background":null}}}}));
    assert_eq!(render::png(&plan(&s, 100000)).unwrap(), before);
}
