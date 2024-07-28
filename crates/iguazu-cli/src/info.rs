use std::{fs::File, io::Write, path::PathBuf};

use clap::Args;
use iguazu::schema::{Entity, EntityKind, Field, NestedField};
use owo_colors::OwoColorize;

#[derive(Args)]
#[command(about = "Describe the entities in the file")]
pub struct Cli {
    /// Filename
    file: PathBuf,

    /// Override format for import (inferred by extension by default)
    #[arg(short='I', long)]
    import_format: Option<String>,
}

pub fn main(args: &Cli) -> Result<(), String> {
    let filename = args.file.file_name().map_or("".to_string(), |n| n.to_string_lossy().to_string());
    let importer = if let Some(format) = &args.import_format {
        iguazu::import::IMPORTERS.by_name(format).ok_or_else(|| format!("No importer named `{}`", format))?
    } else {
        iguazu::import::IMPORTERS.first_for_filename(&filename).ok_or_else(|| format!("No importer matched filename `{}`", filename))?
    };
    
    let file = File::open(&args.file).map_err(|e| format!("Failed to open {}: {}", args.file.display(), e))?;
    let entity = importer.import(file).map_err(|e| format!("Failed to import {}: {}", args.file.display(), e))?;

    info_tree(&mut std::io::stdout().lock(), &filename, &entity);

    Ok(())
}

pub fn info_tree(w: &mut impl Write, root_name: &str, entity: &Entity) {
    info_tree_inner(w, "", root_name, entity).unwrap()
}

fn info_tree_inner(w: &mut impl Write, prefix: &str, name: &str, entity: &Entity) -> std::io::Result<()> {
    match &entity {
        EntityKind::Group { children, .. } => {
            header_line(w, name, "Group")?;
            print_children(w, prefix, children.iter(), info_tree_inner)
        },
        EntityKind::Data { encoding, .. } => {
            info_tree_field(w, prefix, name, encoding)
        }
    }
}

fn info_tree_field(w: &mut impl Write, prefix: &str, name: &str, field: &NestedField) -> std::io::Result<()> {
    match &field.kind {
        Field::Null => {
            header_line(w, name, "Null")
        },
        Field::Bits { .. } => {
            header_line(w, name, "Bits")
        }
        Field::Unsigned { .. } => {
            header_line(w, name, "Unsigned")
        }
        Field::Signed { .. } => {
            header_line(w, name, "Signed")
        }
        Field::Timestamp { .. } => {
            header_line(w, name, "Timestamp")
        }
        Field::Float32 => {
            header_line(w, name, "Float32")
        }
        Field::Tagged { values, .. } => {
            header_line(w, name, "Tagged")?;
            print_children(w, prefix, values.iter(), info_tree_field)
        }
        Field::Struct { children } => {
            header_line(w, name, "Struct")?;
            print_children(w, prefix, children.iter(), info_tree_field)
        },
    }
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

fn header_line(w: &mut impl Write, name: &str, kind: &str) -> std::io::Result<()> {
    writeln!(w, "● {name} ({kind})", name = name.bold())
}
