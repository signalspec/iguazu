use std::io::Write;

use clap::Args;
use iguazu::{cli::ImportOpts, import::IMPORTERS, schema::{BitField, BitLayout, Entity, EntityKind}};
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
        let mut importer = args.import.importer(IMPORTERS).await?;

        let schema = if let Some(schema) = args.import.schema().await? {
            schema
        } else {
            importer.load_schema().await.map_err(|e| e.to_string())?
        };

        info_tree(&mut std::io::stdout().lock(), &filename, &schema);
        Ok(())
    })
}

pub fn info_tree<D>(w: &mut impl Write, root_name: &str, entity: &Entity<D>) {
    info_tree_inner(w, "", root_name, entity).unwrap()
}

fn info_tree_bits(w: &mut impl Write, prefix: &str, name: &str, field: &BitField) -> std::io::Result<()> {
    let width = field.bits.width();
    let kind = if width == 1 { "1 bit".into() } else { format!("{} bits", width) };
    match field.bits {
        BitLayout::Fields(ref fields) => {
            header_line(w, false, name, &kind)?;
            print_children(w, prefix, fields.iter().map(|f| (f.name.as_str(), f)), info_tree_bits)
        }
        BitLayout::Bits(_) => {
            header_line(w, false, name, &kind)
        }
    }
}

fn info_tree_inner<D>(w: &mut impl Write, prefix: &str, name: &str, entity: &Entity<D>) -> std::io::Result<()> {
    match &entity.kind {
        EntityKind::Group { children } => {
            header_line(w, true, name, "Group")?;
            print_children(w, prefix, children.iter(), info_tree_inner)?;
        }
        EntityKind::Record { children }=> {
            header_line(w, true, name, "Record")?;
            print_children(w, prefix, children.iter(), info_tree_inner)?;
        }
        EntityKind::Bits { bits, .. } => {
            header_line(w, true, name, "Bits")?;
            if let BitLayout::Fields(fields) = bits {
                print_children(w, prefix, fields.iter().map(|f| (f.name.as_str(), f)), info_tree_bits)?;
            }
        }
        EntityKind::Character { .. } => {
            header_line(w, true, name, "Character")?;
        }
        EntityKind::Number { .. } => {
            header_line(w, true, name, "Number")?;
        }
        EntityKind::Timestamp { .. } => {
            header_line(w, true, name, "Timestamp")?;
        }
        EntityKind::Enum { .. } => {
            header_line(w, true, name, "Enum")?;
        }
        EntityKind::FixedArray { child, .. } => {
            header_line(w, true, name, "FixedArray")?;
            print_children(w, prefix, [("inner", &**child)].into_iter(), info_tree_inner)?;
        }
        EntityKind::Tuple { child, .. } => {
            header_line(w, true, name, "Tuple")?;
            print_children(w, prefix, [("inner", &**child)].into_iter(), info_tree_inner)?;
        }
        EntityKind::VariableArray { child, .. } => {
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
