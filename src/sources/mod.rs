//! The sources and the entry point for a lookup.

mod bnf;
mod crossref;
mod dnb;
mod google_books;
mod k10plus;
mod loc;
mod marc;
mod open_library;
mod wikidata;

use crate::{Book, BookError, Isbn, Source, SourceFailure};
use reqwest::Client;
use serde::de::DeserializeOwned;
use std::collections::BTreeSet;
use std::time::Duration;

/// Bodies are capped; a single SRU record is a few kilobytes.
const MAX_BODY: usize = 2 * 1024 * 1024;
/// A single source should not hold the page up for long.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// One source's answer: books, or Wikidata items.
pub(crate) struct Fetched {
    pub books: Vec<Book>,
    pub wikidata: Vec<WikidataItem>,
}

#[derive(Clone, Copy)]
pub(crate) struct FetchOptions<'a> {
    pub google_books_api_key: Option<&'a str>,
    pub timeout: Duration,
}

/// A Wikidata item that has the requested ISBN.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WikidataItem {
    pub id: String,
    pub label: Option<String>,
}

impl WikidataItem {
    pub fn url(&self) -> String {
        format!("https://www.wikidata.org/wiki/{}", self.id)
    }
}

/// The result of a lookup. A source that failed is in `failures`; the other
/// sources' books are still returned.
#[derive(Debug, Clone)]
pub struct Lookup {
    pub isbn: Isbn,
    pub books: Vec<Book>,
    pub wikidata: Vec<WikidataItem>,
    pub failures: Vec<SourceFailure>,
}

impl Lookup {
    /// The merged entry, when more than one source returned a book. One
    /// record per source is used, so several editions from one catalogue do
    /// not mix.
    pub fn merged(&self) -> Option<Book> {
        let books: Vec<Book> = Source::FETCHABLE
            .into_iter()
            .filter(|source| *source != Source::Wikidata)
            .filter_map(|source| {
                self.books
                    .iter()
                    .filter(|book| book.source == source)
                    .max_by_key(|book| book.completeness())
                    .cloned()
            })
            .collect();
        if books.len() < 2 {
            return None;
        }
        Book::merge(&books)
    }

    /// The books grouped by source, in source order.
    pub fn by_source(&self) -> Vec<(Source, Vec<&Book>)> {
        Source::FETCHABLE
            .into_iter()
            .filter(|source| *source != Source::Wikidata)
            .filter_map(|source| {
                let books: Vec<&Book> = self.books.iter().filter(|book| book.source == source).collect();
                (!books.is_empty()).then_some((source, books))
            })
            .collect()
    }
}

/// Looks books up by ISBN in the enabled sources.
#[derive(Debug, Clone)]
pub struct BookSources {
    client: Client,
    enabled: BTreeSet<Source>,
    google_books_api_key: Option<String>,
    timeout: Duration,
}

impl BookSources {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            enabled: Source::FETCHABLE.into_iter().collect(),
            google_books_api_key: None,
            timeout: DEFAULT_TIMEOUT,
        }
    }

    /// A client with the given User-Agent, as Wikimedia services require.
    pub fn with_default_client(user_agent: &str) -> Result<Self, BookError> {
        let client = Client::builder().user_agent(user_agent).timeout(Duration::from_secs(30)).build()?;
        Ok(Self::new(client))
    }

    /// Set the Google Books API key. Without one, the legacy GData feed is used.
    pub fn with_google_books_api_key(mut self, key: Option<String>) -> Self {
        self.google_books_api_key = key.filter(|key| !key.trim().is_empty());
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn with_source(mut self, source: Source) -> Self {
        self.enabled.insert(source);
        self
    }

    /// Disable one source, e.g. `without(Source::Loc)` while LoC has no
    /// working ISBN index.
    pub fn without(mut self, source: Source) -> Self {
        self.enabled.remove(&source);
        self
    }

    pub fn without_all(mut self, sources: impl IntoIterator<Item = Source>) -> Self {
        for source in sources {
            self.enabled.remove(&source);
        }
        self
    }

    pub fn is_enabled(&self, source: Source) -> bool {
        self.enabled.contains(&source)
    }

    /// Query all enabled sources concurrently. A failing source does not
    /// discard the others' results.
    pub async fn lookup(&self, isbn: &Isbn) -> Lookup {
        let options =
            FetchOptions { google_books_api_key: self.google_books_api_key.as_deref(), timeout: self.timeout };
        let sources: Vec<Source> = self.enabled.iter().copied().collect();
        let results = futures::future::join_all(sources.into_iter().map(|source| async move {
            let result = fetch(source, &self.client, isbn, &options).await;
            (source, result)
        }))
        .await;

        let mut books = vec![];
        let mut wikidata = vec![];
        let mut failures = vec![];
        for (source, result) in results {
            match result {
                Ok(fetched) => {
                    books.extend(fetched.books);
                    wikidata.extend(fetched.wikidata);
                }
                Err(error) => failures.push(SourceFailure { source, message: error.to_string() }),
            }
        }
        Lookup { isbn: isbn.clone(), books, wikidata, failures }
    }
}

async fn fetch(source: Source, client: &Client, isbn: &Isbn, options: &FetchOptions<'_>) -> Result<Fetched, BookError> {
    let books = |books: Vec<Book>| Fetched { books, wikidata: vec![] };
    match source {
        Source::GoogleBooks => google_books::fetch(client, isbn, options).await.map(books),
        Source::Dnb => dnb::fetch(client, isbn, options).await.map(books),
        Source::K10plus => k10plus::fetch(client, isbn, options).await.map(books),
        Source::Loc => loc::fetch(client, isbn, options).await.map(books),
        Source::OpenLibrary => open_library::fetch(client, isbn, options).await.map(books),
        Source::Bnf => bnf::fetch(client, isbn, options).await.map(books),
        Source::Crossref => crossref::fetch(client, isbn, options).await.map(books),
        Source::Wikidata => {
            wikidata::fetch(client, isbn, options).await.map(|wikidata| Fetched { books: vec![], wikidata })
        }
        Source::Merged => Err(BookError::Disabled),
    }
}

/// GET a text response, with query parameters and a per-request timeout.
pub(crate) async fn get_text(
    client: &Client,
    url: &str,
    query: &[(&str, String)],
    timeout: Duration,
) -> Result<String, BookError> {
    get_text_with(client, url, query, timeout, None).await
}

pub(crate) async fn get_text_with(
    client: &Client,
    url: &str,
    query: &[(&str, String)],
    timeout: Duration,
    accept: Option<&str>,
) -> Result<String, BookError> {
    let mut request = client.get(url).query(query).timeout(timeout);
    if let Some(accept) = accept {
        request = request.header(reqwest::header::ACCEPT, accept);
    }
    let response = request.send().await?.error_for_status()?;
    if response.content_length().is_some_and(|length| length > MAX_BODY as u64) {
        return Err(BookError::TooLarge);
    }
    let text = response.text().await?;
    if text.len() > MAX_BODY {
        return Err(BookError::TooLarge);
    }
    Ok(text)
}

pub(crate) async fn get_json<T: DeserializeOwned>(
    client: &Client,
    url: &str,
    query: &[(&str, String)],
    timeout: Duration,
) -> Result<T, BookError> {
    let text = get_text(client, url, query, timeout).await?;
    Ok(serde_json::from_str(&text)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn book(source: Source, title: &str, publisher: &str) -> Book {
        let mut book = Book::new(source);
        book.title = title.to_string();
        book.publisher = publisher.to_string();
        book
    }

    fn lookup(books: Vec<Book>) -> Lookup {
        Lookup {
            isbn: Isbn::parse("9783423306669").unwrap(),
            books,
            wikidata: vec![],
            failures: vec![],
        }
    }

    #[test]
    fn merged_uses_one_record_per_source() {
        let lookup = lookup(vec![
            book(Source::Dnb, "Short", ""),
            book(Source::Dnb, "Long title", "Publisher"),
            book(Source::K10plus, "Other title", ""),
        ]);
        let merged = lookup.merged().unwrap();
        assert_eq!(merged.source, Source::Merged);
        assert_eq!(merged.title, "Long title");
        assert_eq!(merged.publisher, "Publisher");
    }

    #[test]
    fn one_source_alone_is_not_merged() {
        let lookup = lookup(vec![book(Source::Dnb, "One", ""), book(Source::Dnb, "Two", "")]);
        assert!(lookup.merged().is_none());
    }
}
