//! `render`'s tests that need only the model.

use crate::geom::P2;
use crate::pen::Pen;
use crate::prim::View;
use crate::render::*;
use crate::units::Units;

#[test]
fn spot_elevations_read_signed_heights_in_the_project_units() {
    assert_eq!(
        [height(Units::Millimetres, 0.3), height(Units::Millimetres, 3000.0), height(Units::Millimetres, -600.0)],
        ["±0", "+3000", "-600"]
    );
    assert_eq!([height(Units::Metres, 0.0), height(Units::FeetInches, 3000.0)], ["±0.00", "+9'-10 1/8\""]);
}

#[test]
fn lettering_turns_its_corners_round_while_a_pens_lines_turn_them_square() {
    let view = View { scale: 1.0, cx: 0.0, cy: 0.0, width: 400, height: 200 };
    let mut d = Drawing::new(view);
    d.text("VW", (200.0, 100.0), 80.0, INK);
    d.line(&[P2(-150.0, 0.0), P2(-100.0, 50.0), P2(-50.0, 0.0)], Pen::MEDIUM, INK, false);
    let svg = svg(&d);
    let joins: Vec<&str> = svg.split("stroke-linejoin=\"").skip(1).filter_map(|s| s.split('"').next()).collect();
    assert_eq!(joins.last(), Some(&"miter"), "the pen's line");
    assert!(joins.len() > 2 && joins[..joins.len() - 1].iter().all(|j| *j == "round"), "{joins:?}");
    let (pdf, _) = crate::pdf::contents(&d, P2(100.0, 50.0));
    assert!(pdf.contains(" 1 j ") && pdf.trim_end().rsplit('\n').next().is_some_and(|l| l.contains(" 0 j ")), "{pdf}");
}

/// The pixels `d` paints on paper, RGBA.
fn painted(d: &Drawing) -> Vec<u8> {
    let mut pm = tiny_skia::Pixmap::new(d.view.width, d.view.height).unwrap();
    pm.fill(tiny_skia::Color::from_rgba8(PAPER.0, PAPER.1, PAPER.2, 255));
    paint(&mut pm, d);
    pm.data().to_vec()
}

/// A dense drawing placed by `ci` drawn through `view` for a screen (simplified) or not, as `look` says.
fn dense_drawn(
    cad: &crate::cad::Cad,
    ci: &crate::model::CadImport,
    view: View,
    screen: bool,
    look: Option<&crate::graphics::ImportLook>,
) -> Drawing {
    let mut d = Drawing::new(view);
    d.screen = screen;
    cad_drawing(&mut d, cad, ci, None, look);
    d
}

#[test]
fn a_dense_drawing_zoomed_out_on_a_screen_draws_its_cells_and_looks_as_it_does_drawn_whole() {
    let cad = crate::cad::Cad::of_drawing("dense.pdf", crate::cad::tests::dense(60_000, P2(1000.0, 700.0)));
    let mut ci = crate::model::CadImport {
        cad: String::new(),
        level: crate::ids::Guid::default(),
        at: P2(0.0, 0.0),
        origin: P2(500.0, 350.0),
        scale: 1.0,
        stretch: 1.0,
        angle: 0.0,
        view: None,
        hidden: Default::default(),
        colors: Default::default(),
    };
    // Fitted: the sheet 420 px across.
    let view = View { scale: 0.42, cx: 0.0, cy: 0.0, width: 440, height: 320 };
    let (whole, cells) = (dense_drawn(&cad, &ci, view, false, None), dense_drawn(&cad, &ci, view, true, None));
    assert!(whole.prims.len() > 60_000, "{}", whole.prims.len());
    assert!(cells.prims.len() * 10 < whole.prims.len(), "{}", cells.prims.len());
    // It looks the same: as dark, and a pixel nearer the whole drawing's than the whole drawing moved half a pixel is to it
    // (random strokes under a pixel land on other pixels when moved, so that is as near as the same drawing comes).
    let (a, b) = (painted(&whole), painted(&cells));
    let moved = painted(&dense_drawn(&cad, &ci, View { cx: 0.5 / 0.42, cy: 0.5 / 0.42, ..view }, false, None));
    let off = |p: &[u8]| a.iter().zip(p).map(|(x, y)| f64::from(x.abs_diff(*y))).sum::<f64>() / a.len() as f64;
    let (cells_off, moved_off) = (off(&b), off(&moved));
    assert!(cells_off < moved_off * 0.75, "a channel is {cells_off:.2} levels off on average, moved half a pixel {moved_off:.2}");
    // Each 2 x 2 pixels' mean, as the eye sees a dense drawing, is nearer still.
    let blur = |p: &[u8]| -> Vec<f64> {
        let at = |x: usize, y: usize, c: usize| f64::from(p.get((y * 440 + x) * 4 + c).copied().unwrap_or(255));
        (0..160 * 220 * 4)
            .map(|i| {
                let (c, x, y) = (i % 4, 2 * ((i / 4) % 220), 2 * (i / 4 / 220));
                (at(x, y, c) + at(x + 1, y, c) + at(x, y + 1, c) + at(x + 1, y + 1, c)) / 4.0
            })
            .collect()
    };
    let (ba, bb, bm) = (blur(&a), blur(&b), blur(&moved));
    let blurred = |p: &[f64]| ba.iter().zip(p).map(|(x, y)| (x - y).abs()).sum::<f64>() / ba.len() as f64;
    assert!(blurred(&bb) < 12.0 && blurred(&bb) < blurred(&bm), "{:.2} against {:.2}", blurred(&bb), blurred(&bm));
    let dark = |p: &[u8]| p.iter().map(|v| f64::from(255 - v)).sum::<f64>();
    assert!((dark(&a) - dark(&b)).abs() < dark(&a) * 0.1, "{} against {}", dark(&b), dark(&a));
    // Red is the Doors': hidden in the import, none of it is drawn either way.
    let red = |p: &[u8]| p.chunks(4).filter(|c| c[0] > 150 && c[1] < 120 && c[2] < 120).count();
    assert!(red(&a) > 1000 && red(&b) > 1000, "{} {}", red(&a), red(&b));
    ci.hidden.insert("Doors".into());
    let (whole, cells) = (dense_drawn(&cad, &ci, view, false, None), dense_drawn(&cad, &ci, view, true, None));
    assert_eq!((red(&painted(&whole)), red(&painted(&cells))), (0, 0));
    // A layer's override draws in the cells too: the Hatch green.
    let mut look = crate::graphics::ImportLook::default();
    let green = crate::graphics::Override {
        projection: crate::graphics::Style { color: Some([0, 170, 0]), ..Default::default() },
        ..Default::default()
    };
    look.layers.insert("Hatch".into(), green);
    let greens = |p: &[u8]| p.chunks(4).filter(|c| c[1] > 120 && c[0] < 120 && c[2] < 120).count();
    let (whole, cells) = (dense_drawn(&cad, &ci, view, false, Some(&look)), dense_drawn(&cad, &ci, view, true, Some(&look)));
    let (g0, g1) = (greens(&painted(&whole)), greens(&painted(&cells)));
    assert!(g0 > 1000 && g1 > g0 / 2, "{g1} green pixels against {g0}");
    // Zoomed in, or not for a screen, every entity in view is drawn.
    let near = View { scale: 8.0, cx: -300.0, cy: 100.0, width: 400, height: 300 };
    let shown = cad.entities_in(P2(500.0 - 300.0 - 25.0, 350.0 + 100.0 - 19.0), P2(500.0 - 300.0 + 25.0, 350.0 + 100.0 + 19.0)).len();
    assert!(dense_drawn(&cad, &ci, near, true, None).prims.len() >= shown * 2 / 3, "{shown}");
    assert!(dense_drawn(&cad, &ci, view, false, None).prims.len() > 40_000);
}

/// An import of `cad` placed with its middle `middle` on the origin, as drawn.
fn placed(middle: P2) -> crate::model::CadImport {
    crate::model::CadImport {
        cad: String::new(),
        level: crate::ids::Guid::default(),
        at: P2(0.0, 0.0),
        origin: middle,
        scale: 1.0,
        stretch: 1.0,
        angle: 0.0,
        view: None,
        hidden: Default::default(),
        colors: Default::default(),
    }
}

/// How many of the pixels `p` paints are what `is` says.
fn count(p: &[u8], is: impl Fn(&[u8]) -> bool) -> usize {
    p.chunks(4).filter(|c| is(c)).count()
}

/// The mean of each channel of the pixels `p` paints: its colour and how much of it is inked, taken together.
fn mean(p: &[u8]) -> [f64; 3] {
    let n = (p.len() / 4).max(1) as f64;
    [0, 1, 2].map(|k| p.chunks(4).map(|c| f64::from(c[k])).sum::<f64>() / n)
}

/// Whether two drawings' means are within `levels` of each other in every channel.
fn alike(a: &[u8], b: &[u8], levels: f64) -> bool {
    mean(a).iter().zip(mean(b)).all(|(x, y)| (x - y).abs() < levels)
}

/// The verifier's sheet: a red fill 380 by 260 drawn first, 30 000 short black strokes over it, and a white mask over its
/// left half drawn last, which hides what it covers.
fn under_and_over() -> crate::dxf::Dxf {
    use crate::cad::tests::{drawing, entity, random};
    use crate::dxf::{Color, Shape};
    let rect = |x0: f64, y0: f64, x1: f64, y1: f64| vec![vec![P2(x0, y0), P2(x1, y0), P2(x1, y1), P2(x0, y1)]];
    let mut entities = vec![entity("Zones", Shape::Fill { rings: rect(20.0, 20.0, 400.0, 280.0), solid: true, tile: None })];
    let mut seed = 1;
    for _ in 0..30_000 {
        let at = P2(30.0 + random(&mut seed) * 360.0, 30.0 + random(&mut seed) * 240.0);
        let to = at + P2(random(&mut seed) * 4.0 - 2.0, random(&mut seed) * 4.0 - 2.0);
        entities.push(crate::dxf::Entity { weight: Some(0.2), ..entity("Lines", Shape::Polyline { pts: vec![at, to], closed: false }) });
    }
    entities.push(entity("Mask", Shape::Fill { rings: rect(20.0, 20.0, 120.0, 280.0), solid: true, tile: None }));
    drawing(&[("Zones", Color::True(255, 0, 0)), ("Lines", Color::Index(7)), ("Mask", Color::True(255, 255, 255))], entities)
}

#[test]
fn zoomed_out_a_dense_drawing_keeps_the_pages_order_a_fill_under_its_lines_and_a_mask_over_them() {
    let cad = crate::cad::Cad::of_drawing("zorder.pdf", under_and_over());
    let ci = placed(P2(210.0, 150.0));
    // Inked: a line over the red or the paper, darker than either.
    let dark = |c: &[u8]| c[0] < 150 && c[1] < 100 && c[2] < 100;
    let red = |c: &[u8]| c[0] > 180 && c[1] < 90 && c[2] < 90;
    // Fitted (the sheet 380 px across) and three times as near, whole and from its level of detail.
    for (scale, width) in [(1.0, 440), (3.0, 1300)] {
        let view = View { scale, cx: 0.0, cy: 0.0, width, height: (width as f64 * 0.75) as u32 };
        let (whole, cells) = (dense_drawn(&cad, &ci, view, false, None), dense_drawn(&cad, &ci, view, true, None));
        assert!(cells.prims.len() * 20 < whole.prims.len(), "drawn from its cells: {} prims", cells.prims.len());
        let (a, b) = (painted(&whole), painted(&cells));
        // The lines show over the red as they do drawn whole.
        let (da, db) = (count(&a, dark), count(&b, dark));
        assert!(da > 1000 && db as f64 > da as f64 * 0.6 && (db as f64) < da as f64 * 1.6, "at {scale}: {db} dark pixels against {da}");
        // As much red, and the same colour on the whole.
        assert!(count(&a, red) > 10_000 && count(&b, red) > 10_000, "at {scale}");
        assert!(alike(&a, &b, 10.0), "at {scale}: a mean of {:?} against {:?}", mean(&b), mean(&a));
        // The mask, drawn last, hides the left of it either way: nothing dark, nothing red there.
        let left = |p: &[u8]| -> usize {
            let x = |i: usize| (i % width as usize) as f64;
            let (x0, x1) = (view.px(P2(20.0 - 210.0 + 3.0, 0.0)).0 as f64, view.px(P2(120.0 - 210.0 - 3.0, 0.0)).0 as f64);
            p.chunks(4).enumerate().filter(|(i, c)| x(*i) > x0 && x(*i) < x1 && (dark(c) || red(c))).count()
        };
        assert_eq!((left(&a), left(&b)), (0, 0), "at {scale}: the mask covers the left");
    }
}

#[test]
fn zoomed_out_solid_fills_stay_solid_and_take_the_views_overrides_as_they_do_drawn_whole() {
    use crate::cad::tests::{drawing, entity, random};
    use crate::dxf::{Color, Shape};
    // 20 000 red squares a unit across on Poche (the verifier's sheet of small fills), and over them as many short strokes.
    let mut seed = 3;
    let mut entities = Vec::new();
    let mut strokes = Vec::new();
    for _ in 0..20_000 {
        let at = P2(10.0 + random(&mut seed) * 380.0, 10.0 + random(&mut seed) * 260.0);
        let ring = vec![at, at + P2(1.0, 0.0), at + P2(1.0, 1.0), at + P2(0.0, 1.0)];
        entities.push(entity("Poche", Shape::Fill { rings: vec![ring], solid: true, tile: None }));
        let to = at + P2(random(&mut seed) * 3.0, random(&mut seed) * 3.0) + P2(1.5, 1.5);
        strokes.push(entity("Poche", Shape::Polyline { pts: vec![at + P2(1.5, 1.5), to], closed: false }));
    }
    entities.extend(strokes);
    let d = drawing(&[("Poche", Color::True(230, 20, 20))], entities);
    let cad = crate::cad::Cad::of_drawing("smallfills.pdf", d);
    let ci = placed(P2(200.0, 140.0));
    let red = |c: &[u8]| c[0] > 200 && c[1] < 70 && c[2] < 70;
    let bluish = |c: &[u8]| i32::from(c[2]) > i32::from(c[0]) + 30;
    let pink = |c: &[u8]| c[0] > 230 && c[1] > 110 && c[1] < 170 && c[2] > 110 && c[2] < 170;
    let view = View { scale: 3.0, cx: 0.0, cy: 0.0, width: 1300, height: 900 };
    let both = |look: Option<&crate::graphics::ImportLook>| {
        (painted(&dense_drawn(&cad, &ci, view, false, look)), painted(&dense_drawn(&cad, &ci, view, true, look)))
    };
    let near =
        |a: usize, b: usize, what: &str| assert!(a > 2000 && (b as f64 - a as f64).abs() < a as f64 * 0.3, "{what}: {b} against {a}");
    // As they are: as many solid red pixels, not a pink haze, and the same colour on the whole.
    let (a, b) = both(None);
    assert!(dense_drawn(&cad, &ci, view, true, None).prims.len() < 100, "drawn from its cells");
    near(count(&a, red), count(&b, red), "solid red");
    assert!(alike(&a, &b, 10.0), "a mean of {:?} against {:?}", mean(&b), mean(&a));
    // The layer's Lines in blue: its lines turn blue, its fills stay red, either way.
    let mut look = crate::graphics::ImportLook::default();
    let lines = crate::graphics::Style { color: Some([0, 0, 255]), ..Default::default() };
    look.layers.insert("Poche".into(), crate::graphics::Override { projection: lines, ..Default::default() });
    let (a, b) = both(Some(&look));
    near(count(&a, red), count(&b, red), "red fills under blue lines");
    // As many lines blue (spread a little wider zoomed out, as a cell's ink is over the pixels round it).
    let (ba, bb) = (count(&a, bluish), count(&b, bluish));
    assert!(ba > 2000 && bb as f64 > ba as f64 * 0.7 && (bb as f64) < ba as f64 * 1.6, "blue lines: {bb} against {ba}");
    assert!(alike(&a, &b, 10.0), "a mean of {:?} against {:?}", mean(&b), mean(&a));
    // Halftone: both pale, the fills a pink.
    let mut look = crate::graphics::ImportLook::default();
    look.layers.insert("Poche".into(), crate::graphics::Override { halftone: true, ..Default::default() });
    let (a, b) = both(Some(&look));
    near(count(&a, pink), count(&b, pink), "halftone fills");
    assert_eq!((count(&a, red), count(&b, red)), (0, 0), "no full red left in halftone");
    assert!(alike(&a, &b, 10.0), "a mean of {:?} against {:?}", mean(&b), mean(&a));
}
