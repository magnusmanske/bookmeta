use crate::BookError;
use isbn::{Isbn10, Isbn13};
use std::fmt;

/// An ISBN-10 or ISBN-13, validated and normalised.
///
/// The original formatting is kept for display, while queries use the
/// digits-only ISBN-13 (and ISBN-10, when it exists).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Isbn {
    isbn13: Isbn13,
    isbn10: Option<Isbn10>,
    display: String,
}

impl Isbn {
    /// Parse an ISBN from e.g. `978-3-423-30666-9` or `ISBN 3-446-23903-0`.
    ///
    /// Digits and `X` are kept; everything else is ignored. The check digit
    /// is validated by the `isbn` crate.
    pub fn parse(input: &str) -> Result<Self, BookError> {
        let digits = digits(input);
        if digits.is_empty() {
            return Err(BookError::InvalidIsbn);
        }
        let parsed: isbn::Isbn = digits.parse()?;
        let (isbn13, isbn10) = match parsed {
            isbn::Isbn::_10(isbn10) => (Isbn13::from(isbn10), Some(isbn10)),
            isbn::Isbn::_13(isbn13) => (isbn13, isbn10_from_13(&isbn13)),
        };
        Ok(Self { isbn13, isbn10, display: display_input(input) })
    }

    pub fn isbn13(&self) -> Isbn13 {
        self.isbn13
    }

    pub fn isbn10(&self) -> Option<Isbn10> {
        self.isbn10
    }

    /// The 13 digits, as used for source queries.
    pub fn isbn13_digits(&self) -> String {
        self.isbn13.to_string()
    }

    /// The 10 digits (including a check `X`), when this ISBN has an ISBN-10.
    pub fn isbn10_digits(&self) -> Option<String> {
        self.isbn10.map(|isbn| isbn.to_string())
    }

    /// The hyphenated ISBN-13, or the digits if the ranges are unknown.
    pub fn isbn13_hyphenated(&self) -> String {
        self.isbn13.hyphenate().map(|s| s.to_string()).unwrap_or_else(|_| self.isbn13_digits())
    }

    /// The hyphenated ISBN-10, when present.
    pub fn isbn10_hyphenated(&self) -> Option<String> {
        self.isbn10.and_then(|isbn| isbn.hyphenate().ok()).map(|s| s.to_string())
    }

    /// The ISBN as the user wrote it, with only digits, `X` and hyphens.
    pub fn display(&self) -> &str {
        &self.display
    }

    /// A stable key for caches: the 13 digits.
    pub fn key(&self) -> String {
        self.isbn13_digits()
    }

    /// Whether `value` (possibly hyphenated, or prefixed by `ISBN:`) is this ISBN.
    pub fn matches(&self, value: &str) -> bool {
        let value = digits(value);
        value == self.isbn13_digits() || self.isbn10_digits().is_some_and(|isbn| isbn == value)
    }
}

impl fmt::Display for Isbn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.display)
    }
}

fn digits(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_ascii_digit() || c.eq_ignore_ascii_case(&'X'))
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

fn display_input(input: &str) -> String {
    input
        .chars()
        .filter(|c| c.is_ascii_digit() || c.eq_ignore_ascii_case(&'X') || *c == '-')
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

/// ISBN-10 for a 978-prefixed ISBN-13; `None` for other prefixes.
fn isbn10_from_13(isbn13: &Isbn13) -> Option<Isbn10> {
    let value = isbn13.to_string();
    let body = value.strip_prefix("978")?.get(..9)?;
    let sum: u32 =
        body.chars().enumerate().map(|(index, digit)| (10 - index as u32) * digit.to_digit(10).unwrap_or(0)).sum();
    let check = (11 - (sum % 11)) % 11;
    let check = if check == 10 { "X".to_string() } else { check.to_string() };
    format!("{body}{check}").parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_hyphenated_and_prefixed_isbns() {
        let isbn = Isbn::parse("ISBN 978-3-423-30666-9").unwrap();
        assert_eq!(isbn.isbn13_digits(), "9783423306669");
        assert_eq!(isbn.isbn13_hyphenated(), "978-3-423-30666-9");
        assert_eq!(isbn.display(), "978-3-423-30666-9");
    }

    #[test]
    fn converts_isbn10_to_isbn13() {
        let isbn = Isbn::parse("3-423-30666-1").unwrap();
        assert_eq!(isbn.isbn13_digits(), "9783423306669");
        assert_eq!(isbn.isbn10_digits().as_deref(), Some("3423306661"));
    }

    #[test]
    fn converts_isbn13_to_isbn10() {
        let isbn = Isbn::parse("9780140328721").unwrap();
        assert_eq!(isbn.isbn10_digits().as_deref(), Some("0140328726"));
    }

    #[test]
    fn handles_an_isbn10_check_x() {
        let isbn = Isbn::parse("080442957X").unwrap();
        assert_eq!(isbn.isbn10_digits().as_deref(), Some("080442957X"));
        assert_eq!(isbn.isbn13_digits(), "9780804429573");
    }

    #[test]
    fn rejects_a_bad_check_digit() {
        assert!(Isbn::parse("9780140328722").is_err());
    }

    #[test]
    fn rejects_nothing() {
        assert!(Isbn::parse("not an isbn").is_err());
        assert!(Isbn::parse("").is_err());
    }

    #[test]
    fn matches_both_forms() {
        let isbn = Isbn::parse("9780140328721").unwrap();
        assert!(isbn.matches("0140328726"));
        assert!(isbn.matches("ISBN:978-0-14-032872-1"));
        assert!(!isbn.matches("9780140328722"));
    }
}
