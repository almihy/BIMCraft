//! Manage > Review Warnings, as Revit's: what is questionable in the project without being wrong (openings that overlap, a door
//! past its wall's end, walls on each other, a room not enclosed), listed with Show to select the elements and zoom to them.
//! The status bar says how many there are, and Properties lists those of the selected element. The project is never refused
//! for them; the core works them out ([`bimcraft_core::warnings`]) and the agent is told of the ones its commands make.

use bimcraft_i18n::msg;
use egui::{Align, Button, Context, Id as UiId, Layout, RichText, ScrollArea, Sense, Ui, WidgetInfo, WidgetType, vec2};

use bimcraft_core::inquiry;
use bimcraft_core::model::Id;
use bimcraft_core::warnings::{Severity, Warning};

use crate::icons::{self, Icon};
use crate::lang::{t, tf};
use crate::popup::Modal;
use crate::style::pal;
use crate::{BimApp, Sheet, Status};

/// The Review Warnings dialog while it is open: the row picked.
#[derive(Default)]
pub(crate) struct WarningsDialog {
    picked: Option<usize>,
}

/// What a row of the list says, for a screen reader and the tests: its message.
fn row_label(w: &Warning) -> String {
    w.message.clone()
}

impl BimApp {
    /// Manage > Review Warnings.
    pub(crate) fn open_warnings(&mut self) {
        self.warnings_dialog = Some(WarningsDialog::default());
    }

    /// Select the elements of warning `w` and show them: the plan they are in, zoomed to them. What the warning is about and
    /// no longer exists is left out.
    pub(crate) fn show_warning(&mut self, w: &Warning) {
        let ids: Vec<Id> = w.elements.iter().filter_map(|g| self.session.model.elements.id(*g)).collect();
        if ids.is_empty() {
            return;
        }
        self.selection = ids.clone();
        if let Some((level, [lo, hi])) = inquiry::shown(&self.session.model, &ids) {
            self.open_sheet(Sheet::Plan(level));
            self.canvas.show(level, lo, hi);
        }
        self.status = Status { text: tf("Selected {}.", &[&ids.len().to_string()]), error: false };
    }

    /// The warnings of the project, for what the window shows of them.
    pub(crate) fn project_warnings(&mut self) -> std::sync::Arc<Vec<Warning>> {
        self.session.warnings()
    }

    /// The status bar's count: a triangle and how many warnings the project has, where there are some; a click opens the list.
    pub(crate) fn warnings_button(&mut self, ui: &mut Ui) {
        let n = self.project_warnings().len();
        if n == 0 {
            return;
        }
        let label = if n == 1 { t("1 warning").to_string() } else { tf("{} warnings", &[&n.to_string()]) };
        let text = n.to_string();
        let font = egui::FontId::proportional(12.0);
        let wide = ui.painter().layout_no_wrap(text.clone(), font.clone(), pal().accent_ink).size().x;
        let (rect, r) = ui.allocate_exact_size(vec2(20.0 + wide, 18.0), Sense::click());
        r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, &label));
        if r.hovered() {
            ui.painter().rect_filled(rect, 4.0, pal().hover_fill);
        }
        icons::paint(ui.painter(), Icon::Warning, egui::Rect::from_min_size(rect.min + vec2(1.0, 1.0), vec2(16.0, 16.0)), pal().accent_ink);
        ui.painter().text(egui::pos2(rect.left() + 19.0, rect.center().y), egui::Align2::LEFT_CENTER, text, font, pal().accent_ink);
        if r.on_hover_text(tf("Review Warnings: {} in the project", &[&n.to_string()])).clicked() {
            self.open_warnings();
        }
    }

    /// What Properties says of the selected elements' warnings: each message, under a heading, where there are any.
    pub(crate) fn element_warnings(&mut self, ui: &mut Ui, ids: &[Id]) {
        let all = self.project_warnings();
        let model = std::sync::Arc::clone(&self.session.model);
        let mine: Vec<&Warning> =
            all.iter().filter(|w| w.elements.iter().any(|g| model.elements.id(*g).is_some_and(|id| ids.contains(&id)))).collect();
        if mine.is_empty() {
            return;
        }
        let group = msg!("Warnings");
        let open = !self.collapsed.contains(group);
        if crate::properties::group_header(ui, t(group), open) && !self.collapsed.remove(group) {
            self.collapsed.insert(group);
        }
        if open {
            for w in mine {
                ui.horizontal_top(|ui| {
                    let (rect, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                    icons::paint(ui.painter(), Icon::Warning, rect, pal().accent_ink);
                    ui.add(egui::Label::new(RichText::new(&w.message)).wrap());
                });
            }
        }
    }

    pub(crate) fn warnings_window(&mut self, ctx: &Context) {
        let Some(d) = self.warnings_dialog.as_mut() else { return };
        let all = self.session.warnings();
        let (mut close, mut show) = (false, None);
        let mut picked = d.picked.filter(|i| *i < all.len());
        let r = Modal::new(UiId::new("review-warnings"), t("Review Warnings")).width(640.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.add_space(6.0);
                if all.is_empty() {
                    ui.label(t("The project has no warnings."));
                    return;
                }
                ui.label(if all.len() == 1 {
                    t("1 warning. Show selects what it is about, and zooms to it.").to_string()
                } else {
                    tf("{} warnings. Show selects what one is about, and zooms to it.", &[&all.len().to_string()])
                });
                ui.add_space(4.0);
                ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                    for (i, w) in all.iter().enumerate() {
                        ui.horizontal_top(|ui| {
                            let (rect, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                            let ink = if w.severity == Severity::Note { pal().muted } else { pal().accent_ink };
                            icons::paint(ui.painter(), Icon::Warning, rect, ink);
                            let row = ui.selectable_label(picked == Some(i), RichText::new(row_label(w)));
                            if row.clicked() {
                                picked = Some(i);
                            }
                            if row.double_clicked() {
                                show = Some(i);
                            }
                        });
                    }
                });
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    close = ui.button(t("Close")).clicked();
                    if ui.add_enabled(picked.is_some(), Button::new(t("Show"))).on_hover_text(t("Select what the warning is about and zoom to it")).clicked() {
                        show = picked;
                    }
                });
            });
        });
        d.picked = picked;
        if let Some(i) = show
            && let Some(w) = all.get(i)
        {
            self.show_warning(w);
            self.warnings_dialog = None;
        } else if close || r.should_close() {
            self.warnings_dialog = None;
        }
    }
}
