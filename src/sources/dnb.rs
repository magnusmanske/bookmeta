//! Deutsche Nationalbibliothek, via its SRU service.

use super::{FetchOptions, get_text};
use crate::{Book, BookError, Isbn, Source};
use reqwest::Client;

const URL: &str = "https://services.dnb.de/sru/dnb";

pub(crate) async fn fetch(client: &Client, isbn: &Isbn, options: &FetchOptions<'_>) -> Result<Vec<Book>, BookError> {
    let query = [
        ("version", "1.1".to_string()),
        ("operation", "searchRetrieve".to_string()),
        ("recordSchema", "MARC21-xml".to_string()),
        ("maximumRecords", "1".to_string()),
        ("query", format!("num={}", isbn.isbn13_digits())),
    ];
    let xml = get_text(client, URL, &query, options.timeout).await?;
    super::marc::parse(&xml, Source::Dnb, isbn)
}
