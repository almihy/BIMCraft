//! The one interface: the ribbon over the view, Properties over the Project Browser on the left, the floating cards, the app bar
//! and the view's little tabs.

use egui::Shape;
use egui_kittest::kittest::NodeT;

use super::*;

/// The band across the top of the window: the app bar's ground, as the frame painted it (the topmost band as wide as the window).
fn app_bar_ground(h: &Harness<BimApp>) -> egui::Color32 {
    fn walk(s: &Shape, out: &mut Vec<(f32, egui::Color32)>) {
        match s {
            Shape::Rect(r) if r.rect.width() >= 1000.0 && r.rect.height() < 60.0 => out.push((r.rect.top(), r.fill)),
            Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    h.output().shapes.iter().for_each(|c| walk(&c.shape, &mut out));
    out.into_iter().min_by(|a, b| a.0.total_cmp(&b.0)).map(|(_, fill)| fill).expect("the app bar")
}

/// Relative luminance, 0 (black) to 1 (white).
fn luminance(c: egui::Color32) -> f32 {
    let lin = |v: u8| {
        let v = f32::from(v) / 255.0;
        if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
}

#[test]
fn a_settings_file_from_when_there_were_profiles_opens_the_one_interface_and_settings_offers_none() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("settings.json");
    // BIMCraft's own profile, with its Tools palette down the left: what a window left before there was only one interface.
    let old = json!({"profile": "bimcraft", "palettes": {"left": [{"tabs": ["Tools"]}], "right": [{"tabs": ["Properties"]}, {"tabs": ["Project Browser"]}]}});
    std::fs::write(&file, old.to_string()).unwrap();
    let mut h = app(BOX);
    h.state_mut().use_settings(file.clone());
    h.run();
    assert!(h.query_by_label("Minimize the Ribbon").is_some(), "the ribbon");
    assert!(h.query_by_label("Architecture").is_some());
    let d = &h.state().docking;
    assert_eq!(d.side(crate::dock::Side::Left), [], "nothing of the Tools palette is left: {d:?}");
    assert!(d.shown(crate::dock::Palette::Properties) && d.shown(crate::dock::Palette::Browser), "the palettes the file placed");
    press(&mut h, "Settings");
    for gone in ["Profile", "BIMCraft", "Rev"] {
        assert!(h.query_by_label(gone).is_none(), "no {gone} in Settings");
    }
    // A change rewrites the file, and it keeps no profile and no Tools palette.
    h.get_by_role_and_label(Role::RadioButton, "Compact").click_accesskit();
    h.run();
    press(&mut h, "Close");
    let kept: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert!(kept.get("profile").is_none() && !kept["palettes"].to_string().contains("Tools"), "{kept}");
}

#[test]
fn file_is_on_the_app_bar_over_the_ribbon_and_the_options_bar_floats_over_the_view() {
    let mut h = app(BOX);
    let (file, tab) = (h.get_by_label("File").rect(), h.get_by_label("Architecture").rect());
    assert!(file.bottom() < tab.top(), "File is on the app bar, over the ribbon's tabs: {file:?} {tab:?}");
    let view = h.state().view_rect;
    assert!(!h.state().options_rect.is_positive(), "no bar while nothing is on it");
    tool(&mut h, "Wall");
    let (field, bar) = (h.get_by_label("Location Line:").rect(), h.state().options_rect);
    assert!(bar.contains_rect(field), "the tool's options are on it: {field:?} {bar:?}");
    assert!(view.contains_rect(bar) && bar.width() < view.width(), "a card over the view: {bar:?} {view:?}");
    assert!((bar.center().x - view.center().x).abs() < 1.0, "centred over the view: {bar:?} {view:?}");
    assert_eq!(h.state().view_rect, view, "coming, it takes no room, so the view never moves under it");
    // A click on the bar starts no wall.
    click_at(&mut h, field.left_center() - vec2(30.0, 0.0));
    assert_eq!(ids(&h, "wall").len(), 4);
}

#[test]
fn a_tool_with_no_options_shows_its_name_alone_on_the_options_bar() {
    let mut h = app(BOX);
    tool(&mut h, "Door");
    let bar = h.state().options_rect;
    assert_eq!(h.state().context().as_deref(), Some("Modify | Place Door"));
    assert!(bar.is_positive(), "the bar names the tool: {bar:?}");
    // "Modify | Place Door" at 13 px is about 120 px: the name and the frame's margins, nothing after it.
    assert!(bar.width() < 160.0, "only the name: {bar:?}");
}

/// The bands in the bar's colour the window drew as tall as a ribbon panel's name.
fn name_bands(h: &Harness<BimApp>) -> usize {
    fn walk(s: &Shape, out: &mut usize) {
        match s {
            Shape::Rect(r) if r.fill == crate::style::pal().bar && (r.rect.height() - 16.0).abs() < 0.5 => *out += 1,
            Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let mut out = 0;
    h.output().shapes.iter().for_each(|c| walk(&c.shape, &mut out));
    out
}

#[test]
fn the_window_wears_the_floating_cards_the_app_bar_and_the_views_little_tabs_and_compact_is_its_to_pick() {
    let mut h = app(BOX);
    let ground = app_bar_ground(&h);
    assert!(luminance(ground) > 0.8, "light in the light theme: {ground:?}");
    assert_eq!(crate::style::app_bar_rule().map(|s| s.color), Some(crate::style::pal().border), "a hairline under it");
    let gap = f32::from(crate::style::GAP);
    assert!(h.get_by_label("Architecture").rect().left() > gap, "the ribbon's card stands off the window's edge");
    assert_eq!(name_bands(&h), 0, "the panels' names are on no bands");
    // The view fills the pane and its tabs float over its top left corner, a card as high as a tab.
    let view = h.state().view_rect;
    let tab = h.state().tab_rects.first().map(|(_, r)| *r).unwrap();
    assert!(view.contains_rect(tab), "{tab:?} over {view:?}");
    press(&mut h, "Settings");
    let floating = h.get_by_role_and_label(Role::RadioButton, "Floating");
    assert!(!floating.accesskit_node().is_disabled());
    h.get_by_role_and_label(Role::RadioButton, "Compact").click_accesskit();
    h.run();
    press(&mut h, "Close");
    assert!(h.state().compact);
    let (view, tab) = (h.state().view_rect, h.state().tab_rects.first().map(|(_, r)| *r).unwrap());
    assert!(tab.bottom() <= view.top(), "Compact's tabs are a strip above the view: {tab:?} {view:?}");
}

/// The hairlines in the border's colour the window drew by the row whose value's field is at `field`: (under it, running across
/// the row from its name to the field's end; between the name and the field, running down past it).
fn rules_by(h: &Harness<BimApp>, field: egui::Rect) -> (usize, usize) {
    fn walk(s: &Shape, out: &mut Vec<[egui::Pos2; 2]>) {
        match s {
            Shape::LineSegment { points, stroke } if stroke.color == crate::style::pal().border => out.push(*points),
            Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let mut lines = Vec::new();
    h.output().shapes.iter().for_each(|c| walk(&c.shape, &mut lines));
    let under = lines
        .iter()
        .filter(|[a, b]| {
            (a.y - b.y).abs() < 0.5
                && a.y > field.bottom()
                && a.y < field.bottom() + 8.0
                && a.x.min(b.x) < field.left() - 60.0
                && a.x.max(b.x) >= field.right() - 1.0
        })
        .count();
    let beside = lines
        .iter()
        .filter(|[a, b]| {
            (a.x - b.x).abs() < 0.5
                && a.x < field.left()
                && a.x > field.left() - 20.0
                && a.y.min(b.y) < field.top()
                && a.y.max(b.y) > field.bottom()
        })
        .count();
    (under, beside)
}

#[test]
fn properties_are_the_clean_rows_and_apply_comes_after_them() {
    let mut h = app(BOX);
    click(&mut h, P2(0.0, 3000.0));
    assert_eq!(h.state().selection.len(), 1, "a wall, whose rows run past the palette");
    let field = h.get_by_label("Base Offset").rect();
    assert_eq!(rules_by(&h, field), (0, 0), "no lines between or under the rows");
    let apply = h.get_by_label("Apply").rect();
    assert!(apply.top() > field.bottom() + 40.0, "Apply follows the rows, none of them over it: {apply:?} {field:?}");
    let browser = h.get_by_label_contains("Project Browser ·").rect();
    assert!(apply.top() > browser.top(), "scrolled past the palette's foot with the rows: {apply:?} {browser:?}");
}

/// The docked cards of a side from `from` to `to` across, as the frame painted them: tall, in the panels' colour.
fn palette_cards(h: &Harness<BimApp>, from: f32, to: f32) -> Vec<egui::Rect> {
    fn walk(s: &Shape, out: &mut Vec<egui::Rect>) {
        match s {
            Shape::Rect(r) if r.fill == crate::style::pal().panel && r.rect.height() > 100.0 => out.push(r.rect),
            Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    h.output().shapes.iter().for_each(|c| walk(&c.shape, &mut out));
    out.retain(|r| r.left() >= from && r.left() < to && r.width() < 500.0);
    out
}

#[test]
fn a_long_value_in_a_views_properties_never_pushes_its_card_past_the_one_under_it() {
    // A view's Phasing group holds New Construction, wider than half a palette: Properties over the browser.
    let mut h = app(BOX);
    for look in ["plan", "3D"] {
        if look == "3D" {
            shortcut(&mut h, "3D");
        }
        let cards = palette_cards(&h, 0.0, 300.0);
        let [properties, browser] = cards.as_slice() else { panic!("{look}: two cards: {cards:?}") };
        assert_eq!(properties.right(), browser.right(), "{look}: {properties:?} {browser:?}");
    }
}

#[test]
fn find_runs_a_tool_or_opens_a_view_by_what_is_typed() {
    let mut h = app(BOX);
    let find = |h: &mut Harness<BimApp>, text: &str| {
        h.key_press_modifiers(Modifiers::COMMAND, Key::K);
        h.run();
        assert!(h.state().find.is_some(), "Ctrl+K opens Find");
        h.event(egui::Event::Text(text.to_string()));
        h.run();
        key(h, Key::Enter);
    };
    find(&mut h, "wall");
    assert!(matches!(h.state().tool, Tool::Wall(_)));
    assert!(h.state().find.is_none(), "and closes");
    key(&mut h, Key::Escape);
    key(&mut h, Key::Escape);
    let level_2 = h.state().session.model.levels()[1].0;
    find(&mut h, "plan level 2");
    assert_eq!(h.state().sheet, Sheet::Plan(level_2));
    // The quick access bar has a button for it.
    press(&mut h, "Find");
    assert!(h.state().find.is_some());
    key(&mut h, Key::Escape);
    assert!(h.state().find.is_none(), "Escape closes it");
}

#[test]
fn the_app_bar_is_light_in_the_light_theme_and_dark_in_the_dark_ones_light_canvas_or_not() {
    for (theme, light_canvas, light) in [
        (egui::ThemePreference::Light, false, true),
        (egui::ThemePreference::Dark, false, false),
        (egui::ThemePreference::Dark, true, false),
    ] {
        let mut h = app(BOX);
        h.state_mut().light_canvas = light_canvas;
        h.ctx.set_theme(theme);
        h.run();
        h.run();
        let ground = app_bar_ground(&h);
        assert_eq!(luminance(ground) > 0.5, light, "{theme:?}, light canvas {light_canvas}: {ground:?}");
        assert_eq!(crate::style::app_bar_rule().is_some(), light, "a hairline under a light bar alone");
        let p = crate::style::pal();
        for (what, ink) in [("lettering", p.app_bar_text), ("dimmer lettering", p.app_bar_muted)] {
            for g in [p.app_bar, p.app_bar_hover] {
                assert!(crate::style::contrast(ink, g) >= 4.5, "{theme:?}: {what} {ink:?} on {g:?}");
            }
        }
    }
}
