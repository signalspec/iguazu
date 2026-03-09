use std::io::Write;

use clap::Args;
use iguazu::{cli::ImportOpts, import::IMPORTERS, schema::{Field, FieldKind, Entity}};
use owo_colors::OwoColorize;
use futures_lite::future::block_on;

#[derive(Args)]
#[command(about = "Describe the entities in the file")]
pub struct Cli {
    #[clap(flatten)]
    import: ImportOpts,
}

pub fn main(args: &Cli) -> Result<(), String> {
    block_on(async {
        let filename = args.import.filename.file_name().and_then(|s| s.to_str()).unwrap_or("unknown");
        let last_entity_path = args.import.entity.as_ref().and_then(|e| e.rsplit('.').next());
        let root_name = last_entity_path.unwrap_or(filename);

        let schema = args.import.schema_or_inferred(IMPORTERS).await?;

        info_tree(&mut std::io::stdout().lock(), root_name, &schema);
        Ok(())
    })
}

pub fn info_tree<D, L>(w: &mut impl Write, root_name: &str, entity: &Entity<D, L>) {
    info_tree_inner(w, "", root_name, entity).unwrap()
}

fn info_tree_field(w: &mut impl Write, top: bool, prefix: &str, name: &str, field: &Field) -> std::io::Result<()> {
    match field.kind {
        FieldKind::BitStruct { ref children } => {
            header_line(w, top, name, "Bit Struct")?;
            print_children(w, prefix, children.iter().map(|(name, f)| (name.as_str(), f)), |w, prefix, name, field| {
                info_tree_field(w, false, prefix, name, field)
            })
        }
        FieldKind::Bits { .. } => header_line(w, top, name, "Bits"),
        FieldKind::Null => header_line(w, top, name, "Null"),
        FieldKind::Character => header_line(w, top, name, "Char"),
        FieldKind::Timestamp => header_line(w, top, name, "Timestamp"),
        FieldKind::Int { .. } => header_line(w, top, name, "Int"),
        FieldKind::Signed { .. } => header_line(w, top, name, "Signed Int"),
        FieldKind::Float32 => header_line(w, top, name, "Float32"),
        FieldKind::Float64 => header_line(w, top, name, "Float64"),
        FieldKind::Enum { .. } => header_line(w, top, name, "Enum"),
        FieldKind::Tagged { ref values, .. } => {
            header_line(w, top, name, "Tagged")?;
            print_children(w, prefix, values.iter().map(|(name, f)| (name.as_str(), f)), |w, prefix, name, field| {
                info_tree_field(w, false, prefix, name, field)
            })
        }
    }
}

fn info_tree_inner<D, S>(w: &mut impl Write, prefix: &str, name: &str, entity: &Entity<D, S>) -> std::io::Result<()> {
    match entity {
        Entity::Group { children, .. } => {
            header_line(w, true, name, "Group")?;
            print_children(w, prefix, children.iter(), info_tree_inner)?;
        }
        Entity::Record { children, .. }=> {
            header_line(w, true, name, "Record")?;
            print_children(w, prefix, children.iter(), info_tree_inner)?;
        }
        Entity::Data { field, .. } => {
            info_tree_field(w, true, prefix, name, field)?;
        }
        Entity::Union { .. } => {
            header_line(w, true, name, "Union")?;
        }
        Entity::FixedArray { child, .. } => {
            header_line(w, true, name, "FixedArray")?;
            print_children(w, prefix, [("inner", &**child)].into_iter(), info_tree_inner)?;
        }
        Entity::Tuple { child, .. } => {
            header_line(w, true, name, "Tuple")?;
            print_children(w, prefix, [("inner", &**child)].into_iter(), info_tree_inner)?;
        }
        Entity::VariableArray { child, .. } => {
            header_line(w, true, name, "VariableArray")?;
            print_children(w, prefix, [("inner", &**child)].into_iter(), info_tree_inner)?;
        }
    }
    Ok(())
}

fn print_children<T, W: Write>(
    w: &mut W,
    prefix: &str,
    children: impl Iterator<Item=(impl AsRef<str>, T)>,
    mut f: impl FnMut(&mut W, &str, &str, T) -> std::io::Result<()>,
) -> std::io::Result<()> {
    let mut children = children.peekable();
    while let Some((name, entity)) = children.next() {
        let more_remaining = children.peek().is_some();
        let child_prefix = if more_remaining {
            write!(w, "{prefix}├─")?;
            format!("{prefix}│ ")
        } else {
            write!(w, "{prefix}└─")?;
            format!("{prefix}  ")
        };

        f(w, &child_prefix, name.as_ref(), entity)?;
    }
    Ok(())
}

fn header_line(w: &mut impl Write, filled: bool, name: &str, kind: &str) -> std::io::Result<()> {
    let icon = if filled { "●" } else { "○" };
    writeln!(w, "{icon} {name} ({kind})", name = name.bold())
}
