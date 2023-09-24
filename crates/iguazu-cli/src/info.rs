use std::{fs::File, io::Write, path::PathBuf};

use clap::Args;
use iguazu::entity::Entity;
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
    
    let mut file = File::open(&args.file).map_err(|e| format!("Failed to open {}: {}", args.file.display(), e))?;
    let entity = importer.import(&mut file).map_err(|e| format!("Failed to import {}: {}", args.file.display(), e))?;

    info_tree(&mut std::io::stdout().lock(), &filename, &entity);

    Ok(())
}

pub fn info_tree(w: &mut impl Write, root_name: &str, entity: &Entity) {
    info_tree_inner(w, "", root_name, entity).unwrap()
}

fn info_tree_inner(w: &mut impl Write, prefix: &str, name: &str, entity: &Entity) -> std::io::Result<()> {
    match entity {
        Entity::Group(e) => {
            header_line(w, name, "Group")?;
            children(w, prefix, e.iter())
        },
        Entity::Record(e) => {
            header_line(w, name, "Record")?;
            children(w, prefix, e.fields.iter())
        },
        Entity::Timestamp(_) => {
            header_line(w, name, "Timestamp")
        },
        Entity::Bits(_) => {
            header_line(w, name, "Bits")
        }
        Entity::Scalar(_) => {
            header_line(w, name, "Scalar")
        }
        Entity::Complex(_) => {
            header_line(w, name, "Complex")
        }
        Entity::Enum(_) => {
            header_line(w, name, "Enum")
        }
        Entity::Packet(e) => {
            header_line(w, name, "Packet")?;
            children(w, prefix, [("inner", &*e.inner)].into_iter())
        }
    }
}

fn children<'a>(w: &mut impl Write, prefix: &str, children: impl Iterator<Item=(impl AsRef<str>, &'a Entity)>) -> std::io::Result<()> {
    let mut children = children.peekable();
    while let Some((name, entity)) = children.next() {
        let more_remaining = children.peek().is_some();
        let child_prefix = if more_remaining {
            write!(w, "{prefix}├─")?;
            format!("{prefix}│ ")
        } else {
            write!(w, "{prefix}╰─")?;
            format!("{prefix}  ")
        };

        info_tree_inner(w, &child_prefix, name.as_ref(), entity)?;
    }
    Ok(())
}

fn header_line(w: &mut impl Write, name: &str, kind: &str) -> std::io::Result<()> {
    writeln!(w, "● {name} ({kind})", name = name.bold())
}
