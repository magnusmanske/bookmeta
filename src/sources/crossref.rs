//! Crossref, for books that have a DOI and deposited ISBN metadata.

use super::{FetchOptions, get_json};
use crate::util::{clean, language_code, natural_name, year_from};
use crate::{Book, BookError, Isbn, Source};
use reqwest::Client;
use serde::Deserialize;

const URL: &str = "https://api.crossref.org/works";
const MAILTO: &str = "magnusmanske@toolforge.org";

pub(crate) async fn fetch(client: &Client, isbn: &Isbn, options: &FetchOptions<'_>) -> Result<Vec<Book>, BookError> {
    let query = [
        // Crossref files book chapters under the same ISBN; ask for the book record.
        ("filter", format!("isbn:{},type:book", isbn.isbn13_digits())),
        ("rows", "5".to_string()),
        ("mailto", MAILTO.to_string()),
    ];
    let response: Response = get_json(client, URL, &query, options.timeout).await?;
    let mut books = vec![];
    for item in response.message.items {
        if !is_book(&item.kind) {
            continue;
        }
        if item.isbn.iter().any(|value| isbn.matches(value))
            && let Some(book) = from_item(item, isbn)
        {
            books.push(book);
        }
    }
    Ok(books)
}

fn is_book(kind: &str) -> bool {
    matches!(kind, "book" | "monograph" | "edited-book" | "reference-book" | "book-set" | "book-series")
}

#[derive(Debug, Deserialize)]
struct Response {
    message: Message,
}

#[derive(Debug, Deserialize)]
struct Message {
    #[serde(default)]
    items: Vec<Item>,
}

#[derive(Debug, Deserialize)]
struct Item {
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    title: Vec<String>,
    #[serde(default)]
    subtitle: Vec<String>,
    #[serde(default)]
    author: Vec<Author>,
    publisher: Option<String>,
    #[serde(rename = "publisher-location")]
    publisher_location: Option<String>,
    #[serde(rename = "published-print")]
    published_print: Option<PartialDate>,
    #[serde(rename = "published-online")]
    published_online: Option<PartialDate>,
    issued: Option<PartialDate>,
    #[serde(rename = "ISBN", default)]
    isbn: Vec<String>,
    #[serde(rename = "DOI")]
    doi: Option<String>,
    language: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Author {
    given: Option<String>,
    family: Option<String>,
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PartialDate {
    #[serde(rename = "date-parts", default)]
    date_parts: Vec<Vec<Option<u16>>>,
}

impl PartialDate {
    fn year(&self) -> Option<String> {
        self.date_parts.first().and_then(|parts| parts.first()).and_then(|year| *year).map(|year| year.to_string())
    }
}

fn from_item(item: Item, isbn: &Isbn) -> Option<Book> {
    let mut book = Book::new(Source::Crossref);
    book.title = item.title.first().cloned().unwrap_or_default();
    book.subtitle = item.subtitle.first().cloned().filter(|subtitle| !subtitle.trim().is_empty());
    book.authors = item
        .author
        .into_iter()
        .filter_map(|author| {
            if let Some(name) = author.name {
                return Some(clean(&name));
            }
            let family = author.family.unwrap_or_default();
            let given = author.given.unwrap_or_default();
            let name = format!("{given} {family}").trim().to_string();
            (!name.is_empty()).then(|| natural_name(&name))
        })
        .collect();
    book.publisher = item.publisher.map(|publisher| clean(&publisher)).unwrap_or_default();
    book.places = item
        .publisher_location
        .map(|places| places.split(';').map(clean).filter(|place| !place.is_empty()).collect())
        .unwrap_or_default();
    book.year = item
        .published_print
        .or(item.published_online)
        .or(item.issued)
        .and_then(|date| date.year())
        .or_else(|| item.title.first().map(|title| year_from(title)))
        .unwrap_or_default();
    book.language = item.language.as_deref().and_then(language_code);
    book.ids.crossref_doi = item.doi.map(|doi| clean(&doi));
    book.ids.isbn13 = Some(isbn.isbn13_hyphenated());
    book.ids.isbn10 = isbn.isbn10_hyphenated();
    book.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_book() {
        let response: Response = serde_json::from_str(
            r#"{"message":{"items":[{
                "type":"book",
                "title":["The Rust Programming Language"],
                "author":[{"given":"Steve","family":"Klabnik"},{"given":"Carol","family":"Nichols"}],
                "publisher":"No Starch Press",
                "published-print":{"date-parts":[[2018,6]]},
                "ISBN":["9781593278281"],
                "DOI":"10.1234/example",
                "language":"en"
            }]}}"#,
        )
        .unwrap();
        let isbn = Isbn::parse("9781593278281").unwrap();
        let book = from_item(response.message.items.into_iter().next().unwrap(), &isbn).unwrap();
        assert_eq!(book.title, "The Rust Programming Language");
        assert_eq!(book.authors, ["Steve Klabnik", "Carol Nichols"]);
        assert_eq!(book.publisher, "No Starch Press");
        assert_eq!(book.year, "2018");
        assert_eq!(book.ids.crossref_doi.as_deref(), Some("10.1234/example"));
    }
}
