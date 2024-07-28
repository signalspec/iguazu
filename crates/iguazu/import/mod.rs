use std::{error::Error, fs::File};

use crate::schema::{attribute::SampleRate, Entity};

pub struct Importer {
    pub name: &'static str,
    pub extensions: &'static [&'static str],
    pub import: fn (File) -> Result<Entity, Box<dyn Error>>,
}

impl Importer {
    pub fn matches_filename(&self, name: &str) -> bool {
        self.extensions.iter().any(|ext| name.ends_with(ext))
    }

    pub fn import(&self, f: File) -> Result<Entity, Box<dyn Error>> {
        (self.import)(f)
    }
}

pub struct Importers<T>(pub T);

impl<T> Importers<T> where T: AsRef<[Importer]> {
    pub fn iter(&self) -> std::slice::Iter<Importer> {
        self.0.as_ref().iter()
    }

    pub fn by_name(&self, name: &str) -> Option<&Importer> {
        self.iter().find(|imp| imp.name == name)
    }

    pub fn first_for_filename(&self, fname: &str) -> Option<&Importer> {
        self.iter().find(|imp| imp.matches_filename(fname))
    }
}

impl<'a, T> IntoIterator for &'a Importers<T> where T: AsRef<[Importer]> {
    type Item = &'a Importer;
    type IntoIter = std::slice::Iter<'a, Importer>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

pub const BIN: Importer = Importer {
    name: "bin",
    extensions: &[".bin"],
    import: |f| Ok(crate::storage::binary_file(f)?),
};

pub const LOGIC8: Importer = Importer {
    name: "8ch logic trace - raw binary",
    extensions: &[".logic8"],
    import: |f| Ok(crate::storage::logic8(f, SampleRate(200.0))?),
};

pub const IMPORTERS: Importers<&'static [Importer]> = Importers(&[
    BIN, LOGIC8
]);
