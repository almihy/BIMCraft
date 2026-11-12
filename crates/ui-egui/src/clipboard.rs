//! Copy to Clipboard and Paste, as Revit's: Ctrl+C puts the selection on the clipboard (the session's, in the core: no OS
//! clipboard), Ctrl+V holds what was copied at the cursor, outlined, until a click puts it there (snapping as every tool does),
//! and the Paste list (the Modify panel's) has the aligned ways: to Selected Levels (a list of levels to tick), to Selected Views (a
//! list of the views of the kind copied in, with a search), to the Current View,
//! to a Picked Level (in an elevation or a section, a click on a level line) and to the Same Place. The commands are the core's
//! `copy_to_clipboard` and `paste`.

use bimcraft_core::geom::P2;
use bimcraft_core::model::Id;
use bimcraft_i18n::msg;
use egui::{Align, Button, Context, Event, Id as UiId, Key, Layout, Modifiers, ScrollArea, Ui};
use serde_json::{Value, json};

use crate::icons::Icon;
use crate::lang::{t, tf};
use crate::popup::Modal;
use crate::ribbon;
use crate::{BimApp, Status, Tool, list_row, small_button};

/// The views dialog of Paste Aligned to Selected Views: the views of the kind copied in, each with whether it is ticked, and
/// what is typed to search them.
pub(crate) struct PasteViews {
    views: Vec<(Id, String, bool)>,
    search: String,
}

/// The levels dialog of Paste Aligned to Selected Levels: each level with its elevation, and whether it is ticked.
pub(crate) struct PasteLevels {
    levels: Vec<(Id, String, bool)>,
}

impl BimApp {
    /// Ctrl+C: the selection goes on the clipboard, with the doors and windows in its walls.
    pub(crate) fn copy_selection(&mut self) {
        if self.selection.is_empty() {
            self.status = Status { text: t("select what to copy first").into(), error: true };
            return;
        }
        if let Some(r) = self.run("copy_to_clipboard", json!({"ids": self.selection})) {
            let n = r["count"].as_u64().unwrap_or_default().to_string();
            self.status = Status { text: tf("{} elements copied to the clipboard", &[&n]), error: false };
        }
    }

    /// Ctrl+V: what was copied follows the cursor, outlined, until a click puts it down.
    pub(crate) fn begin_paste(&mut self) {
        if self.session.clipboard_ids().is_empty() {
            self.status = Status { text: t("nothing to paste: select elements and copy them first").into(), error: true };
        } else if self.plan_level().is_none() {
            self.status = Status { text: t("open a plan to paste into").into(), error: true };
        } else {
            self.set_tool(Tool::Paste);
        }
    }

    /// A click with the Paste tool: what was copied, its middle on `at`. Pasted into the plan's level: what stood on another
    /// level comes to this one.
    pub(crate) fn place_paste(&mut self, level: Id, at: P2) {
        let Some(anchor) = self.session.clipboard_anchor() else { return };
        let m = &self.session.model;
        let mut params = json!({"by": [at.0 - anchor.0, at.1 - anchor.1]});
        // Only what stands on a level, and on another than this plan's, is moved to it: grids, sections and the like stand on none.
        if self.session.clipboard_level().is_some_and(|from| from != m.guid(level)) {
            params["levels"] = json!([level]);
        }
        if self.paste(params) {
            self.tool = Tool::Modify;
        }
    }

    /// The Paste list's items that need no dialog.
    pub(crate) fn paste_aligned(&mut self, how: Aligned) {
        let params = match how {
            Aligned::CurrentView => match self.plan_key() {
                Some(view) => json!({"view": view}),
                None => {
                    self.status = Status { text: t("open a plan to paste into").into(), error: true };
                    return;
                }
            },
            Aligned::SamePlace => json!({"same_place": true}),
        };
        self.paste(params);
    }

    /// `paste` with `params`: what was pasted is selected, and what the paste could not keep as it was is said. Whether it pasted.
    fn paste(&mut self, params: Value) -> bool {
        let Some(r) = self.run("paste", params) else { return false };
        self.selection = r["ids"].as_array().map(|a| a.iter().filter_map(Value::as_u64).collect()).unwrap_or_default();
        let said = |key: &str| -> Vec<String> {
            r[key].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default()
        };
        let notes: Vec<String> = said("notes").into_iter().chain(said("left_out")).collect();
        let n = self.selection.len().to_string();
        let text = match notes.first() {
            None => tf("{} elements pasted", &[&n]),
            Some(first) if notes.len() == 1 => tf("{} elements pasted; {}", &[&n, first]),
            Some(first) => tf("{} elements pasted; {} (and {} more)", &[&n, first, &(notes.len() - 1).to_string()]),
        };
        self.status = Status { text, error: false };
        true
    }

    /// Paste Aligned to Picked Level: in an elevation or a section, a click on a level line pastes there, as Revit's does.
    pub(crate) fn begin_paste_to_picked_level(&mut self) {
        if !matches!(self.sheet, crate::Sheet::Section(_)) {
            let text = t("open an elevation or a section to pick a level line");
            self.status = Status { text: text.into(), error: true };
        } else if self.session.clipboard_ids().is_empty() {
            self.status = Status { text: t("nothing to paste: select elements and copy them first").into(), error: true };
        } else {
            self.set_tool(Tool::PasteLevel);
        }
    }

    /// A click in an elevation or a section with Paste Aligned to Picked Level: `found` is what was under it. A level line pastes at
    /// the same place on that level; anything else says so, and the tool goes on.
    pub(crate) fn paste_to_picked_level(&mut self, found: Option<Id>) {
        match found.filter(|id| self.session.model.level(*id).is_some()) {
            Some(level) => {
                if self.paste(json!({"levels": [level]})) {
                    self.tool = Tool::Modify;
                }
            }
            None => self.status = Status { text: t("click a level line").into(), error: true },
        }
    }

    /// Paste Aligned to Selected Levels: the dialog of levels.
    pub(crate) fn open_paste_levels(&mut self) {
        let levels = self.session.model.levels().into_iter().map(|(id, l)| (id, self.session.model.units.with_unit(l.elevation), false));
        let levels: Vec<(Id, String, bool)> =
            levels.map(|(id, z, on)| (id, format!("{} ({z})", self.session.model.level(id).map_or("", |l| l.name.as_str())), on)).collect();
        self.paste_levels = Some(PasteLevels { levels });
    }

    /// Paste Aligned to Selected Views: the dialog of views of the kind the annotation was copied in.
    pub(crate) fn open_paste_views(&mut self) {
        let views: Vec<(Id, String, bool)> = self.session.clipboard_paste_views().into_iter().map(|(id, name)| (id, name, false)).collect();
        if views.is_empty() {
            self.status = Status { text: t("no other view of that kind to paste into").into(), error: true };
            return;
        }
        self.paste_views = Some(PasteViews { views, search: String::new() });
    }

    /// The views dialog, while it is open.
    pub(crate) fn paste_views_window(&mut self, ctx: &Context) {
        let Some(d) = self.paste_views.as_mut() else { return };
        let (mut ok, mut close) = (false, false);
        let r = Modal::new(UiId::new("paste-views"), t("Select Views")).width(340.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.add_space(6.0);
                ui.label(t("Paste to the views:"));
                ui.add(egui::TextEdit::singleline(&mut d.search).hint_text(t("Search")));
                let found = d.search.trim().to_lowercase();
                ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                    for (_, name, on) in d.views.iter_mut().filter(|(_, name, _)| name.to_lowercase().contains(&found)) {
                        ui.checkbox(on, name.as_str());
                    }
                });
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui.button(t("Check All")).clicked() {
                        d.views.iter_mut().for_each(|v| v.2 = true);
                    }
                    if ui.button(t("Check None")).clicked() {
                        d.views.iter_mut().for_each(|v| v.2 = false);
                    }
                });
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    close = ui.button(t("Cancel")).clicked();
                    ok = ui.add_enabled(d.views.iter().any(|v| v.2), Button::new(t("OK"))).clicked();
                });
            });
        });
        if ok && let Some(d) = self.paste_views.take() {
            let views: Vec<Id> = d.views.iter().filter(|v| v.2).map(|v| v.0).collect();
            self.paste(json!({"views": views}));
        } else if close || r.should_close() {
            self.paste_views = None;
        }
    }

    /// The levels dialog, while it is open.
    pub(crate) fn paste_levels_window(&mut self, ctx: &Context) {
        let Some(d) = self.paste_levels.as_mut() else { return };
        let (mut ok, mut close) = (false, false);
        let r = Modal::new(UiId::new("paste-levels"), t("Select Levels")).width(340.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.add_space(6.0);
                ui.label(t("Paste to the levels:"));
                ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                    for (_, name, on) in d.levels.iter_mut().rev() {
                        ui.checkbox(on, name.as_str());
                    }
                });
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui.button(t("Check All")).clicked() {
                        d.levels.iter_mut().for_each(|l| l.2 = true);
                    }
                    if ui.button(t("Check None")).clicked() {
                        d.levels.iter_mut().for_each(|l| l.2 = false);
                    }
                });
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    close = ui.button(t("Cancel")).clicked();
                    ok = ui.add_enabled(d.levels.iter().any(|l| l.2), Button::new(t("OK"))).clicked();
                });
            });
        });
        if ok && let Some(d) = self.paste_levels.take() {
            let levels: Vec<Id> = d.levels.iter().filter(|l| l.2).map(|l| l.0).collect();
            self.paste(json!({"levels": levels}));
        } else if close || r.should_close() {
            self.paste_levels = None;
        }
    }

    /// Ctrl+C and Ctrl+V, as the key or as the window system's copy and paste. Whether one was pressed.
    pub(crate) fn clipboard_keys(&mut self, ctx: &Context) -> bool {
        let (copy, paste) = ctx.input_mut(|i| {
            let copy = i.consume_key(Modifiers::COMMAND, Key::C) | i.events.iter().any(|e| matches!(e, Event::Copy));
            let paste = i.consume_key(Modifiers::COMMAND, Key::V) | i.events.iter().any(|e| matches!(e, Event::Paste(_)));
            i.events.retain(|e| !matches!(e, Event::Copy | Event::Paste(_)));
            (copy, paste)
        });
        if copy {
            self.copy_selection();
        }
        if paste {
            self.begin_paste();
        }
        copy || paste
    }

    /// The Modify panel's Paste button and its list: Copy to Clipboard, Paste from Clipboard and the aligned ways. One button in the
    /// panel's second row, which is the shorter, so it costs the ribbon no width.
    pub(crate) fn clipboard_buttons(&mut self, ui: &mut Ui) {
        let tip = msg!("Paste (Ctrl+V): what was copied, placed by a click; the list copies the selection and has the aligned ways");
        let r = small_button(ui, Icon::Paste, "Paste", self.tool == Tool::Paste, tip);
        ribbon::choices(ui, &r, false, |ui| {
            ui.set_width(230.0);
            ui.spacing_mut().item_spacing.y = 0.0;
            let selected = !self.selection.is_empty();
            let tip = msg!("Copy to Clipboard (Ctrl+C): the selection, with the doors and windows in its walls");
            if ui.add_enabled_ui(selected, |ui| list_row(ui, Icon::Copy, "Copy to Clipboard", false, tip)).inner.clicked() {
                self.copy_selection();
            }
            ui.separator();
            let held = !self.session.clipboard_ids().is_empty();
            ui.add_enabled_ui(held, |ui| {
                let tip = msg!("Paste from Clipboard (Ctrl+V): what was copied, placed by a click");
                if list_row(ui, Icon::Paste, "Paste from Clipboard", false, tip).clicked() {
                    self.begin_paste();
                }
                let tip = msg!("Aligned to Selected Levels: at the same place on each level ticked, its heights moved with the level");
                if list_row(ui, Icon::Paste, "Aligned to Selected Levels…", false, tip).clicked() {
                    self.open_paste_levels();
                }
                // As in Revit, the view must be a different one from the view the elements were copied in.
                let tip = msg!(
                    "Aligned to Current View: at the same place, on the level of the plan on screen, which is not the one copied from"
                );
                let copied_here = self.plan_level().is_some_and(|l| self.session.clipboard_level() == Some(self.session.model.guid(l)));
                if ui.add_enabled_ui(!copied_here, |ui| list_row(ui, Icon::Paste, "Aligned to Current View", false, tip)).inner.clicked() {
                    self.paste_aligned(Aligned::CurrentView);
                }
                // As in Revit, a level is picked by its line in an elevation or a section.
                let tip =
                    msg!("Aligned to Picked Level: at the same place, on the level whose line is clicked in an elevation or a section");
                let in_section = matches!(self.sheet, crate::Sheet::Section(_));
                if ui.add_enabled_ui(in_section, |ui| list_row(ui, Icon::Paste, "Aligned to Picked Level", false, tip)).inner.clicked() {
                    self.begin_paste_to_picked_level();
                }
                // The annotation copied goes into other views of its kind, which the dialog lists.
                let tip = msg!(
                    "Aligned to Selected Views: what belongs to a view (dimensions, tags, notes, detail lines) at the same place in each view ticked, of the kind it was copied in"
                );
                let any_view = !self.session.clipboard_paste_views().is_empty();
                if ui.add_enabled_ui(any_view, |ui| list_row(ui, Icon::Paste, "Aligned to Selected Views…", false, tip)).inner.clicked() {
                    self.open_paste_views();
                }
                let tip = msg!("Aligned to Same Place: on top of what was copied");
                if list_row(ui, Icon::Paste, "Aligned to Same Place", false, tip).clicked() {
                    self.paste_aligned(Aligned::SamePlace);
                }
            });
        });
    }
}

/// The ways of Paste Aligned that take no dialog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Aligned {
    CurrentView,
    SamePlace,
}
