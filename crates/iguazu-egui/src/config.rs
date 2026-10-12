use std::{borrow::Cow, collections::HashMap};

use egui::{Align, Button, Grid, Key, Label, Layout, Modifiers, PopupKind, RichText, ScrollArea, Sense, TextEdit, text::{CCursor, CCursorRange}, text_edit::TextEditOutput};
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

    pub fn inner(&self) -> &T {
        &self.configurable
    }

    pub fn inner_mut(&mut self) -> &mut T {
        &mut self.configurable
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

                    let edit = TextEdit::singleline(&mut state.value).id_salt(opt.name).show(ui);

                    if edit.response.has_focus() || edit.response.lost_focus() {
                        let suggestions = self.configurable.get_suggestions(opt.name);
                        if !suggestions.is_empty() {
                            suggestions_dropdown(ui, state, &edit, suggestions);
                        }
                    }

                    if edit.response.lost_focus() {
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

fn select_all(ui: &mut egui::Ui, edit: &TextEditOutput) {
    let mut state = edit.state.clone();
    state.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(0), CCursor::new(usize::MAX))));
    state.store(ui.ctx(), edit.response.id)
}

fn suggestions_dropdown(ui: &mut egui::Ui, state: &mut FieldState, edit: &TextEditOutput, suggestions: Cow<'_, [std::borrow::Cow<'_, str>]>) {
    let (up, down) = ui.input_mut(|input| (
        input.count_and_consume_key(Modifiers::NONE, Key::ArrowUp),
        input.count_and_consume_key(Modifiers::NONE, Key::ArrowDown),
    ));

    if up > 0 || down > 0 {
        let selected_i = suggestions.iter().position(|v| v[..] == state.value);
        let new_i = selected_i
            .and_then(|i| i.checked_sub(up))
            .and_then(|i| i.checked_add(down))
            .or((down > 0).then_some(0));

        if let Some(i) = new_i && let Some(v) = suggestions.get(i) {
            state.value = v.to_string();
        }

        select_all(ui, edit);
    }

    egui::Popup::from_response(&edit.response)
        .id(edit.response.id.with("suggestions"))
        .width(edit.response.rect.width())
        .kind(PopupKind::Menu)
        .sense(Sense::empty())
        .layout(Layout::top_down_justified(Align::Min))
        .show(|ui| {
            ui.set_min_width(ui.available_width());
            ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                for suggestion in suggestions.iter() {
                    let item = Button::selectable(state.value == &suggestion[..], &suggestion[..])
                        .sense(Sense::CLICK); // Not focusable so that tab goes to next field.
                    if ui.add(item).clicked() {
                        state.value = suggestion.to_string();
                    }
                }
            });
        });
}
