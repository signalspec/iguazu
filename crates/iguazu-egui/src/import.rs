use egui::{Button, Frame, RichText, Ui};
use iguazu::import::Importer;

use crate::config::ConfigEdit;

pub struct ImportUi {
    config_edit: ConfigEdit<Box<dyn Importer>>,
}
pub enum ImportResponse {
    Accepted,
    Cancelled,
}

impl ImportUi {
    pub fn new(importer: Box<dyn Importer>) -> Self {
        Self { config_edit: ConfigEdit::new(importer) }
    }

    pub fn show(&mut self, ui: &mut Ui) -> Option<ImportResponse> {
        let mut response = None;

        Frame::new().inner_margin(16.0).show(ui, |ui| {
            ui.heading("Import options");
            ui.add_space(16.0);

            self.config_edit.show(ui);

            ui.with_layout(egui::Layout::bottom_up(egui::Align::Max), |ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Max), |ui| {
                    let button_font = egui::FontId::proportional(24.0);

                    let btn_response = ui.add_enabled(!self.config_edit.is_error(), Button::new(RichText::new("Import").font(button_font.clone())));
                    if btn_response.clicked() {
                        response = Some(ImportResponse::Accepted);
                    }

                    let btn_response = ui.add(Button::new(RichText::new("Cancel").font(button_font.clone())));
                    if btn_response.clicked() {
                        response = Some(ImportResponse::Cancelled);
                    }
                });
            });
        });

        response
    }

    pub fn into_inner(self) -> Box<dyn Importer> {
        self.config_edit.into_inner()
    }
}
