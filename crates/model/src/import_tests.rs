//! `import`'s tests that need only the model.
use crate::geom::P2;
use crate::geom::v3::{cross, dot};
use crate::import::*;
use crate::model::{Element, Function, Layer, MAX_POINTS, Model};
use crate::step::{self};

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
