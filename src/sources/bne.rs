//! Biblioteca Nacional de España, via the Alma SRU service of its catalogue.

use super::{FetchOptions, get_text};
use crate::{Book, BookError, Isbn, Source};
use reqwest::Client;

/// The BNE's Alma instance, which also serves the catalogue.
const URL: &str = "https://catalogo.bne.es/view/sru/34BNE_INST";

pub(crate) async fn fetch(
    client: &Client,
    isbn: &Isbn,
    options: &FetchOptions<'_>,
) -> Result<Vec<Book>, BookError> {
    let query = [
        ("operation", "searchRetrieve".to_string()),
        ("version", "1.2".to_string()),
        ("recordSchema", "marcxml".to_string()),
        ("maximumRecords", "1".to_string()),
        ("query", format!("alma.isbn={}", isbn.isbn13_digits())),
    ];
    let xml = get_text(client, URL, &query, options.timeout).await?;
    super::marc::parse(&xml, Source::Bne, isbn)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="no"?>
<searchRetrieveResponse xmlns="http://www.loc.gov/zing/srw/">
  <version>1.2</version>
  <numberOfRecords>1</numberOfRecords>
  <records><record>
    <recordSchema>marcxml</recordSchema>
    <recordPacking>xml</recordPacking>
    <recordData>
      <record xmlns="http://www.loc.gov/MARC21/slim">
        <leader>00000cam a2200000 i 4500</leader>
        <controlfield tag="001">991053211349708606</controlfield>
        <controlfield tag="003">SpMaBN</controlfield>
        <controlfield tag="005">20251210131353.0</controlfield>
        <controlfield tag="008">151007s2015    sp a   | |||| 000 f spa^^</controlfield>
        <datafield ind1=" " ind2=" " tag="020">
          <subfield code="a">978-84-204-1214-6</subfield>
        </datafield>
        <datafield ind1=" " ind2=" " tag="035">
          <subfield code="a">(OCoLC)927798880</subfield>
        </datafield>
        <datafield ind1="1" ind2=" " tag="100">
          <subfield code="a">Cervantes Saavedra, Miguel de</subfield>
          <subfield code="d">1547-1616</subfield>
          <subfield code="4">aut</subfield>
        </datafield>
        <datafield ind1="1" ind2="0" tag="245">
          <subfield code="a">Don Quijote de la Mancha :</subfield>
          <subfield code="b">edición conmemorativa IV centenario Cervantes</subfield>
          <subfield code="c">Miguel de Cervantes</subfield>
        </datafield>
        <datafield ind1=" " ind2=" " tag="250">
          <subfield code="a">2ª ed.</subfield>
        </datafield>
        <datafield ind1=" " ind2=" " tag="260">
          <subfield code="a">Madrid</subfield>
          <subfield code="b">Real Academia Española</subfield>
          <subfield code="b">Asociación de Academias de la Lengua Española</subfield>
          <subfield code="c">2015</subfield>
        </datafield>
        <datafield ind1=" " ind2=" " tag="300">
          <subfield code="a">CXXI, 1249 p.</subfield>
          <subfield code="b">il.</subfield>
          <subfield code="c">21 cm</subfield>
        </datafield>
      </record>
    </recordData>
  </record></records>
</searchRetrieveResponse>"#;

    #[test]
    fn parses_the_marcxml_record() {
        let isbn = Isbn::parse("9788420412146").unwrap();
        let books = super::super::marc::parse(FIXTURE, Source::Bne, &isbn).unwrap();
        let book = &books[0];
        assert_eq!(book.title, "Don Quijote de la Mancha");
        assert_eq!(
            book.subtitle.as_deref(),
            Some("edición conmemorativa IV centenario Cervantes")
        );
        assert_eq!(book.authors, ["Miguel de Cervantes Saavedra"]);
        assert_eq!(book.publisher, "Real Academia Española");
        assert_eq!(book.places, ["Madrid"]);
        assert_eq!(book.year, "2015");
        assert_eq!(book.pages, Some(1249));
        // Taken from the fixed field, as the record has no 041.
        assert_eq!(book.language.as_deref(), Some("es"));
        assert_eq!(book.ids.bne.as_deref(), Some("991053211349708606"));
        assert_eq!(book.ids.oclc.as_deref(), Some("927798880"));
    }

    #[test]
    fn an_empty_answer_has_no_books() {
        let isbn = Isbn::parse("9788420412146").unwrap();
        let empty = r#"<?xml version="1.0" encoding="UTF-8"?>
<searchRetrieveResponse xmlns="http://www.loc.gov/zing/srw/">
  <numberOfRecords>0</numberOfRecords>
  <records></records>
</searchRetrieveResponse>"#;
        assert!(
            super::super::marc::parse(empty, Source::Bne, &isbn)
                .unwrap()
                .is_empty()
        );
    }
}
