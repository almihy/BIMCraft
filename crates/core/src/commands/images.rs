//! Raster images and PDF pages placed in views, as Revit's Insert > Import PDF and Image and Manage > Manage Images
//! (docs/areas/cad-import.md, "Images and PDFs"): `import_pdf`, `import_image`, `pdf_pages` and `manage_images`. A page's
//! vector lines are read by `cad_read`, `cad_query` and `cad_outlines`, and picked by `wall` and the rest, as an import's.

use std::path::Path;

use serde_json::{Map, Value, json};

use super::{R, absent, bad, beside, count_opt, ext, num, only, point_opt, read_mm, read_point, string, view_param, view_title};
use crate::engine::Session;
use crate::geom::P2;
use crate::model::{Element, Id, MAX_TEXT, Model};
use crate::raster::{self, DrawLayer, ImageInstance, ImageType};

/// The keys `import_pdf` takes, and `import_image`.
const PDF_KEYS: &[&str] = &["path", "image", "name", "page", "resolution", "view", "level", "at", "scale", "width", "angle", "layer"];
const IMAGE_KEYS: &[&str] = &["path", "image", "name", "view", "level", "at", "scale", "width", "height", "angle", "layer"];

/// The bytes of the file `path` names (beside the project when it's relative), held to [`raster::MAX_FILE`], and its name.
fn read_file(c: &str, s: &Session, path: &str) -> Result<(Vec<u8>, String), String> {
    let full = beside(s, path);
    let len = s.files.given.size(&full).map_err(|e| bad(c, format!("{}: {e}", full.display())))?;
    if len > raster::MAX_FILE as u64 {
        return Err(bad(c, format!("{} is too large ({len} bytes; up to {})", full.display(), raster::MAX_FILE)));
    }
    let bytes = s.files.given.read(&full).map_err(|e| bad(c, format!("{}: {e}", full.display())))?;
    let source = Path::new(path).file_name().map_or_else(|| path.to_string(), |f| f.to_string_lossy().into_owned());
    Ok((bytes, source.chars().take(MAX_TEXT).collect()))
}

/// `name`, or the first of `name 2`, `name 3`… that the project's images don't have.
fn free_name(m: &Model, name: &str) -> String {
    let name: String = name.trim().chars().take(MAX_TEXT - 8).collect();
    if !m.image_types.contains_key(&name) {
        return name;
    }
    (2..).map(|n| format!("{name} {n}")).find(|n| !m.image_types.contains_key(n)).unwrap_or(name)
}

/// The name a file's image is given unless told one: its file's name, and the page of a PDF of more than one.
fn default_name(path: &str, page: Option<(usize, usize)>) -> String {
    let file = Path::new(path).file_name().map_or_else(|| "Image".to_string(), |f| f.to_string_lossy().into_owned());
    match page {
        Some((n, pages)) if pages > 1 => format!("{file} (page {n})"),
        _ => file,
    }
}

/// An image the project holds, by name, for `image`.
fn held<'m>(c: &str, m: &'m Model, name: &str) -> Result<&'m ImageType, String> {
    m.image_types.get(name).ok_or_else(|| {
        let names: Vec<&str> = m.image_types.keys().take(30).map(String::as_str).collect();
        let held = if names.is_empty() { "none".into() } else { names.join(", ") };
        bad(c, format!("the project holds no image named `{name}` (it holds: {held})"))
    })
}

/// Import PDF: page `page` of a PDF file rasterised at `resolution`, with its vector lines, placed in a view.
pub(super) fn import_pdf(s: &mut Session, p: &Value) -> R {
    let c = "import_pdf";
    only(c, p, PDF_KEYS)?;
    let (name, new, notes) = match (string(c, p, "path")?, string(c, p, "image")?) {
        (Some(_), Some(_)) => return Err(bad(c, "give `path`, a PDF file to import, or `image`, one the project holds; not both")),
        (None, None) => return Err(bad(c, "give `path`, a PDF file to import, or `image`, the name of one the project holds")),
        (None, Some(n)) => {
            if !absent(p.get("page")) || !absent(p.get("resolution")) {
                return Err(bad(c, "`page` and `resolution` are a file's, given with `path`; an image the project holds is placed as it is"));
            }
            held(c, &s.model, &n)?;
            (n, None, Vec::new())
        }
        (Some(path), None) => {
            let full = beside(s, &path);
            if ext(&full) != "pdf" {
                return Err(bad(c, format!("{}: Import PDF reads a .pdf file; a picture comes in with `import_image`", full.display())));
            }
            let page = count_opt(c, p, "page", 1)?;
            let dpi = match num(c, p, "resolution")? {
                None => raster::DEFAULT_DPI,
                Some(r) if raster::RESOLUTIONS.iter().any(|d| f64::from(*d) == r) => r as u32,
                Some(r) => {
                    let all: Vec<String> = raster::RESOLUTIONS.iter().map(u32::to_string).collect();
                    return Err(bad(c, format!("`resolution` is one of {} dpi, as Revit offers, not {r}", all.join(", "))));
                }
            };
            let (bytes, source) = read_file(c, s, &path)?;
            let pages = raster::pdf::Document::open(bytes.clone()).map_err(|e| bad(c, format!("{path}: {e}")))?.pages();
            let (t, notes) = ImageType::pdf(&source, bytes, page, dpi).map_err(|e| bad(c, format!("{path}: {e}")))?;
            let name = match string(c, p, "name")? {
                Some(n) if n.trim().is_empty() => return Err(bad(c, "an image needs a `name`")),
                Some(n) => n,
                None => default_name(&path, Some((page, pages))),
            };
            (free_name(&s.model, &name), Some(t), notes)
        }
    };
    place(c, s, p, &name, new, notes, true)
}

/// Insert > Image: a PNG, JPEG or BMP file, taken at 72 dpi as Revit takes one, placed in a view.
pub(super) fn import_image(s: &mut Session, p: &Value) -> R {
    let c = "import_image";
    only(c, p, IMAGE_KEYS)?;
    let (name, new) = match (string(c, p, "path")?, string(c, p, "image")?) {
        (Some(_), Some(_)) => return Err(bad(c, "give `path`, a picture to bring in, or `image`, one the project holds; not both")),
        (None, None) => return Err(bad(c, "give `path`, a picture to bring in, or `image`, the name of one the project holds")),
        (None, Some(n)) => {
            held(c, &s.model, &n)?;
            (n, None)
        }
        (Some(path), None) => {
            let full = beside(s, &path);
            match ext(&full).as_str() {
                "png" | "jpg" | "jpeg" | "jpe" | "bmp" => {}
                "pdf" => return Err(bad(c, format!("{}: a PDF comes in with `import_pdf`", full.display()))),
                other => return Err(bad(c, format!("{}: a .{other} file isn't a picture BIMCraft reads; it reads PNG, JPEG and BMP", full.display()))),
            }
            let (bytes, source) = read_file(c, s, &path)?;
            let t = ImageType::picture(&source, &bytes).map_err(|e| bad(c, format!("{path}: {e}")))?;
            let name = match string(c, p, "name")? {
                Some(n) if n.trim().is_empty() => return Err(bad(c, "an image needs a `name`")),
                Some(n) => n,
                None => default_name(&path, None),
            };
            (free_name(&s.model, &name), Some(t))
        }
    };
    place(c, s, p, &name, new, Vec::new(), false)
}

/// The image `name` (`new`, when it comes in now) placed in the view `p` says, as `import_pdf` and `import_image` place it.
fn place(c: &str, s: &mut Session, p: &Value, name: &str, new: Option<ImageType>, notes: Vec<String>, pdf: bool) -> R {
    let m = &s.model;
    if new.is_some() && m.image_types.len() >= raster::MAX_TYPES {
        return Err(bad(c, format!("the project holds {} images already; delete one with `manage_images` first", raster::MAX_TYPES)));
    }
    let view = view_param(c, m, p, true)?;
    let sheet = m.sheet(view).is_some();
    match m.elements.get(view) {
        Some(Element::Level(_) | Element::PlanView(_) | Element::Section(_) | Element::Elevation(_) | Element::Sheet(_)) => {}
        _ => {
            return Err(bad(
                c,
                format!("{} isn't a 2D view: an image goes in a plan, a ceiling plan, a section, an elevation or a sheet", view_title(m, view)),
            ));
        }
    }
    let t = match &new {
        Some(t) => t.clone(),
        None => held(c, m, name)?.clone(),
    };
    let stated = t.stated_scale();
    // The scale it comes in at: on a sheet, its paper's; in a model view, the one given, else the page's own, else its paper's.
    let scale = match num(c, p, "scale")? {
        Some(_) if sheet => return Err(bad(c, "on a sheet an image is the size of its paper; give a `width` to size it")),
        Some(k) if k.is_finite() && (1e-6..=1e6).contains(&k) => k,
        Some(k) => return Err(bad(c, format!("`scale` is how many mm of the model a mm of the image stands for (100 at 1 : 100), not {k}"))),
        None if sheet => 1.0,
        None => stated.unwrap_or(1.0),
    };
    let [w0, h0] = t.size();
    let (mut width, mut height) = (w0 * scale, h0 * scale);
    match (num(c, p, "width")?, if pdf { None } else { num(c, p, "height")? }) {
        (Some(w), Some(h)) => (width, height) = (w, h),
        (Some(w), None) => (width, height) = (w, h0 * w / w0),
        (None, Some(h)) => (width, height) = (w0 * h / h0, h),
        (None, None) => {}
    }
    if !(width.is_finite() && height.is_finite() && width >= raster::MIN_SIDE && height >= raster::MIN_SIDE) {
        return Err(bad(c, format!("`width` and `height` are the image's size in mm, from {} up, not {width} by {height}", raster::MIN_SIDE)));
    }
    let at = match point_opt(c, p, "at")? {
        Some(q) => q,
        // The middle of a sheet's paper; the origin of a model view.
        None => m.sheet(view).and_then(|sh| crate::model::paper(&sh.size)).map_or(P2(0.0, 0.0), |q| q * 0.5),
    };
    let angle = num(c, p, "angle")?.unwrap_or(0.0);
    let layer = match string(c, p, "layer")? {
        None => DrawLayer::Background,
        Some(l) => DrawLayer::from_name(&l).ok_or_else(|| bad(c, format!("`layer` is background or foreground, not {l}")))?,
    };
    let im = ImageInstance {
        type_name: name.to_string(),
        view: m.guid(view),
        at,
        width,
        height,
        angle,
        mirrored: false,
        lock: true,
        snaps: true,
        layer,
    };
    let corners = im.corners();
    let m = s.model_mut();
    if let Some(t) = new {
        m.image_types.insert(name.to_string(), t.clone());
    }
    let id = m.insert(Element::Image(im));
    let mut reply = json!({
        "id": id, "image": name, "source": t.source(), "dpi": t.dpi(), "pixels": [t.pixels().0, t.pixels().1],
        "size": [read_mm(w0), read_mm(h0)], "width": read_mm(width), "height": read_mm(height), "scale": scale,
        "corners": corners.map(read_point), "lines": t.lines().map_or(0, |l| l.dxf().entities.len()),
    });
    if let Some(page) = t.page() {
        reply["page"] = json!(page);
        reply["stated_scale"] = json!(stated);
    }
    if !notes.is_empty() {
        reply["notes"] = json!(notes);
    }
    Ok(reply)
}

/// What a PDF file's pages are: each one's size, mm, and the scale it says it is at.
pub(super) fn pdf_pages(s: &mut Session, p: &Value) -> R {
    let c = "pdf_pages";
    only(c, p, &["path"])?;
    let path = string(c, p, "path")?.ok_or_else(|| bad(c, "`path`, a PDF file, is required"))?;
    let (bytes, source) = read_file(c, s, &path)?;
    let doc = raster::pdf::Document::open(bytes).map_err(|e| bad(c, format!("{path}: {e}")))?;
    let pages: Vec<Value> = (1..=doc.pages())
        .take(1000)
        .map(|n| {
            let size = doc.size(n).map_or([0.0, 0.0], |s| s.map(read_mm));
            json!({"page": n, "size": size, "stated_scale": doc.scale(n)})
        })
        .collect();
    Ok(json!({"source": source, "pages": doc.pages(), "list": pages}))
}

/// The image instances of image `name`.
fn instances(m: &Model, name: &str) -> Vec<Id> {
    m.rare("image").into_iter().filter(|id| matches!(m.elements.get(*id), Some(Element::Image(im)) if im.type_name == name)).collect()
}

/// The images a project holds, each with what places it; `delete` removes images and every instance of them.
pub(super) fn manage_images(s: &mut Session, p: &Value) -> R {
    let c = "manage_images";
    only(c, p, &["delete"])?;
    let m = &s.model;
    let mut deleted: Vec<String> = Vec::new();
    let mut gone: Vec<Id> = Vec::new();
    match p.get("delete") {
        v if absent(v) => {}
        Some(Value::Array(a)) if a.len() <= raster::MAX_TYPES => {
            for v in a {
                let n = v.as_str().ok_or_else(|| bad(c, format!("`delete` lists images by name, not {v}")))?;
                held(c, m, n)?;
                gone.extend(instances(m, n));
                deleted.push(n.to_string());
            }
        }
        Some(v) => return Err(bad(c, format!("`delete` is a list of the names of images to delete, not {v}"))),
        None => {}
    }
    if !deleted.is_empty() {
        let mm = s.model_mut();
        for id in &gone {
            mm.elements.remove(*id);
        }
        for n in &deleted {
            mm.image_types.remove(n);
        }
    }
    let m = &s.model;
    let list: Vec<Value> = m
        .image_types
        .iter()
        .map(|(name, t)| {
            json!({
                "name": name, "source": t.source(), "kind": if t.page().is_some() { "pdf" } else { "image" }, "page": t.page(),
                "dpi": t.dpi(), "pixels": [t.pixels().0, t.pixels().1], "size": t.size().map(read_mm),
                "stated_scale": t.stated_scale(), "lines": t.lines().map_or(0, |l| l.dxf().entities.len()),
                "instances": instances(m, name),
            })
        })
        .collect();
    Ok(json!({"images": list, "deleted": deleted, "removed": gone}))
}

/// What `modify` sets of image `im`, from `fields`: `horizontal_scale` and `vertical_scale` as its `width` and `height`, and,
/// with its proportions locked, the one of them not given in proportion to the one given (Revit's Lock Proportions).
pub(super) fn sized(c: &str, m: &Model, im: &ImageInstance, fields: &Map<String, Value>) -> Result<Map<String, Value>, String> {
    let mut out = fields.clone();
    let t = held(c, m, &im.type_name)?;
    let [w0, h0] = t.size();
    for (key, side, natural) in [("horizontal_scale", "width", w0), ("vertical_scale", "height", h0)] {
        if let Some(v) = out.remove(key) {
            let k = v.as_f64().filter(|k| k.is_finite() && *k > 0.0).ok_or_else(|| bad(c, format!("`{key}` is a number over 0, not {v}")))?;
            if out.contains_key(side) {
                return Err(bad(c, format!("give `{key}` or `{side}`, not both")));
            }
            out.insert(side.into(), json!(k * natural));
        }
    }
    let lock = match out.get("lock") {
        Some(Value::Bool(b)) => *b,
        _ => im.lock,
    };
    let num = |k: &str| out.get(k).and_then(Value::as_f64);
    if lock && im.width > 0.0 && im.height > 0.0 {
        match (num("width"), num("height")) {
            (Some(w), None) => {
                out.insert("height".into(), json!(im.height * w / im.width));
            }
            (None, Some(h)) => {
                out.insert("width".into(), json!(im.width * h / im.height));
            }
            _ => {}
        }
    }
    if let Some(Value::String(l)) = out.get("layer") {
        let layer = DrawLayer::from_name(l).ok_or_else(|| bad(c, format!("`layer` is background or foreground, not {l}")))?;
        out.insert("layer".into(), json!(layer.name()));
    }
    Ok(out)
}

/// Revit's Scale (RE): images and CAD imports made `factor` times bigger about `origin`; or, graphically, by two reference
/// points, so the length from `origin` to `from` becomes `length` mm (or reaches as far as `to`).
pub(super) fn scale_elements(s: &mut Session, p: &Value) -> R {
    let c = "scale_elements";
    only(c, p, &["ids", "origin", "factor", "from", "to", "length"])?;
    let m = &s.model;
    let ids = super::ids_param(c, m, p)?;
    super::pin::refuse_pinned(c, m, &ids, super::pin::Held::Move)?;
    let origin = point_opt(c, p, "origin")?.ok_or_else(|| bad(c, "`origin` [x, y], the point that stays where it is, is required"))?;
    let factor = match (num(c, p, "factor")?, point_opt(c, p, "from")?, point_opt(c, p, "to")?, num(c, p, "length")?) {
        (Some(k), None, None, None) => k,
        (None, Some(from), Some(to), None) => to.dist(origin) / from.dist(origin),
        (None, Some(from), None, Some(len)) => len / from.dist(origin),
        _ => {
            return Err(bad(
                c,
                "give `factor`, or the reference `from` [x, y] and either `to` [x, y] or `length` mm: what the length from                  `origin` to `from` becomes",
            ));
        }
    };
    if !(factor.is_finite() && factor > 0.0 && (1e-6..=1e6).contains(&factor)) {
        return Err(bad(c, format!("the scale works out at {factor}; it is a number over 0 (the reference point can't be the origin)")));
    }
    let mut changed = Vec::new();
    for &id in &ids {
        let mut e = m.elements.get(id).cloned().ok_or_else(|| bad(c, format!("no element {id}")))?;
        match &mut e {
            Element::Image(im) => {
                im.at = super::micron(origin + (im.at - origin) * factor);
                (im.width, im.height) = (im.width * factor, im.height * factor);
            }
            Element::CadImport(ci) => {
                ci.at = super::micron(origin + (ci.at - origin) * factor);
                ci.scale *= factor;
            }
            other => {
                return Err(bad(c, format!("{} {id} isn't scaled: Scale works on images, PDFs and CAD imports", other.category())));
            }
        }
        changed.push((id, e));
    }
    let m = s.model_mut();
    for (id, e) in changed {
        m.elements.insert(id, e);
    }
    Ok(json!({"scaled": ids, "factor": factor}))
}
