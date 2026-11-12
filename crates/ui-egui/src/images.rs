//! Raster images and PDFs, as Revit's: Insert > Import PDF asks for a file, then a box for its page (thumbnails, Page to
//! Import) and its resolution; Insert > Image asks for a picture. Either then follows the cursor as its extents crossed by
//! two lines until a click places it in the view on screen (`import_pdf`, `import_image`). A selected image shows its frame
//! and a grip at each corner, which a drag scales (its proportions kept while Lock Proportions is on); Manage > Manage
//! Images lists the project's images, places one again and deletes them.

use std::path::PathBuf;

use bimcraft_core::geom::P2;
use bimcraft_core::model::{Element, Id, Model};
use bimcraft_core::raster::{self, pdf::Document};
use bimcraft_core::render::View;
use egui::{
    Align, Button, Color32, Context, Grid, Id as UiId, Layout, Painter, Pos2, Rect, RichText, Sense, Shape, Stroke, StrokeKind,
    TextureHandle, TextureOptions, Ui, WidgetInfo, WidgetType, pos2, vec2,
};
use serde_json::{Value, json};

use crate::lang::{t, tf};
use crate::popup::Modal;
use crate::style::pal;
use crate::{BimApp, Tool};

/// The side of a page's thumbnail in the box, px.
const THUMB: u32 = 96;
/// The most pages the box shows thumbnails of; past them, a page is picked by its number.
const THUMBS: usize = 60;

/// Import PDF's box: the file, its pages' sizes and stated scales, the page picked and the resolution, and the thumbnails
/// made so far (one a frame).
pub(crate) struct PdfBox {
    path: PathBuf,
    doc: Option<Document>,
    sizes: Vec<[f64; 2]>,
    scales: Vec<Option<f64>>,
    page: usize,
    page_text: String,
    dpi: u32,
    thumbs: Vec<Option<TextureHandle>>,
    error: Option<String>,
}

/// An image being placed: the command that places it, its params but the view and the point, and its size where it
/// comes in, mm of paper (and the scale its page states, which a model view takes).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Placing {
    command: &'static str,
    params: Value,
    size: [f64; 2],
    stated: Option<f64>,
}

impl BimApp {
    /// Insert > Import PDF: the file the user picks, then the box for it.
    pub(crate) fn import_pdf_dialog(&mut self, ctx: &Context) {
        let ask = crate::dialog::Ask::new().add_filter(t("PDF files"), &["pdf"]).set_title(t("Import PDF"));
        self.ask_file(ctx, ask, false, |app, path| app.import_pdf_path(path));
    }

    /// Insert > Image: the picture the user picks, then it follows the cursor.
    pub(crate) fn import_image_dialog(&mut self, ctx: &Context) {
        let ask = crate::dialog::Ask::new()
            .add_filter(t("Image files (PNG, JPEG, BMP)"), &["png", "jpg", "jpeg", "bmp"])
            .set_title(t("Import Image"));
        self.ask_file(ctx, ask, false, |app, path| app.import_image_path(path));
    }

    /// Opens Import PDF's box for the file at `path`.
    pub(crate) fn import_pdf_path(&mut self, path: PathBuf) {
        let read = self.session.files.given.read(&path).map_err(|e| e.to_string()).and_then(Document::open);
        let (doc, error) = match read {
            Ok(d) => (Some(d), None),
            Err(e) => (None, Some(e)),
        };
        let n = doc.as_ref().map_or(0, Document::pages);
        let sizes = (1..=n).map(|p| doc.as_ref().and_then(|d| d.size(p).ok()).unwrap_or([0.0, 0.0])).collect();
        let scales = (1..=n).map(|p| doc.as_ref().and_then(|d| d.scale(p))).collect();
        self.pdf_box = Some(PdfBox {
            path,
            doc,
            sizes,
            scales,
            page: 1,
            page_text: "1".into(),
            dpi: raster::DEFAULT_DPI,
            thumbs: Vec::new(),
            error,
        });
    }

    /// The picture at `path` follows the cursor, to be placed by a click.
    pub(crate) fn import_image_path(&mut self, path: PathBuf) {
        let size = self.session.files.given.read(&path).map_err(|e| e.to_string()).and_then(|b| raster::picture_size(&b));
        match size {
            Ok(size) => self.start_placing(Placing {
                command: "import_image",
                params: json!({"path": path.to_string_lossy()}),
                size,
                stated: None,
            }),
            Err(e) => {
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                self.status = crate::Status { text: format!("{name}: {e}"), error: true };
            }
        }
    }

    /// The command the image being placed runs: `import_pdf` or `import_image`.
    pub(crate) fn placing_command(&self) -> Option<&'static str> {
        self.placing.as_ref().map(|p| p.command)
    }

    fn start_placing(&mut self, placing: Placing) {
        self.placing = Some(placing);
        self.start_tool(Tool::PlaceImage);
    }

    /// The image of the project's named `name` placed again (Manage Images' Place Instance).
    pub(crate) fn place_instance(&mut self, name: &str) {
        let Some(t) = self.session.model.image_types.get(name) else { return };
        let command = if t.page().is_some() { "import_pdf" } else { "import_image" };
        let (size, stated) = (t.size(), t.stated_scale());
        self.start_placing(Placing { command, params: json!({"image": name}), size, stated });
    }

    pub(crate) fn import_pdf_window(&mut self, ctx: &Context) {
        let Some(b) = self.pdf_box.as_mut() else { return };
        // One page's thumbnail a frame, so a long file doesn't hold the window.
        if let Some(doc) = &b.doc
            && b.thumbs.len() < b.sizes.len().min(THUMBS)
        {
            let n = b.thumbs.len() + 1;
            let [w, h] = b.sizes.get(n - 1).copied().unwrap_or([1.0, 1.0]);
            let dpi = (f64::from(THUMB) / (w.max(h) / 25.4).max(1e-3)).clamp(1.0, 72.0);
            let tex = doc.render(n, dpi).ok().map(|(w, h, rgba)| {
                let img = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba);
                ctx.load_texture(format!("pdf page {n}"), img, TextureOptions::LINEAR)
            });
            b.thumbs.push(tex);
            ctx.request_repaint();
        }
        let (mut ok, mut close) = (false, false);
        let r = Modal::new(UiId::new("import-pdf"), t("Import PDF")).width(520.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.add_space(6.0);
                let file = b.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                Grid::new("import-pdf-grid").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                    ui.label(t("Filename"));
                    ui.label(file);
                    ui.end_row();
                });
                if !b.thumbs.is_empty() {
                    ui.add_space(6.0);
                    egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            for (i, tex) in b.thumbs.iter().enumerate() {
                                let n = i + 1;
                                let (rect, response) = ui.allocate_exact_size(vec2(THUMB as f32 + 8.0, THUMB as f32 + 24.0), Sense::click());
                                response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, tf("Page {}", &[&n.to_string()])));
                                let picked = b.page == n;
                                let p = ui.painter_at(rect);
                                if picked || response.hovered() {
                                    p.rect_filled(rect, 4.0, if picked { pal().accent.gamma_multiply(0.25) } else { pal().border });
                                }
                                if let Some(tex) = tex {
                                    let size = tex.size_vec2();
                                    let k = (THUMB as f32 / size.x.max(size.y)).min(1.0);
                                    let img = Rect::from_center_size(rect.center() - vec2(0.0, 8.0), size * k);
                                    p.image(tex.id(), img, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                                    p.rect_stroke(img, 0.0, Stroke::new(1.0, pal().border), StrokeKind::Outside);
                                }
                                p.text(rect.center_bottom() - vec2(0.0, 4.0), egui::Align2::CENTER_BOTTOM, n.to_string(), egui::FontId::proportional(11.0), pal().text);
                                if response.clicked() {
                                    b.page = n;
                                    b.page_text = n.to_string();
                                }
                            }
                        });
                    });
                }
                ui.add_space(6.0);
                Grid::new("import-pdf-settings").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                    let l = ui.label(t("Page to Import"));
                    if ui.add_sized([80.0, 20.0], egui::TextEdit::singleline(&mut b.page_text)).labelled_by(l.id).changed()
                        && let Ok(n) = b.page_text.trim().parse::<usize>()
                        && (1..=b.sizes.len()).contains(&n)
                    {
                        b.page = n;
                    }
                    ui.end_row();
                    let l = ui.label(t("Resolution"));
                    let c = crate::icons::combo("import-pdf-dpi").selected_text(format!("{} DPI", b.dpi)).show_ui(ui, |ui| {
                        for d in raster::RESOLUTIONS {
                            ui.selectable_value(&mut b.dpi, d, format!("{d} DPI"));
                        }
                    });
                    c.response.labelled_by(l.id);
                    ui.end_row();
                    ui.label(t("Page Size"));
                    let [w, h] = b.sizes.get(b.page - 1).copied().unwrap_or([0.0, 0.0]);
                    ui.label(format!("{w:.0} × {h:.0} mm"));
                    ui.end_row();
                    if let Some(k) = b.scales.get(b.page - 1).copied().flatten() {
                        ui.label(t("Scale"));
                        ui.label(format!("1 : {}", (k * 100.0).round() / 100.0));
                        ui.end_row();
                    }
                });
                if let Some(e) = &b.error {
                    ui.add_space(6.0);
                    ui.label(RichText::new(e).color(pal().error));
                }
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    close = ui.button(t("Cancel")).clicked();
                    ok = ui.add_enabled(b.doc.is_some(), Button::new(t("OK"))).clicked();
                });
            });
        });
        if ok {
            let size = b.sizes.get(b.page - 1).copied().unwrap_or([0.0, 0.0]);
            let stated = b.scales.get(b.page - 1).copied().flatten();
            let params = json!({"path": b.path.to_string_lossy(), "page": b.page, "resolution": b.dpi});
            self.pdf_box = None;
            self.start_placing(Placing { command: "import_pdf", params, size, stated });
        } else if close || r.should_close() {
            self.pdf_box = None;
        }
    }

    /// The image being placed, its middle at `at` in view `view` (a plan, a section or elevation, a sheet): placed, selected,
    /// and Modify again. A refusal says why on the status bar and keeps the tool.
    pub(crate) fn place_image(&mut self, view: Id, at: P2) {
        let Some(pl) = self.placing.clone() else { return };
        let mut p = pl.params.clone();
        p["view"] = json!(view);
        p["at"] = json!([at.0, at.1]);
        if let Some(v) = self.run(pl.command, p) {
            self.placing = None;
            self.selection = v["id"].as_u64().into_iter().collect();
            let notes: Vec<&str> = v["notes"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
            if !notes.is_empty() {
                self.status = crate::Status { text: notes.join("; "), error: false };
            }
            self.start_tool(Tool::Modify);
        }
    }

    /// The size the image being placed comes in at in view `view`, mm of its terms: its paper's on a sheet, else times the
    /// scale its page states.
    fn placing_size(&self, view: Id) -> Option<[f64; 2]> {
        let pl = self.placing.as_ref()?;
        let sheet = self.session.model.sheet(view).is_some();
        let k = if sheet { 1.0 } else { pl.stated.unwrap_or(1.0) };
        Some(pl.size.map(|s| s * k))
    }

    /// Where the image being placed would go, as Revit shows it: its extents round `at`, crossed corner to corner.
    pub(crate) fn image_ghost(&self, p: &Painter, origin: Pos2, v: &View, view: Id, at: P2) {
        if self.tool != Tool::PlaceImage {
            return;
        }
        let Some([w, h]) = self.placing_size(view) else { return };
        let screen = |q: P2| {
            let (x, y) = v.px(q);
            origin + vec2(x, y)
        };
        let c = [P2(-0.5, 0.5), P2(0.5, 0.5), P2(0.5, -0.5), P2(-0.5, -0.5)].map(|k| screen(P2(at.0 + k.0 * w, at.1 + k.1 * h)));
        let stroke = Stroke::new(1.5, pal().accent);
        p.add(Shape::closed_line(c.to_vec(), stroke));
        p.line_segment([c[0], c[2]], stroke);
        p.line_segment([c[1], c[3]], stroke);
    }

    /// The selected image's frame and its corner grips, in `rect` through `v` (a plan's, a section's or a sheet's): a drag
    /// from a corner scales it about the corner across from it, in proportion while Lock Proportions is on.
    pub(crate) fn image_grips(&mut self, ui: &mut Ui, m: &Model, v: &View, rect: Rect) {
        let (&[id], Tool::Modify) = (self.selection.as_slice(), &self.tool) else { return };
        let Some(Element::Image(im)) = m.elements.get(id) else { return };
        let screen = |q: P2| {
            let (x, y) = v.px(q);
            rect.min + vec2(x, y)
        };
        let model = |q: Pos2| v.mm((q.x - rect.min.x, q.y - rect.min.y));
        let corners = im.corners();
        let p = ui.painter_at(rect);
        let held_key = ui.id().with(("image grip held", id));
        let mut drawn: Vec<Pos2> = corners.map(screen).to_vec();
        for (k, corner) in corners.iter().enumerate() {
            let at = screen(*corner);
            let key = ui.id().with(("image grip", id, k));
            let response = self.control(ui, Rect::from_center_size(at, vec2(12.0, 12.0)), key);
            response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, tf("Corner {} of image {}", &[&(k + 1).to_string(), &id.to_string()])));
            if response.drag_started() && !self.refuse_pinned_ids(&[id]) {
                ui.memory_mut(|mem| mem.data.insert_temp(held_key, k));
            }
            let held: Option<usize> = ui.memory(|mem| mem.data.get_temp(held_key));
            if held != Some(k) || !(response.dragged() || response.drag_stopped()) {
                continue;
            }
            let Some(now) = ui.input(|i| i.pointer.latest_pos()) else { continue };
            let Some(sized) = resized(im, k, model(now)) else { continue };
            drawn = sized.corners().map(screen).to_vec();
            if response.drag_stopped() {
                ui.memory_mut(|mem| mem.data.remove::<usize>(held_key));
                self.run(
                    "modify",
                    json!({"id": id, "at": [sized.at.0, sized.at.1], "width": sized.width, "height": sized.height}),
                );
            }
        }
        let stroke = Stroke::new(1.0, pal().accent);
        p.add(Shape::closed_line(drawn.clone(), stroke));
        for at in drawn {
            let r = Rect::from_center_size(at, vec2(8.0, 8.0));
            p.rect_filled(r, 1.0, pal().surface);
            p.rect_stroke(r, 1.0, Stroke::new(1.5, pal().accent), StrokeKind::Middle);
        }
    }

    /// Bring to Front and Send to Back, the Arrange panel of the images selected: their Draw Layer.
    pub(crate) fn arrange(&mut self, front: bool) {
        let ids = self.selected_images();
        if !ids.is_empty() {
            self.run("modify", json!({"ids": ids, "layer": if front { "foreground" } else { "background" }}));
        }
    }

    /// The images among the selection.
    pub(crate) fn selected_images(&self) -> Vec<Id> {
        let m = &self.session.model;
        self.selection.iter().copied().filter(|id| matches!(m.elements.get(*id), Some(Element::Image(_)))).collect()
    }

    /// Whether the selection is all of what Scale works on, images and CAD imports, and has some.
    pub(crate) fn scalable_selected(&self) -> bool {
        let m = &self.session.model;
        !self.selection.is_empty()
            && self.selection.iter().all(|id| matches!(m.elements.get(*id), Some(Element::Image(_) | Element::CadImport(_))))
    }

    /// A click of Scale at `at`, in the view's terms: the origin, the first reference point, then the second, which scales.
    pub(crate) fn scale_click(&mut self, at: P2) {
        let Tool::Scale(pts) = &self.tool else { return };
        match pts.as_slice() {
            [origin, from] => {
                let p = json!({"ids": self.selection, "origin": [origin.0, origin.1], "from": [from.0, from.1], "to": [at.0, at.1]});
                if self.run("scale_elements", p).is_some() {
                    self.start_tool(Tool::Modify);
                }
            }
            _ => {
                let mut pts = pts.clone();
                pts.push(at);
                self.tool = Tool::Scale(pts);
            }
        }
    }

    /// Scale's Options Bar: once the origin and the first reference are clicked, the length the reference becomes, typed.
    pub(crate) fn scale_options(&mut self, ui: &mut Ui) {
        let Tool::Scale(pts) = &self.tool else { return };
        let pts = pts.clone();
        let l = ui.label(t("Length:"));
        let r = ui.add_enabled(pts.len() == 2, egui::TextEdit::singleline(&mut self.scale_length).desired_width(80.0)).labelled_by(l.id);
        if r.lost_focus()
            && ui.input(|i| i.key_pressed(egui::Key::Enter))
            && let [origin, from] = pts.as_slice()
            && let Some(len) = self.session.model.units.parse(&self.scale_length).filter(|d| *d > 0.0)
        {
            let p = json!({"ids": self.selection, "origin": [origin.0, origin.1], "from": [from.0, from.1], "length": len});
            if self.run("scale_elements", p).is_some() {
                self.scale_length.clear();
                self.start_tool(Tool::Modify);
            }
        }
    }

    /// Manage > Manage Images: each image the project holds, how many times it is placed, Place Instance and Delete.
    pub(crate) fn manage_images_window(&mut self, ctx: &Context) {
        if !self.images_window {
            return;
        }
        let m = std::sync::Arc::clone(&self.session.model);
        let count = |name: &str| {
            m.rare("image").into_iter().filter(|id| matches!(m.elements.get(*id), Some(Element::Image(im)) if im.type_name == name)).count()
        };
        let (mut place, mut delete, mut close) = (None, None, false);
        let r = Modal::new(UiId::new("manage-images"), t("Manage Images")).width(620.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                if m.image_types.is_empty() {
                    ui.label(t("The project holds no images."));
                }
                Grid::new("manage-images-grid").num_columns(6).striped(true).spacing([12.0, 6.0]).show(ui, |ui| {
                    for h in ["Name", "Count", "Type", "Page", "Resolution", "Loaded from file"] {
                        ui.label(RichText::new(t(h)).strong());
                    }
                    ui.end_row();
                    for (name, ty) in &m.image_types {
                        let picked = self.images_picked.as_deref() == Some(name.as_str());
                        if ui.selectable_label(picked, name).clicked() {
                            self.images_picked = Some(name.clone());
                        }
                        ui.label(count(name).to_string());
                        ui.label(if ty.page().is_some() { "PDF" } else { t("Image") });
                        ui.label(ty.page().map(|p| p.to_string()).unwrap_or_default());
                        ui.label(format!("{} DPI", ty.dpi()));
                        ui.label(ty.source());
                        ui.end_row();
                    }
                });
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    close = ui.button(t("OK")).clicked();
                    let picked = self.images_picked.clone().filter(|n| m.image_types.contains_key(n));
                    if ui.add_enabled(picked.is_some(), Button::new(t("Delete"))).clicked() {
                        delete = picked.clone();
                    }
                    if ui.add_enabled(picked.is_some(), Button::new(t("Place Instance"))).clicked() {
                        place = picked;
                    }
                });
            });
        });
        if let Some(name) = delete {
            self.run("manage_images", json!({"delete": [name]}));
            self.images_picked = None;
        }
        if let Some(name) = place {
            self.images_window = false;
            self.place_instance(&name);
        } else if close || r.should_close() {
            self.images_window = false;
        }
    }
}

/// Image `im` with its corner `k` (of [`raster::ImageInstance::corners`]) taken to `to`, the corner across from it staying
/// where it is: its proportions kept when locked. None when it would have no size.
pub(crate) fn resized(im: &raster::ImageInstance, k: usize, to: P2) -> Option<raster::ImageInstance> {
    let corners = im.corners();
    let anchor = *corners.get((k + 2) % 4)?;
    let from = *corners.get(k)?;
    let (sin, cos) = im.angle.to_radians().sin_cos();
    let (x, y) = (P2(cos, sin), P2(-sin, cos));
    // Which way the corner lies from the anchor along the image's own sides.
    let (sx, sy) = ((from - anchor).dot(x).signum(), (from - anchor).dot(y).signum());
    let d = to - anchor;
    let (mut w, mut h) = ((d.dot(x) * sx).max(0.0), (d.dot(y) * sy).max(0.0));
    if im.lock && im.width > 0.0 && im.height > 0.0 {
        // Along the diagonal: the drag's reach over the corner's, as far as it goes.
        let s = (w * im.width + h * im.height) / (im.width * im.width + im.height * im.height);
        (w, h) = (im.width * s, im.height * s);
    }
    if w < raster::MIN_SIDE || h < raster::MIN_SIDE {
        return None;
    }
    let at = anchor + x * (sx * w / 2.0) + y * (sy * h / 2.0);
    Some(raster::ImageInstance { at, width: w, height: h, ..im.clone() })
}
