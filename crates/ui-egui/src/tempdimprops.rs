//! Manage > Temporary Dimensions, as Revit's Temporary Dimension Properties: what the temporary dimensions of walls measure to and
//! from (centrelines, faces, the core's centre or the core's faces) and what those of doors and windows do (centrelines or
//! openings). Kept in the project: OK runs the `temporary_dimension_settings` command, and the dimensions of the selected wall,
//! door or window follow it.

use bimcraft_core::tempdims::{OpeningLines, WallLines};
use bimcraft_i18n::msg;
use egui::{Align, Button, Context, Id as UiId, Layout, Ui};
use serde_json::json;

use crate::BimApp;
use crate::lang::t;
use crate::popup::Modal;

/// The dialog's draft: the choices, until OK.
pub(crate) struct TempDimDialog {
    walls: WallLines,
    openings: OpeningLines,
}

impl TempDimDialog {
    pub(crate) fn new(now: bimcraft_core::tempdims::Settings) -> TempDimDialog {
        TempDimDialog { walls: now.walls, openings: now.openings }
    }
}

/// What a choice of walls is called.
fn wall_name(l: WallLines) -> &'static str {
    match l {
        WallLines::Centerlines => msg!("Centerlines"),
        WallLines::Faces => msg!("Faces"),
        WallLines::CoreCentre => msg!("Center of Core"),
        WallLines::CoreFaces => msg!("Faces of Core"),
    }
}

/// What a choice of doors and windows is called.
fn opening_name(l: OpeningLines) -> &'static str {
    match l {
        OpeningLines::Centerlines => msg!("Centerlines"),
        OpeningLines::Openings => msg!("Openings"),
    }
}

impl BimApp {
    /// Open the dialog on the project's settings.
    pub(crate) fn open_temp_dim_properties(&mut self) {
        self.temp_dim_dialog = Some(TempDimDialog::new(self.session.model.temporary_dimensions));
    }

    /// The Temporary Dimension Properties dialog, while it is open.
    pub(crate) fn temp_dim_window(&mut self, ctx: &Context) {
        let Some(d) = self.temp_dim_dialog.as_mut() else { return };
        let (mut ok, mut close) = (false, false);
        let r = Modal::new(UiId::new("temporary-dimension-properties"), t("Temporary Dimension Properties")).width(360.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.add_space(6.0);
                ui.strong(t("Walls"));
                for l in WallLines::ALL {
                    ui.radio_value(&mut d.walls, l, t(wall_name(l)));
                }
                ui.add_space(8.0);
                ui.strong(t("Doors and Windows"));
                for l in OpeningLines::ALL {
                    ui.radio_value(&mut d.openings, l, t(opening_name(l)));
                }
                ui.add_space(4.0);
            });
            crate::popup::footer(ui, |ui: &mut Ui| {
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    close = ui.add(Button::new(t("Cancel"))).clicked();
                    ok = ui.add(Button::new(t("OK"))).clicked();
                });
            });
        });
        if ok && let Some(d) = self.temp_dim_dialog.take() {
            self.run("temporary_dimension_settings", json!({"walls": d.walls.name(), "openings": d.openings.name()}));
        } else if close || r.should_close() {
            self.temp_dim_dialog = None;
        }
    }
}
