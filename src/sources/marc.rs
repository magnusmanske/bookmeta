//! MARCXML records, as DNB, K10plus and BNE return them.

use crate::util::{clean, language_code, natural_name, pages_from, year_from};
use crate::{Book, BookError, Source};
use roxmltree::Node;

/// Parse the first bibliographic MARCXML record in an SRU response.
pub(crate) fn parse(xml: &str, source: Source, isbn: &crate::Isbn) -> Result<Vec<Book>, BookError> {
    let document = roxmltree::Document::parse(xml)?;
    let Some(record) = document.descendants().find(|node| {
        node.is_element()
            && node.tag_name().name() == "record"
            && node.children().any(|child| {
                child.is_element() && matches!(child.tag_name().name(), "leader" | "datafield" | "controlfield")
            })
    }) else {
        return Ok(vec![]);
    };

    let mut book = Book::new(source);
    let control_id = record
        .children()
        .find(|node| {
            node.is_element() && node.tag_name().name() == "controlfield" && node.attribute("tag") == Some("001")
        })
        .and_then(|node| node.text())
        .map(clean)
        .filter(|id| !id.is_empty());
    // The fixed field records the language of the item; many BNE records have
    // no 041 field.
    let fixed_language = record
        .children()
        .find(|node| {
            node.is_element() && node.tag_name().name() == "controlfield" && node.attribute("tag") == Some("008")
        })
        .and_then(|node| node.text())
        .and_then(|text| text.get(35..38))
        .and_then(language_code);

    for field in record.children().filter(|node| node.is_element() && node.tag_name().name() == "datafield") {
        match field.attribute("tag").unwrap_or_default() {
            "035" => {
                for value in subfields(field, "a") {
                    if let Some(oclc) = value.strip_prefix("(OCoLC)") {
                        book.ids.oclc.get_or_insert_with(|| clean(oclc));
                    }
                }
            }
            "041" => {
                if let Some(language) = subfields(field, "a").first().and_then(|code| language_code(code)) {
                    book.language.get_or_insert(language);
                }
            }
            "100" | "110" => {
                if let Some(author) = primary_name(field, field.attribute("tag") == Some("110")) {
                    book.authors.push(author);
                }
            }
            "245" => {
                book.title = subfields(field, "a").first().cloned().unwrap_or_default();
                book.subtitle = subfields(field, "b").first().cloned().filter(|subtitle| !subtitle.is_empty());
            }
            "260" | "264" => {
                // 264 with ind2=1 is publication; other 264 fields are
                // copyright or production statements.
                if book.publisher.is_empty() || field.attribute("ind2") == Some("1") {
                    if let Some(publisher) = subfields(field, "b").first().map(|value| clean(value)) {
                        book.publisher = publisher;
                    }
                    if book.places.is_empty() {
                        book.places = subfields(field, "a");
                    }
                    if book.year.is_empty()
                        && let Some(year) = subfields(field, "c").first().map(|value| year_from(value))
                    {
                        book.year = year;
                    }
                }
            }
            "300" => {
                if let Some(pages) = subfields(field, "a").first().and_then(|value| pages_from(value)) {
                    book.pages.get_or_insert(pages);
                }
            }
            "700" | "710" => {
                if is_author(field)
                    && let Some(author) = primary_name(field, field.attribute("tag") == Some("710"))
                {
                    book.authors.push(author);
                }
            }
            _ => {}
        }
    }

    if book.language.is_none() {
        book.language = fixed_language;
    }

    // The record was found by this ISBN; it may list other editions too.
    book.ids.isbn13 = Some(isbn.isbn13_hyphenated());
    book.ids.isbn10 = isbn.isbn10_hyphenated();
    match source {
        Source::Dnb => book.ids.dnb = control_id,
        Source::K10plus => book.ids.k10plus = control_id,
        Source::Bne => book.ids.bne = control_id,
        _ => {}
    }
    Ok(book.finish().into_iter().collect())
}

fn subfields(field: Node<'_, '_>, code: &str) -> Vec<String> {
    field
        .children()
        .filter(|node| {
            node.is_element() && node.tag_name().name() == "subfield" && node.attribute("code") == Some(code)
        })
        .filter_map(|node| node.text())
        .map(clean)
        .filter(|value| !value.is_empty())
        .collect()
}

fn primary_name(field: Node<'_, '_>, corporate: bool) -> Option<String> {
    let name = subfields(field, "a").into_iter().next()?;
    Some(if corporate { name } else { natural_name(&name) })
}

/// `700` fields hold translators and illustrators too; only keep authors.
fn is_author(field: Node<'_, '_>) -> bool {
    let roles = subfields(field, "4");
    if roles.is_empty() {
        return true;
    }
    roles.iter().any(|role| {
        let role = role.to_lowercase();
        role.contains("aut") || role.contains("verfasser") || role.contains("autor") || role.contains("author")
    })
}
