//! The engine's own families of the site, Revit's Planting and Site categories: trees, shrubs and hedges; bike racks,
//! benches and play equipment. Drawn here, original, after nobody's; a project loads one when it first places it
//! ([`crate::ground::load_family`]), as Revit loads a family from its library.

use std::collections::BTreeMap;

use bimcraft_i18n::msg;
use serde_json::{Value, json};

use super::Family;

pub const TREE: &str = msg!("Tree");
pub const SHRUB: &str = msg!("Shrub");
pub const HEDGE: &str = msg!("Hedge");
pub const BIKE_RACK: &str = msg!("Bike Rack");
pub const BENCH: &str = msg!("Bench");
pub const PLAY: &str = msg!("Play Equipment");
pub const CROWN: &str = msg!("Crown Diameter");
pub const HEIGHT: &str = "Height";
pub const TRUNK: &str = msg!("Trunk Diameter");
pub const LENGTH: &str = msg!("Length");
pub const WIDTH: &str = "Width";
pub const SPACES: &str = msg!("Spaces");
pub const RADIUS: &str = msg!("Radius");
/// Revit's Parking: the engine's parking space, its types and its parameters.
pub const PARKING_SPACE: &str = msg!("Parking Space");
pub const STANDARD: &str = msg!("Standard");
pub const ACCESSIBLE: &str = msg!("Accessible");
pub const EV: &str = "EV";
pub const MOTORCYCLE: &str = msg!("Motorcycle");
pub const DEPTH: &str = "Depth";
pub const ANGLE: &str = msg!("Angle");
pub const MARK: &str = "Mark";
/// Its subcategories, as Revit's Parking has them: the lines of a bay, and what is painted in it.
pub const STALL_LINES: &str = msg!("Stall Lines");
pub const MARKINGS: &str = msg!("Markings");
/// The materials they are made of, made in a project that hasn't them, with these colours.
pub const MATERIALS: [(&str, [u8; 3]); 4] =
    [(msg!("Foliage"), [95, 143, 74]), (msg!("Bark"), [107, 79, 58]), ("Metal", [150, 152, 158]), PARKING_PAINT];
/// What a parking space's pad is painted with.
pub const PARKING_PAINT: (&str, [u8; 3]) = (msg!("Parking Paint"), [148, 152, 160]);

/// The site's families, by name in `language`.
pub fn site_library(language: crate::language::Language) -> BTreeMap<String, Family> {
    let w = |s: &str| language.word(s).to_string();
    let (crown, height, trunk, length, width, spaces, radius) = (w(CROWN), w(HEIGHT), w(TRUNK), w(LENGTH), w(WIDTH), w(SPACES), w(RADIUS));
    let (foliage, bark, metal) = (w(MATERIALS[0].0), w(MATERIALS[1].0), w(MATERIALS[2].0));
    let size = |name: &str, value: f64| json!({"name": name, "value": value, "instance": true});
    let type_size = |name: &str, value: f64| json!({"name": name, "value": value});
    let ring = |c: &str| json!({"arc": {"centre": [0, 0], "radius": format!("{c} / 2"), "from": 0, "to": 360}, "show": {}});
    let mut families: Vec<(String, Value)> = Vec::new();
    // A tree: its trunk to a third of its height, its crown over that up to its height, round; in plan its crown's circle
    // and its trunk.
    families.push((
        w(TREE),
        json!({
            "category": "planting",
            "parameters": [size(&crown, 6000.0), size(&height, 8000.0), type_size(&trunk, 300.0)],
            "types": {"Deciduous": {}, "Small": {&trunk: 200}, "Conifer": {&trunk: 400}},
            "forms": [
                {"circle": {"centre": [0, 0], "radius": format!("{trunk} / 2")}, "from": 0, "to": format!("{height} * 0.35"), "material": bark},
                {"circle": {"centre": [0, 0], "radius": format!("{crown} / 2")}, "from": format!("{height} * 0.35"), "to": height, "material": foliage}
            ],
            "symbols": [ring(&crown), {"dot": {"centre": [0, 0], "radius": format!("{trunk} / 2")}, "show": {}}]
        }),
    ));
    families.push((
        w(SHRUB),
        json!({
            "category": "planting",
            "parameters": [size(&crown, 1500.0), size(&height, 1000.0)],
            "types": {"Shrub": {}},
            "forms": [{"circle": {"centre": [0, 0], "radius": format!("{crown} / 2")}, "from": 0, "to": height, "material": foliage}],
            "symbols": [ring(&crown)]
        }),
    ));
    // A hedge: a length of it, centred on where it stands, along its x.
    let (hl, hw) = (format!("{length} / 2"), format!("{width} / 2"));
    let box_ = |l: &str, wd: &str| json!([[format!("-{l}"), format!("-{wd}")], [l, format!("-{wd}")], [l, wd], [format!("-{l}"), wd]]);
    families.push((
        w(HEDGE),
        json!({
            "category": "planting",
            "parameters": [size(&length, 3000.0), size(&width, 1000.0), size(&height, 1500.0)],
            "types": {"Hedge": {}},
            "forms": [{"profile": box_(&hl, &hw), "from": 0, "to": height, "material": foliage}],
            "symbols": [{"line": [[format!("-{hl}"), format!("-{hw}")], [&hl, format!("-{hw}")], [&hl, &hw], [format!("-{hl}"), &hw], [format!("-{hl}"), format!("-{hw}")]], "show": {}}]
        }),
    ));
    // A bike rack: a rail a metre a space, 750 high.
    let rl = format!("{spaces} * 500");
    families.push((
        w(BIKE_RACK),
        json!({
            "category": "sitecomponent",
            "parameters": [size(&spaces, 6.0), type_size(&width, 2000.0)],
            "types": {"Sheffield": {}},
            "forms": [{"profile": box_(&rl, "40"), "from": 0, "to": 750, "material": metal}],
            "symbols": [{"line": [[format!("-{rl}"), format!("-{width} / 2")], [&rl, format!("-{width} / 2")], [&rl, format!("{width} / 2")], [format!("-{rl}"), format!("{width} / 2")], [format!("-{rl}"), format!("-{width} / 2")]], "show": {}}]
        }),
    ));
    families.push((
        w(BENCH),
        json!({
            "category": "sitecomponent",
            "parameters": [type_size(&length, 1800.0)],
            "types": {"Bench": {}},
            "forms": [{"profile": box_(&format!("{length} / 2"), "250"), "from": 400, "to": 450, "material": metal}],
            "symbols": [{"line": [[format!("-{length} / 2"), -250], [format!("{length} / 2"), -250], [format!("{length} / 2"), 250], [format!("-{length} / 2"), 250], [format!("-{length} / 2"), -250]], "show": {}}]
        }),
    ));
    families.push((
        w(PLAY),
        json!({
            "category": "sitecomponent",
            "parameters": [size(&radius, 4000.0)],
            "types": {"Play Equipment": {}},
            "forms": [{"circle": {"centre": [0, 0], "radius": 600}, "from": 0, "to": 2500, "material": metal}],
            "symbols": [{"arc": {"centre": [0, 0], "radius": radius, "from": 0, "to": 360}, "show": {}}]
        }),
    ));
    families.push((w(PARKING_SPACE), parking_space(language)));
    families.into_iter().filter_map(|(name, v)| serde_json::from_value(v).ok().map(|f| (name, f))).collect()
}

/// Revit's Parking Space: a bay, its origin in its middle, its x along the aisle and its y into it (its open side, the mouth,
/// at y = -Depth / 2). Its sides lean at Angle to the aisle (90: square), Width apart square to them, Depth from the aisle to
/// its back. Types Standard, Accessible (its access aisle within its width), EV and Motorcycle; a Mark each (its number).
/// In plan: its stall lines (its two sides and its back) and its markings (a wheelchair's wheel and head; EV); in 3D a pad
/// of paint on the ground.
fn parking_space(language: crate::language::Language) -> Value {
    let w = |s: &str| language.word(s).to_string();
    let (width, depth, angle, mark) = (w(WIDTH), w(DEPTH), w(ANGLE), w(MARK));
    let (lines, markings) = (w(STALL_LINES), w(MARKINGS));
    let (standard, accessible, motorcycle) = (w(STANDARD), w(ACCESSIBLE), w(MOTORCYCLE));
    // Half the bay along the aisle, and how far its back leans past its mouth, each way.
    let half = format!("{width} / sin({angle}) / 2");
    let lean = format!("{depth} * cos({angle}) / sin({angle}) / 2");
    let corner = |x: &str, y: &str| json!([x, y]);
    let (y0, y1) = (format!("-{depth} / 2"), format!("{depth} / 2"));
    let ml = corner(&format!("-({half}) - ({lean})"), &y0);
    let bl = corner(&format!("-({half}) + ({lean})"), &y1);
    let br = corner(&format!("({half}) + ({lean})"), &y1);
    let mr = corner(&format!("({half}) - ({lean})"), &y0);
    let paint = w(PARKING_PAINT.0);
    json!({
        "category": "parking",
        "parameters": [
            {"name": width, "value": 2500.0, "group": "Dimensions"},
            {"name": depth, "value": 5000.0, "group": "Dimensions"},
            {"name": angle, "kind": "angle", "value": 90.0, "group": "Dimensions"},
            {"name": mark, "kind": "text", "instance": true, "value": "", "group": "Identity Data"}
        ],
        "types": {
            &standard: {},
            &accessible: {&width: 3700.0},
            EV: {},
            &motorcycle: {&width: 1000.0, &depth: 2200.0}
        },
        "subcategories": [{"name": lines}, {"name": markings}],
        "forms": [{"profile": [ml, mr, br, bl], "from": 0, "to": 5, "material": paint, "show": {"plan": false}}],
        "symbols": [
            {"line": [ml, bl, br, mr], "show": {}, "subcategory": lines},
            {"arc": {"centre": [0, 0], "radius": 500, "from": 0, "to": 360}, "show": {}, "subcategory": markings, "types": [accessible]},
            {"dot": {"centre": [0, 800], "radius": 150}, "show": {}, "subcategory": markings, "types": [accessible]},
            {"text": {"at": [0, -250], "height": 500, "value": "EV", "align": "center"}, "show": {}, "subcategory": markings, "types": [EV]}
        ]
    })
}
