use std::{fmt, str};
pub struct DisplayUtf8Lossy<'a> {
    input: &'a [u8],
    truncated: bool,
}

impl<'a> DisplayUtf8Lossy<'a> {
    pub fn new(input: &'a [u8]) -> DisplayUtf8Lossy<'a> {
        DisplayUtf8Lossy { input, truncated: false }
    }

    pub fn truncated(input: &'a [u8]) -> DisplayUtf8Lossy<'a> {
        DisplayUtf8Lossy { input, truncated: true }
    }
}

impl fmt::Display for DisplayUtf8Lossy<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut input = self.input;
        loop {
            match str::from_utf8(input) {
                Ok(valid) => {
                    return write!(f, "{}", valid);
                }
                Err(error) => {
                    let (valid, rest) = input.split_at(error.valid_up_to());
                    let valid = unsafe { str::from_utf8_unchecked(valid) };
                    write!(f, "{}", valid)?;

                    if let Some(invalid_sequence_length) = error.error_len() {
                        write!(f, "\u{FFFD}")?;
                        input = &rest[invalid_sequence_length..]
                    } else {
                        // Incomplete UTF-8 sequence at the end of input
                        if !self.truncated {
                            write!(f, "\u{FFFD}")?;
                        }
                        return Ok(());
                    }
                }
            }
        }
    }
}

#[test]
fn test_display_utf8_lossy() {
    assert_eq!(format!("{}", DisplayUtf8Lossy::new(b"abc")), "abc");
    assert_eq!(format!("{}", DisplayUtf8Lossy::new(b"abc\xFFdef")), "abc�def");
    assert_eq!(format!("{}", DisplayUtf8Lossy::new(b"abc\xC2")), "abc�");
    assert_eq!(format!("{}", DisplayUtf8Lossy::truncated(b"abc\xC2")), "abc");
}