//! The Checks palette: the hosting page's checks of the model against the firm's rules and the client's brief, shown while
//! the model is drawn, as Revit shows its warnings. The page runs the checks (on its server, after each save) and gives
//! them to the window ([`BimApp::set_checks`]); each rule and requirement is listed passing or flagged, with why, the
//! flagged first. A click on one selects what it names and opens the plan that shows it, zoomed to it, and opens its
//! detail; the rule itself is the page's (its project's setup), so Open Rule asks the page to show it
//! ([`BimApp::take_rule_request`]). The status bar counts the flags; a click there opens the palette. A window no page hosts
//! has no checks and no palette.

use bimcraft_core::geom::P2;
use bimcraft_core::inquiry;
use bimcraft_core::model::Id;
use egui::text::{LayoutJob, TextWrapping};
use egui::{Align2, Color32, FontId, Rect, RichText, Sense, Stroke, Ui, Vec2, WidgetInfo, WidgetType, pos2, vec2};
use serde_json::Value;

use crate::dock::Palette;
use crate::icons::{self, Icon};
use crate::lang::{t, tf};
use crate::style::pal;
use crate::{BimApp, Sheet};

/// How far a check's answer goes: broken (a hard rule, a soft one, a check that could not run), accepted by a person
/// while broken, not checked automatically, or passing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum State {
    Failed,
    Crashed,
    Warning,
    Waived,
    Unchecked,
    Passed,
}

impl State {
    fn of(status: &str) -> State {
        match status {
            "fail" => State::Failed,
            "error" => State::Crashed,
            "warn" => State::Warning,
            "waived" => State::Waived,
            "pass" => State::Passed,
            _ => State::Unchecked,
        }
    }

    /// Whether it needs a person: what the status bar counts.
    pub fn flagged(self) -> bool {
        matches!(self, State::Failed | State::Crashed | State::Warning)
    }

    /// What its group is headed.
    fn group(self) -> &'static str {
        match self {
            State::Failed | State::Crashed | State::Warning => "Flagged",
            State::Waived => "Waived",
            State::Unchecked => "Not checked",
            State::Passed => "Passed",
        }
    }

    /// What it is called on its own, for a screen reader and the detail.
    fn word(self) -> &'static str {
        match self {
            State::Failed => "Failed",
            State::Crashed => "Check failed to run",
            State::Warning => "Warning",
            State::Waived => "Waived",
            State::Unchecked => "Not checked",
            State::Passed => "Passed",
        }
    }

    fn colour(self) -> Color32 {
        match self {
            State::Failed | State::Crashed => pal().error,
            State::Warning => {
                if pal().panel.r() < 128 {
                    Color32::from_rgb(0xF0, 0xB4, 0x29)
                } else {
                    Color32::from_rgb(0xB4, 0x6B, 0x00)
                }
            }
            State::Passed => {
                if pal().panel.r() < 128 {
                    Color32::from_rgb(0x4C, 0xC3, 0x7E)
                } else {
                    Color32::from_rgb(0x1E, 0x88, 0x4B)
                }
            }
            State::Waived | State::Unchecked => pal().muted,
        }
    }
}

/// A finding of a check: what is wrong, and the elements it names (the page's names for them: an element's id with a
/// letter before it, "w100023", or its GUID).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Finding {
    pub message: String,
    pub elements: Vec<String>,
}

/// One rule, requirement or design check, or one problem in the model itself, as the palette lists it.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    /// What tells it apart: the rule's id, or "model:" and its place for a problem in the model.
    pub key: String,
    pub title: String,
    pub state: State,
    /// The rule as written.
    pub text: String,
    pub findings: Vec<Finding>,
    /// Whose rule: "code", "company", "project", "client", "design" (Arqiflow's own), "model".
    pub scope: String,
    pub hard: bool,
    /// Who accepted it, and why.
    pub waiver: Option<(String, String)>,
    /// Why the check could not run.
    pub error: Option<String>,
    /// The rule the page shows, in its project's setup: none for Arqiflow's own checks and the model's problems.
    pub rule: Option<String>,
}

impl Row {
    /// Its one line of why: what it found first, or else the rule as written.
    pub fn why(&self) -> &str {
        self.findings.first().map(|f| f.message.as_str()).filter(|m| !m.is_empty()).unwrap_or(&self.text)
    }

    /// Every element it names, each once.
    fn elements(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for e in self.findings.iter().flat_map(|f| &f.elements) {
            if !out.contains(e) {
                out.push(e.clone());
            }
        }
        out
    }
}

/// The checks as the page last gave them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Report {
    /// Flagged first, then waived, not checked and passed; the model's own problems before the rules.
    pub rows: Vec<Row>,
    /// Why the page could not check the model, when it could not.
    pub error: Option<String>,
}

/// Longest text a check's field keeps: a page's text is hostile.
const MAX_TEXT: usize = 4000;
/// Most rows, findings to a row and elements to a finding kept.
const MAX_ROWS: usize = 2000;
const MAX_FINDINGS: usize = 500;
const MAX_ELEMENTS: usize = 2000;

fn text(v: &Value) -> String {
    let s = v.as_str().unwrap_or_default();
    let end = (0..=MAX_TEXT.min(s.len())).rev().find(|i| s.is_char_boundary(*i)).unwrap_or(0);
    s.get(..end).unwrap_or_default().trim().to_string()
}

fn findings(v: &Value) -> Vec<Finding> {
    v.as_array()
        .into_iter()
        .flatten()
        .take(MAX_FINDINGS)
        .map(|f| Finding {
            message: text(&f["message"]),
            elements: f["elements"].as_array().into_iter().flatten().take(MAX_ELEMENTS).map(text).filter(|e| !e.is_empty()).collect(),
        })
        .collect()
}

impl Report {
    /// Reads the page's checks, as Arqiflow's `GET /api/proposals/{id}/checks` answers (`{results, integrity, error?}`):
    /// kindly, as a file is read, leaving out what it doesn't know. Anything but an object is an error.
    pub fn read(json: &str) -> Result<Report, String> {
        let v: Value = serde_json::from_str(json).map_err(|e| format!("The checks are not JSON: {e}"))?;
        if !v.is_object() {
            return Err("The checks are not an object.".to_string());
        }
        let mut rows: Vec<Row> = findings(&v["integrity"])
            .into_iter()
            .enumerate()
            .map(|(i, f)| Row {
                key: format!("model:{i}"),
                title: t("Modelling problem").to_string(),
                state: State::Failed,
                text: String::new(),
                findings: vec![f],
                scope: "model".to_string(),
                hard: true,
                waiver: None,
                error: None,
                rule: None,
            })
            .collect();
        for r in v["results"].as_array().into_iter().flatten().take(MAX_ROWS) {
            let key = text(&r["rule_id"]);
            let scope = text(&r["scope"]);
            let waiver = r["waiver"].is_object().then(|| (text(&r["waiver"]["by"]), text(&r["waiver"]["reason"])));
            let error = Some(text(&r["error"])).filter(|e| !e.is_empty());
            // The firm's, the project's and the client's lines are in the project's setup; Arqiflow's own checks are not.
            let rule = (!key.is_empty() && scope != "design").then(|| key.clone());
            let title = Some(text(&r["title"])).filter(|t| !t.is_empty()).unwrap_or_else(|| key.clone());
            rows.push(Row {
                key,
                title,
                state: State::of(r["status"].as_str().unwrap_or_default()),
                text: text(&r["text"]),
                findings: findings(&r["findings"]),
                scope,
                hard: r["hard"].as_bool().unwrap_or(true),
                waiver,
                error,
                rule,
            });
        }
        // Flagged first, failed before warnings; the rest in the page's order.
        rows.sort_by_key(|r| r.state);
        let error = Some(text(&v["error"])).filter(|e| !e.is_empty());
        Ok(Report { rows, error })
    }

    /// How many need a person.
    pub fn flags(&self) -> usize {
        self.rows.iter().filter(|r| r.state.flagged()).count()
    }
}

/// The palette's state: the checks the page gave (none until it gives them: being checked), which rows show their detail,
/// and the rule the person asked the page to open.
#[derive(Default)]
pub struct Checks {
    pub report: Option<Report>,
    pub open: std::collections::BTreeSet<String>,
    pub asked: Option<String>,
}

/// What a scope is called.
fn scope_word(scope: &str) -> &'static str {
    match scope {
        "code" => "Building code",
        "company" => "Firm rule",
        "project" => "Project rule",
        "client" => "Client requirement",
        "design" => "Design check",
        "model" => "Model",
        _ => "Rule",
    }
}

/// A state's mark, a disc of its colour: a tick when passed, an exclamation when flagged, a dash when waived, a ring when
/// not checked.
pub(crate) fn mark(p: &egui::Painter, centre: egui::Pos2, r: f32, state: State) {
    let c = state.colour();
    let ink = if pal().panel.r() < 128 { Color32::from_gray(20) } else { Color32::WHITE };
    let w = (r * 0.28).max(1.4);
    match state {
        State::Unchecked => {
            p.circle_stroke(centre, r - 0.75, Stroke::new(1.5, c));
        }
        State::Passed => {
            p.circle_filled(centre, r, c);
            let pts = [centre + vec2(-0.45 * r, 0.02 * r), centre + vec2(-0.12 * r, 0.35 * r), centre + vec2(0.48 * r, -0.33 * r)];
            p.line_segment([pts[0], pts[1]], Stroke::new(w, ink));
            p.line_segment([pts[1], pts[2]], Stroke::new(w, ink));
        }
        State::Waived => {
            p.circle_filled(centre, r, c);
            p.line_segment([centre + vec2(-0.45 * r, 0.0), centre + vec2(0.45 * r, 0.0)], Stroke::new(w, ink));
        }
        State::Failed | State::Crashed | State::Warning => {
            p.circle_filled(centre, r, c);
            p.line_segment([centre + vec2(0.0, -0.52 * r), centre + vec2(0.0, 0.12 * r)], Stroke::new(w, ink));
            p.circle_filled(centre + vec2(0.0, 0.45 * r), w * 0.62, ink);
        }
    }
}

/// `s` on one line, cut with an ellipsis to `width`.
fn one_line(ui: &Ui, s: &str, font: FontId, colour: Color32, width: f32) -> std::sync::Arc<egui::Galley> {
    let mut job = LayoutJob::simple_singleline(s.replace('\n', " "), font, colour);
    job.wrap = TextWrapping { max_width: width.max(10.0), max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
    ui.painter().layout_job(job)
}

impl BimApp {
    /// The checks the page gives (see [`Report::read`]); `null` or nothing: the model is being checked. A row whose detail
    /// was open stays open.
    pub fn set_checks(&mut self, json: &str) -> Result<(), String> {
        let json = json.trim();
        if json.is_empty() || json == "null" {
            self.checks.report = None;
            return Ok(());
        }
        let report = Report::read(json)?;
        self.checks.open.retain(|k| report.rows.iter().any(|r| &r.key == k));
        self.checks.report = Some(report);
        Ok(())
    }

    /// The checks the window has, none while they are being made.
    pub fn checks(&self) -> Option<&Report> {
        self.checks.report.as_ref()
    }

    /// The rule the person asked to see since the last call (Open Rule): its id, for the page to show it where the project's
    /// rules are kept.
    pub fn take_rule_request(&mut self) -> Option<String> {
        self.checks.asked.take()
    }

    /// Whether the window offers the Checks palette: a page hosts it, and so checks the model.
    pub(crate) fn offers_checks(&self) -> bool {
        self.hosted
    }

    /// The palettes the window offers: the Checks too when a page hosts it.
    pub(crate) fn palettes(&self) -> Vec<Palette> {
        let mut all = Palette::ALL.to_vec();
        if self.offers_checks() {
            all.push(Palette::Checks);
        }
        all
    }

    /// The layout the window starts in: Revit's, and the Checks down the right side when a page hosts the window.
    pub(crate) fn first_docking(&self) -> crate::dock::Docking {
        let mut d = crate::dock::Docking::default();
        if self.offers_checks() {
            d.put(Palette::Checks, crate::dock::Target::Side(crate::dock::Side::Right));
        }
        d
    }

    /// Opens the Checks palette in front of its stack.
    pub(crate) fn open_checks(&mut self) {
        self.show_palette(Palette::Checks, true);
        self.docking.activate(Palette::Checks);
    }

    /// The elements `names` (the page's names: "w100023", an id, a GUID) are, as the model has them now.
    pub(crate) fn named_elements(&self, names: &[String]) -> Vec<Id> {
        let m = &self.session.model;
        let mut out = Vec::new();
        for n in names {
            let found = inquiry::one(m, n).or_else(|| {
                // The page's name: a letter or two before the element's id, and maybe more after it ("p100042-st3").
                let digits: String = n.chars().skip_while(|c| !c.is_ascii_digit()).take_while(char::is_ascii_digit).collect();
                (!digits.is_empty()).then(|| inquiry::one(m, &digits)).flatten()
            });
            if let Some(id) = found
                && !out.contains(&id)
            {
                out.push(id);
            }
        }
        out
    }

    /// Selects the elements `names` are and opens the plan that shows them, zoomed to them: what a click on a flag does.
    /// Whether any was found.
    pub(crate) fn show_named(&mut self, names: &[String]) -> bool {
        let ids = self.named_elements(names);
        if ids.is_empty() {
            return false;
        }
        self.selection = ids.clone();
        if let Some((level, [mut lo, mut hi])) = inquiry::shown(&self.session.model, &ids) {
            // A room is drawn as its tag: the view takes in the whole of what it covers.
            let m = &self.session.model;
            for id in &ids {
                if let Some(bimcraft_core::model::Element::Room(r)) = m.elements.get(*id) {
                    for p in m.room_outline(r).0 {
                        (lo, hi) = (P2(lo.0.min(p.0), lo.1.min(p.1)), P2(hi.0.max(p.0), hi.1.max(p.1)));
                    }
                }
            }
            self.open_sheet(Sheet::Plan(level));
            self.canvas.show(level, lo, hi);
        }
        true
    }

    /// The status bar's count of flags: a disc and the number, a click opening the palette. Nothing until the page gave
    /// checks.
    pub(crate) fn checks_badge(&mut self, ui: &mut Ui) {
        if !self.offers_checks() {
            return;
        }
        let Some(report) = &self.checks.report else { return };
        let flags = report.flags();
        let worst = report.rows.iter().map(|r| r.state).min().filter(|s| s.flagged()).unwrap_or(State::Passed);
        let label = match flags {
            0 => t("Checks: no flags").to_string(),
            1 => t("Checks: 1 flag").to_string(),
            n => tf("Checks: {} flags", &[&n.to_string()]),
        };
        let font = FontId::proportional(12.0);
        let number = ui.painter().layout_no_wrap(flags.to_string(), font, pal().text);
        let (rect, r) = ui.allocate_exact_size(vec2(14.0 + 4.0 + number.size().x + 10.0, 20.0), Sense::click());
        r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, &label));
        let p = ui.painter();
        if r.hovered() {
            p.rect_filled(rect, 4.0, pal().hover_fill);
        }
        mark(p, rect.left_center() + vec2(5.0 + 7.0, 0.0), 7.0, worst);
        p.galley(rect.left_center() + vec2(5.0 + 14.0 + 4.0, -number.size().y / 2.0), number, pal().text);
        if r.on_hover_text(&label).clicked() {
            self.open_checks();
        }
    }

    /// The Checks palette's body: what it says at a glance, then the rows in their groups.
    pub(crate) fn checks_palette(&mut self, ui: &mut Ui) {
        let Some(report) = self.checks.report.clone() else {
            egui::Frame::new().inner_margin(12.0).show(ui, |ui| {
                ui.label(RichText::new(t("Checking the model…")).color(pal().muted));
            });
            return;
        };
        let flags = report.flags();
        let passed = report.rows.iter().filter(|r| r.state == State::Passed).count();
        egui::Frame::new().inner_margin(egui::Margin { left: 12, right: 12, top: 10, bottom: 8 }).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let (headline, state) = match (flags, report.rows.is_empty()) {
                (_, true) if report.error.is_some() => (t("Not checked").to_string(), State::Unchecked),
                (_, true) => (t("No rules to check").to_string(), State::Unchecked),
                (0, false) => (t("No flags").to_string(), State::Passed),
                (1, false) => (t("1 flag").to_string(), State::Failed),
                (n, false) => (tf("{} flags", &[&n.to_string()]), State::Failed),
            };
            let state = if state == State::Failed { report.rows.iter().map(|r| r.state).min().unwrap_or(State::Failed) } else { state };
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
                mark(ui.painter(), r.center(), 8.0, state);
                ui.label(RichText::new(&headline).font(crate::style::semibold(14.0)).color(pal().text));
            });
            let sub = if report.rows.is_empty() && report.error.is_some() {
                String::new()
            } else if report.rows.is_empty() {
                t("No rules or client requirements apply to this project yet.").to_string()
            } else if passed == report.rows.len() {
                t("Every check passed.").to_string()
            } else {
                tf("{} passed.", &[&passed.to_string()])
            };
            if !sub.is_empty() {
                ui.label(RichText::new(sub).color(pal().muted).size(12.0));
            }
            if let Some(e) = &report.error {
                ui.label(RichText::new(e).color(pal().error).size(12.0));
            }
        });
        ui.separator();
        let scroll = egui::ScrollArea::vertical().auto_shrink([false, false]).min_scrolled_height(0.0).max_width(ui.available_width());
        scroll.id_salt("checks").show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let mut group = "";
            for row in &report.rows {
                let g = row.state.group();
                if g != group {
                    group = g;
                    let n = report.rows.iter().filter(|r| r.state.group() == g).count();
                    self.group_heading(ui, g, n);
                }
                self.check_row(ui, row);
            }
        });
    }

    fn group_heading(&self, ui: &mut Ui, group: &str, n: usize) {
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::hover());
        let p = ui.painter();
        p.rect_filled(rect, 0.0, pal().bar);
        let ink = if group == "Flagged" { pal().error } else { pal().muted };
        p.text(rect.left_center() + vec2(12.0, 0.0), Align2::LEFT_CENTER, format!("{}  {n}", t(group)), crate::style::semibold(12.0), ink);
    }

    /// A row: its mark, its title and one line of why; a click selects what it names and zooms to it, and opens its detail.
    fn check_row(&mut self, ui: &mut Ui, row: &Row) {
        let open = self.checks.open.contains(&row.key);
        let width = ui.available_width();
        let title_font = crate::style::semibold(12.5);
        let why_font = FontId::proportional(12.0);
        let text_w = width - 12.0 - 16.0 - 8.0 - 26.0;
        let title = one_line(ui, &row.title, title_font, pal().text, text_w);
        let why = one_line(ui, row.why(), why_font, pal().muted, text_w);
        let h = 10.0 + title.size().y + 2.0 + why.size().y + 10.0;
        let (rect, r) = ui.allocate_exact_size(vec2(width, h), Sense::click());
        let label = format!("{} — {}", row.title, t(row.state.word()));
        r.widget_info(|| WidgetInfo::selected(WidgetType::Button, true, open, &label));
        let p = ui.painter();
        if r.hovered() || open {
            p.rect_filled(rect, 0.0, if open { pal().select_fill } else { pal().hover_fill });
        }
        if r.has_focus() {
            p.rect_stroke(rect.shrink(1.0), 2.0, crate::style::focus_ring(), egui::StrokeKind::Inside);
        }
        let top = rect.top() + 10.0;
        mark(p, pos2(rect.left() + 12.0 + 7.0, top + title.size().y / 2.0), 7.0, row.state);
        let x = rect.left() + 12.0 + 16.0 + 8.0;
        p.galley(pos2(x, top), title.clone(), pal().text);
        p.galley(pos2(x, top + title.size().y + 2.0), why, pal().muted);
        // The chevron that says the detail is there.
        let c = pos2(rect.right() - 16.0, top + title.size().y / 2.0);
        let (a, b, d) = if open {
            (c + vec2(-4.0, 2.0), c + vec2(0.0, -2.0), c + vec2(4.0, 2.0))
        } else {
            (c + vec2(-4.0, -2.0), c + vec2(0.0, 2.0), c + vec2(4.0, -2.0))
        };
        let s = Stroke::new(1.4, pal().muted);
        p.line_segment([a, b], s);
        p.line_segment([b, d], s);
        let r = r.on_hover_text(row.why());
        if r.clicked() {
            if open {
                self.checks.open.remove(&row.key);
            } else {
                self.checks.open.insert(row.key.clone());
            }
            let names = row.elements();
            self.show_named(&names);
        }
        if self.checks.open.contains(&row.key) {
            self.check_detail(ui, row);
        }
        let y = ui.min_rect().bottom();
        ui.painter().line_segment([pos2(rect.left(), y), pos2(rect.right(), y)], Stroke::new(1.0, pal().border));
    }

    /// A row's detail: the rule as written, each finding (a click selects what it names), whose rule it is, its waiver, and
    /// Open Rule.
    fn check_detail(&mut self, ui: &mut Ui, row: &Row) {
        egui::Frame::new().inner_margin(egui::Margin { left: 36, right: 12, top: 2, bottom: 10 }).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 6.0;
            ui.label(RichText::new(t(row.state.word())).color(row.state.colour()).size(12.0).strong());
            if !row.text.is_empty() && row.scope != "model" {
                ui.label(RichText::new(&row.text).size(12.0).color(pal().text));
            }
            for (i, f) in row.findings.iter().enumerate() {
                let found = !self.named_elements(&f.elements).is_empty();
                if found {
                    let r = ui.add(egui::Button::new(RichText::new(&f.message).size(12.0)).wrap().frame(false));
                    let r = r.on_hover_text(t("Select and zoom to it"));
                    if r.clicked() {
                        self.show_named(&f.elements);
                    }
                } else if !f.message.is_empty() {
                    ui.label(RichText::new(&f.message).size(12.0).color(pal().text));
                }
                if i >= 20 {
                    let more = row.findings.len() - i - 1;
                    if more > 0 {
                        ui.label(RichText::new(tf("And {} more.", &[&more.to_string()])).size(12.0).color(pal().muted));
                    }
                    break;
                }
            }
            if let Some(e) = &row.error {
                ui.label(RichText::new(e).size(11.5).monospace().color(pal().muted));
            }
            if let Some((by, why)) = &row.waiver {
                ui.label(RichText::new(tf("Waived by {}: {}", &[by.as_str(), why.as_str()])).size(12.0).color(pal().muted));
            }
            let mut meta = t(scope_word(&row.scope)).to_string();
            if !row.hard && !matches!(row.scope.as_str(), "design" | "model") {
                meta = format!("{meta} · {}", t("Soft rule"));
            }
            ui.horizontal(|ui| {
                ui.label(RichText::new(meta).size(11.5).color(pal().muted));
                if let Some(rule) = &row.rule {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let label = if row.scope == "client" { t("Open Requirement") } else { t("Open Rule") };
                        if icon_text_button(ui, Icon::Link, label).on_hover_text(t("Show it in Project info")).clicked() {
                            self.checks.asked = Some(rule.clone());
                        }
                    });
                }
            });
        });
    }
}

/// A small button of an icon and a word.
fn icon_text_button(ui: &mut Ui, icon: Icon, label: &str) -> egui::Response {
    let font = FontId::proportional(12.0);
    let g = ui.painter().layout_no_wrap(label.to_string(), font, pal().text);
    let size = Vec2::new(8.0 + 14.0 + 5.0 + g.size().x + 9.0, 24.0);
    let (rect, r) = ui.allocate_exact_size(size, Sense::click());
    r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, label));
    let p = ui.painter();
    let fill = if r.hovered() { pal().hover_fill } else { Color32::TRANSPARENT };
    p.rect(rect, 4.0, fill, Stroke::new(1.0, pal().border), egui::StrokeKind::Inside);
    if r.has_focus() {
        p.rect_stroke(rect, 4.0, crate::style::focus_ring(), egui::StrokeKind::Inside);
    }
    icons::paint(p, icon, Rect::from_center_size(rect.left_center() + vec2(8.0 + 7.0, 0.0), vec2(14.0, 14.0)), pal().icon);
    p.galley(rect.left_center() + vec2(8.0 + 14.0 + 5.0, -g.size().y / 2.0), g, pal().text);
    r
}
