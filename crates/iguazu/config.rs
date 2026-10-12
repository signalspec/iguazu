use std::borrow::Cow;

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

    /// Get suggested values for an option.
    fn get_suggestions(&self, _option: &str) -> Cow<'_, [Cow<'_, str>]> {
        (&[]).into()
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

    fn get_suggestions(&self, option: &str) -> Cow<'_, [Cow<'_, str>]> {
        self.as_ref().get_suggestions(option)
    }
}

/// Build a [`Cow`] slice of [`Cow`] strings, for [`Configurable::get_suggestions`].
/// ```
/// # use iguazu::cow_strs;
/// let _ = cow_strs!["a", "b"];
/// ```
#[macro_export]
macro_rules! cow_strs {
    [$($s:expr),* $(,)?] => {
        ::std::borrow::Cow::Borrowed(&[$(::std::borrow::Cow::Borrowed($s)),*])
    };
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
        "" | "no" | "off" | "none" | "default" => Ok(None),
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

pub fn parse_rate(value: &str) -> Result<f64, String> {
    let (num, scale) = if let Some(num) = value.strip_suffix("GHz") {
        (num, 1e9)
    } else if let Some(num) = value.strip_suffix("G") {
        (num, 1e9)
    } else if let Some(num) = value.strip_suffix("MHz") {
        (num, 1e6)
    } else if let Some(num) = value.strip_suffix("M") {
        (num, 1e6)
    } else if let Some(num) = value.strip_suffix("kHz") {
        (num, 1e3)
    } else if let Some(num) = value.strip_suffix("k") {
        (num, 1e3)
    } else if let Some(num) = value.strip_suffix("Hz") {
        (num, 1.0)
    } else {
        (value, 1.0)
    };

    let num = num.trim().parse::<f64>().map_err(|_| "Invalid frequency value".to_owned())?;
    Ok(num * scale)
}

pub fn format_rate(value: f64) -> String {
    if value >= 1e9 {
        format!("{} GHz", value / 1e9)
    } else if value >= 1e6 {
        format!("{} MHz", value / 1e6)
    } else if value >= 1e3 {
        format!("{} kHz", value / 1e3)
    } else {
        format!("{} Hz", value)
    }
}

pub fn parse_time(value: &str) -> Result<f64, String> {
    let (num, scale) = if let Some(num) = value.strip_suffix("min") {
        (num, 60.0)
    } else if let Some(num) = value.strip_suffix("ms") {
        (num, 1e-3)
    } else if let Some(num) = value.strip_suffix("s") {
        (num, 1.0)
    } else {
        (value, 1.0)
    };

    let num = num.trim().parse::<f64>().map_err(|_| "Invalid time value".to_owned())?;
    Ok(num * scale)
}

pub fn format_time(value: f64) -> String {
    if value >= 60.0 && value % 60.0 == 0.0 {
        format!("{}min", value / 60.0)
    } else if value >= 1.0 || value == 0.0 {
        format!("{}s", value)
    } else {
        format!("{}ms", value * 1e3)
    }
}
