//! Book metadata by ISBN, from several public catalogues.
//!
//! The crate is deliberately independent of any web framework or wiki: it
//! returns plain [`Book`] records, which a caller maps to wiki markup or to
//! Wikidata statements.
//!
//! ```no_run
//! # async fn run() -> Result<(), bookmeta::BookError> {
//! let sources = bookmeta::BookSources::with_default_client("my-tool/1.0")?;
//! let isbn = bookmeta::Isbn::parse("978-3-423-30666-9")?;
//! let lookup = sources.lookup(&isbn).await;
//! for book in &lookup.books {
//!     println!("{}: {}", book.source.label(), book.display_title());
//! }
//! # Ok(())
//! # }
//! ```

mod book;
mod error;
mod isbn;
pub mod sources;
mod util;

pub use book::{Book, BookIds, Source, SourceFailure};
pub use error::BookError;
pub use isbn::Isbn;
pub use sources::{BookSources, Lookup, WikidataItem};
