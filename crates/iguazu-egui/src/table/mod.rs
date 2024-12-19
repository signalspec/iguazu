use std::{collections::BTreeMap, ops::Range};

use egui::Margin;
use iguazu::{schema::{Entity, EntityStream}, view::{NumberView, TextView, View, ViewManager}, IdxRange};
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
        let view_manager = ViewCache::with(ui);
        let mut delegate = Delegate::new(entity, view_manager);
        let table = delegate.table();
        table.show(ui, &mut delegate);
    }
}

enum ColumnEntity<'e> {
    Index,
    Entity(&'e EntityStream),
    //String(View, View),
    //Tiles
}

enum ColumnData{
    Index,
    Text(TextView)
}

struct Delegate<'e> {
    view_manager: ViewCache,
    columns: Vec<ColumnEntity<'e>>,
    column_data: Vec<ColumnData>,
    headers: BTreeMap<(usize, usize, usize), String>
}

impl<'e> Delegate<'e> {
    fn new(entity: &'e mut EntityStream, view_manager: ViewCache) -> Self {
        let mut columns = Vec::new();
        let mut headers = BTreeMap::new();

        columns.push(ColumnEntity::Index);
    
        fn inner<'e>(
            depth: usize,
            data: &mut Vec<ColumnEntity<'e>>,
            headers: &mut BTreeMap<(usize, usize, usize), String>,
            entity: &'e EntityStream,
        ) {
            match entity.kind {
                iguazu::schema::EntityKind::Group => {}
                iguazu::schema::EntityKind::Record => {
                    for (name, child) in &entity.children {
                        let start = data.len();
                        inner(depth + 1, data, headers, child);
                        let end = data.len();
                        headers.insert((depth, start, end), name.clone());
                    }
                }
                _ => {
                    data.push(ColumnEntity::Entity(entity))
                }
            }
        }
    
        inner(0, &mut columns, &mut headers, entity);
    
        Self { columns, headers, view_manager, column_data: Vec::new() }
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

impl egui_table::TableDelegate for Delegate<'_> {
    fn header_cell_ui(&mut self, ui: &mut egui::Ui, cell: &egui_table::HeaderCellInfo) {
        let egui_table::HeaderCellInfo {
            col_range,
            row_nr,
            ..
        } = cell;

        let label = self.headers.get(&(*row_nr, col_range.start, col_range.end)).unwrap();

        egui::Frame::none()
            .inner_margin(Margin::symmetric(4.0, 0.0))
            .show(ui, |ui| {
                ui.heading(label.to_owned());
            });
    }

    fn prepare(&mut self, info: &egui_table::PrefetchInfo) {
        let egui_table::PrefetchInfo {
            visible_rows,
            ..
        } = info;

        let range = IdxRange { min: visible_rows.start, max: visible_rows.end };

        let column_data = self.columns.iter().map(|c| {
            match c {
                ColumnEntity::Index => ColumnData::Index,
                ColumnEntity::Entity(e) => {
                    ColumnData::Text(self.view_manager.text_view(e, range))
                }
            }

        }).collect();

        self.column_data = column_data;
    }

    fn cell_ui(&mut self, ui: &mut egui::Ui, cell: &egui_table::CellInfo) {
        let egui_table::CellInfo { row_nr, col_nr, .. } = *cell;

        if row_nr % 2 == 1 {
            ui.painter()
                .rect_filled(ui.max_rect(), 0.0, ui.visuals().faint_bg_color);
        }

        let col = &self.column_data[col_nr];

        egui::Frame::none()
            .inner_margin(Margin::symmetric(4.0, 0.0))
            .show(ui, |ui| {
                match col {
                    ColumnData::Index => {
                        ui.label(format!("{row_nr}"));
                    }
                    ColumnData::Text(ref v) => {
                        let v = v.format(row_nr).to_string();
                        ui.label(v);
                    }
                }
                
            });
    }
}