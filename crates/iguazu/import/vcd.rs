use std::{error::Error, io::{BufReader, Read}, collections::HashMap};

use indexmap::IndexMap;
use vcd::{Parser, ScopeItem, IdCode, Value};

use crate::{in_memory::MemoryStreamWriter, entity::{Entity, Timestamp, SampleRate}};

pub fn import(read: &mut dyn Read) -> Result<Entity, Box<dyn Error>> {
    let mut parser = Parser::new(BufReader::new(read));
    let mut writers = HashMap::new();

    let header = parser.parse_header()?;

    let (timescale_amt, timescale_unit) = header.timescale.ok_or("no timescale specified")?;
    let clock = SampleRate::new(timescale_unit.divisor(), timescale_amt as u64);

    fn build_entity(
        clock: SampleRate,
        writers: &mut HashMap<IdCode, (Value, MemoryStreamWriter<u64>)>,
        items: &[ScopeItem]
    ) -> Entity {
        let children = IndexMap::from_iter(items.iter().flat_map(|item| {
            match item {
                ScopeItem::Scope(s) => {
                    Some((s.identifier.clone(), build_entity(clock, writers, &s.items)))
                },
                ScopeItem::Var(v) if v.size == 1 => {
                    let writer = MemoryStreamWriter::new();
                    
                    let field = Timestamp {
                        base_clock: clock,
                        color: None,
                        data: writer.stream().clone(),
                    };

                    writers.insert(v.code, (Value::V0, writer));

                    Some((v.reference.clone(), Entity::Timestamp(field)))
                },
                _ => None
            }
        }));
        Entity::Group(children)
    }

    let entity = build_entity(clock, &mut writers, &header.items);
    let mut ts = 0;
    while let Some(evt) = parser.next().transpose()? {
        match evt {
            vcd::Command::Timestamp(t) => ts = ts.max(t),
            vcd::Command::ChangeScalar(id, value) => {
                let Some((last_value, w)) = writers.get_mut(&id) else {
                    eprintln!("Change for unknown id {id} at line {l}", l = parser.line());
                    continue;
                };
                if *last_value != value && matches!(value, Value::V0 | Value::V1) {
                    w.push(ts);
                    *last_value = value;
                }
            },
            vcd::Command::ChangeVector(_, _) => {},
            vcd::Command::ChangeReal(_, _) => {},
            vcd::Command::ChangeString(_, _) => {},
            _ => {},
        }
    }

    Ok(entity)
}