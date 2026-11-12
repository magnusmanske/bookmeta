//! K10plus, the German union catalogue, via its SRU service.

use super::{FetchOptions, get_text};
use crate::{Book, BookError, Isbn, Source};
use reqwest::Client;

const URL: &str = "https://sru.k10plus.de/gvk";

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
        ("recordSchema", "marcxml".to_string()),
        ("maximumRecords", "1".to_string()),
        ("query", format!("pica.isb={value}")),
    ];
    let xml = get_text(client, URL, &query, options.timeout).await?;
    super::marc::parse(&xml, Source::K10plus, isbn)
}
