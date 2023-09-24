use std::{io::Read, error::Error};

use crate::entity::Entity;

pub struct Importer {
    name: &'static str,
    extensions: &'static [&'static str],
    import: fn (&mut dyn Read) -> Result<Entity, Box<dyn Error>>,
}

impl Importer {
    pub const fn name(&self) -> &'static str { self.name }
    pub const fn extensions(&self) -> &'static [&'static str] { self.extensions }

    pub fn matches_filename(&self, name: &str) -> bool {
        self.extensions.iter().any(|ext| name.ends_with(ext))
    }

    pub fn import(&self, r: &mut dyn Read) -> Result<Entity, Box<dyn Error>> {
        (self.import)(r)
    }
}

pub struct Importers<T>(pub T);

impl<T> Importers<T> where T: AsRef<[Importer]> {
    pub fn iter(&self) -> std::slice::Iter<Importer> {
        self.0.as_ref().iter()
    }

    pub fn by_name(&self, name: &str) -> Option<&Importer> {
        self.iter().find(|imp| imp.name() == name)
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

#[cfg(feature = "vcd")]
mod vcd;

#[cfg(feature = "vcd")]
pub const VCD: Importer = Importer {
    name: "vcd",
    extensions: &[".vcd"],
    import: vcd::import,
};

pub const IMPORTERS: Importers<&'static[Importer]> = Importers(&[
    #[cfg(feature = "vcd")] VCD,
]);
