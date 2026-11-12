//! Bibliothèque nationale de France, via its SRU service.

use super::{FetchOptions, get_text};
use crate::util::{clean, language_code, natural_name, pages_from, split_publisher, split_title, year_from};
use crate::{Book, BookError, Isbn, Source};
use reqwest::Client;
use roxmltree::Node;

const URL: &str = "https://catalogue.bnf.fr/api/SRU";
const DC: &str = "http://purl.org/dc/elements/1.1/";
/// BnF wraps the Dublin Core record in an `oai_dc:dc` element.
const OAI_DC: &str = "http://www.openarchives.org/OAI/2.0/oai_dc/";

pub(crate) async fn fetch(client: &Client, isbn: &Isbn, options: &FetchOptions<'_>) -> Result<Vec<Book>, BookError> {
    let query = [
        ("version", "1.2".to_string()),
        ("operation", "searchRetrieve".to_string()),
        ("query", format!("bib.isbn all \"{}\"", isbn.isbn13_digits())),
        ("recordSchema", "dublincore".to_string()),
        ("maximumRecords", "1".to_string()),
    ];
    let xml = get_text(client, URL, &query, options.timeout).await?;
    parse(&xml, isbn)
}

fn parse(xml: &str, isbn: &Isbn) -> Result<Vec<Book>, BookError> {
    let document = roxmltree::Document::parse(xml)?;
    let count = document
        .descendants()
        .find(|node| node.is_element() && node.tag_name().name() == "numberOfRecords")
        .and_then(|node| node.text())
        .and_then(|text| text.trim().parse::<u32>().ok())
        .unwrap_or(0);
    if count == 0 {
        return Ok(vec![]);
    }
    let Some(dc) = document
        .descendants()
        .find(|node| node.is_element() && node.tag_name().name() == "dc"
            && matches!(node.tag_name().namespace(), Some(DC | OAI_DC)))
    else {
        return Ok(vec![]);
    };

    let mut book = Book::new(Source::Bnf);
    let titles = texts(dc, "title");
    if let Some(title) = titles.first() {
        let (title, subtitle) = split_title(title);
        book.title = title;
        book.subtitle = subtitle;
    }

    let mut authors: Vec<String> =
        texts(dc, "creator").into_iter().chain(texts(dc, "contributor")).map(|name| natural_name(&name)).collect();
    authors.retain(|name| !name.is_empty());
    book.authors = authors;

    if let Some(publisher) = texts(dc, "publisher").first() {
        let (publisher, places) = split_publisher(publisher);
        book.publisher = publisher;
        book.places = places;
    }
    book.year = texts(dc, "date").first().map(|date| year_from(date)).unwrap_or_default();
    book.language = texts(dc, "language").first().and_then(|code| language_code(code));
    book.pages = texts(dc, "format").iter().find_map(|format| pages_from(format));
    book.ids.isbn13 = Some(isbn.isbn13_hyphenated());
    book.ids.isbn10 = isbn.isbn10_hyphenated();
    book.ids.bnf = document
        .descendants()
        .find(|node| node.is_element() && node.tag_name().name() == "recordIdentifier")
        .and_then(|node| node.text())
        .map(clean)
        .filter(|id| !id.is_empty());
    Ok(book.finish().into_iter().collect())
}

fn texts(node: Node<'_, '_>, name: &str) -> Vec<String> {
    node.children()
        .filter(|child| {
            child.is_element() && child.tag_name().name() == name && child.tag_name().namespace() == Some(DC)
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
<srw:searchRetrieveResponse xmlns:srw="http://www.loc.gov/zing/srw/">
<srw:version>1.2</srw:version>
<srw:numberOfRecords>1</srw:numberOfRecords>
<srw:records><srw:record><srw:recordData>
<oai_dc:dc xmlns:oai_dc="http://www.openarchives.org/OAI/2.0/oai_dc/" xmlns:dc="http://purl.org/dc/elements/1.1/">
<dc:title>Le petit prince / Antoine de Saint-Exupéry</dc:title>
<dc:creator>Saint-Exupéry, Antoine de (1900-1944). Auteur du texte</dc:creator>
<dc:publisher>Gallimard (Paris)</dc:publisher>
<dc:date>2007</dc:date>
<dc:format>1 vol. (113 p.) : ill.</dc:format>
<dc:language>fre</dc:language>
</oai_dc:dc>
</srw:recordData><srw:recordIdentifier>ark:/12148/cb41023439w</srw:recordIdentifier></srw:record></srw:records>
</srw:searchRetrieveResponse>"#;

    #[test]
    fn parses_dublin_core() {
        let isbn = Isbn::parse("9782070612758").unwrap();
        let books = parse(FIXTURE, &isbn).unwrap();
        let book = &books[0];
        assert_eq!(book.title, "Le petit prince");
        assert_eq!(book.authors, ["Antoine de Saint-Exupéry"]);
        assert_eq!(book.publisher, "Gallimard");
        assert_eq!(book.places, ["Paris"]);
        assert_eq!(book.year, "2007");
        assert_eq!(book.pages, Some(113));
        assert_eq!(book.language.as_deref(), Some("fr"));
    }
}
