//! Koninklijke Bibliotheek, the National Library of the Netherlands, via its
//! SRU service. The records are Dublin Core.

use super::{FetchOptions, get_text};
use crate::util::{
    clean, language_code, natural_name, pages_from_extent, split_publisher, split_title, year_from,
};
use crate::{Book, BookError, Isbn, Source};
use reqwest::Client;
use roxmltree::{Document, Node};

const URL: &str = "https://jsru.kb.nl/sru/sru";
const DC: &str = "http://purl.org/dc/elements/1.1/";
const DCTERMS: &str = "http://purl.org/dc/terms/";
const DCX: &str = "http://krait.kb.nl/coop/tel/handbook/telterms.html";
/// The general catalogue, which includes the national bibliography.
const COLLECTION: &str = "GGC";

pub(crate) async fn fetch(
    client: &Client,
    isbn: &Isbn,
    options: &FetchOptions<'_>,
) -> Result<Vec<Book>, BookError> {
    // Older records are indexed by ISBN-10 only.
    let mut terms = format!("dc.identifier=\"{}\"", isbn.isbn13_digits());
    if let Some(isbn10) = isbn.isbn10_digits() {
        terms.push_str(&format!(" or dc.identifier=\"{isbn10}\""));
    }
    let query = [
        ("version", "1.2".to_string()),
        ("operation", "searchRetrieve".to_string()),
        ("x-collection", COLLECTION.to_string()),
        ("recordSchema", "dc".to_string()),
        ("maximumRecords", "3".to_string()),
        ("query", terms),
    ];
    let xml = get_text(client, URL, &query, options.timeout).await?;
    parse(&xml, isbn)
}

fn parse(xml: &str, isbn: &Isbn) -> Result<Vec<Book>, BookError> {
    let document = Document::parse(xml)?;
    let count = document
        .descendants()
        .find(|node| node.is_element() && node.tag_name().name() == "numberOfRecords")
        .and_then(|node| node.text())
        .and_then(|text| text.trim().parse::<u32>().ok())
        .unwrap_or(0);
    if count == 0 {
        return Ok(vec![]);
    }

    let mut books = vec![];
    for data in document
        .descendants()
        .filter(|node| node.is_element() && node.tag_name().name() == "recordData")
    {
        let identifiers = texts(data, DC, "identifier");
        // The OR query can return records for neighbouring ISBNs.
        if !identifiers.iter().any(|value| isbn.matches(value)) {
            continue;
        }
        if let Some(book) = from_record(data, &identifiers, isbn) {
            books.push(book);
        }
    }
    Ok(books)
}

fn from_record(data: Node<'_, '_>, identifiers: &[String], isbn: &Isbn) -> Option<Book> {
    let mut book = Book::new(Source::Kb);
    if let Some(title) = texts(data, DC, "title").first() {
        let (title, subtitle) = split_title(title);
        book.title = title;
        book.subtitle = subtitle;
    }
    book.authors = texts(data, DC, "creator")
        .into_iter()
        .chain(texts(data, DC, "contributor"))
        .map(|name| natural_name(&name))
        .filter(|name| !name.is_empty())
        .collect();
    if let Some(publisher) = texts(data, DC, "publisher").into_iter().next() {
        let (publisher, places) = split_publisher(&publisher);
        book.publisher = publisher;
        book.places = places;
    }
    book.year = texts(data, DC, "date")
        .first()
        .map(|date| year_from(date))
        .unwrap_or_default();
    // The catalogue lists `dut` next to the language's names.
    book.language = texts(data, DC, "language")
        .iter()
        .find_map(|code| language_code(code));
    // Extents mix page counts and sizes: `236 p` and `19 cm`.
    book.pages = texts(data, DCTERMS, "extent")
        .iter()
        .find_map(|extent| pages_from_extent(extent));
    book.ids.kb = texts(data, DCX, "recordIdentifier")
        .first()
        .and_then(|value| ppn(value))
        .or_else(|| identifiers.iter().find_map(|value| ppn(value)));
    book.ids.isbn13 = Some(isbn.isbn13_hyphenated());
    book.ids.isbn10 = isbn.isbn10_hyphenated();
    book.finish()
}

/// The PPN of a catalogue record, from `...PPN?PPN=162735464` or
/// `...?urn=PPN:38303521X`.
fn ppn(value: &str) -> Option<String> {
    ["PPN=", "PPN:"].iter().find_map(|marker| {
        value
            .split_once(marker)
            .map(|(_, ppn)| clean(ppn))
            .filter(|ppn| !ppn.is_empty())
    })
}

fn texts(node: Node<'_, '_>, namespace: &str, name: &str) -> Vec<String> {
    node.children()
        .filter(|child| {
            child.is_element()
                && child.tag_name().name() == name
                && child.tag_name().namespace() == Some(namespace)
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
<srw:searchRetrieveResponse xmlns:srw="http://www.loc.gov/zing/srw/"
    xmlns:dc="http://purl.org/dc/elements/1.1/"
    xmlns:dcterms="http://purl.org/dc/terms/"
    xmlns:dcx="http://krait.kb.nl/coop/tel/handbook/telterms.html">
  <srw:version>1.2</srw:version>
  <srw:numberOfRecords>1</srw:numberOfRecords>
  <srw:records><srw:record>
    <srw:recordPacking>xml</srw:recordPacking>
    <srw:recordSchema>info:srw/schema/1/dc-v1.1</srw:recordSchema>
    <srw:recordData>
      <dc:creator>Mulisch, Harry</dc:creator>
      <dc:date>Wed Jan 01 01:00:00 CET 1997</dc:date>
      <dc:identifier>782318665</dc:identifier>
      <dc:identifier>9023437209</dc:identifier>
      <dc:language>nederlands</dc:language>
      <dc:language>dut</dc:language>
      <dc:publisher>Amsterdam : De Bezige Bij</dc:publisher>
      <dc:subject>Romans (teksten)</dc:subject>
      <dc:title>De aanslag</dc:title>
      <dcterms:extent>236 p</dcterms:extent>
      <dcterms:extent>19 cm</dcterms:extent>
      <dcx:recordIdentifier>http://picarta.pica.nl/DB=2.4/PPN?PPN=162735464</dcx:recordIdentifier>
    </srw:recordData>
  </srw:record></srw:records>
</srw:searchRetrieveResponse>"#;

    #[test]
    fn parses_dublin_core() {
        let isbn = Isbn::parse("9789023437208").unwrap();
        let books = parse(FIXTURE, &isbn).unwrap();
        let book = &books[0];
        assert_eq!(book.title, "De aanslag");
        assert_eq!(book.authors, ["Harry Mulisch"]);
        assert_eq!(book.publisher, "De Bezige Bij");
        assert_eq!(book.places, ["Amsterdam"]);
        assert_eq!(book.year, "1997");
        assert_eq!(book.pages, Some(236));
        assert_eq!(book.language.as_deref(), Some("nl"));
        assert_eq!(book.ids.kb.as_deref(), Some("162735464"));
    }

    #[test]
    fn records_for_other_isbns_are_ignored() {
        let isbn = Isbn::parse("9780140328721").unwrap();
        assert!(parse(FIXTURE, &isbn).unwrap().is_empty());
    }

    #[test]
    fn an_empty_answer_has_no_books() {
        let isbn = Isbn::parse("9789023437208").unwrap();
        let empty = FIXTURE.replace("<srw:numberOfRecords>1<", "<srw:numberOfRecords>0<");
        assert!(parse(&empty, &isbn).unwrap().is_empty());
    }
}
