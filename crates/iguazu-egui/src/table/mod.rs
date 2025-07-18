use std::collections::BTreeMap;

use ecow::EcoString;
use egui::Margin;
use iguazu::{schema::{Entity, EntityStream}, view::{TextView, ViewManager}};
use itertools::Itertools;

use crate::ViewerContext;

pub struct TableView {
}

impl TableView {
    pub fn new() -> Self {
        Self {}
    }
    
    pub fn show(
        &mut self,
        vcx: &mut ViewerContext,
        ui: &mut egui::Ui,
        entity: &mut EntityStream,
    ) {
        let mut delegate = Delegate::new(entity, &vcx.view_manager);
        let table = delegate.table();
        table.show(ui, &mut delegate);
    }
}

enum Column<'a> {
    Index,
    Text(TextView<'a>),
    //String(View, View),
    //Tiles
}

struct Delegate<'a> {
    columns: Vec<Column<'a>>,
    headers: BTreeMap<(usize, usize, usize), EcoString>,
    n_rows: u64,
}

impl<'a> Delegate<'a> {
    fn new(entity: &mut EntityStream, view_manager: &'a ViewManager) -> Self {
        let mut columns = Vec::new();
        let mut headers = BTreeMap::new();

        columns.push(Column::Index);

        let mut n_rows = 0;
    
        fn inner<'a>(
            vm: &'a ViewManager,
            depth: usize,
            data: &mut Vec<Column<'a>>,
            headers: &mut BTreeMap<(usize, usize, usize), EcoString>,
            n_rows: &mut u64,
            entity: &EntityStream,
        ) {
            match entity {
                Entity::Group { .. } => {}
                Entity::Record { children, .. } => {
                    for (name, child) in children {
                        let start = data.len();
                        inner(vm, depth + 1, data, headers, n_rows, child);
                        let end = data.len();
                        headers.insert((depth, start, end), name.clone());
                    }
                }
                _ => {
                    let view = vm.text_view(entity);
                    *n_rows = (*n_rows).max(view.state().end);
                    data.push(Column::Text(view))
                }
            }
        }
    
        inner(view_manager, 0, &mut columns, &mut headers, &mut n_rows, entity);
    
        Self { columns, headers, n_rows }
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
            .num_rows(self.n_rows)
            .columns(columns)
            .headers(headers)
            .num_sticky_cols(1)
    }
}

impl<'a> egui_table::TableDelegate for Delegate<'a> {
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
                ui.heading(label.to_string());
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
                    Column::Text(v) => {
                        let v = v.format(row_nr).to_string();
                        ui.label(v);
                    }
                }
                
            });
    }
}