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
