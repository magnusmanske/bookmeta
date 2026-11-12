/// What can go wrong when one source is queried.
///
/// A failure of one source is not fatal: [`crate::BookSources::lookup`]
/// collects the failures and returns the books the other sources found.
#[derive(Debug, thiserror::Error)]
pub enum BookError {
    #[error("invalid ISBN")]
    InvalidIsbn,
    #[error("invalid ISBN: {0}")]
    Isbn(String),
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("XML error: {0}")]
    Xml(#[from] roxmltree::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("unexpected data: {0}")]
    Data(String),
    #[error("response too large")]
    TooLarge,
    #[error("source is disabled")]
    Disabled,
}

impl From<isbn::IsbnError> for BookError {
    fn from(error: isbn::IsbnError) -> Self {
        Self::Isbn(error.to_string())
    }
}
