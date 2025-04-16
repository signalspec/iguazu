use std::collections::BTreeMap;

use egui::Margin;
use iguazu::{schema::EntityStream, view::{TextView, ViewManager}};
use itertools::Itertools;

use crate::{cache::ViewCache, ViewerContext};

pub struct TableView {
}

impl TableView {
    pub fn new() -> Self {
        Self {}
    }
    
    pub fn show(
        &mut self,
        _vcx: &mut ViewerContext,
        ui: &mut egui::Ui,
        entity: &mut EntityStream,
    ) {
        let mut view_manager = ViewCache::with(ui);
        let mut delegate = Delegate::new(entity, &mut view_manager);
        let table = delegate.table();
        table.show(ui, &mut delegate);
    }
}

enum Column {
    Index,
    Text(TextView),
    //String(View, View),
    //Tiles
}

struct Delegate {
    columns: Vec<Column>,
    headers: BTreeMap<(usize, usize, usize), String>
}

impl Delegate {
    fn new(entity: &mut EntityStream, view_manager: &mut ViewCache) -> Self {
        let mut columns = Vec::new();
        let mut headers = BTreeMap::new();

        columns.push(Column::Index);
    
        fn inner(
            vm: &mut ViewCache,
            depth: usize,
            data: &mut Vec<Column>,
            headers: &mut BTreeMap<(usize, usize, usize), String>,
            entity: &EntityStream,
        ) {
            match entity.kind {
                iguazu::schema::EntityKind::Group => {}
                iguazu::schema::EntityKind::Record => {
                    for (name, child) in &entity.children {
                        let start = data.len();
                        inner(vm, depth + 1, data, headers, child);
                        let end = data.len();
                        headers.insert((depth, start, end), name.clone());
                    }
                }
                _ => {
                    data.push(Column::Text(vm.text_view(entity)))
                }
            }
        }
    
        inner(view_manager, 0, &mut columns, &mut headers, entity);
    
        Self { columns, headers }
    }

    fn table(&self) -> egui_table::Table {
        let columns: Vec<_> = self.columns.iter().map(|_| {
            egui_table::Column::new(100.0)
                    .range(10.0..=500.0)
                    .resizable(true)
        }).collect();
    
        let headers: Vec<_> = self.headers.keys()
            .chunk_by(|(depth, _, _)| depth)
            .into_iter()
            .map(|(_, cols)| {
                let groups = cols.map(|(_, start, end)| *start..*end).collect();
                let height = 24.0;
                egui_table::HeaderRow { height, groups }
            }).collect();

        egui_table::Table::new()
            .num_rows(100)
            .columns(columns)
            .headers(headers)
            .num_sticky_cols(1)
    }
}

impl egui_table::TableDelegate for Delegate {
    fn header_cell_ui(&mut self, ui: &mut egui::Ui, cell: &egui_table::HeaderCellInfo) {
        let egui_table::HeaderCellInfo {
            col_range,
            row_nr,
            ..
        } = cell;

        let label = self.headers.get(&(*row_nr, col_range.start, col_range.end)).unwrap();

        egui::Frame::new()
            .inner_margin(Margin::symmetric(4, 0))
            .show(ui, |ui| {
                ui.heading(label.to_owned());
            });
    }

    fn cell_ui(&mut self, ui: &mut egui::Ui, cell: &egui_table::CellInfo) {
        let egui_table::CellInfo { row_nr, col_nr, .. } = *cell;

        if row_nr % 2 == 1 {
            ui.painter()
                .rect_filled(ui.max_rect(), 0.0, ui.visuals().faint_bg_color);
        }

        let col = &self.columns[col_nr];

        egui::Frame::new()
            .inner_margin(Margin::symmetric(4, 0))
            .show(ui, |ui| {
                match col {
                    Column::Index => {
                        ui.label(format!("{row_nr}"));
                    }
                    Column::Text(ref v) => {
                        let v = v.format(row_nr).to_string();
                        ui.label(v);
                    }
                }
                
            });
    }
}