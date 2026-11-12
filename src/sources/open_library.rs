//! Open Library, using the `jscmd=data` API and the edition JSON as fallback.

use super::{FetchOptions, get_json};
use crate::util::{clean, language_code, natural_name, year_from};
use crate::{Book, BookError, Isbn, Source};
use reqwest::Client;
use serde::Deserialize;
use std::collections::HashMap;

const API_URL: &str = "https://openlibrary.org/api/books";

pub(crate) async fn fetch(client: &Client, isbn: &Isbn, options: &FetchOptions<'_>) -> Result<Vec<Book>, BookError> {
    let query = [
        ("bibkeys", format!("ISBN:{}", isbn.isbn13_digits())),
        ("format", "json".to_string()),
        ("jscmd", "data".to_string()),
    ];
    let data: HashMap<String, DataBook> = get_json(client, API_URL, &query, options.timeout).await?;
    if let Some(book) = data.into_values().next()
        && let Some(book) = from_data(book, isbn)
    {
        return Ok(vec![book]);
    }
    for value in [Some(isbn.isbn13_digits()), isbn.isbn10_digits()].into_iter().flatten() {
        if let Some(book) = fetch_edition(client, &value, isbn, options).await? {
            return Ok(vec![book]);
        }
    }
    Ok(vec![])
}

async fn fetch_edition(
    client: &Client,
    value: &str,
    isbn: &Isbn,
    options: &FetchOptions<'_>,
) -> Result<Option<Book>, BookError> {
    let url = format!("https://openlibrary.org/isbn/{value}.json");
    let response = client.get(&url).timeout(options.timeout).send().await?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let text = response.error_for_status()?.text().await?;
    if text.len() > super::MAX_BODY {
        return Err(BookError::TooLarge);
    }
    if text.trim().is_empty() {
        return Ok(None);
    }
    let edition: Edition = serde_json::from_str(&text)?;
    Ok(from_edition(edition, isbn))
}

#[derive(Debug, Deserialize)]
struct Name {
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DataBook {
    title: Option<String>,
    subtitle: Option<String>,
    #[serde(default)]
    authors: Vec<Name>,
    #[serde(default)]
    publishers: Vec<Name>,
    #[serde(default)]
    publish_places: Vec<Name>,
    publish_date: Option<String>,
    number_of_pages: Option<u32>,
    key: Option<String>,
}

fn from_data(data: DataBook, isbn: &Isbn) -> Option<Book> {
    let mut book = Book::new(Source::OpenLibrary);
    book.title = data.title.unwrap_or_default();
    book.subtitle = data.subtitle;
    book.authors = data.authors.into_iter().filter_map(|author| author.name).map(|name| natural_name(&name)).collect();
    book.publisher =
        data.publishers.into_iter().find_map(|publisher| publisher.name).map(|name| clean(&name)).unwrap_or_default();
    book.places = data.publish_places.into_iter().filter_map(|place| place.name).map(|place| clean(&place)).collect();
    book.year = data.publish_date.as_deref().map(year_from).unwrap_or_default();
    book.pages = data.number_of_pages;
    book.ids.open_library = data.key.as_deref().and_then(|key| key.strip_prefix("/books/")).map(str::to_string);
    book.ids.isbn13 = Some(isbn.isbn13_hyphenated());
    book.ids.isbn10 = isbn.isbn10_hyphenated();
    book.finish()
}

#[derive(Debug, Deserialize)]
struct KeyRef {
    key: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Edition {
    key: Option<String>,
    title: Option<String>,
    subtitle: Option<String>,
    #[serde(default)]
    publishers: Vec<String>,
    #[serde(default)]
    publish_places: Vec<String>,
    publish_date: Option<String>,
    number_of_pages: Option<u32>,
    #[serde(default)]
    languages: Vec<KeyRef>,
}

fn from_edition(edition: Edition, isbn: &Isbn) -> Option<Book> {
    let mut book = Book::new(Source::OpenLibrary);
    book.title = edition.title.unwrap_or_default();
    book.subtitle = edition.subtitle;
    book.publisher = edition.publishers.into_iter().next().map(|publisher| clean(&publisher)).unwrap_or_default();
    book.places = edition.publish_places.into_iter().map(|place| clean(&place)).collect();
    book.year = edition.publish_date.as_deref().map(year_from).unwrap_or_default();
    book.pages = edition.number_of_pages;
    book.language = edition
        .languages
        .into_iter()
        .find_map(|language| language.key)
        .and_then(|key| key.strip_prefix("/languages/").map(str::to_string))
        .and_then(|code| language_code(&code));
    book.ids.open_library = edition.key.as_deref().and_then(|key| key.strip_prefix("/books/")).map(str::to_string);
    book.ids.isbn13 = Some(isbn.isbn13_hyphenated());
    book.ids.isbn10 = isbn.isbn10_hyphenated();
    book.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"{
      "ISBN:9780140328721": {
        "url": "http://openlibrary.org/books/OL7353617M/Fantastic_Mr._Fox",
        "key": "/books/OL7353617M",
        "title": "Fantastic Mr. Fox",
        "authors": [{"url": "http://openlibrary.org/authors/OL34184A/Roald_Dahl", "name": "Roald Dahl"}],
        "number_of_pages": 96,
        "publishers": [{"name": "Puffin"}],
        "publish_places": [{"name": "London"}],
        "publish_date": "October 1, 1988"
      }
    }"#;

    #[test]
    fn parses_the_data_api() {
        let data: HashMap<String, DataBook> = serde_json::from_str(FIXTURE).unwrap();
        let isbn = Isbn::parse("9780140328721").unwrap();
        let book = from_data(data.into_values().next().unwrap(), &isbn).unwrap();
        assert_eq!(book.title, "Fantastic Mr. Fox");
        assert_eq!(book.authors, ["Roald Dahl"]);
        assert_eq!(book.publisher, "Puffin");
        assert_eq!(book.places, ["London"]);
        assert_eq!(book.year, "1988");
        assert_eq!(book.pages, Some(96));
        assert_eq!(book.ids.open_library.as_deref(), Some("OL7353617M"));
    }

    #[test]
    fn parses_the_edition_json() {
        let edition: Edition = serde_json::from_str(
            r#"{"key":"/books/OL7353617M","title":"Fantastic Mr. Fox","publishers":["Puffin"],
                "publish_places":["London"],"publish_date":"1988","number_of_pages":96,
                "languages":[{"key":"/languages/eng"}]}"#,
        )
        .unwrap();
        let isbn = Isbn::parse("9780140328721").unwrap();
        let book = from_edition(edition, &isbn).unwrap();
        assert_eq!(book.publisher, "Puffin");
        assert_eq!(book.places, ["London"]);
        assert_eq!(book.language.as_deref(), Some("en"));
    }
}
