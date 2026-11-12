//! Google Books, using the JSON API when a key is configured and the legacy
//! GData feed otherwise.

use super::{FetchOptions, get_json, get_text};
use crate::util::{clean, hyphenate_isbn, language_code, natural_name, pages_from, split_publisher, year_from};
use crate::{Book, BookError, Isbn, Source};
use reqwest::Client;
use roxmltree::Node;
use serde::Deserialize;

const GDATA_URL: &str = "https://books.google.com/books/feeds/volumes";
const API_URL: &str = "https://www.googleapis.com/books/v1/volumes";
const DC_TERMS: &str = "http://purl.org/dc/terms";
const ATOM: &str = "http://www.w3.org/2005/Atom";

pub(crate) async fn fetch(client: &Client, isbn: &Isbn, options: &FetchOptions<'_>) -> Result<Vec<Book>, BookError> {
    match options.google_books_api_key {
        Some(key) => fetch_api(client, isbn, key, options).await,
        None => fetch_gdata(client, isbn, options).await,
    }
}

async fn fetch_gdata(client: &Client, isbn: &Isbn, options: &FetchOptions<'_>) -> Result<Vec<Book>, BookError> {
    let query = [("q", format!("isbn:{}", isbn.isbn13_digits())), ("max-results", "10".to_string())];
    let xml = get_text(client, GDATA_URL, &query, options.timeout).await?;
    parse_gdata(&xml, isbn)
}

fn parse_gdata(xml: &str, isbn: &Isbn) -> Result<Vec<Book>, BookError> {
    let document = roxmltree::Document::parse(xml)?;
    let mut books = vec![];
    for entry in document.descendants().filter(|node| {
        node.is_element() && node.tag_name().name() == "entry" && node.tag_name().namespace() == Some(ATOM)
    }) {
        let identifiers = texts(entry, DC_TERMS, "identifier");
        let isbns: Vec<&str> = identifiers.iter().filter_map(|identifier| identifier.strip_prefix("ISBN:")).collect();
        if isbns.is_empty() || !isbns.iter().any(|value| isbn.matches(value)) {
            continue;
        }

        let mut titles: Vec<String> =
            texts(entry, DC_TERMS, "title").into_iter().filter(|title| !title.starts_with('[')).collect();
        let title = titles.first().cloned().unwrap_or_default();
        let subtitle = if titles.len() > 1 { Some(titles.remove(1)) } else { None };
        let mut book = Book::new(Source::GoogleBooks);
        book.title = title;
        book.subtitle = subtitle;
        book.authors = texts(entry, DC_TERMS, "creator").into_iter().map(|name| natural_name(&name)).collect();
        if let Some(publisher) = texts(entry, DC_TERMS, "publisher").first() {
            let (publisher, places) = split_publisher(publisher);
            book.publisher = publisher;
            book.places = places;
        }
        book.year = texts(entry, DC_TERMS, "date").first().map(|date| year_from(date)).unwrap_or_default();
        book.language = texts(entry, DC_TERMS, "language").first().and_then(|code| language_code(code));
        book.pages = texts(entry, DC_TERMS, "format").iter().find_map(|format| pages_from(format));
        book.ids.google_books = entry
            .children()
            .find(|node| {
                node.is_element() && node.tag_name().name() == "id" && node.tag_name().namespace() == Some(ATOM)
            })
            .and_then(|node| node.text())
            .and_then(|id| id.rsplit('/').next())
            .map(str::to_string);
        book.ids.isbn13 = Some(isbn.isbn13_hyphenated());
        book.ids.isbn10 = isbn.isbn10_hyphenated();
        if let Some(book) = book.finish() {
            books.push(book);
        }
    }
    Ok(books)
}

#[derive(Debug, Deserialize)]
struct ApiResponse {
    #[serde(default)]
    items: Vec<ApiItem>,
}

#[derive(Debug, Deserialize)]
struct ApiItem {
    id: String,
    #[serde(rename = "volumeInfo", default)]
    volume_info: VolumeInfo,
}

#[derive(Debug, Default, Deserialize)]
struct VolumeInfo {
    title: Option<String>,
    subtitle: Option<String>,
    #[serde(default)]
    authors: Vec<String>,
    publisher: Option<String>,
    #[serde(rename = "publishedDate")]
    published_date: Option<String>,
    #[serde(rename = "pageCount")]
    page_count: Option<u32>,
    language: Option<String>,
    #[serde(rename = "industryIdentifiers", default)]
    industry_identifiers: Vec<IndustryIdentifier>,
}

#[derive(Debug, Deserialize)]
struct IndustryIdentifier {
    #[serde(rename = "type")]
    kind: String,
    identifier: String,
}

async fn fetch_api(
    client: &Client,
    isbn: &Isbn,
    key: &str,
    options: &FetchOptions<'_>,
) -> Result<Vec<Book>, BookError> {
    let query = [("q", format!("isbn:{}", isbn.isbn13_digits())), ("key", key.to_string())];
    let response: ApiResponse = get_json(client, API_URL, &query, options.timeout).await?;
    let mut books = vec![];
    for item in response.items {
        let info = item.volume_info;
        let identifiers: Vec<&IndustryIdentifier> = info
            .industry_identifiers
            .iter()
            .filter(|identifier| matches!(identifier.kind.as_str(), "ISBN_10" | "ISBN_13"))
            .collect();
        if identifiers.is_empty() || !identifiers.iter().any(|identifier| isbn.matches(&identifier.identifier)) {
            continue;
        }
        let mut book = Book::new(Source::GoogleBooks);
        book.title = info.title.unwrap_or_default();
        book.subtitle = info.subtitle;
        book.authors = info.authors.into_iter().map(|name| natural_name(&name)).collect();
        if let Some(publisher) = info.publisher {
            let (publisher, places) = split_publisher(&publisher);
            book.publisher = publisher;
            book.places = places;
        }
        book.year = info.published_date.as_deref().map(year_from).unwrap_or_default();
        book.language = info.language.as_deref().and_then(language_code);
        book.pages = info.page_count;
        book.ids.google_books = Some(item.id);
        book.ids.isbn13 = identifiers
            .iter()
            .find(|identifier| identifier.kind == "ISBN_13")
            .and_then(|identifier| hyphenate_isbn(&identifier.identifier))
            .or_else(|| Some(isbn.isbn13_hyphenated()));
        book.ids.isbn10 = identifiers
            .iter()
            .find(|identifier| identifier.kind == "ISBN_10")
            .and_then(|identifier| hyphenate_isbn(&identifier.identifier))
            .or_else(|| isbn.isbn10_hyphenated());
        if let Some(book) = book.finish() {
            books.push(book);
        }
    }
    Ok(books)
}

fn texts(node: Node<'_, '_>, namespace: &str, name: &str) -> Vec<String> {
    node.children()
        .filter(|child| {
            child.is_element() && child.tag_name().name() == name && child.tag_name().namespace() == Some(namespace)
        })
        .filter_map(|child| child.text())
        .map(clean)
        .filter(|text| !text.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom" xmlns:dc="http://purl.org/dc/terms">
 <title>Search results for isbn:9780140328721</title>
 <entry>
  <id>http://www.google.com/books/feeds/volumes/DlQbmJc5WlQC</id>
  <title>Fantastic Mr. Fox</title>
  <dc:creator>Roald Dahl</dc:creator>
  <dc:title>Fantastic Mr. Fox</dc:title>
  <dc:date>1988</dc:date>
  <dc:publisher>Puffin</dc:publisher>
  <dc:format>96 pages</dc:format>
  <dc:language>en</dc:language>
  <dc:identifier>DlQbmJc5WlQC</dc:identifier>
  <dc:identifier>ISBN:0140328726</dc:identifier>
  <dc:identifier>ISBN:9780140328721</dc:identifier>
 </entry>
</feed>"#;

    #[test]
    fn parses_gdata_and_matches_the_isbn() {
        let isbn = Isbn::parse("9780140328721").unwrap();
        let books = parse_gdata(FIXTURE, &isbn).unwrap();
        let book = &books[0];
        assert_eq!(book.title, "Fantastic Mr. Fox");
        assert_eq!(book.authors, ["Roald Dahl"]);
        assert_eq!(book.publisher, "Puffin");
        assert_eq!(book.year, "1988");
        assert_eq!(book.pages, Some(96));
        assert_eq!(book.language.as_deref(), Some("en"));
        assert_eq!(book.ids.google_books.as_deref(), Some("DlQbmJc5WlQC"));
    }

    #[test]
    fn rejects_an_entry_for_another_isbn() {
        let isbn = Isbn::parse("9780140328721").unwrap();
        let other = FIXTURE.replace("9780140328721", "9780140328728").replace("0140328726", "0140328728");
        let books = parse_gdata(&other, &isbn).unwrap();
        assert!(books.is_empty());
    }
}
