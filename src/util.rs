//! Small text helpers shared by the source parsers.

use unicode_normalization::UnicodeNormalization;

/// Trim whitespace and the punctuation catalogues put between fields, and
/// normalize the Unicode composition (MARC data often has combining marks).
pub(crate) fn clean(text: &str) -> String {
    let text: String = text.nfc().collect();
    text.trim().trim_end_matches([':', ',', ';', '/', ' ']).trim().to_string()
}

/// The first four-digit year in `text`, or an empty string.
pub(crate) fn year_from(text: &str) -> String {
    let mut run = String::new();
    for character in text.chars() {
        if character.is_ascii_digit() {
            run.push(character);
        } else {
            if run.len() == 4 {
                return run;
            }
            run.clear();
        }
    }
    if run.len() == 4 { run } else { String::new() }
}

/// A page count from e.g. `452 S.`, `452 pages`, or `1 vol. (113 p.)`.
pub(crate) fn pages_from(text: &str) -> Option<u32> {
    let lower = text.to_lowercase();
    let bytes = lower.as_bytes();
    let mut first = None;
    let mut index = 0;
    while index < bytes.len() {
        if !bytes[index].is_ascii_digit() {
            index += 1;
            continue;
        }
        let start = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        let page_count = lower.get(start..index)?.parse::<u32>().ok()?;
        let rest = lower.get(index..).unwrap_or_default().trim_start();
        if rest.starts_with('p') || rest.starts_with("page") || rest.starts_with("seite") || rest.starts_with("s.") {
            return Some(page_count);
        }
        first.get_or_insert(page_count);
    }
    first
}

/// `Kater, Michael H. [Verfasser]` or `Saint-Exupéry, Antoine de (1900-1944). Auteur du texte`
/// as `Michael H. Kater` / `Antoine de Saint-Exupéry`.
pub(crate) fn natural_name(name: &str) -> String {
    let mut name = name.trim().to_string();
    remove_delimited(&mut name, '[', ']');
    remove_delimited(&mut name, '(', ')');
    if let Some((left, right)) = name.split_once(". ")
        && is_role(right)
    {
        name = left.to_string();
    }
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    let parts: Vec<&str> = name.split(',').map(str::trim).filter(|part| !part.is_empty()).collect();
    if parts.len() >= 2 {
        parts.iter().rev().copied().collect::<Vec<_>>().join(" ")
    } else {
        name.trim_end_matches('.').trim().to_string()
    }
}

fn remove_delimited(text: &mut String, open: char, close: char) {
    while let (Some(start), Some(end)) = (text.find(open), text.find(close)) {
        if end <= start {
            break;
        }
        text.replace_range(start..=end, " ");
    }
}

fn is_role(text: &str) -> bool {
    let text = text.to_lowercase();
    [
        "auteur",
        "verfasser",
        "autor",
        "author",
        "hrsg",
        "herausgeber",
        "translator",
        "übersetzer",
        "editor",
        "compiler",
        "ed.",
    ]
    .iter()
    .any(|role| text.starts_with(role))
}

/// Split a catalogue title such as `Title : Subtitle / Author` into title and
/// subtitle, dropping the statement of responsibility.
pub(crate) fn split_title(raw: &str) -> (String, Option<String>) {
    let raw = raw.split(" / ").next().unwrap_or(raw).trim();
    match raw.split_once(" : ") {
        Some((title, subtitle)) => {
            let title = clean(title);
            let subtitle = clean(subtitle);
            if subtitle.is_empty() { (title, None) } else { (title, Some(subtitle)) }
        }
        None => (clean(raw), None),
    }
}

/// Split `München ; Wien : Verlag` or `Gallimard (Paris)` into publisher and places.
pub(crate) fn split_publisher(raw: &str) -> (String, Vec<String>) {
    let raw = raw.trim();
    if let Some((places, publisher)) = raw.split_once(" : ") {
        let places: Vec<String> = places.split(';').map(clean).filter(|place| !place.is_empty()).collect();
        return (clean(publisher), places);
    }
    if let Some((publisher, place)) = raw.rsplit_once(" (")
        && let Some(place) = place.strip_suffix(')')
    {
        return (clean(publisher), vec![clean(place)].into_iter().filter(|place| !place.is_empty()).collect());
    }
    (clean(raw), vec![])
}

/// The Wikidata language code for a MARC or ISO 639 code, when it is a common one.
pub(crate) fn language_code(code: &str) -> Option<String> {
    let code = code.trim().to_lowercase();
    let mapped = match code.as_str() {
        "ara" | "ar" => "ar",
        "cat" | "ca" => "ca",
        "chi" | "zho" | "zh" => "zh",
        "cze" | "ces" | "cs" => "cs",
        "dan" | "da" => "da",
        "dut" | "nld" | "nl" => "nl",
        "eng" | "en" => "en",
        "fin" | "fi" => "fi",
        "fre" | "fra" | "fr" => "fr",
        "ger" | "deu" | "de" => "de",
        "gre" | "ell" | "el" => "el",
        "heb" | "he" => "he",
        "hun" | "hu" => "hu",
        "ita" | "it" => "it",
        "jpn" | "ja" => "ja",
        "kor" | "ko" => "ko",
        "lat" | "la" => "la",
        "nor" | "no" => "no",
        "pol" | "pl" => "pl",
        "por" | "pt" => "pt",
        "rum" | "ron" | "ro" => "ro",
        "rus" | "ru" => "ru",
        "spa" | "es" => "es",
        "swe" | "sv" => "sv",
        "tur" | "tr" => "tr",
        "ukr" | "uk" => "uk",
        _ => return None,
    };
    Some(mapped.to_string())
}

/// The ISBN hyphenation for an identifier, when valid.
pub(crate) fn hyphenate_isbn(value: &str) -> Option<String> {
    let digits: String = value.chars().filter(|c| c.is_ascii_digit() || c.eq_ignore_ascii_case(&'X')).collect();
    let isbn: isbn::Isbn = digits.parse().ok()?;
    isbn.hyphenate().ok().map(|hyphenated| hyphenated.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn years() {
        assert_eq!(year_from("October 1, 1988"), "1988");
        assert_eq!(year_from("1998"), "1998");
        assert_eq!(year_from("no year"), "");
        assert_eq!(year_from("٢٠٢٠"), "");
    }

    #[test]
    fn page_counts() {
        assert_eq!(pages_from("452 S."), Some(452));
        assert_eq!(pages_from("452 pages"), Some(452));
        assert_eq!(pages_from("1 vol. (113 p.) : ill."), Some(113));
        assert_eq!(pages_from("no pages"), None);
    }

    #[test]
    fn names() {
        assert_eq!(natural_name("Kater, Michael H. [Verfasser]"), "Michael H. Kater");
        assert_eq!(natural_name("Saint-Exupéry, Antoine de (1900-1944). Auteur du texte"), "Antoine de Saint-Exupéry");
        assert_eq!(natural_name("Roald Dahl"), "Roald Dahl");
    }

    #[test]
    fn titles() {
        assert_eq!(
            split_title("Gewagtes Spiel : Jazz im Nationalsozialismus / Michael H. Kater."),
            ("Gewagtes Spiel".to_string(), Some("Jazz im Nationalsozialismus".to_string()))
        );
        assert_eq!(split_title("Fantastic Mr. Fox"), ("Fantastic Mr. Fox".to_string(), None));
    }

    #[test]
    fn publishers() {
        assert_eq!(
            split_publisher("München ; Wien : Verlag"),
            ("Verlag".to_string(), vec!["München".to_string(), "Wien".to_string()])
        );
        assert_eq!(split_publisher("Gallimard (Paris)"), ("Gallimard".to_string(), vec!["Paris".to_string()]));
        assert_eq!(split_publisher("Puffin"), ("Puffin".to_string(), vec![]));
    }

    #[test]
    fn languages() {
        assert_eq!(language_code("ger").as_deref(), Some("de"));
        assert_eq!(language_code("fre").as_deref(), Some("fr"));
        assert_eq!(language_code("en").as_deref(), Some("en"));
        assert_eq!(language_code("xxx"), None);
    }

    #[test]
    fn normalizes_combining_marks() {
        assert_eq!(clean("Mu\u{308}nchen"), "München");
    }
}
