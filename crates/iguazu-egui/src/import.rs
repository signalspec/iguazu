use std::collections::HashMap;

use egui::{Button, Frame, Grid, Label, RichText, Ui};
use iguazu::import::Importer;

pub struct ImportUi {
    importer: Box<dyn Importer>,
    field_state: HashMap<&'static str, FieldState>,
}

struct FieldState {
    value: String,
    error: Option<String>,
}

pub enum ImportResponse {
    Accepted,
    Cancelled,
}

impl ImportUi {
    pub fn new(importer: Box<dyn Importer>) -> Self {
        Self { importer, field_state: HashMap::new() }
    }

    pub fn is_error(&self) -> bool {
        self.field_state.values().any(|state| state.error.is_some())
    }

    pub fn show(&mut self, ui: &mut Ui) -> Option<ImportResponse> {
        let mut response = None;

        Frame::new().inner_margin(16.0).show(ui, |ui| {
            ui.heading("Import options");
            ui.add_space(16.0);
            Grid::new("import_opts").num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
                for opt in self.importer.options() {
                    let state = self.field_state.entry(opt.name).or_insert_with(|| {
                        FieldState {
                            value: self.importer.get(opt.name).unwrap_or_default(),
                            error: None,
                        }
                    });

                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
                        ui.add(
                            Label::new(opt.name).selectable(false)
                        ).on_hover_text(opt.description);
                    });

                    ui.vertical(|ui| {
                        let error_color = ui.style().visuals.error_fg_color;

                        if state.error.is_some() {
                            let widgets = &mut ui.style_mut().visuals.widgets;
                            widgets.inactive.bg_stroke.color = error_color;
                            widgets.inactive.bg_stroke.width = 1.0;
                            widgets.active.bg_stroke.color = error_color;
                            widgets.active.bg_stroke.width = 1.0;
                            widgets.hovered.bg_stroke.color = error_color;
                            widgets.hovered.bg_stroke.width = 1.0;
                            widgets.open.bg_stroke.color = error_color;
                            widgets.open.bg_stroke.width = 1.0;
                        }

                        let edit_response = ui.text_edit_singleline(&mut state.value)
                            .on_hover_text(opt.description);

                        if edit_response.lost_focus() {
                            state.error = self.importer.set(opt.name, &state.value).err();

                            if state.error.is_none() {
                                // Read back the value to normalize it
                                state.value = self.importer.get(opt.name).unwrap_or_default();
                            }
                        }

                        ui.add_visible(
                            state.error.is_some(),
                            egui::Label::new(
                                RichText::new(state.error.as_deref().unwrap_or(""))
                                    .color(error_color),
                            ),
                        );
                    });

                    ui.end_row();
                }
            });

            ui.with_layout(egui::Layout::bottom_up(egui::Align::Max), |ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Max), |ui| {
                    let button_font = egui::FontId::proportional(24.0);

                    let btn_response = ui.add_enabled(!self.is_error(), Button::new(RichText::new("Import").font(button_font.clone())));
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
        self.importer
    }
}
