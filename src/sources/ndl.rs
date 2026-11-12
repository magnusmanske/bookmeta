//! National Diet Library of Japan, via its SRU service. The records are
//! DCNDL, an RDF flavour of Dublin Core.

use super::{FetchOptions, get_text};
use crate::util::{clean, language_code, natural_name, pages_from_extent, year_from};
use crate::{Book, BookError, Isbn, Source};
use reqwest::Client;
use roxmltree::{Document, Node};

const URL: &str = "https://ndlsearch.ndl.go.jp/api/sru";
const DC: &str = "http://purl.org/dc/elements/1.1/";
const DCTERMS: &str = "http://purl.org/dc/terms/";
const DCNDL: &str = "http://ndl.go.jp/dcndl/terms/";
const FOAF: &str = "http://xmlns.com/foaf/0.1/";
const RDF: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";

pub(crate) async fn fetch(
    client: &Client,
    isbn: &Isbn,
    options: &FetchOptions<'_>,
) -> Result<Vec<Book>, BookError> {
    // `recordPacking=xml` keeps the RDF from being escaped into a string.
    let query = [
        ("operation", "searchRetrieve".to_string()),
        ("version", "1.2".to_string()),
        ("recordSchema", "dcndl".to_string()),
        ("recordPacking", "xml".to_string()),
        ("maximumRecords", "3".to_string()),
        ("query", format!("isbn={}", isbn.isbn13_digits())),
    ];
    let xml = get_text(client, URL, &query, options.timeout).await?;
    parse(&xml, isbn)
}

fn parse(xml: &str, isbn: &Isbn) -> Result<Vec<Book>, BookError> {
    let document = Document::parse(xml)?;
    // Several records can name the same ISBN: the edition itself, other
    // printings that list only its ISBN-10, and repositories' holdings.
    let mut exact = vec![];
    let mut by_isbn10 = vec![];
    for data in document
        .descendants()
        .filter(|node| node.is_element() && node.tag_name().name() == "recordData")
    {
        // A record holds a bibliographic resource and, for some repositories,
        // the holdings of each library.
        let Some(bib) = data.descendants().find(|node| {
            node.is_element()
                && node.tag_name().name() == "BibResource"
                && node.tag_name().namespace() == Some(DCNDL)
                && node
                    .children()
                    .any(|child| child.is_element() && child.tag_name().name() == "title")
        }) else {
            continue;
        };
        if let Some(book) = from_bib(data, bib, isbn) {
            if names_isbn13(bib, isbn) {
                exact.push(book);
            } else {
                by_isbn10.push(book);
            }
        }
    }
    Ok(if exact.is_empty() { by_isbn10 } else { exact })
}

/// Whether the record names this ISBN-13 itself, not merely its ISBN-10 form.
fn names_isbn13(bib: Node<'_, '_>, isbn: &Isbn) -> bool {
    let isbn13 = isbn.isbn13_digits();
    texts(bib, DCTERMS, "identifier")
        .iter()
        .any(|value| value.chars().filter(char::is_ascii_digit).collect::<String>() == isbn13)
}

fn from_bib(data: Node<'_, '_>, bib: Node<'_, '_>, isbn: &Isbn) -> Option<Book> {
    // The ISBN search also returns neighbouring editions.
    let identifiers = texts(bib, DCTERMS, "identifier");
    if !identifiers.iter().any(|value| isbn.matches(value)) {
        return None;
    }

    let mut book = Book::new(Source::Ndl);
    book.title = texts(bib, DCTERMS, "title")
        .into_iter()
        .next()
        .unwrap_or_default();
    book.authors = authors(bib);
    book.publisher = agents(bib, "publisher")
        .into_iter()
        .next()
        .or_else(|| texts(bib, DC, "publisher").into_iter().next())
        .unwrap_or_default();
    let date = texts(bib, DCTERMS, "date")
        .into_iter()
        .next()
        .or_else(|| texts(bib, DCTERMS, "issued").into_iter().next());
    book.year = date.map(|date| year_from(&date)).unwrap_or_default();
    book.language = texts(bib, DCTERMS, "language")
        .iter()
        .find_map(|code| language_code(code));
    book.pages = texts(bib, DCTERMS, "extent")
        .iter()
        .find_map(|extent| pages_from_extent(extent));
    // The NII bibliographic ID is on the bibliographic resource or, for newer
    // records, on the item.
    book.ids.ndl = identifier(data, "NIIBibID").or_else(|| identifier(data, "NDLBibID"));
    book.ids.isbn13 = Some(isbn.isbn13_hyphenated());
    book.ids.isbn10 = isbn.isbn10_hyphenated();
    book.finish()
}

fn authors(bib: Node<'_, '_>) -> Vec<String> {
    let names: Vec<String> = agents(bib, "creator");
    if names.is_empty() {
        // `dc:creator` is plain text with a trailing role, e.g. `夏目漱石著`.
        return texts(bib, DC, "creator")
            .iter()
            .map(|name| person_name(strip_role(name)))
            .filter(|name| !name.is_empty())
            .collect();
    }
    names
        .iter()
        .map(|name| person_name(name))
        .filter(|name| !name.is_empty())
        .collect()
}

/// The name of each `dcterms:creator` or `dcterms:publisher`, which wrap it in
/// a FOAF agent.
fn agents(node: Node<'_, '_>, name: &str) -> Vec<String> {
    node.children()
        .filter(|child| {
            child.is_element()
                && child.tag_name().name() == name
                && child.tag_name().namespace() == Some(DCTERMS)
        })
        .filter_map(|agent| {
            let text = agent
                .descendants()
                .find(|child| {
                    child.is_element()
                        && child.tag_name().name() == "name"
                        && child.tag_name().namespace() == Some(FOAF)
                })
                .and_then(|child| child.text())
                .or_else(|| agent.text())?;
            Some(clean(text))
        })
        .filter(|value| !value.is_empty())
        .collect()
}

/// The value of a `dcterms:identifier` of the given NDL datatype, such as the
/// `NIIBibID`.
fn identifier(node: Node<'_, '_>, kind: &str) -> Option<String> {
    node.descendants()
        .filter(|child| {
            child.is_element()
                && child.tag_name().name() == "identifier"
                && child.tag_name().namespace() == Some(DCTERMS)
        })
        .find(|child| {
            child
                .attribute((RDF, "datatype"))
                .is_some_and(|datatype| datatype.ends_with(kind))
        })
        .and_then(|child| child.text())
        .map(clean)
        .filter(|id| !id.is_empty())
}

/// NDL writes Japanese names family name first, so only Western names are
/// reordered.
fn person_name(value: &str) -> String {
    let value = without_life_dates(value);
    if value.chars().any(is_cjk) {
        value
            .replace(',', " ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    } else {
        natural_name(&value)
    }
}

/// `井上, 夢人, 1950-` as `井上, 夢人`.
fn without_life_dates(value: &str) -> String {
    let mut parts: Vec<&str> = value.split(',').map(str::trim).collect();
    while parts.len() > 1 && parts.last().is_some_and(|part| is_life_dates(part)) {
        parts.pop();
    }
    parts.join(", ")
}

fn is_life_dates(part: &str) -> bool {
    !part.is_empty() && part.chars().all(|character| character.is_ascii_digit() || character == '-')
}

fn is_cjk(character: char) -> bool {
    matches!(
        character as u32,
        0x3000..=0x30FF | 0x31F0..=0x31FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0xFF00..=0xFFEF
    )
}

/// `夏目漱石著` as `夏目漱石`.
fn strip_role(value: &str) -> &str {
    value.trim_end_matches([
        '著', '編', '訳', '監', '修', '原', '述', '画', ' ', '\u{3000}',
    ])
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
<searchRetrieveResponse xmlns="http://www.loc.gov/zing/srw/">
  <version>1.2</version>
  <numberOfRecords>1</numberOfRecords>
  <records><record>
    <recordSchema>dcndl</recordSchema>
    <recordPacking>xml</recordPacking>
    <recordData>
      <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
          xmlns:dc="http://purl.org/dc/elements/1.1/"
          xmlns:dcterms="http://purl.org/dc/terms/"
          xmlns:dcndl="http://ndl.go.jp/dcndl/terms/"
          xmlns:foaf="http://xmlns.com/foaf/0.1/">
        <dcndl:BibResource rdf:about="https://ndlsearch.ndl.go.jp/books/R100000136-I1#material">
          <dcterms:identifier rdf:datatype="http://ndl.go.jp/dcndl/terms/NIIBibID">BA6465493X</dcterms:identifier>
          <dcterms:identifier rdf:datatype="http://ndl.go.jp/dcndl/terms/ISBN">9784101010014</dcterms:identifier>
          <dcterms:title>吾輩は猫である</dcterms:title>
          <dcterms:creator>
            <foaf:Agent><foaf:name>夏目, 漱石</foaf:name></foaf:Agent>
          </dcterms:creator>
          <dc:creator>夏目漱石著</dc:creator>
          <dcterms:publisher>
            <foaf:Agent><foaf:name>新潮社</foaf:name></foaf:Agent>
          </dcterms:publisher>
          <dcterms:date>2003.6</dcterms:date>
          <dcterms:issued rdf:datatype="http://purl.org/dc/terms/W3CDTF">2003</dcterms:issued>
          <dcterms:language rdf:datatype="http://purl.org/dc/terms/ISO639-2">jpn</dcterms:language>
          <dcterms:extent>16cm</dcterms:extent>
        </dcndl:BibResource>
      </rdf:RDF>
    </recordData>
  </record></records>
</searchRetrieveResponse>"#;

    #[test]
    fn parses_dcndl() {
        let isbn = Isbn::parse("9784101010014").unwrap();
        let books = parse(FIXTURE, &isbn).unwrap();
        let book = &books[0];
        assert_eq!(book.title, "吾輩は猫である");
        assert_eq!(book.authors, ["夏目 漱石"]);
        assert_eq!(book.publisher, "新潮社");
        assert_eq!(book.year, "2003");
        assert_eq!(book.language.as_deref(), Some("ja"));
        // `16cm` is a size, not a page count.
        assert_eq!(book.pages, None);
        assert_eq!(book.ids.ndl.as_deref(), Some("BA6465493X"));
    }

    #[test]
    fn a_neighbouring_edition_is_ignored() {
        let isbn = Isbn::parse("9784101010014").unwrap();
        let other = FIXTURE.replace("9784101010014", "9780140328721");
        assert!(parse(&other, &isbn).unwrap().is_empty());
    }

    #[test]
    fn a_record_that_names_only_the_isbn10_is_a_fallback() {
        let isbn = Isbn::parse("9784101010014").unwrap();
        // The same record twice: once with the ISBN-13, once with the ISBN-10.
        let isbn10_only = FIXTURE.replace("9784101010014", "4101010013");
        let both = isbn10_only.replace(
            "</record></records>",
            &format!(
                "</record><record><recordData>{}</recordData></record></records>",
                &FIXTURE[FIXTURE.find("<recordData>").unwrap() + 12..FIXTURE.find("</recordData>").unwrap()]
            ),
        );
        let books = parse(&both, &isbn).unwrap();
        assert_eq!(books.len(), 1);
    }

    #[test]
    fn page_counts_from_an_extent() {
        let isbn = Isbn::parse("9784101010014").unwrap();
        let with_pages = FIXTURE.replace("<dcterms:extent>16cm", "<dcterms:extent>610p ; 16cm");
        assert_eq!(parse(&with_pages, &isbn).unwrap()[0].pages, Some(610));
    }

    #[test]
    fn names() {
        assert_eq!(person_name("夏目, 漱石"), "夏目 漱石");
        assert_eq!(person_name("井上, 夢人, 1950-"), "井上 夢人");
        assert_eq!(person_name("Mansfield, Katherine, 1888-1923"), "Katherine Mansfield");
        assert_eq!(person_name("Mansfield, Katherine"), "Katherine Mansfield");
        assert_eq!(strip_role("夏目漱石著"), "夏目漱石");
    }

    #[test]
    fn the_ndl_id_falls_back_to_the_ndl_bib_id() {
        let isbn = Isbn::parse("9784101010014").unwrap();
        let record = FIXTURE.replace("terms/NIIBibID", "terms/NDLBibID").replace("BA6465493X", "000007485841");
        let books = parse(&record, &isbn).unwrap();
        assert_eq!(books[0].ids.ndl.as_deref(), Some("000007485841"));
    }
}
