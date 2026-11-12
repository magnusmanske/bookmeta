//! Goodreads.
//!
//! Goodreads retired its public API in 2020, but the book page that
//! `book/isbn/<isbn>` redirects to still embeds the record as JSON in a
//! `__NEXT_DATA__` script tag. That JSON is read directly, so no HTML parsing
//! is needed.

use crate::util::{clean, hyphenate_isbn, language_code, natural_name};
use crate::{Book, BookError, Isbn, Source};
use reqwest::Client;
use serde::Deserialize;
use std::collections::HashMap;

use super::FetchOptions;

const URL: &str = "https://www.goodreads.com/book/isbn";

pub(crate) async fn fetch(
    client: &Client,
    isbn: &Isbn,
    options: &FetchOptions<'_>,
) -> Result<Vec<Book>, BookError> {
    // The ISBN-10 form of this route answers 404 for books that the ISBN-13
    // form finds, so always query the ISBN-13.
    let url = format!("{URL}/{}", isbn.isbn13_digits());
    let response = client.get(&url).timeout(options.timeout).send().await?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(vec![]);
    }
    if response
        .content_length()
        .is_some_and(|length| length > super::MAX_BODY as u64)
    {
        return Err(BookError::TooLarge);
    }
    let text = response.error_for_status()?.text().await?;
    if text.len() > super::MAX_BODY {
        return Err(BookError::TooLarge);
    }
    parse(&text, isbn)
}

#[derive(Debug, Deserialize)]
struct Page {
    props: Props,
}

#[derive(Debug, Deserialize)]
struct Props {
    #[serde(rename = "pageProps")]
    page_props: PageProps,
}

#[derive(Debug, Deserialize)]
struct PageProps {
    #[serde(rename = "apolloState", default)]
    apollo_state: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct Entry {
    #[serde(rename = "legacyId")]
    legacy_id: Option<u64>,
    title: Option<String>,
    #[serde(rename = "primaryContributorEdge")]
    primary_contributor_edge: Option<Edge>,
    #[serde(rename = "secondaryContributorEdges", default)]
    secondary_contributor_edges: Vec<Edge>,
    details: Option<Details>,
}

#[derive(Debug, Deserialize)]
struct Edge {
    node: NodeRef,
    role: Option<String>,
}

#[derive(Debug, Deserialize)]
struct NodeRef {
    #[serde(rename = "__ref")]
    reference: String,
}

#[derive(Debug, Deserialize)]
struct Details {
    publisher: Option<String>,
    #[serde(rename = "numPages")]
    num_pages: Option<u32>,
    /// Milliseconds since the Unix epoch.
    #[serde(rename = "publicationTime")]
    publication_time: Option<i64>,
    isbn: Option<String>,
    isbn13: Option<String>,
    language: Option<Named>,
}

#[derive(Debug, Deserialize)]
struct Named {
    name: Option<String>,
}

/// The JSON from the `__NEXT_DATA__` script tag, when the page has one.
fn next_data(html: &str) -> Option<&str> {
    const MARKER: &str = "id=\"__NEXT_DATA__\"";
    let marker = html.find(MARKER)?;
    let start = html.get(marker..)?.find('>')? + marker + 1;
    let rest = html.get(start..)?;
    let end = rest.find("</script>")?;
    Some(rest.get(..end)?.trim())
}

fn parse(html: &str, isbn: &Isbn) -> Result<Vec<Book>, BookError> {
    let Some(json) = next_data(html) else {
        return Err(BookError::Data("no __NEXT_DATA__ in the page".to_string()));
    };
    let page: Page = serde_json::from_str(json)?;

    // Authors are referenced by ID, so collect the names first.
    let contributors: HashMap<&str, &str> = page
        .props
        .page_props
        .apollo_state
        .iter()
        .filter(|(key, _)| key.starts_with("Contributor:"))
        .filter_map(|(key, value)| Some((key.as_str(), value.get("name")?.as_str()?)))
        .collect();

    let mut books = vec![];
    for (key, value) in &page.props.page_props.apollo_state {
        if !key.starts_with("Book:") {
            continue;
        }
        let Ok(entry) = serde_json::from_value::<Entry>(value.clone()) else {
            continue;
        };
        let Some(details) = &entry.details else {
            continue;
        };
        if !matches_isbn(details, isbn) {
            continue;
        }
        if let Some(book) = from_entry(entry, &contributors, isbn) {
            books.push(book);
        }
    }
    Ok(books)
}

fn matches_isbn(details: &Details, isbn: &Isbn) -> bool {
    [details.isbn13.as_deref(), details.isbn.as_deref()]
        .into_iter()
        .flatten()
        .any(|value| isbn.matches(value))
}

fn from_entry(entry: Entry, contributors: &HashMap<&str, &str>, isbn: &Isbn) -> Option<Book> {
    // `details` was checked by the caller, but it is consumed here.
    let details = entry.details?;
    let mut book = Book::new(Source::Goodreads);
    book.title = entry.title.unwrap_or_default();
    book.authors = entry
        .primary_contributor_edge
        .iter()
        .chain(entry.secondary_contributor_edges.iter())
        .filter(|edge| edge.role.as_deref().is_none_or(|role| role == "Author"))
        .filter_map(|edge| contributors.get(edge.node.reference.as_str()))
        .map(|name| natural_name(name))
        .collect();
    book.publisher = details.publisher.as_deref().map(clean).unwrap_or_default();
    book.year = details
        .publication_time
        .map(year_from_unix_ms)
        .unwrap_or_default();
    book.language = details
        .language
        .and_then(|language| language.name)
        .and_then(|name| language_from_name(&name));
    book.pages = details.num_pages;
    book.ids.goodreads = entry.legacy_id.map(|id| id.to_string());
    // Prefer the edition's own identifiers, falling back to the requested ISBN.
    book.ids.isbn13 = details
        .isbn13
        .as_deref()
        .and_then(hyphenate_isbn)
        .or_else(|| Some(isbn.isbn13_hyphenated()));
    book.ids.isbn10 = details
        .isbn
        .as_deref()
        .and_then(hyphenate_isbn)
        .or_else(|| isbn.isbn10_hyphenated());
    book.finish()
}

/// The year of a `publicationTime` (milliseconds since the Unix epoch, UTC).
fn year_from_unix_ms(milliseconds: i64) -> String {
    let days = milliseconds.div_euclid(86_400_000);
    let (year, _, _) = civil_from_days(days);
    year.to_string()
}

/// Year, month and day for days since 1970-01-01, after Howard Hinnant's
/// `civil_from_days`.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let days = days + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let day_of_era = (days - era * 146_097) as u64;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_prime + 2) / 5 + 1) as u32;
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    } as u32;
    let year = year_of_era as i64 + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// Goodreads writes language names; map the common ones to Wikidata codes.
fn language_from_name(name: &str) -> Option<String> {
    let name = name.trim().to_lowercase();
    let code = match name.as_str() {
        "arabic" => "ar",
        "catalan" => "ca",
        "chinese" => "zh",
        "czech" => "cs",
        "danish" => "da",
        "dutch" => "nl",
        "english" => "en",
        "finnish" => "fi",
        "french" => "fr",
        "german" => "de",
        "greek" => "el",
        "hebrew" => "he",
        "hungarian" => "hu",
        "italian" => "it",
        "japanese" => "ja",
        "korean" => "ko",
        "latin" => "la",
        "norwegian" => "no",
        "polish" => "pl",
        "portuguese" => "pt",
        "romanian" => "ro",
        "russian" => "ru",
        "spanish" => "es",
        "swedish" => "sv",
        "turkish" => "tr",
        "ukrainian" => "uk",
        other => return language_code(other),
    };
    Some(code.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"<!DOCTYPE html><html><head>
<script id="__NEXT_DATA__" type="application/json">{"props":{"pageProps":{"apolloState":{
"Book:kca://book/amzn1.gr.book.v1.other":{"__typename":"Book","legacyId":6693,
  "title":"A different edition",
  "details":{"isbn":"0000000000","isbn13":"9780000000000"}},
"Book:kca://book/amzn1.gr.book.v1.dahl":{"__typename":"Book","legacyId":1507552,
  "title":"Fantastic Mr. Fox",
  "primaryContributorEdge":{"node":{"__ref":"Contributor:dahl"},"role":"Author"},
  "secondaryContributorEdges":[{"node":{"__ref":"Contributor:ross"},"role":"Illustrator"}],
  "details":{"asin":"0140328726","format":"Paperback","numPages":90,
    "publicationTime":591692400000,"publisher":"Puffin",
    "isbn":"0140328726","isbn13":"9780140328721",
    "language":{"__typename":"Language","name":"English"}}},
"Contributor:dahl":{"__typename":"Contributor","name":"Roald Dahl"},
"Contributor:ross":{"__typename":"Contributor","name":"Tony Ross"}
}}}}</script></head><body></body></html>"#;

    #[test]
    fn parses_the_embedded_record() {
        let isbn = Isbn::parse("9780140328721").unwrap();
        let books = parse(FIXTURE, &isbn).unwrap();
        assert_eq!(books.len(), 1);
        let book = &books[0];
        assert_eq!(book.title, "Fantastic Mr. Fox");
        assert_eq!(book.authors, ["Roald Dahl"]);
        assert_eq!(book.publisher, "Puffin");
        assert_eq!(book.year, "1988");
        assert_eq!(book.pages, Some(90));
        assert_eq!(book.language.as_deref(), Some("en"));
        assert_eq!(book.ids.goodreads.as_deref(), Some("1507552"));
        assert_eq!(book.ids.isbn13.as_deref(), Some("978-0-14-032872-1"));
        assert_eq!(book.ids.isbn10.as_deref(), Some("0-14-032872-6"));
    }

    #[test]
    fn other_editions_are_ignored() {
        let isbn = Isbn::parse("9781593278281").unwrap();
        assert!(parse(FIXTURE, &isbn).unwrap().is_empty());
    }

    #[test]
    fn a_page_without_next_data_is_an_error() {
        let isbn = Isbn::parse("9780140328721").unwrap();
        assert!(matches!(
            parse("<html><body>Not found</body></html>", &isbn),
            Err(BookError::Data(_))
        ));
    }

    #[test]
    fn years_from_publication_time() {
        assert_eq!(year_from_unix_ms(591_692_400_000), "1988");
        assert_eq!(year_from_unix_ms(1_529_996_400_000), "2018");
        assert_eq!(year_from_unix_ms(0), "1970");
    }

    #[test]
    fn language_names() {
        assert_eq!(language_from_name("English").as_deref(), Some("en"));
        assert_eq!(language_from_name("German").as_deref(), Some("de"));
        assert_eq!(language_from_name("eng").as_deref(), Some("en"));
        assert_eq!(language_from_name("Klingon"), None);
    }
}
