//! The pictures in Properties: a room's colour, a floor's slope.

use super::*;

#[test]
fn a_doors_properties_have_no_flip_rows_or_pictures_as_revits_have_none() {
    // Revit flips a door with the arrows on the plan (and Space); its Properties have no Flip Hand, Flip Facing or pictures of it.
    let script = format!(
        "{BOX}
door {{\"at\": [2000, 0]}}
window {{\"at\": [5000, 0]}}"
    );
    let mut h = app(&script);
    for id in [ids(&h, "door")[0], ids(&h, "window")[0]] {
        h.state_mut().selection = vec![id];
        h.run();
        assert!(h.query_by_label("Host Wall").is_some(), "the palette shows its properties");
        for label in ["Flip Hand", "Flip Facing", "As placed", "Flipped hand", "Flipped facing", "Flipped both", "Opens"] {
            assert!(h.query_by_label(label).is_none(), "{label}");
        }
    }
}

#[test]
fn a_rooms_colour_on_plans_is_picked_from_swatches() {
    let script = format!("{BOX}\nroom {{\"point\": [4000, 3000]}}");
    let mut h = app(&script);
    let room = ids(&h, "room")[0];
    h.state_mut().selection = vec![room];
    h.run();
    h.get_by_role_and_label(Role::RadioButton, "Sage").click();
    h.run();
    assert_eq!(field(&h, room, "colour"), json!([214, 229, 210]));
    h.get_by_role_and_label(Role::RadioButton, "No fill").click();
    h.run();
    assert!(field(&h, room, "colour").is_null());
}

#[test]
fn a_floors_slope_is_drawn_at_its_angle_and_stepped_on_its_picture() {
    let script = format!("{BOX}\nfloor {{\"slope\": 2, \"arrow\": [[0, 3000], [8000, 3000]]}}\nfloor {{\"offset\": 3000}}");
    let mut h = app(&script);
    let [sloped, flat] = ids(&h, "floor")[..] else { panic!("two floors") };
    h.state_mut().selection = vec![sloped];
    h.run();
    // Its section at its angle, which the picture says.
    assert!(h.query_by_label("Floor slope: 2° (3.49%)").is_some());
    let slope = |h: &Harness<BimApp>| field(h, sloped, "slope").as_f64().unwrap_or(0.0);
    // A step sets it, through the command the Slope field runs.
    press(&mut h, "1%");
    assert!((slope(&h).to_radians().tan() - 0.01).abs() < 1e-9, "{}", slope(&h));
    press(&mut h, "Flat");
    assert_eq!(slope(&h), 0.0);
    // One that falls toward its arrow's head keeps falling.
    h.state_mut().session.execute("modify", &json!({"id": sloped, "slope": -3})).unwrap();
    h.run();
    press(&mut h, "2%");
    assert!((slope(&h).to_radians().tan() + 0.02).abs() < 1e-9, "{}", slope(&h));
    // With no slope arrow it is flat, and the picture starts the tool that draws one.
    h.state_mut().selection = vec![flat];
    h.run();
    assert!(h.query_by_label("1%").is_none());
    press(&mut h, "Draw a slope arrow");
    assert!(matches!(h.state().tool, Tool::SlopeArrow(None)));
}

#[test]
fn what_an_element_is_tied_to_is_a_link_that_selects_it() {
    let script = format!("{BOX}\ndoor {{\"at\": [2000, 0]}}\nroof {{}}");
    let mut h = app(&script);
    let (walls, door, roof) = (ids(&h, "wall"), ids(&h, "door")[0], ids(&h, "roof")[0]);
    let (south, east, west) = (walls[0], walls[1], walls[3]);
    let session = &mut h.state_mut().session;
    session.execute("modify", &json!({"ids": [south], "attach_top": roof})).unwrap();
    let d = session.execute("dimension", &json!({"refs": [{"id": west}, {"id": east}], "at": [4000, 3000]})).unwrap()["id"].clone();
    session.execute("dimension_constraint", &json!({"id": d, "locked": [0]})).unwrap();
    // By the link's own action: a wall's rows run below the palette's fold.
    let follow = |h: &mut Harness<BimApp>, link: String| {
        h.get_by_role_and_label(Role::Link, &link).click_accesskit();
        h.run();
        h.state().selection.clone()
    };
    h.state_mut().selection = vec![door];
    h.run();
    // The door's host; the wall's openings and the roof its top is attached to; the walls attached to the roof.
    assert_eq!(follow(&mut h, format!("Wall {south}")), [south]);
    assert_eq!(follow(&mut h, format!("Roof {roof}")), [roof]);
    assert_eq!(follow(&mut h, format!("Wall {south}")), [south]);
    assert_eq!(follow(&mut h, format!("Door {door}")), [door]);
    // The locked dimension that holds a wall.
    h.state_mut().selection = vec![west];
    h.run();
    assert_eq!(follow(&mut h, format!("Dimension {d}")), [d.as_u64().unwrap_or_default()]);
}

#[test]
fn a_walls_top_offset_is_a_row_under_its_top_constraint() {
    let script = format!("{BOX}\nwall {{\"points\": [[0, -2000], [4000, -2000]], \"height\": 2700}}");
    let mut h = app(&script);
    let (connected, free) = (ids(&h, "wall")[0], ids(&h, "wall")[4]);
    let value = |h: &Harness<BimApp>| h.get_by_role_and_label(Role::TextInput, "Top Offset").value().unwrap_or_default();
    // To Level 2, it shows 0 where the wall's JSON leaves the offset out, and sets one through `modify`.
    h.state_mut().selection = vec![connected];
    h.run();
    assert_eq!(value(&h), "0");
    type_into(&mut h, "Top Offset", "300");
    key(&mut h, Key::Enter);
    assert_eq!((field(&h, connected, "top_offset"), field(&h, connected, "height")), (json!(300.0), json!(3300.0)));
    assert_eq!(h.state().session.history().last(), Some(&"Modify"));
    assert_eq!(value(&h), "300");
    // A negative one takes the top down; one that takes it to the base is refused, the wall staying as it was.
    type_into(&mut h, "Top Offset", "-3000");
    key(&mut h, Key::Enter);
    assert!(h.state().status.error && h.state().status.text.contains("tall"), "{}", h.state().status.text);
    assert_eq!(field(&h, connected, "top_offset"), json!(300.0));
    // An unconnected wall has no top constraint to offset from: no row.
    h.state_mut().selection = vec![free];
    h.run();
    assert!(h.query_by_role_and_label(Role::TextInput, "Top Offset").is_none());
}

#[test]
fn a_group_header_named_like_another_in_the_palette_folds_when_clicked() {
    // A door of a family with a Yes/No parameter has two groups called Other: the element's (Host Wall) and the family's (Fire
    // Rated). Their bars are two widgets, and one click lands on one.
    let script = format!(
        "{BOX}
family {{\"name\": \"Flush Door\", \"category\": \"door\", \"parameters\": [{{\"name\": \"Width\", \"kind\": \"length\", \"instance\": true, \"value\": 800}}, {{\"name\": \"Height\", \"kind\": \"length\", \"instance\": true, \"value\": 2100}}, {{\"name\": \"Fire Rated\", \"kind\": \"yesno\", \"instance\": true, \"value\": 0}}], \"types\": {{\"800\": {{}}}}}}
door {{\"at\": [2000, 0], \"family\": \"Flush Door\", \"type\": \"800\"}}"
    );
    let mut h = app(&script);
    let door = ids(&h, "door")[0];
    h.state_mut().selection = vec![door];
    h.run();
    assert!(h.query_by_label("Host Wall").is_some() && h.query_by_label("Fire Rated").is_some(), "both groups show");
    let bars = h.get_all_by_label("Other").count();
    assert_eq!(bars, 2, "two bars named Other");
    h.get_all_by_label("Other").next().expect("a bar").click();
    h.run();
    assert!(h.query_by_label("Host Wall").is_none(), "folded");
    h.get_all_by_label("Other").next().expect("a bar").click();
    h.run();
    assert!(h.query_by_label("Host Wall").is_some(), "unfolded again");
}
