/// An option for [`Configurable`].
pub struct OptionDescription {
    /// The name of the option passed to `set` and `get`.
    pub name: &'static str,

    /// A human-readable description of the option.
    pub description: &'static str,
}

pub trait Configurable {
    /// List available options.
    fn options(&self) -> &'static [ OptionDescription ] {
        &[]
    }

    /// Set an option.
    ///
    /// The option keys and allowed values depend on the type.
    fn set(&mut self, option: &str, value: &str) -> Result<(), String> {
        let _ = (option, value);
        Err(format!("Unknown option"))
    }

    /// Get the current value of an option.
    fn get(&self, option: &str) -> Option<String> {
        let _ = option;
        None
    }
}

impl<T: Configurable + ?Sized> Configurable for Box<T> {
    fn options(&self) -> &'static [OptionDescription] {
        self.as_ref().options()
    }

    fn set(&mut self, option: &str, value: &str) -> Result<(), String> {
        self.as_mut().set(option, value)
    }

    fn get(&self, option: &str) -> Option<String> {
        self.as_ref().get(option)
    }
}

pub fn parse_char_byte(value: &str) -> Result<u8, String> {
    match value {
        "\\t" => Ok(b'\t'),
        "\\n" => Ok(b'\n'),
        "\\r" => Ok(b'\r'),
        s if s.starts_with("\\x") && s.len() == 4 => {
            u8::from_str_radix(&s[2..], 16).map_err(|_| format!("Invalid hex byte value"))
        },
        s if s.len() == 1 => Ok(s.as_bytes()[0]),
        _ => Err("Invalid single-byte value".into()),
    }
}

pub fn parse_optional<T>(f: impl FnOnce(&str) -> Result<T, String>, value: &str) -> Result<Option<T>, String> {
    match value {
        "" | "no" | "off" | "none" => Ok(None),
        _ => f(value).map(Some),
    }
}

pub fn parse_bool(value: &str) -> Result<bool, String> {
    match value {
        "true" | "yes" | "y" | "on" | "1" => Ok(true),
        "false" | "no" | "n" | "off" | "0" => Ok(false),
        _ => Err("Invalid boolean value".into()),
    }
}
