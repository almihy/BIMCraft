//! Manage > Object Styles: each model category's pens, cut and in projection, edited as a draft and applied as one command.

use super::*;
use bimcraft_core::pen::Pen;
use bimcraft_core::styles::ObjectStyle;

#[test]
fn object_styles_set_each_categorys_pens_in_one_step() {
    let mut h = app(BOX);
    press(&mut h, "Manage");
    press(&mut h, "Object Styles");
    assert!(h.state().object_styles_dialog.is_some());
    // A row for each model category, its pens as a project starts with them; nothing is the project's until OK.
    assert!(h.query_by_label("Walls: Cut").is_some() && h.query_by_label("Railings: Projection").is_some());
    choose(&mut h, "Walls: Cut", "13 · 0.70 mm");
    choose(&mut h, "Doors: Projection", "0 · Hairline");
    assert!(h.state().session.model.object_styles.is_empty());
    let steps = h.state().session.history().len();
    press(&mut h, "OK");
    assert!(h.state().object_styles_dialog.is_none(), "OK closes it");
    let m = &h.state().session.model;
    assert_eq!(m.object_style("wall"), ObjectStyle { projection: Pen(4), cut: Pen(13) });
    assert_eq!(m.object_style("door").projection, Pen(0));
    assert_eq!(m.object_styles.len(), 2, "only what differs from a project's start");
    assert_eq!(h.state().session.history().len(), steps + 1, "one undo step");
    assert_eq!(h.state().session.history().last(), Some(&"Object Styles"));
    // Cancel drops what was edited; Reset, applied, is the project's start again.
    press(&mut h, "Object Styles");
    choose(&mut h, "Floors: Cut", "1 · 0.10 mm");
    press(&mut h, "Cancel");
    assert_eq!(h.state().session.model.object_style("floor").cut, Pen(10));
    press(&mut h, "Object Styles");
    press(&mut h, "Reset");
    press(&mut h, "OK");
    assert!(h.state().session.model.object_styles.is_empty());
}

#[test]
fn object_styles_lists_each_familys_subcategories_under_their_category_and_applies_their_settings() {
    let mut h = app(BOX);
    h.state_mut()
        .session
        .execute(
            "family",
            &json!({"name": "Sample Block", "category": "furniture", "types": {"A": {}},
                    "forms": [{"profile": [[0, 0], [100, 0], [100, 100], [0, 100]], "from": 0, "to": 100}],
                    "subcategories": [{"name": "Edge", "projection": 2}, {"name": "Rim"}]}),
        )
        .unwrap();
    h.run();
    press(&mut h, "Manage");
    press(&mut h, "Object Styles");
    // A row under Furniture for each subcategory: weights, colour, line pattern and material, as Revit's dialog has them.
    for what in ["Projection", "Cut", "Color", "Line Pattern", "Material"] {
        assert!(h.query_by_label(&format!("Furniture: Rim: {what}")).is_some(), "a {what} field for the Rim subcategory");
    }
    assert_eq!(combo_value(&h, "Furniture: Edge: Projection"), "2 · 0.13 mm", "the family's own weight");
    assert_eq!(combo_value(&h, "Furniture: Rim: Projection"), "By Category");
    // Edited, and not the project's until OK.
    choose(&mut h, "Furniture: Rim: Projection", "1 · 0.10 mm");
    choose(&mut h, "Furniture: Rim: Cut", "1 · 0.10 mm");
    choose(&mut h, "Furniture: Rim: Line Pattern", "Dash");
    assert!(h.state().session.model.subcategory_style("furniture", "Rim").is_none());
    let steps = h.state().session.history().len();
    press(&mut h, "OK");
    let m = &h.state().session.model;
    let rim = m.subcategory_style("furniture", "Rim").expect("applied");
    assert_eq!((rim.projection, rim.cut, rim.pattern.as_deref()), (Some(Pen(1)), Some(Pen(1)), Some("Dash")));
    assert!(m.object_styles.is_empty(), "no category changed");
    assert_eq!(h.state().session.history().len(), steps + 1, "one undo step");
    // The row that wasn't touched is as it was, and the dialog opens on what the project has now.
    assert_eq!(m.subcategory_style("furniture", "Edge").and_then(|s| s.projection), Some(Pen(2)));
    press(&mut h, "Object Styles");
    assert_eq!(combo_value(&h, "Furniture: Rim: Line Pattern"), "Dash");
    // Back to its category's: a subcategory with nothing set is no style.
    choose(&mut h, "Furniture: Rim: Projection", "By Category");
    choose(&mut h, "Furniture: Rim: Cut", "By Category");
    choose(&mut h, "Furniture: Rim: Line Pattern", "Solid");
    press(&mut h, "OK");
    assert!(h.state().session.model.subcategory_style("furniture", "Rim").is_none());
}
