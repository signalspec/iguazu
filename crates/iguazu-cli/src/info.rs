use std::{io::Write, path::PathBuf, sync::Arc};

use clap::Args;
use iguazu::{io::{FsFile, ReadableFile}, schema::{Entity, EntityKind, EntitySchema, EntityStream}};
use owo_colors::OwoColorize;
use futures_lite::future::block_on;

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
    let file = Arc::new(FsFile::new(args.file.clone()));
    let filename = file.filename().unwrap_or("unknown").to_owned();
    let importer = if let Some(format) = &args.import_format {
        iguazu::import::IMPORTERS.by_name(format).ok_or_else(|| format!("No importer named `{}`", format))?
    } else {
        iguazu::import::IMPORTERS.first_for_filename(&filename).ok_or_else(|| format!("No importer matched filename `{}`", filename))?
    };
    
    let mut importer = importer.import(file);
    let schema = block_on(importer.load_schema()).map_err(|e| format!("Failed to import {}: {}", args.file.display(), e))?;

    info_tree(&mut std::io::stdout().lock(), &filename, &schema);

    Ok(())
}

pub fn info_tree<D>(w: &mut impl Write, root_name: &str, entity: &Entity<D>) {
    info_tree_inner(w, "", root_name, entity).unwrap()
}

fn info_tree_inner<D>(w: &mut impl Write, prefix: &str, name: &str, entity: &Entity<D>) -> std::io::Result<()> {
    match &entity.kind {
        EntityKind::Group => {
            header_line(w, name, "Group")?;
        }
        EntityKind::Record => {
            header_line(w, name, "Record")?;
        }
        EntityKind::Bits { .. } => {
            header_line(w, name, "Bits")?;
        }
        EntityKind::Logic { .. } => {
            header_line(w, name, "Logic")?;
        }
        EntityKind::Unsigned { .. } => {
            header_line(w, name, "Unsigned Int")?;
        }
        EntityKind::Signed { .. } => {
            header_line(w, name, "Signed Int")?;
        }
        EntityKind::Float { .. } => {
            header_line(w, name, "Float")?;
        }
        EntityKind::Timestamp { .. } => {
            header_line(w, name, "Timestamp")?;
        }
        EntityKind::Enum { .. } => {
            header_line(w, name, "Enum")?;
        }
        EntityKind::FixedArray { .. } => {
            header_line(w, name, "FixedArray")?;
        }
        EntityKind::Tuple { .. } => {
            header_line(w, name, "Tuple")?;
        }
        EntityKind::VariableArray { .. } => {
            header_line(w, name, "VariableArray")?;
        }
    }
    print_children(w, prefix, entity.children.iter(), info_tree_inner)
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
