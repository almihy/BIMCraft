//! `ifc`'s tests that drive the Session (moved out of `ifc.rs`).

use crate::model::Element;
use std::collections::BTreeMap;

use crate::engine::Session;
use crate::ifc::*;
use serde_json::json;
use std::collections::BTreeSet;

#[test]
fn step_strings_escape_quotes_backslashes_and_other_scripts() {
    assert_eq!(s("it's"), "'it''s'");
    assert_eq!(s(r"a\b"), r"'a\\b'");
    assert_eq!(s("Área"), r"'\X2\00C1\X0\rea'");
    // Past the Basic Multilingual Plane, UCS-4: as IFC readers expect, not UTF-16's surrogates.
    assert_eq!(s("Tile 😀"), r"'Tile \X4\0001F600\X0\'");
    assert_eq!(s("x');#9=IFCWALL("), "'x'');#9=IFCWALL('", "a name can't end the string early");
}

#[test]
fn reals_always_carry_a_decimal_point() {
    assert_eq!([real(1500.0), real(0.00001), real(-0.0), real(-200.5), real(f64::NAN)], ["1500.", "0.00001", "0.", "-200.5", "0."]);
}

#[test]
fn global_ids_are_22_base64_characters_and_stable() {
    let a = guid(7, 5, 12);
    assert_eq!(a.len(), 22);
    assert!(matches!(a.as_bytes()[0], b'0'..=b'3'), "{a}");
    assert!(a.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'$'), "{a}");
    assert_eq!(a, guid(7, 5, 12));
    let all: BTreeSet<String> = (0..2000).flat_map(|id| [guid(7, 5, id), guid(7, 6, id), guid(8, 5, id)]).collect();
    assert_eq!(all.len(), 6000);
}

#[test]
fn timestamps_are_utc_iso_dates() {
    assert_eq!(utc(0), "1970-01-01T00:00:00");
    assert_eq!(utc(951_868_800), "2000-03-01T00:00:00");
    assert_eq!(utc(1_000_000_000), "2001-09-09T01:46:40");
}

fn sample_ifc() -> String {
    let mut s = Session::new();
    s.script(crate::sample::SAMPLE).unwrap();
    export(&s.model)
}

#[test]
fn the_export_has_the_spatial_tree_and_every_element() {
    let f = sample_ifc();
    assert!(f.starts_with("ISO-10303-21;") && f.trim_end().ends_with("END-ISO-10303-21;"));
    assert!(f.contains("FILE_SCHEMA(('IFC4'));"));
    let n = |e: &str| f.matches(&format!("={e}(")).count();
    assert_eq!([n("IFCPROJECT"), n("IFCSITE"), n("IFCBUILDING"), n("IFCBUILDINGSTOREY")], [1, 1, 1, 2]);
    assert_eq!([n("IFCWALL"), n("IFCDOOR"), n("IFCWINDOW"), n("IFCSLAB"), n("IFCSPACE")], [7, 4, 5, 1, 4]);
    assert_eq!([n("IFCOPENINGELEMENT"), n("IFCRELVOIDSELEMENT"), n("IFCRELFILLSELEMENT")], [9, 9, 9]);
    assert!(f.contains("'Kitchen'"), "rooms keep their names");
}

#[test]
fn components_export_as_their_categorys_elements_with_their_materials() {
    let mut s = Session::new();
    s.script(
        r#"component {"family": "Table", "type": "1600 x 900", "at": [2000, 3000], "angle": 30}
           component {"family": "WC", "at": [500, 0], "mirrored": true}"#,
    )
    .unwrap();
    let f = export(&s.model);
    let n = |e: &str| f.matches(&format!("={e}(")).count();
    assert_eq!([n("IFCFURNITURE"), n("IFCSANITARYTERMINAL")], [1, 1]);
    assert!(f.contains("'Table:1600 x 900'"), "named by family and type, as Revit names them");
    // Each form a closed face set: the table's top and four legs, the WC's three parts.
    assert_eq!(n("IFCTRIANGULATEDFACESET"), 8);
    // Each material once, its colour styling the forms made of it.
    assert_eq!(n("IFCMATERIAL"), 2);
    assert!(f.contains("IFCMATERIAL('Timber'") && f.contains("IFCMATERIAL('Porcelain'"));
    assert_eq!([n("IFCSTYLEDITEM"), n("IFCRELASSOCIATESMATERIAL")], [2 + 8, 2]);
    // In their storey.
    let table = f.lines().find(|l| l.contains("=IFCFURNITURE(")).and_then(|l| l.split('=').next()).unwrap().to_string();
    assert!(
        f.lines()
            .any(|l| l.contains("IFCRELCONTAINEDINSPATIALSTRUCTURE(") && l.contains(&format!("{table},"))
                || l.contains(&format!("{table})")))
    );
}

#[test]
fn family_doors_and_windows_export_their_forms_as_face_sets_in_their_materials() {
    let mut s = Session::new();
    s.script(
        r#"wall {"points": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]], "closed": true}
           door {"at": [2000, 0], "family": "Single Door", "type": "800 x 2100"}
           window {"at": [8000, 3000], "family": "Casement Window"}
           door {"at": [5000, 0]}"#,
    )
    .unwrap();
    let f = export(&s.model);
    dangling(&f);
    let n = |e: &str| f.matches(&format!("={e}(")).count();
    assert_eq!([n("IFCDOOR"), n("IFCWINDOW"), n("IFCOPENINGELEMENT")], [2, 1, 3]);
    assert!(f.contains("'Single Door:800 x 2100'") && f.contains("'Casement Window:1200 x 1500'"), "named by family and type");
    assert!(f.contains("'Single 900 x 2100'"), "a project's own type, by its name");
    // The door's forms: its jambs, head, leaf and two handles; the window's: its frame's four, and two sashes (cut
    // round their panes) with a pane each.
    assert_eq!(n("IFCTRIANGULATEDFACESET"), 6 + 4 + 4);
    // Each material they're made of written once, with the families' door and window sharing Timber; each of the two made of its set.
    for material in ["Timber", "Metal", "Glass"] {
        assert!(f.contains(&format!("IFCMATERIAL('{material}'")), "{material}");
    }
    assert_eq!(n("IFCRELASSOCIATESMATERIAL"), 2, "the family door and the family window; the plain door has none");
}

#[test]
fn compound_curved_and_attached_walls_export_their_layers_and_shapes() {
    let mut s = Session::new();
    s.script(
        r#"type {"category": "wall", "name": "Cavity", "layers": [{"function": "finish", "material": "Brick", "thickness": 100}, {"function": "structure", "material": "Concrete block", "thickness": 200}]}
           wall {"points": [[0, 0], [0, 6000], [8000, 6000], [8000, 0]], "type": "Cavity"}
           wall {"points": [[8000, 0], [0, 0]], "arc": -60, "type": "Cavity", "location": "interior-finish"}
           window {"at": [4000, -1100]}
           roof {"gable": true, "boundary": [[-600, -1800], [8600, -1800], [8600, 6600], [-600, 6600]]}
           modify {"ids": [100002, 100004], "attach_top": 100007}
           door {"at": [4000, 6000]}"#,
    )
    .unwrap();
    let f = export(&s.model);
    let n = |e: &str| f.matches(&format!("={e}(")).count();
    assert_eq!([n("IFCMATERIAL"), n("IFCMATERIALLAYER"), n("IFCMATERIALLAYERSET")], [2, 2, 1]);
    // One use for the walls on their centre lines, one for the curved one on its interior face.
    assert_eq!([n("IFCMATERIALLAYERSETUSAGE"), n("IFCRELASSOCIATESMATERIAL")], [2, 2]);
    assert!(
        f.contains("IFCMATERIALLAYERSETUSAGE(") && f.contains(".AXIS2.,.NEGATIVE.,150.,$)") && f.contains(".AXIS2.,.NEGATIVE.,300.,$)")
    );
    assert!(f.contains("IFCMATERIALLAYER(#") && f.contains("'LoadBearing'") && f.contains("'Brick'"));
    // The walls attached to the roof, and the roof, as closed shells; the others swept up.
    assert_eq!(f.matches("'Body','Brep'").count(), 3);
    assert_eq!([n("IFCWALL"), n("IFCOPENINGELEMENT"), n("IFCWINDOW")], [4, 2, 1]);
    // Every hole, through a straight wall or a curved one, has a size.
    for line in f.lines().filter(|l| l.contains("=IFCRECTANGLEPROFILEDEF(")) {
        assert!(!line.contains(",0.)") && !line.contains(",0.,"), "{line}");
    }
    dangling(&f);
}

#[test]
fn a_rooms_colour_styles_the_solid_of_its_spaces_body() {
    let mut s = Session::new();
    s.script(
        r#"wall {"points": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]], "closed": true}
           room {"point": [4000, 3000]}"#,
    )
    .unwrap();
    let styled = |f: &str| f.matches("=IFCSTYLEDITEM(").count();
    let plain = styled(&export(&s.model));
    let room = s.model.elements.iter().find(|(_, e)| e.category() == "room").map(|(id, _)| *id).unwrap();
    s.execute("modify", &json!({"id": room, "colour": [214, 229, 210]})).unwrap();
    let f = export(&s.model);
    dangling(&f);
    assert_eq!(styled(&f), plain + 1, "one style, for the one room with a colour");
    // `#n=...` of reference `#n`, and the first reference in it after `after`.
    let line = |n: &str| f.lines().find(|l| l.starts_with(&format!("{n}="))).unwrap_or_else(|| panic!("no {n}")).to_string();
    let after =
        |l: &str, after: &str| format!("#{}", l.split_once(after).unwrap().1.chars().take_while(char::is_ascii_digit).collect::<String>());
    // The space's body, its shape's representation's solid: the item its style is on.
    let space = f.lines().find(|l| l.contains("=IFCSPACE(")).unwrap();
    let shape = space.split(',').nth(6).unwrap();
    let solid = after(&line(&after(&line(shape), "(#")), ",(#");
    assert!(line(&solid).contains("=IFCEXTRUDEDAREASOLID("), "{}", line(&solid));
    let item = f.lines().find(|l| l.contains(&format!("=IFCSTYLEDITEM({solid},"))).unwrap();
    // Its surface style shades it in the room's colour.
    let shading = after(&line(&after(item, ",(#")), ",(#");
    let colour = line(&after(&line(&shading), "(#"));
    let [r, g, b] = [214.0, 229.0, 210.0].map(|c: f64| real(c / 255.0));
    assert!(colour.ends_with(&format!("=IFCCOLOURRGB($,{r},{g},{b});")), "{colour}");
}

/// Every reference in `f` points at an entity written once.
fn dangling(f: &str) {
    let mut defined = BTreeSet::new();
    let mut referenced = BTreeSet::new();
    for line in f.lines().filter(|l| l.starts_with('#')) {
        let (head, body) = line.split_once('=').unwrap();
        assert!(defined.insert(head[1..].parse::<u64>().unwrap()), "{head} written twice");
        for r in body.split('#').skip(1) {
            referenced.insert(r.chars().take_while(char::is_ascii_digit).collect::<String>().parse::<u64>().unwrap());
        }
    }
    let dangling: Vec<_> = referenced.difference(&defined).collect();
    assert!(dangling.is_empty(), "dangling references: {dangling:?}");
}

#[test]
fn every_reference_points_at_an_entity_written_once() {
    let f = sample_ifc();
    let mut defined = BTreeSet::new();
    let mut referenced = BTreeSet::new();
    for line in f.lines().filter(|l| l.starts_with('#')) {
        let (head, body) = line.split_once('=').unwrap();
        assert!(defined.insert(head[1..].parse::<u64>().unwrap()), "{head} written twice");
        for r in body.split('#').skip(1) {
            referenced.insert(r.chars().take_while(char::is_ascii_digit).collect::<String>().parse::<u64>().unwrap());
        }
    }
    let dangling: Vec<_> = referenced.difference(&defined).collect();
    assert!(dangling.is_empty(), "dangling references: {dangling:?}");
}

/// The numbers of the entities of kind `kind` in `f`, in the order they're written.
fn of_kind(f: &str, kind: &str) -> Vec<u64> {
    f.lines().filter(|l| l.contains(&format!("={kind}("))).filter_map(|l| l[1..].split('=').next()?.parse().ok()).collect()
}

/// The properties of the `BIMCraft_Phasing` set of entity `#ent`, as written, in the set's order; none when it has no such set.
fn phasing_of(f: &str, ent: u64) -> Vec<String> {
    set_of(f, ent, PHASING)
}

/// The properties of set `name` of entity `#ent`, as written, in the set's order; none when it has no such set.
fn set_of(f: &str, ent: u64, name: &str) -> Vec<String> {
    let body = |n: &str| -> String {
        let line = f.lines().find(|l| l.starts_with(&format!("#{n}="))).unwrap_or_else(|| panic!("no #{n}"));
        line.split_once('=').unwrap().1.trim_end_matches(';').to_string()
    };
    for rel in f.lines().filter(|l| l.contains("=IFCRELDEFINESBYPROPERTIES(")) {
        // `#7=IFCRELDEFINESBYPROPERTIES('guid',$,$,$,(#12,#13),#40);`
        let (related, set) = rel.split_once("$,$,$,(").unwrap().1.split_once("),#").unwrap();
        if !related.split(',').any(|r| r == format!("#{ent}")) {
            continue;
        }
        let set = body(set.trim_end_matches(");").trim_end_matches(';').trim_end_matches(')'));
        if !set.contains(&format!("'{name}'")) {
            continue;
        }
        let props = set.rsplit_once(",(").unwrap().1.trim_end_matches(')');
        return props.split(',').map(|p| body(p.trim_start_matches('#'))).collect();
    }
    Vec::new()
}

/// A text property as BIMCraft writes one: `note` is its description, `$` for none.
fn text_property(name: &str, note: &str, value: &str) -> String {
    format!("IFCPROPERTYSINGLEVALUE('{name}',{note},IFCTEXT('{value}'),$)")
}

#[test]
fn phases_and_design_options_go_out_in_a_property_set_of_each_element_that_holds_one() {
    use crate::model_tests::graphics_testing::{house, run};
    let implied = s(IMPLIED);
    let mut s = Session::new();
    let walls = house(&mut s);
    let door = run(&mut s, "door", json!({"at": [2000, 0]}))["id"].as_u64().unwrap();
    run(&mut s, "phase", json!({"name": "Demolition", "before": "New Construction"}));
    run(&mut s, "option", json!({"set": "Facade", "options": ["Brick", "Glass"]}));
    for (id, held) in [
        (walls[1], json!({"Phase Created": "New Construction"})),
        (walls[2], json!({"Phase Demolished": "Demolition", "Design Option": "Facade: Glass"})),
        (walls[3], json!({"Phase Created": "Existing"})),
        (door, json!({"Phase Demolished": "Demolition"})),
    ] {
        run(&mut s, "modify", json!({"id": id, "params": held}));
    }
    let f = export(&s.model);
    let wall = of_kind(&f, "IFCWALL");
    assert_eq!(phasing_of(&f, wall[0]), Vec::<String>::new(), "made in the first phase and in no option: nothing to say");
    assert_eq!(phasing_of(&f, wall[1]), [text_property("Phase Created", "$", "New Construction")]);
    assert_eq!(
        phasing_of(&f, wall[2]),
        [
            text_property("Phase Created", &implied, "Existing"),
            text_property("Phase Demolished", "$", "Demolition"),
            text_property("Design Option", "$", "Facade: Glass"),
        ],
        "the first phase is named, and said to be implied"
    );
    assert_eq!(phasing_of(&f, wall[3]), [text_property("Phase Created", "$", "Existing")], "held, so not implied");
    // A door says only what it holds: naming a phase it holds none of would stop it following its wall.
    assert_eq!(phasing_of(&f, of_kind(&f, "IFCDOOR")[0]), [text_property("Phase Demolished", "$", "Demolition")]);
    // The project's phases in order, for another tool to read the elements' by, and its sets.
    let project = phasing_of(&f, of_kind(&f, "IFCPROJECT")[0]);
    assert_eq!(
        project,
        [
            "IFCPROPERTYLISTVALUE('Phases',$,(IFCTEXT('Existing'),IFCTEXT('Demolition'),IFCTEXT('New Construction')),$)".to_string(),
            text_property("Design Option Sets", "$", r#"{"Facade":{"options":["Brick","Glass"],"primary":"Brick"}}"#),
        ],
        "its filters and overrides are the usual ones, left out"
    );
    assert_eq!(f.matches(&format!("'{PHASING}'")).count(), 3 + 1 + 1, "three walls and the door hold a value; and the project");
    dangling(&f);
}

#[test]
fn a_phased_projects_elements_say_what_the_work_does_to_them_in_their_common_set() {
    use crate::model_tests::graphics_testing::{house, run};
    let status = |v: &str| format!("IFCPROPERTYENUMERATEDVALUE('Status',$,(IFCLABEL('{v}')),$)");
    let mut s = Session::new();
    let walls = house(&mut s);
    run(&mut s, "door", json!({"at": [2000, 0]}));
    let window = run(&mut s, "window", json!({"at": [0, 3000]}))["id"].as_u64().unwrap();
    run(&mut s, "phase", json!({"name": "Demolition", "before": "New Construction"}));
    for (id, held) in [
        (walls[0], json!({"Phase Demolished": "Demolition"})),
        (walls[1], json!({"Phase Created": "New Construction"})),
        (walls[2], json!({"Phase Created": "Demolition", "Phase Demolished": "Demolition"})),
        (window, json!({"Phase Created": "New Construction"})),
    ] {
        run(&mut s, "modify", json!({"id": id, "params": held}));
    }
    let f = export(&s.model);
    let wall = of_kind(&f, "IFCWALL");
    let common = |ent| set_of(&f, ent, "Pset_WallCommon");
    assert_eq!(common(wall[0]), [status("DEMOLISH")]);
    assert_eq!(common(wall[1]), [status("NEW")]);
    assert_eq!(common(wall[2]), [status("TEMPORARY")], "made and taken down in the same phase");
    assert_eq!(common(wall[3]), [status("EXISTING")], "no value is the first phase");
    // A door with no phase of its own follows its wall; a window holding one says its own.
    assert_eq!(set_of(&f, of_kind(&f, "IFCDOOR")[0], "Pset_DoorCommon"), [status("DEMOLISH")]);
    assert_eq!(set_of(&f, of_kind(&f, "IFCWINDOW")[0], "Pset_WindowCommon"), [status("NEW")]);
    dangling(&f);
}

/// The occurrences an `IfcRelDefinesByType` of the type object `#kind` lists.
fn occurrences_of(f: &str, kind: u64) -> Vec<u64> {
    let rel = f.lines().find(|l| l.contains("=IFCRELDEFINESBYTYPE(") && l.trim_end_matches(';').ends_with(&format!(",#{kind})"))).unwrap();
    let list = rel.split_once("$,$,$,(").unwrap().1.split_once("),#").unwrap().0;
    list.split(',').map(|r| r.trim_start_matches('#').parse().unwrap()).collect()
}

#[test]
fn each_type_is_written_once_and_related_to_its_occurrences() {
    use crate::model_tests::graphics_testing::{house, run};
    let mut s = Session::new();
    house(&mut s);
    run(&mut s, "type", json!({"category": "beam", "name": "B 150x600", "width": 150, "depth": 600}));
    for y in [0, 4000] {
        run(&mut s, "beam", json!({"points": [[0, y], [5000, y]], "type": "B 150x600", "offset": 3000}));
    }
    run(&mut s, "structural_column", json!({"at": [0, 0], "width": 300, "depth": 300}));
    run(&mut s, "floor", json!({"boundary": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]]}));
    let f = export(&s.model);
    let near = |a: f64, b: f64| (a - b).abs() < 1e-6 * b.abs().max(1.0);
    let beams = of_kind(&f, "IFCBEAM");
    assert_eq!(beams.len(), 2);
    let kinds = of_kind(&f, "IFCBEAMTYPE");
    assert_eq!(kinds.len(), 1, "one type for the two beams");
    assert!(f.contains("IFCBEAMTYPE(") && f.contains("'B 150x600',$,$,$,$,$,$,.BEAM.)"));
    assert_eq!(occurrences_of(&f, kinds[0]), beams);
    let q = quantities_of(&f, beams[0], "Qto_BeamBaseQuantities");
    assert!(near(q["Length"], 5000.0) && near(q["CrossSectionArea"], 0.09) && !q.contains_key("NetVolume"), "{q:?}");
    assert!(near(q["GrossVolume"], 0.45) && near(q["OuterSurfaceArea"], 2.0 * (0.15 + 0.6) * 5.0), "the sides only: {q:?}");
    assert!(!q.contains_key("GrossSurfaceArea"), "outer surface and section are given apart: {q:?}");
    let columns = of_kind(&f, "IFCCOLUMN");
    let types = of_kind(&f, "IFCCOLUMNTYPE");
    assert_eq!((columns.len(), types.len()), (1, 1));
    assert_eq!(occurrences_of(&f, types[0]), columns);
    let q = quantities_of(&f, columns[0], "Qto_ColumnBaseQuantities");
    assert!(near(q["CrossSectionArea"], 0.09) && near(q["GrossVolume"], 0.09 * q["Length"] / 1e3), "{q:?}");
    assert!(near(q["OuterSurfaceArea"], 4.0 * 0.3 * q["Length"] / 1e3) && !q.contains_key("GrossSurfaceArea"), "{q:?}");
    // Walls and slabs have theirs, one for each type used.
    let walls: BTreeSet<String> =
        s.model.elements.iter().filter_map(|(_, e)| if let Element::Wall(w) = e { Some(w.type_name.clone()) } else { None }).collect();
    assert_eq!(of_kind(&f, "IFCWALLTYPE").len(), walls.len());
    assert_eq!(of_kind(&f, "IFCSLABTYPE").len(), 1, "one type for the floor");
    // The same on every export, so a diff of two shows only what changed.
    assert_eq!(f, export(&s.model));
    dangling(&f);
}

/// The entity lines of kind `class` in `f`, as written.
fn entities(f: &str, class: &str) -> Vec<String> {
    f.lines().filter(|l| l.contains(&format!("={class}("))).map(str::to_string).collect()
}

/// The attributes of an entity line, as the STEP text spells them (strings with their quotes), split at the commas
/// that are outside strings.
fn attributes(line: &str) -> Vec<String> {
    let body = line.split_once('(').unwrap().1.strip_suffix(");").unwrap();
    let (mut out, mut cur, mut quoted) = (Vec::new(), String::new(), false);
    for c in body.chars() {
        match c {
            '\'' => {
                quoted = !quoted;
                cur.push(c);
            }
            ',' if !quoted => out.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    out.push(cur);
    out
}

/// The attributes of the type object of `class` named `name` in `f`; there is one.
fn type_object(f: &str, class: &str, name: &str) -> Vec<String> {
    let named: Vec<Vec<String>> =
        entities(f, class).iter().map(|l| attributes(l.as_str())).filter(|a| a[2] == format!("'{name}'")).collect();
    assert_eq!(named.len(), 1, "one {class} named {name}");
    named.into_iter().next().unwrap()
}

/// An entity line's attributes, split at the commas outside strings and outside parentheses: a list is one attribute.
fn top_level_attributes(line: &str) -> Vec<String> {
    let body = line.split_once('(').unwrap().1.strip_suffix(");").unwrap();
    let (mut out, mut cur, mut quoted, mut depth) = (Vec::new(), String::new(), false, 0);
    for c in body.chars() {
        match c {
            '\'' => {
                quoted = !quoted;
                cur.push(c);
            }
            '(' if !quoted => {
                depth += 1;
                cur.push(c);
            }
            ')' if !quoted => {
                depth -= 1;
                cur.push(c);
            }
            ',' if !quoted && depth == 0 => out.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    out.push(cur);
    out
}

/// The properties of the set `name` that type object `t` (the line `type_object` found) lists in its `HasPropertySets`, as
/// written, in the set's order; none when it has no such set.
fn type_set_of(f: &str, t: &str, name: &str) -> Vec<String> {
    let entity = |n: &str| -> String {
        let line = f.lines().find(|l| l.starts_with(&format!("{n}="))).unwrap_or_else(|| panic!("no {n}"));
        line.to_string()
    };
    let list = |a: &str| -> Vec<String> {
        a.trim_matches(|c| c == '(' || c == ')').split(',').filter(|x| !x.is_empty() && *x != "$").map(str::to_string).collect()
    };
    for set in list(&top_level_attributes(t)[5]) {
        let attrs = top_level_attributes(&entity(&set));
        if attrs[2] == format!("'{name}'") {
            return list(&attrs[4]).iter().map(|p| entity(p).split_once('=').unwrap().1.trim_end_matches(';').to_string()).collect();
        }
    }
    Vec::new()
}

/// The line of the one type object of `class` named `name`.
fn type_line(f: &str, class: &str, name: &str) -> String {
    let named: Vec<String> = entities(f, class).into_iter().filter(|l| attributes(l.as_str())[2] == format!("'{name}'")).collect();
    assert_eq!(named.len(), 1, "one {class} named {name}");
    named.into_iter().next().unwrap()
}

/// The type name of the first element of `category` in the model: a door's, window's, ceiling's or roof's.
fn type_name_of(s: &Session, category: &str) -> String {
    s.model
        .elements
        .values()
        .filter(|e| e.category() == category)
        .find_map(|e| match e {
            Element::Door(o) | Element::Window(o) => Some(o.type_name.clone()),
            Element::Ceiling(c) => Some(c.type_name.clone()),
            Element::Roof(r) => Some(r.type_name.clone()),
            _ => None,
        })
        .unwrap()
}

/// The type objects of `type_class` in `f`, once every `class` occurrence in `f` is related to one of them and to no
/// other: each occurrence listed by exactly one `IfcRelDefinesByType`.
fn typed_once(f: &str, class: &str, type_class: &str) -> Vec<u64> {
    let types = of_kind(f, type_class);
    let mut related: Vec<u64> = types.iter().flat_map(|t| occurrences_of(f, *t)).collect();
    related.sort_unstable();
    let mut all = of_kind(f, class);
    all.sort_unstable();
    assert_eq!(related, all, "each {class} related to exactly one {type_class}");
    types
}

#[test]
fn doors_windows_roofs_and_ceilings_get_type_objects_each_occurrence_related_once() {
    use crate::model_tests::graphics_testing::{house, run};
    let mut s = Session::new();
    house(&mut s);
    run(&mut s, "door", json!({"at": [2000, 0]}));
    run(&mut s, "door", json!({"at": [5000, 0]}));
    run(&mut s, "window", json!({"at": [8000, 3000]}));
    run(&mut s, "ceiling", json!({"point": [4000, 3000]}));
    run(&mut s, "roof", json!({}));
    let f = export(&s.model);
    dangling(&f);
    // A door's type carries, after its PredefinedType, its OperationType, whether the occurrence's own parameters take
    // precedence (its sizes are its own: yes) and a user-defined operation (none).
    assert_eq!(typed_once(&f, "IFCDOOR", "IFCDOORTYPE").len(), 1, "one type for both doors");
    let door = type_object(&f, "IFCDOORTYPE", &type_name_of(&s, "door"));
    assert_eq!(door.len(), 13, "IfcDoorType's attributes");
    assert_eq!(door[9..].join(","), ".DOOR.,.SINGLE_SWING_LEFT.,.T.,$");
    // A window's type carries its PartitioningType the same way.
    assert_eq!(typed_once(&f, "IFCWINDOW", "IFCWINDOWTYPE").len(), 1);
    let window = type_object(&f, "IFCWINDOWTYPE", &type_name_of(&s, "window"));
    assert_eq!(window.len(), 13, "IfcWindowType's attributes");
    assert_eq!(window[9..].join(","), ".WINDOW.,.SINGLE_PANEL.,.T.,$");
    assert_eq!(typed_once(&f, "IFCCOVERING", "IFCCOVERINGTYPE").len(), 1);
    let ceiling = type_object(&f, "IFCCOVERINGTYPE", &type_name_of(&s, "ceiling"));
    assert_eq!((ceiling.len(), ceiling[9].as_str()), (10, ".CEILING."));
    assert_eq!(typed_once(&f, "IFCROOF", "IFCROOFTYPE").len(), 1);
    let roof = type_object(&f, "IFCROOFTYPE", &type_name_of(&s, "roof"));
    assert_eq!((roof.len(), roof[9].as_str()), (10, ".NOTDEFINED."), "a roof sloping 30 degrees, the default");
}

#[test]
fn a_roof_type_is_flat_only_when_every_roof_of_it_is_flat() {
    use crate::model_tests::graphics_testing::run;
    let mut s = Session::new();
    run(&mut s, "roof", json!({"boundary": [[0, 0], [4000, 0], [4000, 4000], [0, 4000]], "slope": 0}));
    let f = export(&s.model);
    dangling(&f);
    assert_eq!(typed_once(&f, "IFCROOF", "IFCROOFTYPE").len(), 1);
    assert_eq!(type_object(&f, "IFCROOFTYPE", &type_name_of(&s, "roof"))[9], ".FLAT_ROOF.");
    // A sloped roof of the same type: the type is no longer flat, and both roofs are of it.
    run(&mut s, "roof", json!({"boundary": [[10000, 0], [14000, 0], [14000, 4000], [10000, 4000]]}));
    let f = export(&s.model);
    dangling(&f);
    assert_eq!(typed_once(&f, "IFCROOF", "IFCROOFTYPE").len(), 1, "one type for both roofs");
    assert_eq!(type_object(&f, "IFCROOFTYPE", &type_name_of(&s, "roof"))[9], ".NOTDEFINED.");
}

#[test]
fn components_get_type_objects_of_the_class_each_is_written_as_with_their_attribute_counts() {
    use crate::model_tests::graphics_testing::run;
    // A starter family of each class a component is written as.
    let families = [
        ("Table", "IFCFURNITURE"),
        ("Kitchen Sink", "IFCSANITARYTERMINAL"),
        ("Floor Drain", "IFCWASTETERMINAL"),
        ("Elbow 90", "IFCPIPEFITTING"),
        ("Gate Valve", "IFCVALVE"),
        ("Distribution Board", "IFCELECTRICDISTRIBUTIONBOARD"),
        ("Outlet", "IFCOUTLET"),
        ("Switch", "IFCSWITCHINGDEVICE"),
        ("Ceiling Luminaire", "IFCLIGHTFIXTURE"),
        ("Air Terminal", "IFCBUILDINGELEMENTPROXY"),
    ];
    let mut s = Session::new();
    for (i, (family, _)) in families.iter().enumerate() {
        run(&mut s, "component", json!({"family": family, "at": [i as f64 * 2000.0, 0.0]}));
    }
    let f = export(&s.model);
    dangling(&f);
    for (family, class) in families {
        let type_class = format!("{class}TYPE");
        typed_once(&f, class, &type_class);
        let type_name = s
            .model
            .elements
            .values()
            .find_map(|e| match e {
                Element::Component(cp) if cp.family == family => Some(cp.type_name.clone()),
                _ => None,
            })
            .unwrap();
        let attrs = type_object(&f, &type_class, &format!("{family}:{type_name}"));
        // Furniture's type has its AssemblyPlace (mandatory) ahead of its PredefinedType; the others have ten attributes.
        let count = if class == "IFCFURNITURE" { 11 } else { 10 };
        // Its predefined type is its family's kind's (a kitchen sink's .SINK.), or not defined.
        let fam = s.model.families.get(family).unwrap();
        let predefined = if fam.kind.is_some() { fam.ifc().1 } else { ".NOTDEFINED.".into() };
        assert_eq!((attrs.len(), attrs[count - 1].as_str()), (count, predefined.as_ref()), "{type_class}");
        if class == "IFCFURNITURE" {
            assert_eq!(attrs[9], ".NOTDEFINED.", "its assembly place");
        }
    }
}

#[test]
fn discipline_runs_get_the_type_objects_of_their_segment_classes() {
    use crate::model_tests::graphics_testing::run;
    let mut s = Session::new();
    run(&mut s, "pipe", json!({"points": [[0, 6000], [6000, 6000]], "offset": 500}));
    run(&mut s, "duct", json!({"points": [[0, 7000], [6000, 7000]], "offset": 2500}));
    run(&mut s, "conduit", json!({"points": [[0, 8000], [6000, 8000]], "offset": 2500}));
    run(&mut s, "cable_tray", json!({"points": [[0, 9000], [6000, 9000]], "offset": 2700}));
    run(&mut s, "wire", json!({"points": [[0, 10000], [6000, 10000]], "offset": 2500}));
    run(&mut s, "cabling", json!({"points": [[0, 11000], [6000, 11000]], "offset": 2500}));
    let f = export(&s.model);
    dangling(&f);
    // The PredefinedType of each type is the one its runs are written with (the last attribute of each type object).
    let kinds = |class: &str| -> Vec<String> { entities(&f, class).iter().map(|l| attributes(l).pop().unwrap()).collect() };
    typed_once(&f, "IFCPIPESEGMENT", "IFCPIPESEGMENTTYPE");
    assert_eq!(kinds("IFCPIPESEGMENTTYPE"), [".RIGIDSEGMENT."]);
    typed_once(&f, "IFCDUCTSEGMENT", "IFCDUCTSEGMENTTYPE");
    assert_eq!(kinds("IFCDUCTSEGMENTTYPE"), [".RIGIDSEGMENT."]);
    // A conduit and a cable tray are both cable carrier segments, each its own type.
    assert_eq!(typed_once(&f, "IFCCABLECARRIERSEGMENT", "IFCCABLECARRIERSEGMENTTYPE").len(), 2);
    let mut carriers = kinds("IFCCABLECARRIERSEGMENTTYPE");
    carriers.sort();
    assert_eq!(carriers, [".CABLETRAYSEGMENT.", ".CONDUITSEGMENT."]);
    // A wire and structured cabling are both cable segments.
    typed_once(&f, "IFCCABLESEGMENT", "IFCCABLESEGMENTTYPE");
    let cables = kinds("IFCCABLESEGMENTTYPE");
    assert!(!cables.is_empty() && cables.iter().all(|k| k == ".CABLESEGMENT."), "{cables:?}");
}

/// The whole line of entity `#n` in `f`, as written.
fn line(f: &str, n: u64) -> String {
    let head = format!("#{n}=");
    f.lines().find(|l| l.starts_with(&head)).unwrap_or_else(|| panic!("no #{n}")).to_string()
}

/// The number a STEP reference names: `#12` is 12.
fn number(reference: &str) -> u64 {
    reference.trim_start_matches('#').parse().unwrap_or_else(|_| panic!("not a reference: {reference}"))
}

/// What `IfcRelAssociatesMaterial` gives entity `#ent`: the number of the material, set or usage. There is exactly one.
fn material_of(f: &str, ent: u64) -> u64 {
    let given: Vec<u64> = f
        .lines()
        .filter(|l| l.contains("=IFCRELASSOCIATESMATERIAL("))
        .filter_map(|l| {
            let (list, material) = l.split_once("$,$,$,(")?.1.split_once("),#")?;
            list.split(',').any(|r| r == format!("#{ent}")).then(|| number(material.trim_end_matches(");")))
        })
        .collect();
    assert_eq!(given.len(), 1, "#{ent} is associated with one material, set or usage");
    given[0]
}

/// The one profile of material profile set `#set`: its material (none when `$`) and its profile definition, by number.
fn the_profile(f: &str, set: u64) -> (Option<u64>, u64) {
    let set = attributes(&line(f, set));
    assert_eq!(set.len(), 4, "IfcMaterialProfileSet's attributes");
    let profiles = set[2].trim_matches(|c| c == '(' || c == ')');
    assert!(!profiles.contains(','), "one profile: {profiles}");
    let profile = attributes(&line(f, number(profiles)));
    assert_eq!(profile.len(), 6, "IfcMaterialProfile's attributes");
    ((profile[2] != "$").then(|| number(&profile[2])), number(&profile[3]))
}

/// The set and the cardinal point of the usage entity `#ent` is associated with.
fn usage_of(f: &str, ent: u64) -> (u64, String) {
    let usage = line(f, material_of(f, ent));
    assert!(usage.contains("=IFCMATERIALPROFILESETUSAGE("), "{usage}");
    let a = attributes(&usage);
    assert_eq!((a.len(), a[2].as_str()), (3, "$"), "IfcMaterialProfileSetUsage's attributes: {usage}");
    (number(&a[0]), a[1].clone())
}

/// The set of the type object `#kind`, its profile definition's line and its material's line (empty when it has none).
fn section_of(f: &str, kind: u64) -> (u64, String, String) {
    let set = material_of(f, kind);
    assert!(line(f, set).contains("=IFCMATERIALPROFILESET("), "{}", line(f, set));
    let (material, profile) = the_profile(f, set);
    (set, line(f, profile), material.map(|m| line(f, m)).unwrap_or_default())
}

#[test]
fn a_beam_type_holds_its_section_in_a_material_profile_set_that_each_beam_uses() {
    use crate::model_tests::graphics_testing::{house, run};
    let mut s = Session::new();
    house(&mut s);
    run(&mut s, "type", json!({"category": "beam", "name": "B 150x600", "width": 150, "depth": 600, "material": "Concrete"}));
    run(&mut s, "beam", json!({"points": [[0, 0], [5000, 0]], "type": "B 150x600", "offset": 3000}));
    run(&mut s, "beam", json!({"points": [[0, 4000], [5000, 4000]], "type": "B 150x600", "offset": 3000, "justify": "bottom"}));
    let f = export(&s.model);
    dangling(&f);
    // The type holds one set: one profile, of the type's material, a 150 by 600 rectangle.
    let kind = of_kind(&f, "IFCBEAMTYPE");
    assert_eq!(kind.len(), 1);
    let (set, profile, material) = section_of(&f, kind[0]);
    assert_eq!(of_kind(&f, "IFCMATERIALPROFILESET"), [set], "one set for the type");
    assert!(line(&f, set).contains("=IFCMATERIALPROFILESET('B 150x600',$,(#"), "{}", line(&f, set));
    assert!(material.contains("=IFCMATERIAL('Concrete'"), "{material}");
    assert!(profile.contains("=IFCRECTANGLEPROFILEDEF(.AREA."), "{profile}");
    let rectangle = attributes(&profile);
    assert_eq!((rectangle[3].as_str(), rectangle[4].as_str()), ("150.", "600."), "{profile}");
    // Both beams use the set, each by a usage with the cardinal point it is placed by: the top of its section (a beam's
    // default) is 8, its bottom 2.
    let beams = of_kind(&f, "IFCBEAM");
    assert_eq!(beams.len(), 2);
    let usages: Vec<(u64, String)> = beams.iter().map(|b| usage_of(&f, *b)).collect();
    assert!(usages.iter().all(|(of, _)| *of == set), "{usages:?}");
    let mut points: Vec<&str> = usages.iter().map(|(_, c)| c.as_str()).collect();
    points.sort_unstable();
    assert_eq!(points, ["2", "8"]);
    // The profile is written once, and both bodies sweep it.
    assert_eq!(of_kind(&f, "IFCRECTANGLEPROFILEDEF").len(), 1);
    let shared = the_profile(&f, set).1;
    let swept = entities(&f, "IFCEXTRUDEDAREASOLID").iter().filter(|l| attributes(l)[0] == format!("#{shared}")).count();
    assert_eq!(swept, 2, "both bodies sweep the type's profile");
    // The material is in the set, so a beam has no other: one for the type, one for each cardinal point's usage.
    assert_eq!(of_kind(&f, "IFCRELASSOCIATESMATERIAL").len(), 3);
    assert_eq!(f, export(&s.model), "the same on every export");
}

#[test]
fn a_beam_type_with_no_material_gives_its_profile_none_and_a_round_one_a_circle() {
    use crate::model_tests::graphics_testing::run;
    let mut s = Session::new();
    run(&mut s, "type", json!({"category": "beam", "name": "Plain", "width": 120, "depth": 300}));
    run(&mut s, "beam", json!({"points": [[0, 0], [4000, 0]], "type": "Plain", "offset": 3000, "justify": "centre"}));
    run(&mut s, "type", json!({"category": "beam", "name": "Tube 160", "width": 160, "round": true, "material": "Metal"}));
    run(&mut s, "beam", json!({"points": [[0, 2000], [4000, 2000]], "type": "Tube 160", "offset": 3000}));
    let f = export(&s.model);
    dangling(&f);
    let kinds = of_kind(&f, "IFCBEAMTYPE");
    assert_eq!(kinds.len(), 2);
    let by_name = |name: &str| *kinds.iter().find(|k| line(&f, **k).contains(&format!("'{name}'"))).unwrap();
    let (_, profile, material) = section_of(&f, by_name("Plain"));
    assert_eq!(material, "", "no material, no IfcMaterial");
    assert!(attributes(&profile)[3] == "120." && attributes(&profile)[4] == "300.", "{profile}");
    // A round section is a circle of half its diameter, as its body is.
    let (_, tube, material) = section_of(&f, by_name("Tube 160"));
    assert!(tube.contains("=IFCCIRCLEPROFILEDEF(.AREA."), "{tube}");
    assert_eq!(attributes(&tube)[3], "80.", "{tube}");
    assert!(material.contains("=IFCMATERIAL('Metal'"), "{material}");
    // Centred on the line it was drawn at is 5.
    let plain = of_kind(&f, "IFCBEAM").into_iter().find(|b| occurrences_of(&f, by_name("Plain")).contains(b)).unwrap();
    assert_eq!(usage_of(&f, plain).1, "5");
}

#[test]
fn a_columns_type_holds_a_circle_of_its_diameter_or_a_rectangle_and_its_columns_use_it() {
    use crate::model_tests::graphics_testing::run;
    let mut s = Session::new();
    run(&mut s, "structural_column", json!({"at": [0, 0], "type": "Concrete Round 400"}));
    run(&mut s, "structural_column", json!({"at": [3000, 0], "type": "Concrete Round 400"}));
    run(&mut s, "structural_column", json!({"at": [6000, 0], "type": "Concrete 200 x 500"}));
    run(&mut s, "column", json!({"at": [9000, 0]}));
    let f = export(&s.model);
    dangling(&f);
    let types = of_kind(&f, "IFCCOLUMNTYPE");
    assert_eq!(types.len(), 3, "the round, the 200 by 500 and the default column");
    let mut circles = 0;
    for kind in &types {
        let (set, profile, material) = section_of(&f, *kind);
        assert_eq!(material, "", "a column's type has no material");
        // Every column of the type uses its set, centred: the mid-depth centre, 5.
        for column in occurrences_of(&f, *kind) {
            assert_eq!(usage_of(&f, column), (set, "5".to_string()));
        }
        if profile.contains("=IFCCIRCLEPROFILEDEF(") {
            circles += 1;
            // 400 across: a radius of 200, for both columns of that type.
            assert_eq!(attributes(&profile)[3], "200.", "{profile}");
            assert_eq!(occurrences_of(&f, *kind).len(), 2);
        }
    }
    assert_eq!(circles, 1);
    assert!(entities(&f, "IFCRECTANGLEPROFILEDEF").iter().any(|r| attributes(r)[3] == "200." && attributes(r)[4] == "500."));
    // Three usages: the two round columns share one, all five columns were related once.
    assert_eq!(of_kind(&f, "IFCMATERIALPROFILESETUSAGE").len(), 3);
}

#[test]
fn a_pad_footings_type_has_a_profile_set_and_a_strip_footing_or_a_pipe_has_none() {
    use crate::model_tests::graphics_testing::run;
    let mut s = Session::new();
    run(&mut s, "footing", json!({"at": [0, 0]}));
    run(&mut s, "strip_footing", json!({"points": [[0, 3000], [4000, 3000]]}));
    run(&mut s, "pipe", json!({"points": [[0, 6000], [4000, 6000]], "offset": 500}));
    let f = export(&s.model);
    dangling(&f);
    let kind = of_kind(&f, "IFCFOOTINGTYPE");
    assert_eq!(kind.len(), 1, "the pad's type; the strip footing has none");
    let (set, profile, _) = section_of(&f, kind[0]);
    assert_eq!(of_kind(&f, "IFCMATERIALPROFILESET"), [set]);
    let pad = s.model.types.footing.values().next().unwrap();
    assert!(profile.contains("=IFCRECTANGLEPROFILEDEF("), "{profile}");
    assert_eq!(attributes(&profile)[3..5], [real(pad.width), real(pad.depth)]);
    let footing = occurrences_of(&f, kind[0]);
    assert_eq!(usage_of(&f, footing[0]), (set, "5".to_string()));
    // A strip footing and a pipe keep the plain material they have.
    for class in ["IFCFOOTING", "IFCPIPESEGMENT"] {
        let plain: Vec<String> =
            of_kind(&f, class).into_iter().filter(|e| !footing.contains(e)).map(|e| line(&f, material_of(&f, e))).collect();
        assert_eq!(plain.len(), 1, "{class}");
        assert!(plain[0].contains("=IFCMATERIAL("), "{}", plain[0]);
    }
}

#[test]
fn types_of_one_name_and_two_sections_stay_two_types_each_with_its_own_set_and_global_ids() {
    use crate::model_tests::graphics_testing::run;
    let mut s = Session::new();
    // The architectural and the structural column categories each have their own types; both may call one "C 300".
    run(&mut s, "type", json!({"category": "column", "name": "C 300", "width": 300, "depth": 300}));
    run(&mut s, "type", json!({"category": "structuralcolumn", "name": "C 300", "width": 250, "depth": 600}));
    run(&mut s, "column", json!({"at": [0, 0], "type": "C 300"}));
    run(&mut s, "structural_column", json!({"at": [3000, 0], "type": "C 300"}));
    let f = export(&s.model);
    dangling(&f);
    let kinds = of_kind(&f, "IFCCOLUMNTYPE");
    assert_eq!(kinds.len(), 2, "two sections, two types");
    let mut sections: Vec<(String, String)> = kinds
        .iter()
        .map(|k| {
            let (_, profile, _) = section_of(&f, *k);
            (attributes(&profile)[3].clone(), attributes(&profile)[4].clone())
        })
        .collect();
    sections.sort();
    assert_eq!(sections, [("250.".to_string(), "600.".to_string()), ("300.".to_string(), "300.".to_string())]);
    // No two entities of the file share a GlobalId.
    let mut ids = BTreeSet::new();
    for l in f.lines().filter(|l| l.contains("=IFCREL") || l.contains("TYPE(") || l.contains("=IFCCOLUMN(")) {
        let first = attributes(l).swap_remove(0);
        if first.len() == 24 && first.starts_with('\'') {
            assert!(ids.insert(first.clone()), "GlobalId {first} is written twice: {l}");
        }
    }
}

#[test]
fn every_type_object_of_the_sample_house_relates_occurrences_of_its_class_once() {
    let f = sample_ifc();
    dangling(&f);
    let type_classes: BTreeSet<String> = f
        .lines()
        .filter_map(|l| {
            let class = l.split_once('=')?.1.split_once('(')?.0;
            (class.starts_with("IFC") && !class.starts_with("IFCREL") && class.ends_with("TYPE")).then(|| class.to_string())
        })
        .collect();
    let expected = ["IFCCOVERINGTYPE", "IFCDOORTYPE", "IFCFURNITURETYPE", "IFCROOFTYPE", "IFCSLABTYPE", "IFCWALLTYPE", "IFCWINDOWTYPE"];
    assert_eq!(type_classes, expected.map(str::to_string).into_iter().collect::<BTreeSet<_>>());
    let mut listed = BTreeSet::new();
    for class in &type_classes {
        let occurrence = class.strip_suffix("TYPE").unwrap();
        let of_class = of_kind(&f, occurrence);
        for t in of_kind(&f, class) {
            for o in occurrences_of(&f, t) {
                assert!(of_class.contains(&o), "#{o} is listed by {class} but is not a {occurrence}");
                assert!(listed.insert(o), "#{o} is related to more than one type");
            }
        }
        // Attributes IFC4 gives the type: thirteen for a door's or a window's, eleven for a furniture's, else ten.
        let count = match class.as_str() {
            "IFCDOORTYPE" | "IFCWINDOWTYPE" => 13,
            "IFCFURNITURETYPE" => 11,
            _ => 10,
        };
        for line in entities(&f, class) {
            assert_eq!(attributes(&line).len(), count, "{line}");
        }
    }
}

#[test]
fn a_structural_elements_common_set_holds_both_what_it_carries_and_its_status() {
    use crate::model_tests::graphics_testing::run;
    let mut s = Session::new();
    let col = run(&mut s, "structural_column", json!({"at": [0, 0]}))["id"].as_u64().unwrap();
    run(&mut s, "modify", json!({"id": col, "params": {"Phase Created": "New Construction"}}));
    let f = export(&s.model);
    let ent = of_kind(&f, "IFCCOLUMN")[0];
    assert_eq!(
        set_of(&f, ent, "Pset_ColumnCommon"),
        ["IFCPROPERTYSINGLEVALUE('LoadBearing',$,IFCBOOLEAN(.T.),$)", "IFCPROPERTYENUMERATEDVALUE('Status',$,(IFCLABEL('NEW')),$)"],
        "one set, not two of the same name"
    );
    assert_eq!(f.matches("'Pset_ColumnCommon'").count(), 1);
    dangling(&f);
}

#[test]
fn a_project_with_no_phasing_gives_no_status() {
    use crate::model_tests::graphics_testing::{house, run};
    let mut s = Session::new();
    house(&mut s);
    run(&mut s, "structural_column", json!({"at": [0, 0]}));
    let f = export(&s.model);
    assert!(!f.contains("'Status'"), "all of it is new work: nothing to say");
    assert!(!f.contains("'Pset_WallCommon'"));
    assert_eq!(set_of(&f, of_kind(&f, "IFCCOLUMN")[0], "Pset_ColumnCommon"), ["IFCPROPERTYSINGLEVALUE('LoadBearing',$,IFCBOOLEAN(.T.),$)"]);
}

#[test]
fn a_projects_own_phase_filters_and_overrides_go_out_as_text_on_the_project() {
    use crate::model_tests::graphics_testing::run;
    let filter = json!({"name": "Renovation", "new": "overridden", "existing": "by-category", "demolished": "not-displayed", "temporary": "not-displayed"});
    let overrides = json!({"overrides": {"new": {"cut": {"color": [0, 0, 200]}}}});
    // No element holds a value: the project's own filters, its own overrides, or both, are what it says.
    for (filters, own, says) in [
        (false, true, vec!["Phases", "Phase Overrides"]),
        (true, false, vec!["Phases", "Phase Filters"]),
        (true, true, vec!["Phases", "Phase Filters", "Phase Overrides"]),
    ] {
        let mut s = Session::new();
        if filters {
            run(&mut s, "phase_filter", filter.clone());
        }
        if own {
            run(&mut s, "phase", overrides.clone());
        }
        let f = export(&s.model);
        let project = phasing_of(&f, of_kind(&f, "IFCPROJECT")[0]);
        let names: Vec<&str> = project.iter().map(|p| p.split('\'').nth(1).unwrap()).collect();
        assert_eq!(names, says);
        if let Some(p) = project.iter().find(|p| p.contains("'Phase Filters'")) {
            assert!(p.contains(
                r#""Renovation":{"new":"overridden","existing":"by-category","demolished":"not-displayed","temporary":"not-displayed"}"#
            ));
            assert!(p.contains(r#""Show All":"#), "all of them, the usual ones too");
        }
        if let Some(p) = project.iter().find(|p| p.contains("'Phase Overrides'")) {
            assert!(p.contains(r#""new":{"cut":{"color":[0,0,200]}}"#), "{p}");
        }
        dangling(&f);
    }
}

#[test]
fn the_phases_are_listed_when_an_element_holds_one_or_the_project_has_phases_of_its_own() {
    use crate::model_tests::graphics_testing::{house, run};
    let on_project = |s: &Session| {
        let f = export(&s.model);
        phasing_of(&f, of_kind(&f, "IFCPROJECT")[0])
    };
    // Only its phases: its filters, overrides and sets are the usual ones, or none.
    let mut s = Session::new();
    let walls = house(&mut s);
    assert_eq!(on_project(&s), Vec::<String>::new());
    run(&mut s, "modify", json!({"id": walls[1], "params": {"Phase Created": "New Construction"}}));
    assert_eq!(on_project(&s), ["IFCPROPERTYLISTVALUE('Phases',$,(IFCTEXT('Existing'),IFCTEXT('New Construction')),$)"]);
    // A phase of its own, with nothing in it yet.
    let mut t = Session::new();
    run(&mut t, "phase", json!({"name": "Demolition", "before": "New Construction"}));
    assert_eq!(
        on_project(&t),
        ["IFCPROPERTYLISTVALUE('Phases',$,(IFCTEXT('Existing'),IFCTEXT('Demolition'),IFCTEXT('New Construction')),$)"]
    );
}

#[test]
fn a_project_in_another_language_that_uses_no_phasing_says_nothing_of_it() {
    use crate::model_tests::graphics_testing::{house, run};
    let mut s = Session::new();
    run(&mut s, "new", json!({"language": "pt"}));
    let walls = house(&mut s);
    assert!(!export(&s.model).contains(PHASING), "its phases and filters are its language's, as a new project has them");
    run(&mut s, "modify", json!({"id": walls[1], "params": {"Phase Created": "Nova construção"}}));
    let f = export(&s.model);
    let project = phasing_of(&f, of_kind(&f, "IFCPROJECT")[0]);
    let names: Vec<&str> = project.iter().map(|p| p.split('\'').nth(1).unwrap()).collect();
    // What a project read from IFC starts with is in English, so its filters go too, or they'd come back in English.
    assert_eq!(names, ["Phases", "Phase Filters"]);
    assert_eq!(project[0], r"IFCPROPERTYLISTVALUE('Phases',$,(IFCTEXT('Existente'),IFCTEXT('Nova constru\X2\00E7\X0\\X2\00E3\X0\o')),$)");
}

#[test]
fn a_project_that_uses_no_phasing_says_nothing_of_it() {
    use crate::model_tests::graphics_testing::{house, run};
    assert!(!sample_ifc().contains(PHASING), "the sample doesn't use phases, so its file has no set for them");
    // A phase named, which is no new value, or a view that shows one, doesn't count as using them.
    let mut s = Session::new();
    house(&mut s);
    run(&mut s, "graphics", json!({"view": 100000, "phase": "Existing", "phase_filter": "Show New"}));
    assert!(!export(&s.model).contains(PHASING));
    // An option set does.
    run(&mut s, "option", json!({"set": "Facade", "options": ["Brick", "Glass"]}));
    assert_eq!(export(&s.model).matches(&format!("'{PHASING}'")).count(), 1);
}

/// The quantities of set `name` of entity `#ent`, by their names; none when it has no such set.
fn quantities_of(f: &str, ent: u64, name: &str) -> BTreeMap<String, f64> {
    let body = |n: &str| -> String {
        let line = f.lines().find(|l| l.starts_with(&format!("#{n}="))).unwrap_or_else(|| panic!("no #{n}"));
        line.split_once('=').unwrap().1.trim_end_matches(';').to_string()
    };
    for rel in f.lines().filter(|l| l.contains("=IFCRELDEFINESBYPROPERTIES(")) {
        let (related, set) = rel.split_once("$,$,$,(").unwrap().1.split_once("),#").unwrap();
        if !related.split(',').any(|r| r == format!("#{ent}")) {
            continue;
        }
        let set = body(set.trim_end_matches(';').trim_end_matches(')'));
        if !set.starts_with("IFCELEMENTQUANTITY(") || !set.contains(&format!("'{name}'")) {
            continue;
        }
        let qs = set.rsplit_once(",(").unwrap().1.trim_end_matches(')');
        return qs
            .split(',')
            .map(|q| {
                // `IFCQUANTITYAREA('NetSideArea',$,$,12.5,$)`
                let q = body(q.trim_start_matches('#'));
                let fields: Vec<&str> = q.split_once('(').unwrap().1.split(',').collect();
                (fields[0].trim_matches('\'').to_string(), fields[3].parse().unwrap())
            })
            .collect();
    }
    BTreeMap::new()
}

#[test]
fn walls_slabs_doors_and_windows_carry_their_base_quantities() {
    use crate::model_tests::graphics_testing::{house, run};
    let mut s = Session::new();
    let walls = house(&mut s);
    let door = run(&mut s, "door", json!({"at": [2000, 0]}))["id"].as_u64().unwrap();
    let window = run(&mut s, "window", json!({"at": [4000, 0]}))["id"].as_u64().unwrap();
    let floor = run(
        &mut s,
        "floor",
        json!({"boundary": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]], "openings": [[[1000, 1000], [2000, 1000], [2000, 2000], [1000, 2000]]]}),
    )["id"]
        .as_u64()
        .unwrap();
    let f = export(&s.model);
    let near = |a: f64, b: f64| (a - b).abs() < 1e-6 * b.abs().max(1.0);
    let (m, wall) = (&s.model, walls[0]);
    let Some(Element::Wall(wl)) = m.elements.get(wall) else { panic!("no wall") };
    let (height, thick) = (m.top_z(wl) - m.base_z(wl), m.thickness(wl));
    let size = |id: u64| m.opening_type(m.elements.get(id).unwrap()).unwrap();
    let (d, wn) = (size(door), size(window));
    let sill = match m.elements.get(window) {
        Some(Element::Window(o)) => o.sill,
        _ => panic!("no window"),
    };
    let q = quantities_of(&f, of_kind(&f, "IFCWALL")[0], "Qto_WallBaseQuantities");
    let gross = 8000.0 * height / 1e6;
    let cut = (d.width * d.height + wn.width * wn.height.min(height - sill)) / 1e6;
    assert!(near(q["Length"], 8000.0) && near(q["Width"], thick) && near(q["Height"], height), "{q:?}");
    assert!(near(q["GrossSideArea"], gross), "{q:?}");
    assert!(near(q["NetSideArea"], gross - cut), "less the door and the window: {q:?}");
    assert!(near(q["NetVolume"], (gross - cut) * thick / 1e3), "{q:?}");
    // A wall with nothing in it: net is gross.
    let q = quantities_of(&f, of_kind(&f, "IFCWALL")[1], "Qto_WallBaseQuantities");
    assert!(near(q["NetSideArea"], q["GrossSideArea"]), "{q:?}");
    let q = quantities_of(&f, of_kind(&f, "IFCDOOR")[0], "Qto_DoorBaseQuantities");
    assert!(near(q["Width"], d.width) && near(q["Height"], d.height) && near(q["Area"], d.width * d.height / 1e6), "{q:?}");
    assert!(near(q["Perimeter"], 2.0 * (d.width + d.height)), "{q:?}");
    let q = quantities_of(&f, of_kind(&f, "IFCWINDOW")[0], "Qto_WindowBaseQuantities");
    assert!(near(q["Area"], wn.width * wn.height / 1e6), "{q:?}");
    let slab = *of_kind(&f, "IFCSLAB").last().unwrap();
    let q = quantities_of(&f, slab, "Qto_SlabBaseQuantities");
    assert!(near(q["GrossArea"], 48.0) && near(q["NetArea"], 47.0) && near(q["Perimeter"], 28000.0), "{q:?}");
    assert!(near(q["NetVolume"], 47.0 * q["Width"] / 1e3), "{q:?}");
    dangling(&f);
    // Attached to a roof or a floor, a wall has no single height: no `Height`, its sides and volumes its pieces.
    let mut attached = (**m).clone();
    let guid = m.guid(floor);
    if let Some(Element::Wall(w)) = attached.elements.get_mut(wall) {
        w.attach_top = Some(guid);
    }
    let names: Vec<&str> = wall_quantities(&attached, wall).iter().map(|(n, _, _)| *n).collect();
    assert_eq!(names, ["Length", "Width", "GrossSideArea", "NetSideArea", "GrossVolume", "NetVolume"]);
}

/// Every closed shell in `f` is closed: each edge of its faces, from one point to the next round a bound, is run once
/// each way, so the shell has no gap and its faces turn one way.
fn closed(f: &str) {
    let bodies: BTreeMap<u64, &str> = f
        .lines()
        .filter(|l| l.starts_with('#'))
        .filter_map(|l| {
            let (head, body) = l.split_once('=')?;
            Some((head[1..].parse().ok()?, body.trim_end_matches(';')))
        })
        .collect();
    let list = |body: &str| -> Vec<u64> {
        let inner = body.split_once("((").map_or("", |(_, r)| r.split("))").next().unwrap_or(""));
        inner.split(',').filter_map(|r| r.trim().trim_start_matches('#').parse().ok()).collect()
    };
    let shells: Vec<&str> = bodies.values().copied().filter(|b| b.starts_with("IFCCLOSEDSHELL(")).collect();
    assert!(!shells.is_empty(), "no closed shell");
    for shell in shells {
        let mut edges: BTreeMap<(u64, u64), usize> = BTreeMap::new();
        for face in list(shell) {
            for bound in list(bodies[&face]) {
                let b = bodies[&bound];
                assert!(b.ends_with(",.T.)"), "{b}");
                let lp: u64 = b.split_once("(#").unwrap().1.split(',').next().unwrap().parse().unwrap();
                let pts = list(bodies[&lp]);
                for (a, z) in pts.iter().zip(pts.iter().cycle().skip(1)) {
                    *edges.entry((*a, *z)).or_default() += 1;
                }
            }
        }
        for (&(a, z), &n) in &edges {
            assert_eq!(n, 1, "edge #{a} to #{z} run {n} times");
            assert!(edges.contains_key(&(z, a)), "edge #{a} to #{z} has no face the other side: {shell}");
        }
    }
}

#[test]
fn a_shaped_slab_is_one_closed_brep_and_sloped_and_shaped_slabs_give_their_volumes() {
    use crate::model_tests::graphics_testing::run;
    let near = |a: f64, b: f64| (a - b).abs() < 1e-6 * b.abs().max(1.0);
    let boundary = json!([[0, 0], [8000, 0], [8000, 6000], [0, 6000]]);
    let mut s = Session::new();
    let f =
        run(&mut s, "floor", json!({"boundary": boundary, "openings": [[[1000, 1000], [2000, 1000], [2000, 2000], [1000, 2000]]]}))["id"]
            .as_u64()
            .unwrap();
    let shape = json!({"id": f, "points": [{"at": [5000, 3000], "z": -30}], "lines": [{"a": [5000, 3000], "b": [8000, 6000]}]});
    run(&mut s, "edit_shape", shape);
    let text = export(&s.model);
    // One solid: its top's planes, the same planes under them, and its sides round its outline and its opening.
    assert_eq!(of_kind(&text, "IFCFACETEDBREP").len(), 1, "one solid for the slab");
    closed(&text);
    dangling(&text);
    let q = quantities_of(&text, *of_kind(&text, "IFCSLAB").last().unwrap(), "Qto_SlabBaseQuantities");
    let depth = q["Width"];
    assert!(depth > 0.0 && near(q["NetVolume"], 47.0 * depth / 1e3) && near(q["GrossVolume"], 48.0 * depth / 1e3), "{q:?}");
    // A sloped slab: its plan area through its depth plumb.
    let mut s = Session::new();
    run(&mut s, "floor", json!({"boundary": boundary, "slope": 10, "arrow": [[0, 0], [8000, 0]]}));
    let text = export(&s.model);
    let q = quantities_of(&text, *of_kind(&text, "IFCSLAB").last().unwrap(), "Qto_SlabBaseQuantities");
    let plumb = q["Width"];
    assert!(near(q["NetVolume"], 48.0 * plumb / 1e3) && near(q["GrossVolume"], 48.0 * plumb / 1e3), "{q:?}");
}

#[test]
fn a_shaped_slab_over_a_variable_layer_has_a_level_underside_in_ifc() {
    use crate::model_tests::graphics_testing::run;
    let mut s = Session::new();
    let layers =
        json!([{"function": "finish", "thickness": 10}, {"function": "substrate", "thickness": 60, "variable": true}, {"thickness": 200}]);
    run(&mut s, "type", json!({"category": "floor", "name": "Terrace", "layers": layers}));
    let boundary = json!([[0, 0], [8000, 0], [8000, 6000], [0, 6000]]);
    let f = run(&mut s, "floor", json!({"boundary": boundary, "type": "Terrace"}))["id"].as_u64().unwrap();
    run(&mut s, "edit_shape", json!({"id": f, "points": [{"at": [4000, 3000], "z": -30}]}));
    let text = export(&s.model);
    closed(&text);
    // 48 m² of 270 mm less the pyramid the drain takes out of its top: 48 m² by 30 mm over 3.
    let q = quantities_of(&text, *of_kind(&text, "IFCSLAB").last().unwrap(), "Qto_SlabBaseQuantities");
    let want = 48.0 * 0.270 - 48.0 * 0.030 / 3.0;
    assert!((q["NetVolume"] - want).abs() < 1e-6 && (q["GrossVolume"] - want).abs() < 1e-6, "{q:?}");
    // Every corner of its underside is at -270 mm.
    assert!(text.contains("IFCCARTESIANPOINT((0.,0.,-270.))") && !text.contains("IFCCARTESIANPOINT((4000.,3000.,-300.))"));
}

#[test]
fn schedules_and_ifc_read_their_quantities_from_one_place() {
    use crate::model_tests::graphics_testing::{house, run};
    let mut s = Session::new();
    let walls = house(&mut s);
    run(&mut s, "door", json!({"at": [2000, 0]}));
    run(&mut s, "window", json!({"at": [4000, 0]}));
    let boundary = json!([[0, 0], [8000, 0], [8000, 6000], [0, 6000]]);
    let hole = json!([[[1000, 1000], [2000, 1000], [2000, 2000], [1000, 2000]]]);
    let floor = run(&mut s, "floor", json!({"boundary": boundary, "openings": hole}))["id"].as_u64().unwrap();
    let ceiling = run(&mut s, "ceiling", json!({"boundary": boundary, "openings": hole}))["id"].as_u64().unwrap();
    run(&mut s, "level", json!({"name": "Roof level", "elevation": 3000}));
    let roof = run(&mut s, "roof", json!({"boundary": boundary, "overhang": 0, "slope": 30}))["id"].as_u64().unwrap();
    let f = export(&s.model);
    let m = &s.model;
    let json = |id: u64| crate::json::element_json(m, id);
    let close = |a: f64, b: f64| (a - b).abs() < 0.006;
    let (ifc_wall, ifc_floor) = (of_kind(&f, "IFCWALL"), *of_kind(&f, "IFCSLAB").last().unwrap());
    // A wall's net side and volume, and a floor's net area and volume: the schedule's cells are the file's numbers.
    let q = quantities_of(&f, ifc_wall[0], "Qto_WallBaseQuantities");
    let j = json(walls[0]);
    assert!(
        close(j["area_m2"].as_f64().unwrap(), q["NetSideArea"]) && close(j["volume_m3"].as_f64().unwrap(), q["NetVolume"]),
        "{j} {q:?}"
    );
    let q = quantities_of(&f, ifc_floor, "Qto_SlabBaseQuantities");
    let j = json(floor);
    assert!(close(j["area_m2"].as_f64().unwrap(), q["NetArea"]) && close(j["volume_m3"].as_f64().unwrap(), q["NetVolume"]), "{j} {q:?}");
    assert!(close(j["perimeter_m"].as_f64().unwrap() * 1000.0, q["Perimeter"]), "{j} {q:?}");
    let q = quantities_of(&f, of_kind(&f, "IFCCOVERING")[0], "Qto_CoveringBaseQuantities");
    let j = json(ceiling);
    assert!(close(j["area_m2"].as_f64().unwrap(), q["NetArea"]) && close(q["GrossArea"], 48.0), "{j} {q:?}");
    let q = quantities_of(&f, of_kind(&f, "IFCROOF")[0], "Qto_RoofBaseQuantities");
    let j = json(roof);
    assert!(
        close(j["surface_area_m2"].as_f64().unwrap(), q["NetArea"]) && close(j["area_m2"].as_f64().unwrap(), q["ProjectedArea"]),
        "{j} {q:?}"
    );
    assert!(q["GrossArea"] > q["ProjectedArea"], "{q:?}");
}

#[test]
fn a_roofs_net_area_leaves_out_what_the_shafts_and_openings_take_and_its_gross_area_does_not() {
    use crate::model_tests::graphics_testing::run;
    let mut s = Session::new();
    run(&mut s, "type", json!({"category": "roof", "name": "R", "thickness": 300}));
    run(&mut s, "level", json!({"name": "Roof level", "elevation": 3000}));
    let boundary = json!([[0, 0], [8000, 0], [8000, 6000], [0, 6000]]);
    // A flat roof of 48 m², a shaft of 1 m² through it, an opening of its own of 2 m².
    run(
        &mut s,
        "roof",
        json!({"boundary": boundary, "overhang": 0, "slope": 0, "type": "R", "openings": [[[5000, 1000], [7000, 1000], [7000, 2000], [5000, 2000]]]}),
    );
    run(&mut s, "shaft", json!({"boundary": [[1000, 1000], [2000, 1000], [2000, 2000], [1000, 2000]], "height": 4000}));
    let f = export(&s.model);
    let near = |a: f64, b: f64| (a - b).abs() < 1e-6 * b.abs().max(1.0);
    let q = quantities_of(&f, of_kind(&f, "IFCROOF")[0], "Qto_RoofBaseQuantities");
    assert!(near(q["GrossArea"], 48.0) && near(q["NetArea"], 45.0) && near(q["ProjectedArea"], 45.0), "{q:?}");
    let id = s.model.elements.iter().find(|(_, e)| matches!(e, Element::Roof(_))).map(|(id, _)| *id).unwrap();
    let j = crate::json::element_json(&s.model, id);
    assert!(
        (j["surface_area_m2"].as_f64().unwrap() - 45.0).abs() < 0.006 && (j["volume_m3"].as_f64().unwrap() - 13.5).abs() < 0.006,
        "{j}"
    );
}

#[test]
fn a_wall_attached_to_a_floor_above_still_measures_its_side_and_volume() {
    use crate::model_tests::graphics_testing::{house, run};
    let mut s = Session::new();
    let walls = house(&mut s);
    let level = run(&mut s, "level", json!({"name": "Upper", "elevation": 3000}))["id"].as_u64().unwrap();
    let boundary = json!([[0, 0], [8000, 0], [8000, 6000], [0, 6000]]);
    let floor = run(&mut s, "floor", json!({"boundary": boundary, "level": level}))["id"].as_u64().unwrap();
    let free = crate::quantities::wall(&s.model, walls[0]).unwrap();
    let mut m = (*s.model).clone();
    let guid = m.guid(floor);
    if let Some(Element::Wall(wl)) = m.elements.get_mut(walls[0]) {
        wl.attach_top = Some(guid);
    }
    let q = crate::quantities::wall(&m, walls[0]).unwrap();
    assert_eq!(q.height, None);
    assert!(q.gross_volume > 0.0 && (q.gross_side - q.gross_volume / q.width).abs() < 1e-6, "{q:?}");
    // Under the floor, which is 200 thick over a level 3000 up: its underside is 2800. The wall's inside half (y from 0 to 100) is
    // under the floor and stands to 2800; its outside half (y from -100 to 0) is past the floor's edge and stands to its own
    // top, 3000. Mitred at its corners the wall's plan is 810 000 mm² outside the floor's edge and 790 000 within (the
    // trapezoids 8200 and 8000, 8000 and 7800 wide, 100 deep): 810 000 x 3000 + 790 000 x 2800 = 4.642e9 mm³, its side that
    // through 200: 23.21 m².
    let volume = 810_000.0 * 3000.0 + 790_000.0 * 2800.0;
    assert!((q.gross_volume - volume).abs() < 1.0 && (q.gross_side - volume / 200.0).abs() < 1e-3, "{q:?} {free:?}");
    assert!(q.gross_side < free.gross_side, "{q:?} {free:?}");
}

#[test]
fn identity_data_goes_out_as_the_property_sets_ifc_has_for_it() {
    use crate::model_tests::graphics_testing::{house, run};
    let mut s = Session::new();
    house(&mut s);
    let door = run(&mut s, "door", json!({"at": [2000, 0]}))["id"].as_u64().unwrap();
    let wall = s.model.elements.values().find_map(|e| if let Element::Wall(w) = e { Some(w.type_name.clone()) } else { None }).unwrap();
    let (door_family, door_type) = match s.model.elements.get(door) {
        Some(Element::Door(o)) => (o.family.clone(), o.type_name.clone()),
        _ => panic!("no door"),
    };
    let wall_data = json!({"category": "wall", "name": wall, "type_mark": "W1", "fire_rating": "EI 60", "manufacturer": "Acme",
        "model": "X-200", "description": "Brick", "cost": 120.5, "keynote": "04 21 13"});
    run(&mut s, "type", wall_data);
    match door_family {
        Some(f) => run(&mut s, "family", json!({"name": f, "identity": {door_type: {"type_mark": "D1", "fire_rating": "EI 30"}}})),
        None => run(&mut s, "type", json!({"category": "door", "name": door_type, "type_mark": "D1", "fire_rating": "EI 30"})),
    };
    let chair = json!({"name": "Chair", "category": "furniture", "parameters": [{"name": "Width", "kind": "length", "value": 500}],
        "types": {"A": {"Width": 500}}, "forms": [{"plane": "xy", "profile": [[0, 0], [500, 0], [500, 500], [0, 500]], "from": 0, "to": 400}],
        "identity": {"A": {"type_mark": "C1", "fire_rating": "none"}}});
    run(&mut s, "family", chair);
    run(&mut s, "component", json!({"family": "Chair", "type": "A", "at": [1000, 1000]}));
    let f = export(&s.model);
    // The identity data is the type's: it is on the type object, once, and no occurrence holds it.
    let wall_ent = of_kind(&f, "IFCWALL")[0];
    let wall_type = type_line(&f, "IFCWALLTYPE", &wall);
    let common = type_set_of(&f, &wall_type, "Pset_WallCommon");
    assert!(common.contains(&"IFCPROPERTYSINGLEVALUE('Reference',$,IFCIDENTIFIER('W1'),$)".to_string()), "{common:?}");
    assert!(common.contains(&"IFCPROPERTYSINGLEVALUE('FireRating',$,IFCLABEL('EI 60'),$)".to_string()), "{common:?}");
    let maker = type_set_of(&f, &wall_type, MAKER_SET);
    assert_eq!(
        maker,
        ["IFCPROPERTYSINGLEVALUE('Manufacturer',$,IFCLABEL('Acme'),$)", "IFCPROPERTYSINGLEVALUE('ModelLabel',$,IFCLABEL('X-200'),$)"]
    );
    let rest = type_set_of(&f, &wall_type, TYPE_DATA_SET);
    for want in ["'Description',$,IFCTEXT('Brick')", "'Keynote',$,IFCTEXT('04 21 13')", "'Cost',$,IFCMONETARYMEASURE(120.5)"] {
        assert!(rest.iter().any(|p| p.contains(want)), "{want} in {rest:?}");
    }
    assert!(!rest.iter().any(|p| p.contains("Reference") || p.contains("FireRating")), "those are in the common set: {rest:?}");
    assert!(set_of(&f, wall_ent, MAKER_SET).is_empty() && set_of(&f, wall_ent, TYPE_DATA_SET).is_empty(), "no occurrence holds them");
    assert!(
        !set_of(&f, wall_ent, "Pset_WallCommon").iter().any(|p| p.contains("'Reference'") || p.contains("'FireRating'")),
        "nor in its common set"
    );
    let door_type = type_line(&f, "IFCDOORTYPE", &type_name_of(&s, "door"));
    let door_common = type_set_of(&f, &door_type, "Pset_DoorCommon");
    assert!(door_common.iter().any(|p| p.contains("'FireRating'") && p.contains("EI 30")), "{door_common:?}");
    assert!(door_common.iter().any(|p| p.contains("'Reference'") && p.contains("'D1'")), "{door_common:?}");
    // A family's component has no common set: its Type Mark and Fire Rating go in the BIMCraft set, on its type object.
    let component = entities(&f, "IFCFURNITURETYPE");
    assert_eq!(component.len(), 1);
    let rest = type_set_of(&f, &component[0], TYPE_DATA_SET);
    assert!(rest.iter().any(|p| p.contains("'Reference'") && p.contains("'C1'")), "{rest:?}");
    assert!(rest.iter().any(|p| p.contains("'FireRating'")), "{rest:?}");
    assert!(set_of(&f, of_kind(&f, "IFCFURNITURE")[0], TYPE_DATA_SET).is_empty());
    dangling(&f);
}

#[test]
fn a_floor_type_and_a_building_pad_type_of_one_name_are_two_type_objects_each_with_its_data() {
    use crate::model_tests::graphics_testing::run;
    let mut s = Session::new();
    run(&mut s, "type", json!({"category": "floor", "name": "Generic", "thickness": 200, "type_mark": "F1", "manufacturer": "Acme"}));
    run(&mut s, "floor", json!({"boundary": [[0, 0], [4000, 0], [4000, 4000], [0, 4000]], "type": "Generic"}));
    run(&mut s, "building_pad", json!({"boundary": [[10000, 0], [14000, 0], [14000, 4000], [10000, 4000]], "type": "Generic"}));
    let f = export(&s.model);
    let types = entities(&f, "IFCSLABTYPE");
    assert_eq!(types.len(), 2, "a slab type for each kind of element: {types:?}");
    let predefined: BTreeSet<String> = types.iter().map(|l| top_level_attributes(l)[9].clone()).collect();
    assert_eq!(predefined, [".BASESLAB.".to_string(), ".FLOOR.".to_string()].into(), "neither is made NOTDEFINED by the other: {types:?}");
    let guids: BTreeSet<String> = types.iter().map(|l| top_level_attributes(l)[0].clone()).collect();
    assert_eq!(guids.len(), 2, "each has a GlobalId of its own");
    for t in &types {
        // (A slab has its common set; a pad, which has none, holds it in the BIMCraft set.)
        let mut mark = type_set_of(&f, t, "Pset_SlabCommon");
        mark.extend(type_set_of(&f, t, TYPE_DATA_SET));
        assert!(mark.iter().any(|p| p.contains("'Reference'") && p.contains("'F1'")), "each holds the type's data: {t}: {mark:?}");
        let maker = type_set_of(&f, t, MAKER_SET);
        assert!(maker.iter().any(|p| p.contains("Acme")), "{maker:?}");
    }
    // A project with one of them exports as it did: the GlobalId has no kind in it.
    let mut t = Session::new();
    run(&mut t, "type", json!({"category": "floor", "name": "Generic", "thickness": 200}));
    run(&mut t, "floor", json!({"boundary": [[0, 0], [4000, 0], [4000, 4000], [0, 4000]], "type": "Generic"}));
    let alone = entities(&export(&t.model), "IFCSLABTYPE");
    let plain = type_hash("IFCSLABTYPE", "Generic", &None);
    assert_eq!(alone.len(), 1);
    assert_eq!(top_level_attributes(&alone[0])[0], crate::ifc::s(&guid(t.model.uid, TYPES, plain)), "{}", alone[0]);
    dangling(&f);
}

#[test]
fn a_type_object_keeps_its_global_id_when_another_kind_of_element_names_its_type_alike_later() {
    use crate::model_tests::graphics_testing::run;
    let mut s = Session::new();
    run(&mut s, "type", json!({"category": "floor", "name": "Generic", "thickness": 200}));
    run(&mut s, "floor", json!({"boundary": [[0, 0], [4000, 0], [4000, 4000], [0, 4000]], "type": "Generic"}));
    let before = entities(&export(&s.model), "IFCSLABTYPE");
    assert_eq!(before.len(), 1);
    let guid_before = top_level_attributes(&before[0])[0].clone();
    run(&mut s, "building_pad", json!({"boundary": [[10000, 0], [14000, 0], [14000, 4000], [10000, 4000]], "type": "Generic"}));
    let after = entities(&export(&s.model), "IFCSLABTYPE");
    assert_eq!(after.len(), 2, "{after:?}");
    let floor = after.iter().find(|l| top_level_attributes(l)[9] == ".FLOOR.").expect("the floor's type");
    assert_eq!(top_level_attributes(floor)[0], guid_before, "the floor type's GlobalId is not changed by a pad of its name");
    assert_eq!(guid_before, crate::ifc::s(&guid(s.model.uid, TYPES, type_hash("IFCSLABTYPE", "Generic", &None))));
    // The same whichever was made first: the floor keeps the plain one.
    let mut t = Session::new();
    run(&mut t, "type", json!({"category": "floor", "name": "Generic", "thickness": 200}));
    run(&mut t, "building_pad", json!({"boundary": [[10000, 0], [14000, 0], [14000, 4000], [10000, 4000]], "type": "Generic"}));
    run(&mut t, "floor", json!({"boundary": [[0, 0], [4000, 0], [4000, 4000], [0, 4000]], "type": "Generic"}));
    let both = entities(&export(&t.model), "IFCSLABTYPE");
    let floor = both.iter().find(|l| top_level_attributes(l)[9] == ".FLOOR.").expect("the floor's type");
    assert_eq!(top_level_attributes(floor)[0], crate::ifc::s(&guid(t.model.uid, TYPES, type_hash("IFCSLABTYPE", "Generic", &None))));
}

#[test]
fn a_door_type_is_of_its_door_not_of_the_last_element_written_before_it() {
    use crate::model_tests::graphics_testing::{house, run};
    let mut s = Session::new();
    house(&mut s);
    // A door type named as a family's `Family:Type`, and a component of such a family written as a door too: two type
    // objects of one class and name, of two kinds of element.
    let form = json!([{"profile": [[0, 0], [100, 0], [100, 100], [0, 100]], "from": 0, "to": 100}]);
    run(
        &mut s,
        "family",
        json!({"name": "P", "category": "generic", "types": {"T": {}}, "forms": form, "ifc": {"class": "IFCDOOR", "predefined": "DOOR"}}),
    );
    run(&mut s, "type", json!({"category": "door", "name": "P:T", "width": 900, "height": 2100}));
    run(&mut s, "door", json!({"at": [2000, 0], "type": "P:T"}));
    // The component is the last element written before the doors: its kind is what a door's type used to be keyed by.
    run(&mut s, "component", json!({"family": "P", "type": "T", "at": [20000, 20000]}));
    let types = entities(&export(&s.model), "IFCDOORTYPE");
    let guids: BTreeSet<String> = types.iter().map(|l| top_level_attributes(l)[0].clone()).collect();
    assert_eq!((types.len(), guids.len()), (2, 2), "a type object for the door and one for the component: {types:?}");
}
