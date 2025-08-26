use std::{ffi::OsStr, ops::Deref, path::Path};
use serde::{de::{Error as _, Unexpected}, Deserialize, Deserializer, Serialize, Serializer};
use url::Url;

/// A platform-independent relative path that does not contain a `..`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RelativePath(String);

impl RelativePath {
    pub fn from_base(base: &Url, rel: &Url) -> Option<RelativePath> {
        if base.cannot_be_a_base() || base.scheme() != rel.scheme() || base.host_str() != rel.host_str() || base.port() != rel.port() {
            dbg!("Cannot make relative path: base={} rel={}", base, rel);
            return None;
        }

        let last_slash = base.path().rfind('/')?;
        let base_path = &base.path()[..last_slash+1];
        let rel_path = rel.path().strip_prefix(base_path)?;

        rel_path.to_owned().try_into().ok()
    }
}

impl Deref for RelativePath {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<OsStr> for RelativePath {
    fn as_ref(&self) -> &OsStr {
        self.0.as_ref()
    }
}

impl AsRef<Path> for RelativePath {
    fn as_ref(&self) -> &Path {
        self.0.as_ref()
    }
}

pub struct BadRelativePath(pub String);

impl TryFrom<String> for RelativePath {
    type Error = BadRelativePath;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if !value.is_empty() && !value.contains('\\') && !value.contains(":") && !value.contains('\0') && value.split('/').all(|c| c != "" && c != "..") {
            Ok(RelativePath(value))
        } else {
            Err(BadRelativePath(value))
        }
    }
}

impl<'a> Deserialize<'a> for RelativePath {
    fn deserialize<D: Deserializer<'a>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?.try_into().map_err(|BadRelativePath(s)| {
            D::Error::invalid_value(Unexpected::Str(&s), &"relative path")
        })
    }
}

impl Serialize for RelativePath {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

#[test]
fn test_validate() {
    assert!(RelativePath::try_from("foo.txt".to_owned()).is_ok());
    assert!(RelativePath::try_from("a/b".to_owned()).is_ok());

    assert!(RelativePath::try_from("../path".to_owned()).is_err());
    assert!(RelativePath::try_from("invalid/../path".to_owned()).is_err());
    assert!(RelativePath::try_from("/etc/passwd".to_owned()).is_err());
    assert!(RelativePath::try_from("C:\\foo".to_owned()).is_err());
}

#[test]
fn test_relative_path() {
    assert_eq!(&*RelativePath::from_base(
        &Url::parse("file:///tmp/foo.json").unwrap(),
        &Url::parse("file:///tmp/bar.f32").unwrap()
    ).unwrap(), "bar.f32");

    assert_eq!(&*RelativePath::from_base(
        &Url::parse("https://example.com/base/").unwrap(),
        &Url::parse("https://example.com/base/relative/path").unwrap()
    ).unwrap(), "relative/path");

    assert!(RelativePath::from_base(
        &Url::parse("file:///tmp/foo.json").unwrap(),
        &Url::parse("file:///etc/passwd").unwrap()
    ).is_none());
}
