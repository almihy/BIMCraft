//! `import`'s tests that drive the Session (moved out of `import.rs`).

use crate::geom::v3::{cross, dot};
use crate::geom::{self, P2};
use crate::model::{Element, Function, Guid, Id, Layer, MAX_POINTS, Model};
use crate::step::{self};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

use crate::import::*;

/// An IFC4 file in mm whose wall is a triangle mesh with no axis, running north, and whose
/// column takes its shape from its type through a mapped item; `site` places it all.
fn meshes(site: &str) -> String {
    let pts = "(0.,-100.,0.),(5000.,-100.,0.),(5000.,100.,0.),(0.,100.,0.),(0.,-100.,2700.),(5000.,-100.,2700.),(5000.,100.,2700.),(0.,100.,2700.)";
    let tris = "(1,3,2),(1,4,3),(5,6,7),(5,7,8),(1,2,6),(1,6,5),(2,3,7),(2,7,6),(3,4,8),(3,8,7),(4,1,5),(4,5,8)";
    format!(
        "ISO-10303-21;HEADER;FILE_SCHEMA(('IFC4'));ENDSEC;DATA;
#1=IFCSIUNIT(*,.LENGTHUNIT.,.MILLI.,.METRE.);
#2=IFCUNITASSIGNMENT((#1));
#3=IFCPROJECT('p',$,'Meshes',$,$,$,$,$,#2);
#4=IFCCARTESIANPOINT((0.,0.,0.));
#5=IFCAXIS2PLACEMENT3D(#4,$,$);
#6=IFCLOCALPLACEMENT(#62,#5);
#7=IFCBUILDINGSTOREY('s',$,'Level 0',$,$,#6,$,$,.ELEMENT.,0.);
#10=IFCCARTESIANPOINTLIST3D(({pts}));
#11=IFCTRIANGULATEDFACESET(#10,$,.T.,({tris}),$);
#12=IFCSHAPEREPRESENTATION($,'Body','Tessellation',(#11));
#13=IFCPRODUCTDEFINITIONSHAPE($,$,(#12));
#14=IFCCARTESIANPOINT((1000.,2000.,0.));
#15=IFCDIRECTION((0.,1.,0.));
#16=IFCDIRECTION((0.,0.,1.));
#17=IFCAXIS2PLACEMENT3D(#14,#16,#15);
#18=IFCLOCALPLACEMENT(#6,#17);
#19=IFCWALL('w',$,'Wall',$,$,#18,#13,$,$);
#30=IFCRECTANGLEPROFILEDEF(.AREA.,$,$,300.,500.);
#31=IFCEXTRUDEDAREASOLID(#30,$,#16,3000.);
#32=IFCSHAPEREPRESENTATION($,'Body','SweptSolid',(#31));
#33=IFCREPRESENTATIONMAP(#5,#32);
#34=IFCCARTESIANTRANSFORMATIONOPERATOR3D($,$,#4,$,$);
#35=IFCMAPPEDITEM(#33,#34);
#36=IFCSHAPEREPRESENTATION($,'Body','MappedRepresentation',(#35));
#37=IFCPRODUCTDEFINITIONSHAPE($,$,(#36));
#38=IFCCARTESIANPOINT((3000.,4000.,0.));
#39=IFCAXIS2PLACEMENT3D(#38,#16,$);
#40=IFCLOCALPLACEMENT(#6,#39);
#41=IFCCOLUMN('c',$,'Column',$,$,#40,#37,$,$);
#50=IFCRELCONTAINEDINSPATIALSTRUCTURE('r',$,$,$,(#19,#41),#7);
#60=IFCCARTESIANPOINT(({site}));
#61=IFCAXIS2PLACEMENT3D(#60,$,$);
#62=IFCLOCALPLACEMENT($,#61);
ENDSEC;END-ISO-10303-21;"
    )
}

fn only<'a>(m: &'a Model, cat: &str) -> Vec<&'a Element> {
    m.elements.values().filter(|e| e.category() == cat).collect()
}

#[test]
fn an_unmeasurable_railing_does_not_poison_the_whole_import() {
    let original = meshes("0.,0.,0.");
    let extra = "#70=IFCCARTESIANPOINT((0.,0.,0.));\n#71=IFCCARTESIANPOINT((5000.,0.,0.));\n#72=IFCPOLYLINE((#70,#71));\n#73=IFCSHAPEREPRESENTATION($,'Axis','Curve3D',(#72));\n#74=IFCPRODUCTDEFINITIONSHAPE($,$,(#73));\n#75=IFCRAILING('rail',$,'Unmeasurable rail',$,$,#6,#74,$,$);\n";
    let text =
        original.replace("(#19,#41)", "(#19,#41,#75)").replace("ENDSEC;END-ISO-10303-21;", &format!("{extra}ENDSEC;END-ISO-10303-21;"));
    let imported = ifc(text.as_bytes(), "Synthetic").unwrap();
    imported.model.check().unwrap();
    assert_eq!(only(&imported.model, "wall").len(), 1);
    assert_eq!(only(&imported.model, "railing").len(), 0);
    assert!(imported.skipped.iter().any(|s| s.contains("railing height")), "{:?}", imported.skipped);
}

#[test]
fn meshes_and_mapped_shapes_come_in_as_extrusions_do() {
    let i = ifc(meshes("0.,0.,0.").as_bytes(), "x").unwrap();
    assert_eq!((i.skipped.len(), i.moved), (0, None), "{:?}", i.skipped);
    let Some(Element::Wall(w)) = only(&i.model, "wall").first().copied() else { panic!("no wall") };
    assert_eq!((w.start, w.end, i.model.thickness(w), w.height), (P2(1000.0, 2000.0), P2(1000.0, 7000.0), 200.0, 2700.0));
    let Some(Element::Column(c)) = only(&i.model, "column").first().copied() else { panic!("no column") };
    let t = &i.model.types.column[&c.type_name];
    assert_eq!((c.at, c.angle, t.width, t.depth, t.round, c.height), (P2(3000.0, 4000.0), 90.0, 500.0, 300.0, false, 3000.0));
}

#[test]
fn a_cable_swept_round_its_centre_line_comes_in_as_a_wire() {
    // As Renga writes one: a disk 3.7 mm across swept along an indexed polycurve, through a mapped item.
    let text = "ISO-10303-21;HEADER;FILE_SCHEMA(('IFC4'));ENDSEC;DATA;
#1=IFCSIUNIT(*,.LENGTHUNIT.,.MILLI.,.METRE.);
#2=IFCUNITASSIGNMENT((#1));
#3=IFCPROJECT('p',$,'Cables',$,$,$,$,$,#2);
#4=IFCCARTESIANPOINT((0.,0.,0.));
#5=IFCAXIS2PLACEMENT3D(#4,$,$);
#6=IFCLOCALPLACEMENT($,#5);
#7=IFCBUILDINGSTOREY('s',$,'Level 0',$,$,#6,$,$,.ELEMENT.,0.);
#10=IFCCARTESIANPOINTLIST3D(((0.,0.,0.),(3000.,0.,0.)));
#11=IFCINDEXEDPOLYCURVE(#10,(IFCLINEINDEX((1,2))),$);
#12=IFCSWEPTDISKSOLID(#11,3.7,$,$,$);
#13=IFCSHAPEREPRESENTATION($,'Body','AdvancedSweptSolid',(#12));
#14=IFCREPRESENTATIONMAP(#5,#13);
#15=IFCCARTESIANTRANSFORMATIONOPERATOR3D($,$,#4,$,$);
#16=IFCMAPPEDITEM(#14,#15);
#17=IFCSHAPEREPRESENTATION($,'Body','MappedRepresentation',(#16));
#18=IFCPRODUCTDEFINITIONSHAPE($,$,(#17));
#19=IFCCARTESIANPOINT((1000.,2000.,2900.));
#20=IFCAXIS2PLACEMENT3D(#19,$,$);
#21=IFCLOCALPLACEMENT(#6,#20);
#22=IFCCABLESEGMENT('k',$,'Cable',$,$,#21,#18,$,.CABLESEGMENT.);
#30=IFCRELCONTAINEDINSPATIALSTRUCTURE('r',$,$,$,(#22),#7);
ENDSEC;END-ISO-10303-21;";
    let i = ifc(text.as_bytes(), "x").unwrap();
    i.model.check().unwrap();
    assert_eq!(i.skipped.len(), 0, "{:?}", i.skipped);
    let Some(Element::Wire(w)) = only(&i.model, "wire").first().copied() else { panic!("no wire") };
    let t = &i.model.types.runs["wire"][&w.type_name];
    assert_eq!((w.start, w.end, w.offset, w.end_offset), (P2(1000.0, 2000.0), P2(4000.0, 2000.0), 2900.0, None));
    assert!(t.round && (t.width - 7.4).abs() < 0.05, "{} x {}", t.width, t.depth);
}

#[test]
fn a_swept_disk_along_a_curve_of_too_many_segments_is_left_out_not_built() {
    // The cable above with a centre line of `n` points, a millimetre apart: each segment is 26 faces.
    let cable = |n: usize| {
        let points: Vec<String> = (0..n).map(|i| format!("({i}.,0.,0.)")).collect();
        let index: Vec<String> = (1..=n).map(|i| i.to_string()).collect();
        format!(
            "ISO-10303-21;HEADER;FILE_SCHEMA(('IFC4'));ENDSEC;DATA;
#1=IFCSIUNIT(*,.LENGTHUNIT.,.MILLI.,.METRE.);
#2=IFCUNITASSIGNMENT((#1));
#3=IFCPROJECT('p',$,'Cables',$,$,$,$,$,#2);
#4=IFCCARTESIANPOINT((0.,0.,0.));
#5=IFCAXIS2PLACEMENT3D(#4,$,$);
#6=IFCLOCALPLACEMENT($,#5);
#7=IFCBUILDINGSTOREY('s',$,'Level 0',$,$,#6,$,$,.ELEMENT.,0.);
#10=IFCCARTESIANPOINTLIST3D(({}));
#11=IFCINDEXEDPOLYCURVE(#10,(IFCLINEINDEX(({}))),$);
#12=IFCSWEPTDISKSOLID(#11,3.7,$,$,$);
#13=IFCSHAPEREPRESENTATION($,'Body','AdvancedSweptSolid',(#12));
#18=IFCPRODUCTDEFINITIONSHAPE($,$,(#13));
#22=IFCCABLESEGMENT('k',$,'Cable',$,$,#6,#18,$,.CABLESEGMENT.);
#30=IFCRELCONTAINEDINSPATIALSTRUCTURE('r',$,$,$,(#22),#7);
ENDSEC;END-ISO-10303-21;",
            points.join(","),
            index.join(",")
        )
    };
    // MAX_POINTS segments are read; one more is a body BIMCraft will not build, and the cable is left out.
    let within = ifc(cable(MAX_POINTS + 1).as_bytes(), "x").unwrap();
    assert_eq!((only(&within.model, "wire").len(), within.skipped.len()), (1, 0), "{:?}", within.skipped);
    let past = ifc(cable(MAX_POINTS + 2).as_bytes(), "x").unwrap();
    assert!(only(&past.model, "wire").is_empty());
    assert_eq!(past.skipped.len(), 1);
    assert!(past.skipped[0].contains("no body BIMCraft can read"), "{:?}", past.skipped);
}

#[test]
fn room_separation_lines_past_what_a_project_may_hold_are_left_out_not_made() {
    // A storey with a room separation line for each count of `points`, along the x axis 100 mm a point.
    let lines = |points: &[usize]| {
        let mut file = String::from(
            "ISO-10303-21;HEADER;FILE_SCHEMA(('IFC4'));ENDSEC;DATA;
#1=IFCSIUNIT(*,.LENGTHUNIT.,.MILLI.,.METRE.);
#2=IFCUNITASSIGNMENT((#1));
#3=IFCPROJECT('p',$,'Lines',$,$,$,$,$,#2);
#4=IFCCARTESIANPOINT((0.,0.,0.));
#5=IFCAXIS2PLACEMENT3D(#4,$,$);
#6=IFCLOCALPLACEMENT($,#5);
#7=IFCBUILDINGSTOREY('s',$,'Level 0',$,$,#6,$,$,.ELEMENT.,0.);
",
        );
        let mut held = Vec::new();
        for (k, &n) in points.iter().enumerate() {
            let b = 100 + 10 * k;
            let xyz: Vec<String> = (0..n).map(|i| format!("({}.,0.,0.)", i * 100)).collect();
            let index: Vec<String> = (1..=n).map(|i| i.to_string()).collect();
            file += &format!(
                "#{b}=IFCCARTESIANPOINTLIST3D(({}));
#{}=IFCINDEXEDPOLYCURVE(#{b},(IFCLINEINDEX(({}))),$);
#{}=IFCSHAPEREPRESENTATION($,'Axis','Curve2D',(#{}));
#{}=IFCPRODUCTDEFINITIONSHAPE($,$,(#{}));
#{}=IFCVIRTUALELEMENT('v{k}',$,'Room Separation Line',$,'RoomSeparation',#6,#{},$);
",
                xyz.join(","),
                b + 1,
                index.join(","),
                b + 2,
                b + 1,
                b + 3,
                b + 2,
                b + 4,
                b + 3
            );
            held.push(format!("#{}", b + 4));
        }
        file + &format!("#30=IFCRELCONTAINEDINSPATIALSTRUCTURE('r',$,$,$,({}),#7);\nENDSEC;END-ISO-10303-21;", held.join(","))
    };
    let read = |points: &[usize]| {
        let i = ifc(lines(points).as_bytes(), "x").unwrap();
        (only(&i.model, "roomseparator").len(), i.skipped)
    };
    // MAX_POINTS segments of one line are made, one more is left out with its reason.
    assert_eq!(read(&[MAX_POINTS + 1]), (MAX_POINTS, Vec::<String>::new()));
    let (made, skipped) = read(&[MAX_POINTS + 2]);
    assert_eq!(made, 0);
    assert!(skipped.len() == 1 && skipped[0].contains("room separation"), "{skipped:?}");
    // Nor do all the lines together pass MAX_POINTS: the second of two long ones is left out.
    let (made, skipped) = read(&[6000, 6000]);
    assert_eq!(made, 5999);
    assert!(skipped.len() == 1 && skipped[0].contains("room separation"), "{skipped:?}");
}

/// An IFC4 file in mm with a 6 x 4 m slab 200 mm thick, its profile round two holes, an
/// opening element voiding it through the east one, as a mesh would carry it: `slab` makes the
/// slab #35, and what it's part of; the storey holds `within`.
fn holed(slab: &str, within: u32) -> String {
    let rect = |id: u32, (x0, y0): (f64, f64), (x1, y1): (f64, f64)| {
        let pts: Vec<String> = [(x0, y0), (x1, y0), (x1, y1), (x0, y1)]
            .iter()
            .enumerate()
            .map(|(i, (x, y))| format!("#{}=IFCCARTESIANPOINT(({x:.1},{y:.1}));", id + 1 + i as u32))
            .collect();
        format!("{}\n#{id}=IFCPOLYLINE((#{},#{},#{},#{},#{}));", pts.join("\n"), id + 1, id + 2, id + 3, id + 4, id + 1)
    };
    format!(
        "ISO-10303-21;HEADER;FILE_SCHEMA(('IFC4'));ENDSEC;DATA;
#1=IFCSIUNIT(*,.LENGTHUNIT.,.MILLI.,.METRE.);
#2=IFCUNITASSIGNMENT((#1));
#3=IFCPROJECT('p',$,'Holes',$,$,$,$,$,#2);
#4=IFCCARTESIANPOINT((0.,0.,-200.));
#5=IFCAXIS2PLACEMENT3D(#4,$,$);
#6=IFCLOCALPLACEMENT($,#5);
#7=IFCBUILDINGSTOREY('s',$,'Level 0',$,$,$,$,$,.ELEMENT.,0.);
{}
{}
{}
#30=IFCARBITRARYPROFILEDEFWITHVOIDS(.AREA.,$,#10,(#20,#25));
#31=IFCDIRECTION((0.,0.,1.));
#32=IFCEXTRUDEDAREASOLID(#30,$,#31,200.);
#33=IFCSHAPEREPRESENTATION($,'Body','SweptSolid',(#32));
#34=IFCPRODUCTDEFINITIONSHAPE($,$,(#33));
{slab}
#40=IFCCARTESIANPOINT((4500.,1500.));
#41=IFCAXIS2PLACEMENT2D(#40,$);
#42=IFCRECTANGLEPROFILEDEF(.AREA.,$,#41,1000.,1000.);
#43=IFCEXTRUDEDAREASOLID(#42,$,#31,200.);
#44=IFCSHAPEREPRESENTATION($,'Body','SweptSolid',(#43));
#45=IFCPRODUCTDEFINITIONSHAPE($,$,(#44));
#46=IFCOPENINGELEMENT('o',$,'Hole',$,$,#6,#45,$,.OPENING.);
#47=IFCRELVOIDSELEMENT('v',$,$,$,#35,#46);
#50=IFCRELCONTAINEDINSPATIALSTRUCTURE('r',$,$,$,(#{within}),#7);
ENDSEC;END-ISO-10303-21;",
        rect(10, (0.0, 0.0), (6000.0, 4000.0)),
        rect(20, (1000.0, 1000.0), (2500.0, 2000.0)),
        rect(25, (4000.0, 1000.0), (5000.0, 2000.0)),
    )
}

/// `file` with 150 mm of brick then 50 of plaster on element `#id`, its usage stacking them
/// along its own `axis` from where its offset starts them.
fn layered(file: &str, id: u32, axis: &str, offset: f64) -> String {
    let set = "#80=IFCMATERIAL('Brick',$,$);
#81=IFCMATERIAL('Plaster',$,$);
#82=IFCMATERIALLAYER(#80,150.,$,$,$,'Finish',$);
#83=IFCMATERIALLAYER(#81,50.,$,$,$,'LoadBearing',$);
#84=IFCMATERIALLAYERSET((#82,#83),'Brick and plaster',$);";
    let usage = format!(
        "#85=IFCMATERIALLAYERSETUSAGE(#84,.{axis}.,.POSITIVE.,{offset:.1},$);\n#86=IFCRELASSOCIATESMATERIAL('m',$,$,$,(#{id}),#85);"
    );
    file.replace("ENDSEC;END", &format!("{set}\n{usage}\nENDSEC;END"))
}

#[test]
fn layers_stacked_up_an_elements_own_axis_start_from_its_back() {
    // The wall runs north, its own y to the west: its first layer is east, so it runs south.
    let i = ifc(layered(&meshes("0.,0.,0."), 19, "AXIS2", -100.0).as_bytes(), "x").unwrap();
    let Some(Element::Wall(w)) = only(&i.model, "wall").first().copied() else { panic!("no wall") };
    let names = |layers: &[Layer]| layers.iter().map(|l| l.material.clone()).collect::<Vec<_>>();
    assert_eq!(
        (w.start, w.end, names(&i.model.types.wall[&w.type_name].layers)),
        (P2(1000.0, 7000.0), P2(1000.0, 2000.0), vec!["Brick".into(), "Plaster".into()])
    );
    // The slab's first layer is at its bottom.
    let i = ifc(layered(&holed("#35=IFCSLAB('f',$,'Slab',$,$,#6,#34,$,.FLOOR.);", 35), 35, "AXIS3", 0.0).as_bytes(), "x").unwrap();
    let Some(Element::Floor(f)) = only(&i.model, "floor").first().copied() else { panic!("no floor") };
    assert_eq!(names(&i.model.types.floor[&f.type_name].layers), ["Plaster", "Brick"]);
}

/// Each component's extent in the model: its outlines' corners and its heights, to the mm.
fn extent(m: &Model, c: &crate::model::Component) -> (i64, i64, i64, i64, i64, i64) {
    let pieces = m.component_pieces(c);
    let pts: Vec<P2> = pieces.iter().flat_map(|p| p.outline.clone()).collect();
    let r = |x: f64| x.round() as i64;
    let (x, y) = (span(pts.iter().map(|p| p.0)).unwrap_or_default(), span(pts.iter().map(|p| p.1)).unwrap_or_default());
    let z = span(pieces.iter().flat_map(|p| [p.bottom, p.top])).unwrap_or_default();
    (r(x.0), r(x.1), r(y.0), r(y.1), r(z.0), r(z.1))
}

#[test]
fn project_parameters_go_out_in_a_property_set_and_come_back_with_their_kinds() {
    let mut s = crate::engine::Session::new();
    s.script(
        r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true}
           door {"at": [4000, 0]}
           room {"point": [4000, 3000]}
           component {"family": "Chair", "at": [2000, 2000]}
           parameter {"name": "Fire Rating", "categories": ["wall", "door"]}
           parameter {"name": "Cost", "kind": "number", "categories": ["door", "component"]}
           parameter {"name": "Finish Height", "kind": "length", "categories": ["room"]}
           parameter {"name": "Accessible", "kind": "yesno", "categories": ["door", "room"]}"#,
    )
    .unwrap();
    let ids = |m: &Model, cat: &str| -> Vec<Id> { m.elements.iter().filter(|(_, e)| e.category() == cat).map(|(id, _)| *id).collect() };
    let (wall, door, room, chair) =
        (ids(&s.model, "wall")[0], ids(&s.model, "door")[0], ids(&s.model, "room")[0], ids(&s.model, "component")[0]);
    for (id, values) in [
        (wall, json!({"Fire Rating": "60 min"})),
        (door, json!({"Fire Rating": "30 min", "Cost": 450.5, "Accessible": true})),
        (room, json!({"Finish Height": 2400, "Accessible": false})),
        (chair, json!({"Cost": 80})),
    ] {
        s.execute("modify", &json!({"id": id, "params": values})).unwrap();
    }
    let file = crate::ifc::export(&s.model);
    // One property set for each element with values; the elements' quantities are defined by properties too.
    let (sets, quantities) = (file.matches("IFCPROPERTYSET(").count(), file.matches("IFCELEMENTQUANTITY(").count());
    assert_eq!((sets, file.matches("IFCRELDEFINESBYPROPERTIES(").count()), (4, sets + quantities));
    assert!(file.contains("IFCPROPERTYSINGLEVALUE('Finish Height',$,IFCLENGTHMEASURE(2400.),$)"), "{file}");
    assert!(file.contains("IFCPROPERTYSINGLEVALUE('Accessible',$,IFCBOOLEAN(.T.),$)"));
    let i = ifc(file.as_bytes(), "x").unwrap();
    assert_eq!(i.skipped, Vec::<String>::new());
    assert_eq!(i.model.parameters, s.model.parameters, "each kind, for the categories holding one");
    // Numbers as numbers: 2400 and 2400.0 alike.
    let values = |m: &Model, cat: &str| -> Option<BTreeMap<String, serde_json::Value>> {
        let params = m.elements.values().find(|e| e.category() == cat).and_then(Element::params)?;
        Some(params.iter().map(|(k, v)| (k.clone(), v.as_f64().map_or_else(|| v.clone(), |x| json!(x)))).collect())
    };
    for cat in ["wall", "door", "room", "component"] {
        assert_eq!(values(&i.model, cat), values(&s.model, cat), "{cat}");
    }
    // In metres, a length comes in in millimetres.
    let metres = file.replace(".MILLI.,.METRE.", "$,.METRE.").replace("IFCLENGTHMEASURE(2400.)", "IFCLENGTHMEASURE(2.4)");
    let i = ifc(metres.as_bytes(), "x").unwrap();
    assert_eq!(values(&i.model, "room").and_then(|p| p.get("Finish Height").cloned()), Some(json!(2400.0)));
}

#[test]
fn a_window_taller_than_its_wall_comes_in_rising_past_the_top_as_revit_lets_it() {
    let mut s = crate::engine::Session::new();
    s.script(
        r#"wall {"points": [[0,0],[6000,0]], "height": 3000}
           window {"at": [3000, 0]}"#,
    )
    .unwrap();
    // Another program's window rises past its wall's top, as Revit lets one: its OverallHeight, the ninth attribute, is 5000.
    let file: String = crate::ifc::export(&s.model)
        .lines()
        .map(|line| match line.split_once("=IFCWINDOW(") {
            Some((at, args)) => {
                let mut parts: Vec<&str> = args.split(',').collect();
                parts[8] = "5000.";
                format!("{at}=IFCWINDOW({}", parts.join(","))
            }
            None => line.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n");
    let i = ifc(file.as_bytes(), "x").unwrap();
    assert_eq!(i.skipped, Vec::<String>::new(), "nothing is left out");
    let window = only(&i.model, "window");
    assert_eq!(window.len(), 1, "it comes in, its head past the wall's top");
    let t = i.model.opening_type(window[0]).unwrap();
    assert_eq!(t.height, 5000.0, "at its own height");
}

#[test]
fn a_property_named_as_a_parameter_of_a_family_does_not_stop_the_file_from_opening() {
    let mut s = crate::engine::Session::new();
    s.script(
        r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true}
           component {"family": "Chair", "at": [2000, 2000]}
           parameter {"name": "Depth Of", "kind": "length", "categories": ["component"]}
           parameter {"name": "Supplier", "categories": ["component"]}"#,
    )
    .unwrap();
    let chair = s.model.elements.iter().find(|(_, e)| e.category() == "component").map(|(id, _)| *id).unwrap();
    s.execute("modify", &json!({"id": chair, "params": {"Depth Of": 450, "Supplier": "Acme"}})).unwrap();
    // Another program calls the property by the name of a parameter the starter "Base Cabinet" family has.
    assert!(s.model.families.values().any(|f| f.parameters.iter().any(|p| p.name == "Depth")));
    let file = crate::ifc::export(&s.model).replace("'Depth Of'", "'Depth'");
    let i = ifc(file.as_bytes(), "x").unwrap();
    assert!(!i.model.parameters.contains_key("Depth"), "a family's parameter is not a project parameter too");
    let kept = i.model.elements.values().find(|e| e.category() == "component").and_then(Element::params).unwrap();
    assert_eq!(kept.get("Supplier"), Some(&json!("Acme")), "the other values stay");
    assert!(i.skipped.iter().any(|n| n.contains("Depth") && n.contains("family")), "{:?}", i.skipped);
}

#[test]
fn a_property_set_of_more_parameters_than_a_project_holds_comes_in_up_to_them() {
    let mut s = crate::engine::Session::new();
    s.script(r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true}"#).unwrap();
    let file = crate::ifc::export(&s.model);
    // Every wall, related to one set of more parameters than a project holds.
    let walls: Vec<String> =
        file.lines().filter(|l| l.contains("=IFCWALL(")).filter_map(|l| l.split('=').next().map(String::from)).collect();
    let n = crate::model::MAX_PROJECT_PARAMETERS + 50;
    let mut extra = String::new();
    let props: Vec<String> = (0..n)
        .map(|i| {
            extra.push_str(&format!("#{}=IFCPROPERTYSINGLEVALUE('P{i}',$,IFCREAL(1.),$);\n", 900_000 + i));
            format!("#{}", 900_000 + i)
        })
        .collect();
    extra.push_str(&format!("#899998=IFCPROPERTYSET('0aaaaaaaaaaaaaaaaaaaaa',$,'Project Parameters',$,({}));\n", props.join(",")));
    extra.push_str(&format!("#899999=IFCRELDEFINESBYPROPERTIES('0bbbbbbbbbbbbbbbbbbbbb',$,$,$,({}),#899998);\n", walls.join(",")));
    let file = file.replacen("ENDSEC;\nEND-ISO", &format!("{extra}ENDSEC;\nEND-ISO"), 1);
    let i = ifc(file.as_bytes(), "x").unwrap();
    assert_eq!(i.model.parameters.len(), crate::model::MAX_PROJECT_PARAMETERS);
    assert!(i.skipped.iter().any(|s| s.contains("parameter")), "{:?}", i.skipped);
}

#[test]
fn a_rooms_colour_comes_back_from_ifc_and_one_without_stays_without() {
    let mut s = crate::engine::Session::new();
    s.script(
        r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true}
           wall {"points": [[4000,0],[4000,6000]]}
           room {"point": [2000, 3000], "name": "Kitchen"}
           room {"point": [6000, 3000], "name": "Hall"}"#,
    )
    .unwrap();
    let kitchen = s.model.elements.iter().find(|(_, e)| matches!(e, Element::Room(r) if r.name == "Kitchen")).map(|(id, _)| *id);
    s.execute("modify", &json!({"id": kitchen, "colour": [214, 229, 210]})).unwrap();
    let i = ifc(crate::ifc::export(&s.model).as_bytes(), "x").unwrap();
    let colours: BTreeMap<String, Option<[u8; 3]>> =
        i.model.elements.values().filter_map(|e| if let Element::Room(r) = e { Some((r.name.clone(), r.colour)) } else { None }).collect();
    assert_eq!(colours, BTreeMap::from([("Kitchen".to_string(), Some([214, 229, 210])), ("Hall".to_string(), None)]));
}

#[test]
fn components_come_back_from_ifc_where_they_stood() {
    let mut s = crate::engine::Session::new();
    s.script(
        r#"component {"family": "Table", "type": "1600 x 900", "at": [2000, 3000], "angle": 30}
           component {"family": "WC", "at": [-500, 0]}
           component {"family": "WC", "at": [500, 0], "mirrored": true, "offset": 50}
           component {"family": "Bed", "at": [6000, 4000], "angle": -90}
           level {"elevation": 3000, "name": "Level 3"}"#,
    )
    .unwrap();
    let file = crate::ifc::export(&s.model);
    let i = ifc(file.as_bytes(), "x").unwrap();
    assert_eq!(i.skipped, Vec::<String>::new());
    let comps = |m: &Model| -> Vec<crate::model::Component> {
        m.elements.values().filter_map(|e| if let Element::Component(c) = e { Some(c.clone()) } else { None }).collect()
    };
    let (mut before, mut after) = (comps(&s.model), comps(&i.model));
    before.sort_by(|a, b| a.family.cmp(&b.family));
    after.sort_by(|a, b| a.family.cmp(&b.family));
    assert_eq!(after.len(), 4);
    for (a, b) in before.iter().zip(&after) {
        assert_eq!((&a.family, &a.type_name, a.level), (&b.family, &b.type_name, b.level));
        assert_eq!(extent(&s.model, a), extent(&i.model, b), "{} {}", a.family, a.type_name);
    }
    let of = |family: &str| after.iter().rfind(|c| c.family == family).unwrap();
    assert_eq!((of("Table").angle, of("WC").offset), (30.0, 50.0));
    // A mirror image of a type is that type, mirrored.
    let mirrored: Vec<(&str, bool)> = after.iter().filter(|c| c.family == "WC").map(|c| (c.type_name.as_str(), c.mirrored)).collect();
    assert_eq!(mirrored, [("400 x 650", false), ("400 x 650", true)]);
    // Its forms, as the file had them: in their materials, which come in with their colours.
    let bed = &i.model.families["Bed"];
    let mut made_of: Vec<&str> = bed.forms.iter().map(|f| f.material.as_str()).collect();
    made_of.sort_unstable();
    made_of.dedup();
    assert_eq!(made_of, ["Fabric", "Timber"]);
    assert_eq!(i.model.materials["Fabric"].color, s.model.materials["Fabric"].color);
}

/// An IFC4 file in mm whose proxies are named `name` and are each a profile through `ring`'s
/// corners, scaled by its number, extruded `depth` along `dir`.
fn proxies(name: &str, n: usize, ring: &[(f64, f64)], dir: &str, depth: f64) -> String {
    let mut out = String::from(
        "ISO-10303-21;HEADER;FILE_SCHEMA(('IFC4'));ENDSEC;DATA;
#1=IFCSIUNIT(*,.LENGTHUNIT.,.MILLI.,.METRE.);
#2=IFCUNITASSIGNMENT((#1));
#3=IFCPROJECT('p',$,'Proxies',$,$,$,$,$,#2);
#4=IFCCARTESIANPOINT((0.,0.,0.));
#5=IFCAXIS2PLACEMENT3D(#4,$,$);
#6=IFCLOCALPLACEMENT($,#5);
#7=IFCBUILDINGSTOREY('s',$,'Level 0',$,$,#6,$,$,.ELEMENT.,0.);
",
    );
    let mut ids = Vec::new();
    for k in 0..n {
        let base = 100 + k * 20;
        let scale = 1.0 + k as f64 / 10.0;
        let pts: Vec<String> = ring
            .iter()
            .enumerate()
            .map(|(i, (x, y))| format!("#{}=IFCCARTESIANPOINT(({:.1},{:.1}));", base + i, x * scale, y * scale))
            .collect();
        let refs: Vec<String> = (0..ring.len()).chain([0]).map(|i| format!("#{}", base + i)).collect();
        out.push_str(&format!(
            "{}\n#{l}=IFCPOLYLINE(({}));\n#{pr}=IFCARBITRARYCLOSEDPROFILEDEF(.AREA.,$,#{l});\n#{d}=IFCDIRECTION(({dir}));\n#{x}=IFCEXTRUDEDAREASOLID(#{pr},$,#{d},{depth:.1});\n#{rep}=IFCSHAPEREPRESENTATION($,'Body','SweptSolid',(#{x}));\n#{sh}=IFCPRODUCTDEFINITIONSHAPE($,$,(#{rep}));\n#{at}=IFCCARTESIANPOINT(({}.,0.,0.));\n#{ax}=IFCAXIS2PLACEMENT3D(#{at},$,$);\n#{pl}=IFCLOCALPLACEMENT(#6,#{ax});\n#{e}=IFCBUILDINGELEMENTPROXY('g{k}',$,'{name}',$,$,#{pl},#{sh},$,.ELEMENT.);\n",
            pts.join("\n"),
            refs.join(","),
            k * 3000,
            l = base + 10,
            pr = base + 11,
            d = base + 12,
            x = base + 13,
            rep = base + 14,
            sh = base + 15,
            at = base + 16,
            ax = base + 17,
            pl = base + 18,
            e = base + 19,
        ));
        ids.push(format!("#{}", base + 19));
    }
    out.push_str(&format!("#99=IFCRELCONTAINEDINSPATIALSTRUCTURE('r',$,$,$,({}),#7);\nENDSEC;END-ISO-10303-21;", ids.join(",")));
    out
}

/// A closed surface's volume: its triangles' signed tetrahedra, positive while they face out.
fn volume(pieces: &[crate::family::Piece]) -> f64 {
    pieces.iter().flat_map(|p| p.tris.iter()).map(|[a, b, c]| dot(*a, cross(*b, *c)) / 6.0).sum()
}

#[test]
fn a_clockwise_profile_or_a_downward_sweep_comes_in_turned_out() {
    let clockwise = [(0.0, 0.0), (0.0, 300.0), (500.0, 300.0), (500.0, 0.0)];
    for (dir, ring) in [("0.,0.,1.", &clockwise[..]), ("0.,0.,-1.", &[(0.0, 0.0), (500.0, 0.0), (500.0, 300.0), (0.0, 300.0)][..])] {
        let i = ifc(proxies("Box", 1, ring, dir, 450.0).as_bytes(), "x").unwrap();
        let Some(Element::Component(c)) = only(&i.model, "component").first().copied() else { panic!("no component") };
        let v = volume(&i.model.component_local_pieces(c));
        assert!((v - 500.0 * 300.0 * 450.0).abs() < 1.0, "{dir}: {v}");
    }
}

#[test]
fn more_shapes_of_one_family_than_it_holds_leave_only_the_rest_out() {
    let square = [(0.0, 0.0), (300.0, 0.0), (300.0, 300.0), (0.0, 300.0)];
    let n = crate::family::MAX_FORMS + 3;
    let i = ifc(proxies("Thing:A", n, &square, "0.,0.,1.", 400.0).as_bytes(), "x").unwrap();
    assert_eq!(i.model.families["Thing"].types.len(), crate::family::MAX_FORMS);
    assert_eq!(only(&i.model, "component").len(), crate::family::MAX_FORMS);
    assert_eq!(i.skipped.len(), 3, "{:?}", i.skipped);
    assert!(i.skipped.iter().all(|s| s.contains("Thing")), "{:?}", i.skipped);
}

#[test]
fn a_style_that_holds_itself_shades_its_material_once() {
    let style = "#90=IFCMATERIALDEFINITIONREPRESENTATION($,$,(#91),#80);
#91=IFCSTYLEDREPRESENTATION($,'Style','Material',(#92));
#92=IFCSTYLEDITEM($,(#93),$);
#93=IFCSURFACESTYLE('Brick',.BOTH.,(#93,#94));
#94=IFCSURFACESTYLESHADING(#95,$);
#95=IFCCOLOURRGB($,1.,0.,0.);";
    let file = layered(&meshes("0.,0.,0."), 19, "AXIS2", -100.0).replace("ENDSEC;END", &format!("{style}\nENDSEC;END"));
    let i = ifc(file.as_bytes(), "x").unwrap();
    assert_eq!(i.model.materials["Brick"].color, [255, 0, 0]);
}

#[test]
fn a_layer_of_no_thickness_is_a_membrane() {
    let file = layered(&meshes("0.,0.,0."), 19, "AXIS2", -100.0)
        .replace("IFCMATERIALLAYERSET((#82,#83)", "IFCMATERIALLAYERSET((#82,#87,#83)")
        .replace("ENDSEC;END", "#87=IFCMATERIALLAYER(#81,0.,$,$,$,'Finish',$);\nENDSEC;END");
    let i = ifc(file.as_bytes(), "x").unwrap();
    let Some(Element::Wall(w)) = only(&i.model, "wall").first().copied() else { panic!("no wall") };
    let layers = &i.model.types.wall[&w.type_name].layers;
    assert_eq!(
        layers.iter().map(|l| (l.function, l.thickness)).collect::<Vec<_>>(),
        [(Function::Finish, 150.0), (Function::Membrane, 0.0), (Function::Structure, 50.0)]
    );
}

#[test]
fn holes_in_a_slab_come_in_as_its_openings_or_as_the_shafts_through_it() {
    let i = ifc(holed("#35=IFCSLAB('f',$,'Slab',$,$,#6,#34,$,.FLOOR.);", 35).as_bytes(), "x").unwrap();
    assert_eq!(i.skipped, Vec::<String>::new());
    let Some(Element::Floor(f)) = only(&i.model, "floor").first().copied() else { panic!("no floor") };
    assert_eq!(f.openings, [[P2(1000.0, 1000.0), P2(1000.0, 2000.0), P2(2500.0, 2000.0), P2(2500.0, 1000.0)]]);
    let Some(Element::Shaft(sh)) = only(&i.model, "shaft").first().copied() else { panic!("no shaft") };
    assert_eq!(geom::area(&sh.boundary).abs(), 1_000_000.0);
    let floor = i.model.elements.iter().find(|(_, e)| e.category() == "floor").map(|(id, _)| *id);
    assert_eq!(crate::element_json(&i.model, floor.unwrap_or_default())["area_m2"], 21.5, "6 x 4 less 1.5 and 1");
}

#[test]
fn holes_in_a_ceiling_come_in_as_its_openings_or_as_the_shafts_through_it() {
    let i = ifc(holed("#35=IFCCOVERING('f',$,'Ceiling',$,$,#6,#34,$,.CEILING.);", 35).as_bytes(), "x").unwrap();
    assert_eq!(i.skipped, Vec::<String>::new());
    let Some(Element::Ceiling(c)) = only(&i.model, "ceiling").first().copied() else { panic!("no ceiling") };
    assert_eq!(c.openings, [[P2(1000.0, 1000.0), P2(1000.0, 2000.0), P2(2500.0, 2000.0), P2(2500.0, 1000.0)]]);
    assert_eq!(only(&i.model, "shaft").len(), 1);
    let ceiling = i.model.elements.iter().find(|(_, e)| e.category() == "ceiling").map(|(id, _)| *id);
    assert_eq!(crate::element_json(&i.model, ceiling.unwrap_or_default())["area_m2"], 21.5, "6 x 4 less 1.5 and 1");
}

#[test]
fn holes_in_a_roof_come_in_as_its_openings_or_as_the_shafts_through_it() {
    // The slab as a roof, then as the part of one: the opening voids it either way.
    let part = "#35=IFCSLAB('f',$,'Slab',$,$,#6,#34,$,.ROOF.);
#36=IFCROOF('r',$,'Roof',$,$,#6,$,$,.FLAT_ROOF.);
#37=IFCRELAGGREGATES('a',$,$,$,#36,(#35));";
    for (slab, within) in [("#35=IFCROOF('f',$,'Roof',$,$,#6,#34,$,.FLAT_ROOF.);", 35), (part, 36)] {
        let i = ifc(holed(slab, within).as_bytes(), "x").unwrap();
        assert_eq!(i.skipped, Vec::<String>::new());
        let Some(Element::Roof(roof)) = only(&i.model, "roof").first().copied() else { panic!("no roof") };
        assert_eq!(roof.openings, [[P2(1000.0, 1000.0), P2(1000.0, 2000.0), P2(2500.0, 2000.0), P2(2500.0, 1000.0)]]);
        assert_eq!(only(&i.model, "shaft").len(), 1);
    }
}

#[test]
fn from_step_names_elements_as_ifc_does() {
    // A file's text, and the file already parsed with the seed of that text, make one model:
    // the text may be freed once it is parsed.
    let file = holed("#35=IFCSLAB('f',$,'Slab',$,$,#6,#34,$,.FLOOR.);", 35);
    let text = file.as_bytes();
    let hash = seed(text);
    let parsed = step::parse(text).unwrap();
    let from_text = Model { uid: 0, ..ifc(text, "x").unwrap().model };
    let from_file = Model { uid: 0, ..from_step(parsed, "x", hash).unwrap().model };
    assert_eq!(from_file, from_text);
    // The seed names the elements the file leaves unnamed: another seed is another model.
    let other = Model { uid: 0, ..from_step(step::parse(text).unwrap(), "x", hash ^ 1).unwrap().model };
    assert_ne!(other, from_text);
}

#[test]
fn open_seeds_elements_as_ifc_does() {
    // The command path hashes, parses, drops the text and imports: the model is `ifc`'s, GUIDs and all.
    let text = holed("#35=IFCSLAB('f',$,'Slab',$,$,#6,#34,$,.FLOOR.);", 35);
    let dir = std::env::temp_dir().join(format!("bimcraft-test-{}-open-seeds-elements", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("holes.ifc");
    std::fs::write(&path, &text).unwrap();
    let mut s = crate::engine::Session::new();
    s.execute("open", &json!({"path": path})).unwrap();
    let guids = |m: &Model| {
        let mut out: Vec<Guid> = m.elements.with_guids().map(|(_, g, _)| g).collect();
        out.sort_unstable();
        out
    };
    assert_eq!(guids(&s.model), guids(&ifc(text.as_bytes(), "holes").unwrap().model));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_file_reads_the_same_every_time() {
    // Its slab's and its shaft's rings, and a second project, in metres, after its own: each
    // read hashes afresh, so whatever came in a hash's order would change from read to read.
    let more = "#60=IFCSIUNIT(*,.LENGTHUNIT.,$,.METRE.);\n#61=IFCUNITASSIGNMENT((#60));\n#62=IFCPROJECT('q',$,'Other',$,$,$,$,$,#61);";
    let file = holed("#35=IFCSLAB('f',$,'Slab',$,$,#6,#34,$,.FLOOR.);", 35).replace("ENDSEC;END", &format!("{more}\nENDSEC;END"));
    // Each read is a project of its own, its uid seeding its own GlobalIds; the rest the same.
    let read = || Model { uid: 0, ..ifc(file.as_bytes(), "x").unwrap().model };
    let first = read();
    for _ in 0..8 {
        assert_eq!(read(), first);
    }
}

#[test]
fn a_model_at_map_coordinates_comes_near_the_origin() {
    let i = ifc(meshes("500000000.,5000000000.,0.").as_bytes(), "x").unwrap();
    let Some(Element::Wall(w)) = only(&i.model, "wall").first().copied() else { panic!("no wall") };
    assert_eq!((w.start, w.end), (P2(0.0, 0.0), P2(0.0, 5000.0)));
    assert_eq!(i.moved, Some(P2(-500_001_000.0, -5_000_002_000.0)));
}

/// Where element `e` of `m` stands, to tell it from the others of its category: a wall's middle, a door's and a window's the
/// middle of their wall, a component's point, a railing's the length of its host's category (none: 0); there is one of each
/// of the others in these projects.
fn spot(m: &Model, e: &Element) -> Option<(i64, i64)> {
    let at = |p: P2| (p.0.round() as i64, p.1.round() as i64);
    Some(match e {
        Element::Wall(w) => at((w.start + w.end) * 0.5),
        Element::Door(o) | Element::Window(o) => at(m.wall(o.host).map(|w| (w.start + w.end) * 0.5)?),
        Element::Component(c) => at(c.at),
        Element::Railing(r) => (r.host.and_then(|h| m.elements.get(h)).map_or(0, |h| h.category().len() as i64), 0),
        _ => (0, 0),
    })
}

/// What each element of `m` holds of its phase and its option, by category and where it stands: a mark, or a property
/// another program wrote, is no part of what these tests are about.
fn held(m: &Model) -> BTreeMap<(&'static str, (i64, i64)), serde_json::Value> {
    use crate::phasing::{CREATED, DEMOLISHED, OPTION};
    let phased = |e: &Element| -> BTreeMap<String, serde_json::Value> {
        e.params()
            .into_iter()
            .flatten()
            .filter(|(k, _)| [CREATED, DEMOLISHED, OPTION].contains(&k.as_str()))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    };
    m.elements.values().filter(|e| e.params().is_some()).filter_map(|e| Some(((e.category(), spot(m, e)?), json!(phased(e))))).collect()
}

/// What each element of `m` that can be phased is in phase `phase`, keyed as `held` keys them.
fn standing(m: &Model, phase: &str) -> BTreeMap<(&'static str, (i64, i64)), Option<crate::phasing::Status>> {
    let at = crate::phasing::index(m, phase).unwrap_or(0);
    m.elements
        .values()
        .filter(|e| e.params().is_some())
        .filter_map(|e| Some(((e.category(), spot(m, e)?), crate::phasing::status(m, e, at))))
        .collect()
}

/// A project's phasing, as a project file holds it.
fn phasing_of(m: &Model) -> serde_json::Value {
    json!({"phases": m.phases, "filters": m.phase_filters, "overrides": m.phase_overrides, "sets": m.option_sets})
}

#[test]
fn phases_and_design_options_survive_save_as_ifc_and_open() {
    use crate::model_tests::graphics_testing::{house, run};
    let mut s = crate::engine::Session::new();
    let walls = house(&mut s);
    let door = run(&mut s, "door", json!({"at": [2000, 0]}))["id"].as_u64().unwrap();
    run(&mut s, "window", json!({"at": [8000, 3000]}));
    let late = run(&mut s, "door", json!({"at": [8000, 1500]}))["id"].as_u64().unwrap();
    let room = run(&mut s, "room", json!({"point": [4000, 3000]}))["id"].as_u64().unwrap();
    let chair = run(&mut s, "component", json!({"family": "Chair", "at": [2000, 2000]}))["id"].as_u64().unwrap();
    run(&mut s, "phase", json!({"name": "Demolition", "before": "New Construction"}));
    run(&mut s, "phase", json!({"name": "Landscape"}));
    let filter = json!({"name": "Renovation", "new": "overridden", "existing": "by-category", "demolished": "not-displayed", "temporary": "not-displayed"});
    run(&mut s, "phase_filter", filter);
    run(&mut s, "phase", json!({"overrides": {"new": {"cut": {"color": [0, 0, 200]}}}}));
    run(&mut s, "option", json!({"set": "Facade", "options": ["Brick", "Glass"], "primary": "Glass"}));
    run(&mut s, "option", json!({"set": "Roof", "options": ["Hip", "Gable", "Flat"]}));
    for (id, values) in [
        (walls[1], json!({"Phase Created": "New Construction"})),
        (walls[2], json!({"Phase Demolished": "Demolition", "Design Option": "Facade: Brick"})),
        (walls[3], json!({"Phase Created": "Demolition", "Phase Demolished": "New Construction"})),
        // The first phase, said and not: a door that says it doesn't follow its wall, even one made later, and neither
        // does a chair.
        (door, json!({"Phase Created": "Existing"})),
        (late, json!({"Phase Created": "Existing"})),
        (room, json!({"Design Option": "Roof: Gable"})),
        (chair, json!({"Phase Created": "Existing", "Design Option": "Facade: Glass"})),
    ] {
        run(&mut s, "modify", json!({"id": id, "params": values}));
    }
    let path = std::env::temp_dir().join(format!("bimcraft-phasing-{}.ifc", std::process::id()));
    run(&mut s, "save", json!({"path": path.to_str().unwrap()}));
    let mut t = crate::engine::Session::new();
    let r = run(&mut t, "open", json!({"path": path.to_str().unwrap()}));
    let _ = std::fs::remove_file(&path);
    assert_eq!(r["skipped"], json!([]));
    assert_eq!(phasing_of(&t.model), phasing_of(&s.model), "the phases in order, the filters, the overrides and the sets");
    assert_eq!(t.model.phases, ["Existing", "Demolition", "New Construction", "Landscape"]);
    let (before, after) = (held(&s.model), held(&t.model));
    assert_eq!(after.len(), 9, "four walls, two doors, a window, a room and a chair");
    assert_eq!(after, before, "every element holds what it held, and the others hold none");
    // What it comes to, in every phase: the window follows its wall into the new phase, and the door made in the first
    // is there before the wall it is in.
    for phase in &s.model.phases {
        assert_eq!(standing(&t.model, phase), standing(&s.model, phase), "in {phase}");
    }
    assert_eq!(standing(&s.model, "Existing")[&("door", (8000, 3000))], Some(crate::phasing::Status::New));
    assert_eq!(standing(&s.model, "Existing")[&("wall", (8000, 3000))], None);
}

#[test]
fn every_kind_of_element_that_holds_parameters_keeps_its_phase_and_option() {
    use crate::model_tests::graphics_testing::run;
    let mut s = crate::engine::Session::new();
    s.script(
        r#"wall {"points": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]], "closed": true}
           door {"at": [4000, 0]}
           window {"at": [8000, 3000]}
           room {"point": [4000, 3000]}
           floor {}
           ceiling {"point": [4000, 3000]}
           roof {}
           column {"at": [3000, 5500]}
           component {"family": "Chair", "at": [2000, 2000]}
           phase {"name": "Landscape"}
           option {"set": "Facade", "options": ["Brick", "Glass"]}"#,
    )
    .unwrap();
    let stair =
        run(&mut s, "stair", json!({"start": [20000, 0], "angle": 90, "width": 1100, "landings": [{"at": 9, "turn": 180}]}))["id"].clone();
    run(&mut s, "railing", json!({"host": stair}));
    let ramp =
        run(&mut s, "ramp", json!({"start": [30000, 0], "angle": 0, "height": 450, "landings": [{"at": 300, "turn": -90}]}))["id"].clone();
    run(&mut s, "railing", json!({"host": ramp, "side": "left"}));
    run(&mut s, "railing", json!({"path": [[0, -500], [3000, -500]]}));
    // Each of them holds something of its own: the kinds that have more than one differ within the kind.
    let values = [
        json!({"Phase Created": "New Construction"}),
        json!({"Phase Demolished": "New Construction"}),
        json!({"Design Option": "Facade: Glass"}),
        json!({"Phase Created": "Existing", "Design Option": "Facade: Brick"}),
        json!({"Phase Created": "New Construction", "Phase Demolished": "Landscape", "Design Option": "Facade: Glass"}),
    ];
    let ids: Vec<Id> = s.model.elements.iter().filter(|(_, e)| e.params().is_some()).map(|(id, _)| *id).collect();
    for (i, id) in ids.iter().enumerate() {
        run(&mut s, "modify", json!({"id": id, "params": values[i % values.len()]}));
    }
    let path = std::env::temp_dir().join(format!("bimcraft-phasing-kinds-{}.ifc", std::process::id()));
    run(&mut s, "save", json!({"path": path.to_str().unwrap()}));
    let mut t = crate::engine::Session::new();
    let r = run(&mut t, "open", json!({"path": path.to_str().unwrap()}));
    let _ = std::fs::remove_file(&path);
    assert_eq!(r["skipped"], json!([]));
    let (before, after) = (held(&s.model), held(&t.model));
    let kinds: BTreeSet<&str> = before.keys().map(|(cat, _)| *cat).collect();
    assert_eq!(kinds.len(), 12, "all twelve kinds that hold parameters: {kinds:?}");
    assert_eq!(after, before);
    for phase in &s.model.phases {
        assert_eq!(standing(&t.model, phase), standing(&s.model, phase), "in {phase}");
    }
}

#[test]
fn names_in_other_scripts_and_with_quotes_come_back_whole() {
    use crate::model_tests::graphics_testing::{house, run};
    let mut s = crate::engine::Session::new();
    run(&mut s, "new", json!({"language": "pt"}));
    let walls = house(&mut s);
    // What a STEP string and the JSON inside one both have to escape, and what neither writes in ASCII.
    let (phase, set, option) = (r#"O'Neil "Anexo" \ fase ç 😀"#, "Fachada", r#"Vidro "temperado""#);
    run(&mut s, "phase", json!({"name": phase}));
    run(&mut s, "option", json!({"set": set, "options": [option, "Tijolo"]}));
    run(&mut s, "phase_filter", json!({"name": r#"Só o "novo""#, "new": "by-category", "existing": "not-displayed"}));
    run(&mut s, "modify", json!({"id": walls[1], "params": {"Phase Created": phase, "Design Option": format!("{set}: {option}")}}));
    run(&mut s, "modify", json!({"id": walls[2], "params": {"Phase Created": "Nova construção"}}));
    let path = std::env::temp_dir().join(format!("bimcraft-phasing-names-{}.ifc", std::process::id()));
    run(&mut s, "save", json!({"path": path.to_str().unwrap()}));
    let mut t = crate::engine::Session::new();
    let r = run(&mut t, "open", json!({"path": path.to_str().unwrap()}));
    let _ = std::fs::remove_file(&path);
    assert_eq!(r["skipped"], json!([]));
    assert_eq!(phasing_of(&t.model), phasing_of(&s.model));
    assert_eq!(t.model.phases, ["Existente", "Nova construção", phase]);
    assert_eq!(held(&t.model), held(&s.model));
}

/// The file BIMCraft writes of a house with a wall made in a later phase, one made then and taken down in another, one in
/// an option, and the project's own filter and overrides.
fn phased_file() -> String {
    use crate::model_tests::graphics_testing::{house, run};
    let mut s = crate::engine::Session::new();
    let walls = house(&mut s);
    run(&mut s, "phase", json!({"name": "Demolition", "before": "New Construction"}));
    run(&mut s, "option", json!({"set": "Facade", "options": ["Brick", "Glass"]}));
    run(&mut s, "phase_filter", json!({"name": "Renovation", "new": "overridden", "temporary": "not-displayed"}));
    run(&mut s, "phase", json!({"overrides": {"new": {"cut": {"color": [0, 0, 200]}}}}));
    for (id, values) in [
        (walls[1], json!({"Phase Created": "New Construction"})),
        (walls[2], json!({"Phase Created": "Demolition", "Phase Demolished": "New Construction"})),
        (walls[3], json!({"Design Option": "Facade: Glass"})),
    ] {
        run(&mut s, "modify", json!({"id": id, "params": values}));
    }
    crate::ifc::export(&s.model)
}

#[test]
fn an_ifc_without_the_phasing_set_opens_as_a_new_project_does() {
    let new = Model::new("x");
    let usual = phasing_of(&new);
    let mut s = crate::engine::Session::new();
    s.script(crate::sample::SAMPLE).unwrap();
    // A file from another tool, and one of BIMCraft's own that uses no phases.
    for file in [meshes("0.,0.,0."), crate::ifc::export(&s.model)] {
        let i = ifc(file.as_bytes(), "x").unwrap();
        assert_eq!(phasing_of(&i.model), usual);
        assert!(held(&i.model).values().all(|v| v == &json!({})), "no element holds a phase or an option");
        // A set another tool calls its own, with a `Phase Created` in it, isn't BIMCraft's to read.
        let walls: Vec<String> =
            file.lines().filter(|l| l.contains("=IFCWALL(")).filter_map(|l| l.split('=').next().map(String::from)).collect();
        let foreign = format!(
            "#899990=IFCPROPERTYSINGLEVALUE('Phase Created',$,IFCLABEL('New Construction'),$);\n\
             #899991=IFCPROPERTYSET('0aaaaaaaaaaaaaaaaaaaaa',$,'Phasing',$,(#899990));\n\
             #899992=IFCRELDEFINESBYPROPERTIES('0bbbbbbbbbbbbbbbbbbbbb',$,$,$,({}),#899991);\n",
            walls.join(",")
        );
        let end = file.rfind("ENDSEC;").unwrap();
        let theirs = format!("{}{foreign}{}", &file[..end], &file[end..]);
        assert!(theirs.contains("'Phasing'"));
        let i = ifc(theirs.as_bytes(), "x").unwrap();
        assert_eq!(phasing_of(&i.model), usual);
        assert!(held(&i.model).values().all(|v| v == &json!({})), "{:?}", held(&i.model));
    }
}

#[test]
fn a_phasing_set_that_breaks_a_rule_is_left_out_and_the_elements_stay() {
    let file = phased_file();
    let base = ifc(file.as_bytes(), "x").unwrap();
    assert_eq!(base.skipped, Vec::<String>::new());
    let good = held(&base.model);
    let wall = |x, y| ("wall", (x, y));
    // The walls: the south one holds nothing, the east a phase, the north a phase and a demolition, the west an option.
    assert_eq!(good[&wall(8000, 3000)], json!({"Phase Created": "New Construction"}));
    assert_eq!(good[&wall(4000, 6000)], json!({"Phase Created": "Demolition", "Phase Demolished": "New Construction"}));
    assert_eq!(good[&wall(0, 3000)], json!({"Design Option": "Facade: Glass"}));
    let text = |name: &str, value: &str| format!("IFCPROPERTYSINGLEVALUE('{name}',$,IFCTEXT('{value}'),$)");
    // (what to change in the file, what is then left out of the model, and what the import says).
    let cases = [
        (
            text("Phase Created", "New Construction"),
            text("Phase Created", "Nowhere"),
            vec![(wall(8000, 3000), json!({}))],
            "phase or design option",
        ),
        (
            text("Phase Demolished", "New Construction"),
            text("Phase Demolished", "Existing"),
            vec![(wall(4000, 6000), json!({"Phase Created": "Demolition"}))],
            "before",
        ),
        (
            text("Design Option", "Facade: Glass"),
            text("Design Option", "Roof: Gable"),
            vec![(wall(0, 3000), json!({}))],
            "phase or design option",
        ),
        // A project that can't have these phases has none of the values that name a phase it doesn't.
        (
            "IFCTEXT('Existing'),IFCTEXT('Demolition')".into(),
            "IFCTEXT('Existing'),IFCTEXT('Existing')".into(),
            vec![
                (wall(4000, 6000), json!({"Phase Demolished": "New Construction"})),
                (wall(8000, 3000), json!({"Phase Created": "New Construction"})),
            ],
            "the project's phases",
        ),
        // A list with something in it that isn't a name.
        (
            "IFCTEXT('Existing'),IFCTEXT('Demolition')".into(),
            "IFCTEXT('Existing'),IFCINTEGER(7)".into(),
            vec![(wall(4000, 6000), json!({"Phase Demolished": "New Construction"}))],
            "the project's phases: it isn't a list of names",
        ),
        (r#"{"Facade":"#.into(), r#"{"Facade"#.into(), vec![(wall(0, 3000), json!({}))], "the project's design option sets"),
        (r#""primary":"Brick""#.into(), r#""primary":"Nope""#.into(), vec![(wall(0, 3000), json!({}))], "its primary"),
        (r#""temporary":"not-displayed""#.into(), r#""temporary":"gone""#.into(), vec![], "the project's phase filters"),
        ("[0,0,200]".into(), "[0,0,999]".into(), vec![], "the project's phase overrides"),
    ];
    for (from, to, left_out, says) in cases {
        assert!(file.contains(&from), "{from}");
        let i = ifc(file.replacen(&from, &to, 1).as_bytes(), "x").unwrap_or_else(|e| panic!("{to}: {e}"));
        assert!(i.skipped.iter().any(|s| s.contains(says)), "{to}: {says}: {:?}", i.skipped);
        let mut want = good.clone();
        want.extend(left_out);
        let (got, usual) = (held(&i.model), phasing_of(&Model::new("x")));
        assert_eq!(got, want, "{to}");
        // What can't be had is the project's own, not a mix: it has the usual one.
        let kept = phasing_of(&i.model);
        match says {
            s if s.starts_with("the project's phases") => assert_eq!(kept["phases"], usual["phases"]),
            "the project's phase filters" => assert_eq!(kept["filters"], usual["filters"]),
            "the project's phase overrides" => assert_eq!(kept["overrides"], usual["overrides"]),
            "the project's design option sets" | "its primary" => assert_eq!(kept["sets"], usual["sets"]),
            _ => assert_eq!(kept, phasing_of(&base.model), "{to}"),
        }
    }
}

/// `file` with every property called `name` holding `value`, in STEP.
fn with_value(file: &str, name: &str, value: &str) -> String {
    file.lines()
        .map(|l| match l.split_once('=') {
            Some((head, body)) if body.starts_with(&format!("IFCPROPERTYSINGLEVALUE('{name}',")) => {
                format!("{head}=IFCPROPERTYSINGLEVALUE('{name}',$,{value},$);")
            }
            Some((head, body)) if body.starts_with(&format!("IFCPROPERTYLISTVALUE('{name}',")) => {
                format!("{head}=IFCPROPERTYLISTVALUE('{name}',$,{value},$);")
            }
            _ => l.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn nothing_a_phasing_set_holds_stops_a_file_from_opening_or_takes_an_element_with_it() {
    let file = phased_file();
    let base = ifc(file.as_bytes(), "x").unwrap();
    let deep = format!("{}{}", "[".repeat(5000), "]".repeat(5000));
    let nasty = [
        "$".to_string(),
        "IFCTEXT('')".into(),
        "IFCTEXT('x')".into(),
        "IFCTEXT('{')".into(),
        "IFCTEXT('null')".into(),
        "IFCTEXT('[]')".into(),
        "IFCTEXT('{}')".into(),
        r#"IFCTEXT('{"a":1}')"#.into(),
        r#"IFCTEXT('"x"')"#.into(),
        r#"IFCTEXT('{"": {"options": [], "primary": ""}}')"#.into(),
        r#"IFCTEXT('{"S": {"options": ["a"], "primary": "a"}, "T: U": {"options": ["a"], "primary": "a"}}')"#.into(),
        format!("IFCTEXT('{deep}')"),
        format!("IFCTEXT('{}')", "x".repeat(300)),
        "IFCTEXT('\\X2\\D83D\\X0\\')".into(),
        "IFCINTEGER(7)".into(),
        "IFCREAL(1.5)".into(),
        "IFCBOOLEAN(.T.)".into(),
        "(IFCTEXT('a'),IFCTEXT('a'))".into(),
        format!("({})", vec!["IFCTEXT('p')"; 200].join(",")),
        "()".into(),
        "#999".into(),
    ];
    for name in ["Phases", "Phase Filters", "Phase Overrides", "Design Option Sets", "Phase Created", "Phase Demolished", "Design Option"] {
        for value in &nasty {
            let i = ifc(with_value(&file, name, value).as_bytes(), "x").unwrap_or_else(|e| panic!("{name} = {value:.60}: {e}"));
            assert_eq!(i.model.elements.len(), base.model.elements.len(), "{name} = {value:.60}");
            assert!(i.model.check().is_ok(), "{name} = {value:.60}");
        }
    }
}
