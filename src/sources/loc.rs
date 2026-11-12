//! Library of Congress, via its MODS SRU service.
//!
//! The ISBN index is not reliable; an empty answer is treated as "no result",
//! and a failed source does not affect the others.

use super::{FetchOptions, get_text};
use crate::util::{clean, language_code, natural_name, pages_from, year_from};
use crate::{Book, BookError, Isbn, Source};
use reqwest::Client;
use roxmltree::Node;

const URL: &str = "http://lx2.loc.gov:210/lcdb";
const MODS: &str = "http://www.loc.gov/mods/v3";

pub(crate) async fn fetch(client: &Client, isbn: &Isbn, options: &FetchOptions<'_>) -> Result<Vec<Book>, BookError> {
    let mut books = fetch_for(client, &isbn.isbn13_digits(), isbn, options).await?;
    if books.is_empty()
        && let Some(isbn10) = isbn.isbn10_digits()
        && isbn10 != isbn.isbn13_digits()
    {
        books = fetch_for(client, &isbn10, isbn, options).await?;
    }
    Ok(books)
}

async fn fetch_for(client: &Client, value: &str, isbn: &Isbn, options: &FetchOptions<'_>) -> Result<Vec<Book>, BookError> {
    let query = [
        ("version", "1.1".to_string()),
        ("operation", "searchRetrieve".to_string()),
        ("recordSchema", "mods".to_string()),
        ("maximumRecords", "1".to_string()),
        ("query", format!("bath.isbn={value}")),
    ];
    let xml = get_text(client, URL, &query, options.timeout).await?;
    parse(&xml, isbn)
}

fn parse(xml: &str, isbn: &Isbn) -> Result<Vec<Book>, BookError> {
    let document = roxmltree::Document::parse(xml)?;
    let Some(mods) = document.descendants().find(|node| {
        node.is_element() && node.tag_name().name() == "mods" && node.tag_name().namespace() == Some(MODS)
    }) else {
        return Ok(vec![]);
    };

    let mut book = Book::new(Source::Loc);
    let title_info = mods
        .children()
        .find(|node| node.is_element() && node.tag_name().name() == "titleInfo" && node.attribute("type").is_none());
    if let Some(title_info) = title_info {
        book.title = child_texts(title_info, "title").first().cloned().unwrap_or_default();
        book.subtitle = child_texts(title_info, "subTitle").first().cloned();
    }

    let primary: Vec<Node<'_, '_>> = mods
        .children()
        .filter(|node| {
            node.is_element() && node.tag_name().name() == "name" && node.attribute("usage") == Some("primary")
        })
        .collect();
    let names: Vec<Node<'_, '_>> = if primary.is_empty() {
        mods.children().filter(|node| node.is_element() && node.tag_name().name() == "name").collect()
    } else {
        primary
    };
    for name in names {
        for part in name
            .children()
            .filter(|node| node.is_element() && node.tag_name().name() == "namePart")
            .filter(|node| !matches!(node.attribute("type"), Some("date" | "termsOfAddress")))
        {
            if let Some(text) = part.text().map(clean).filter(|text| !text.is_empty()) {
                book.authors.push(natural_name(&text));
            }
        }
    }

    if let Some(origin) = mods.children().find(|node| node.is_element() && node.tag_name().name() == "originInfo") {
        book.publisher = child_texts(origin, "publisher")
            .first()
            .cloned()
            .or_else(|| {
                origin
                    .descendants()
                    .find(|node| node.is_element() && node.tag_name().name() == "namePart")
                    .and_then(|node| node.text())
                    .map(clean)
            })
            .unwrap_or_default();
        book.places = origin
            .descendants()
            .filter(|node| node.is_element() && node.tag_name().name() == "placeTerm")
            .filter(|node| node.attribute("type") != Some("code"))
            .filter_map(|node| node.text())
            .map(clean)
            .filter(|place| !place.is_empty())
            .collect();
        book.year = origin
            .children()
            .find(|node| node.is_element() && node.tag_name().name() == "dateIssued")
            .and_then(|node| node.text())
            .map(year_from)
            .unwrap_or_default();
    }

    book.pages = mods
        .descendants()
        .find(|node| node.is_element() && node.tag_name().name() == "extent")
        .and_then(|node| node.text())
        .and_then(pages_from);
    book.language = mods
        .descendants()
        .find(|node| node.is_element() && node.tag_name().name() == "languageTerm")
        .and_then(|node| node.text())
        .and_then(language_code);
    book.ids.isbn13 = Some(isbn.isbn13_hyphenated());
    book.ids.isbn10 = isbn.isbn10_hyphenated();
    Ok(book.finish().into_iter().collect())
}

fn child_texts(node: Node<'_, '_>, name: &str) -> Vec<String> {
    node.children()
        .filter(|child| child.is_element() && child.tag_name().name() == name)
        .filter_map(|child| child.text())
        .map(clean)
        .filter(|text| !text.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<mods:mods xmlns:mods="http://www.loc.gov/mods/v3">
  <mods:titleInfo>
    <mods:title>Fantastic Mr. Fox</mods:title>
    <mods:subTitle>a story</mods:subTitle>
  </mods:titleInfo>
  <mods:name usage="primary">
    <mods:namePart>Dahl, Roald</mods:namePart>
  </mods:name>
  <mods:originInfo>
    <mods:place><mods:placeTerm type="text">London</mods:placeTerm></mods:place>
    <mods:publisher>Puffin</mods:publisher>
    <mods:dateIssued>1988</mods:dateIssued>
  </mods:originInfo>
  <mods:language><mods:languageTerm type="code">eng</mods:languageTerm></mods:language>
</mods:mods>"#;

    #[test]
    fn parses_mods() {
        let isbn = Isbn::parse("9780140328721").unwrap();
        let books = parse(FIXTURE, &isbn).unwrap();
        let book = &books[0];
        assert_eq!(book.title, "Fantastic Mr. Fox");
        assert_eq!(book.subtitle.as_deref(), Some("a story"));
        assert_eq!(book.authors, ["Roald Dahl"]);
        assert_eq!(book.publisher, "Puffin");
        assert_eq!(book.places, ["London"]);
        assert_eq!(book.year, "1988");
        assert_eq!(book.language.as_deref(), Some("en"));
    }
}
