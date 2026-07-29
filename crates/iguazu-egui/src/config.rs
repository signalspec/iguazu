use std::collections::HashMap;

use egui::{Grid, Label, RichText};
use iguazu::config::Configurable;

pub struct ConfigEdit<T> {
    configurable: T,
    field_state: HashMap<&'static str, FieldState>,
}

struct FieldState {
    value: String,
    error: Option<String>,
}

impl<T: Configurable> ConfigEdit<T> {
    pub fn new(configurable: T) -> Self {
        Self {
            configurable,
            field_state: HashMap::new(),
        }
    }

    pub fn into_inner(self) -> T {
        self.configurable
    }

    pub fn is_error(&self) -> bool {
        self.field_state.values().any(|state| state.error.is_some())
    }

    pub fn show(&mut self, ui: &mut egui::Ui) {
        Grid::new("options").num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
            for opt in self.configurable.options() {
                let state = self.field_state.entry(opt.name).or_insert_with(|| {
                    FieldState {
                        value: self.configurable.get(opt.name).unwrap_or_default(),
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
                        state.error = self.configurable.set(opt.name, &state.value).err();

                        if state.error.is_none() {
                            // Read back the value to normalize it
                            state.value = self.configurable.get(opt.name).unwrap_or_default();
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
    }
}
