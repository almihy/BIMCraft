//! Copy to Clipboard and Paste, as Revit's: Ctrl+C puts the selection on the clipboard (the session's, in the core: no OS
//! clipboard), Ctrl+V holds what was copied at the cursor, outlined, until a click puts it there (snapping as every tool does),
//! and the Clipboard panel's Paste list has the aligned ways: to Selected Levels (a list of levels to tick), to the Current View,
//! to a Picked Level and to the Same Place. The commands are the core's `copy_to_clipboard` and `paste`.

use bimcraft_core::geom::P2;
use bimcraft_core::model::Id;
use bimcraft_i18n::msg;
use egui::{Align, Button, Context, Event, Id as UiId, Key, Layout, Modifiers, ScrollArea, Ui};
use serde_json::{Value, json};

use crate::lang::{t, tf};
use crate::popup::Modal;
use crate::ribbon::{self, panel};
use crate::icons::Icon;
use crate::{BimApp, Status, Tool, big_button, list_row};

/// The levels dialog of Paste Aligned to Selected Levels, or with `single` to a Picked Level: each level with its elevation, and
/// whether it is ticked.
pub(crate) struct PasteLevels {
    levels: Vec<(Id, String, bool)>,
    single: bool,
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
        if self.session.clipboard_level() != Some(m.guid(level)) {
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

    /// Paste Aligned to Selected Levels (or, with `single`, to a Picked Level): the dialog of levels.
    pub(crate) fn open_paste_levels(&mut self, single: bool) {
        let levels = self.session.model.levels().into_iter().map(|(id, l)| (id, self.session.model.units.with_unit(l.elevation), false));
        let levels: Vec<(Id, String, bool)> = levels
            .map(|(id, z, on)| (id, format!("{} ({z})", self.session.model.level(id).map_or("", |l| l.name.as_str())), on))
            .collect();
        self.paste_levels = Some(PasteLevels { levels, single });
    }

    /// The levels dialog, while it is open.
    pub(crate) fn paste_levels_window(&mut self, ctx: &Context) {
        let Some(d) = self.paste_levels.as_mut() else { return };
        let (mut ok, mut close) = (false, false);
        let title = if d.single { t("Pick Level") } else { t("Select Levels") };
        let r = Modal::new(UiId::new("paste-levels"), title).width(340.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.add_space(6.0);
                ui.label(t(if d.single { "Paste to the level:" } else { "Paste to the levels:" }));
                ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                    let single = d.single;
                    let mut picked = None;
                    for (i, (_, name, on)) in d.levels.iter_mut().enumerate().rev() {
                        if single {
                            if ui.radio(*on, name.as_str()).clicked() {
                                picked = Some(i);
                            }
                        } else {
                            ui.checkbox(on, name.as_str());
                        }
                    }
                    if let Some(i) = picked {
                        d.levels.iter_mut().enumerate().for_each(|(j, l)| l.2 = i == j);
                    }
                });
                if !d.single {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        if ui.button(t("Check All")).clicked() {
                            d.levels.iter_mut().for_each(|l| l.2 = true);
                        }
                        if ui.button(t("Check None")).clicked() {
                            d.levels.iter_mut().for_each(|l| l.2 = false);
                        }
                    });
                }
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

    /// The Modify tab's Clipboard panel: Copy to Clipboard, and Paste with its list.
    pub(crate) fn clipboard_panel(&mut self, ui: &mut Ui) {
        panel(ui, msg!("Clipboard"), |ui| {
            let tip = msg!("Copy to Clipboard (Ctrl+C): the selection, with the doors and windows in its walls");
            if ui.add_enabled_ui(!self.selection.is_empty(), |ui| big_button(ui, Icon::Copy, "Copy to Clipboard", false, tip)).inner.clicked()
            {
                self.copy_selection();
            }
            let held = !self.session.clipboard_ids().is_empty();
            let tip = msg!("Paste (Ctrl+V): what was copied, placed by a click; the list has the aligned ways");
            let r = ui.add_enabled_ui(held, |ui| big_button(ui, Icon::Paste, "Paste", self.tool == Tool::Paste, tip)).inner;
            ribbon::choices(ui, &r, false, |ui| {
                ui.set_width(230.0);
                ui.spacing_mut().item_spacing.y = 0.0;
                let tip = msg!("Paste from Clipboard (Ctrl+V): what was copied, placed by a click");
                if list_row(ui, Icon::Paste, "Paste from Clipboard", false, tip).clicked() {
                    self.begin_paste();
                }
                let tip = msg!("Aligned to Selected Levels: at the same place on each level ticked, its heights moved with the level");
                if list_row(ui, Icon::Paste, "Aligned to Selected Levels…", false, tip).clicked() {
                    self.open_paste_levels(false);
                }
                let tip = msg!("Aligned to Current View: at the same place, on the level of the plan on screen");
                if list_row(ui, Icon::Paste, "Aligned to Current View", false, tip).clicked() {
                    self.paste_aligned(Aligned::CurrentView);
                }
                let tip = msg!("Aligned to Picked Level: at the same place, on the level picked from the list");
                if list_row(ui, Icon::Paste, "Aligned to Picked Level…", false, tip).clicked() {
                    self.open_paste_levels(true);
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
