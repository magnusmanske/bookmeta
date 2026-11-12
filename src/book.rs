use crate::util::clean;

/// Where a book record came from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Source {
    #[default]
    GoogleBooks,
    Dnb,
    K10plus,
    Loc,
    OpenLibrary,
    Bnf,
    Bne,
    Crossref,
    Goodreads,
    Wikidata,
    /// The merged entry, which no source fetch produces.
    Merged,
}

impl Source {
    /// The sources that can be queried. [`Source::Merged`] is not one.
    pub const FETCHABLE: [Source; 10] = [
        Source::GoogleBooks,
        Source::Dnb,
        Source::K10plus,
        Source::Loc,
        Source::OpenLibrary,
        Source::Bnf,
        Source::Bne,
        Source::Crossref,
        Source::Goodreads,
        Source::Wikidata,
    ];

    /// The label shown beside a result.
    pub fn label(self) -> &'static str {
        match self {
            Source::GoogleBooks => "Google Books",
            Source::Dnb => "DNB",
            Source::K10plus => "K10plus",
            Source::Loc => "Library of Congress",
            Source::OpenLibrary => "Open Library",
            Source::Bnf => "BnF",
            Source::Bne => "BNE",
            Source::Crossref => "Crossref",
            Source::Goodreads => "Goodreads",
            Source::Wikidata => "Wikidata",
            Source::Merged => "Merged entry",
        }
    }

    /// A short label for logs.
    pub fn short(self) -> &'static str {
        match self {
            Source::GoogleBooks => "google",
            Source::Dnb => "dnb",
            Source::K10plus => "k10plus",
            Source::Loc => "loc",
            Source::OpenLibrary => "openlibrary",
            Source::Bnf => "bnf",
            Source::Bne => "bne",
            Source::Crossref => "crossref",
            Source::Goodreads => "goodreads",
            Source::Wikidata => "wikidata",
            Source::Merged => "merged",
        }
    }

    /// Preference when several records are equally complete: national
    /// library data before commercial or community data.
    pub fn priority(self) -> u8 {
        match self {
            Source::Dnb => 90,
            Source::K10plus => 80,
            Source::Bnf => 70,
            Source::Bne => 68,
            Source::Loc => 60,
            Source::OpenLibrary => 50,
            Source::GoogleBooks => 40,
            Source::Crossref => 30,
            // Community-edited, and often an edition other than the one asked for.
            Source::Goodreads => 20,
            Source::Wikidata | Source::Merged => 0,
        }
    }
}

/// Identifiers a source provided for the edition.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BookIds {
    pub isbn10: Option<String>,
    pub isbn13: Option<String>,
    pub google_books: Option<String>,
    pub open_library: Option<String>,
    pub dnb: Option<String>,
    pub loc: Option<String>,
    pub k10plus: Option<String>,
    pub bnf: Option<String>,
    pub bne: Option<String>,
    pub crossref_doi: Option<String>,
    /// The numeric Goodreads book ID.
    pub goodreads: Option<String>,
    pub oclc: Option<String>,
}

impl BookIds {
    fn fill_from(&mut self, other: &Self) {
        macro_rules! fill {
            ($($field:ident),+ $(,)?) => {
                $(if self.$field.is_none() { self.$field = other.$field.clone(); })+
            };
        }
        fill!(isbn10, isbn13, google_books, open_library, dnb, loc, k10plus, bnf, bne, crossref_doi, goodreads, oclc);
    }
}

/// One catalogue's record for an edition.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Book {
    pub source: Source,
    pub authors: Vec<String>,
    pub title: String,
    pub subtitle: Option<String>,
    pub publisher: String,
    pub places: Vec<String>,
    pub year: String,
    pub language: Option<String>,
    pub pages: Option<u32>,
    pub ids: BookIds,
}

impl Book {
    pub fn new(source: Source) -> Self {
        Self { source, ..Default::default() }
    }

    /// Title and subtitle, as one citation line.
    pub fn display_title(&self) -> String {
        match self.subtitle.as_deref().map(clean) {
            Some(subtitle) if !subtitle.is_empty() => format!("{} : {subtitle}", clean(&self.title)),
            _ => clean(&self.title),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.title.trim().is_empty()
    }

    /// How many useful fields are present; with [`Source::priority`], decides
    /// which record a merged entry starts from.
    pub fn completeness(&self) -> u8 {
        let mut score = 0;
        score += u8::from(!self.title.trim().is_empty()) * 4;
        score += u8::from(!self.authors.is_empty()) * 3;
        score += u8::from(!self.publisher.trim().is_empty()) * 2;
        score += u8::from(!self.places.is_empty());
        score += u8::from(!self.year.trim().is_empty());
        score += u8::from(self.pages.is_some());
        score
    }

    /// Merge several records: start from the most complete one, then fill
    /// only the fields it lacks. Each original record stays visible to the
    /// caller, so conflicting values are not hidden.
    pub fn merge(books: &[Book]) -> Option<Self> {
        let base = books.iter().max_by_key(|book| (book.completeness(), book.source.priority()))?;
        let mut merged = base.clone();
        merged.source = Source::Merged;
        for book in books {
            if merged.authors.is_empty() {
                merged.authors = book.authors.clone();
            }
            if merged.title.trim().is_empty() {
                merged.title = book.title.clone();
            }
            if merged.subtitle.is_none() {
                merged.subtitle = book.subtitle.clone();
            }
            if merged.publisher.trim().is_empty() {
                merged.publisher = book.publisher.clone();
            }
            if merged.places.is_empty() {
                merged.places = book.places.clone();
            }
            if merged.year.trim().is_empty() {
                merged.year = book.year.clone();
            }
            if merged.language.is_none() {
                merged.language = book.language.clone();
            }
            if merged.pages.is_none() {
                merged.pages = book.pages;
            }
            merged.ids.fill_from(&book.ids);
        }
        Some(merged)
    }

    pub(crate) fn finish(mut self) -> Option<Self> {
        self.authors = clean_list(self.authors);
        self.places = clean_list(self.places);
        self.title = clean(&self.title);
        self.subtitle = self.subtitle.map(|subtitle| clean(&subtitle)).filter(|subtitle| !subtitle.is_empty());
        self.publisher = clean(&self.publisher);
        self.year = clean(&self.year);
        self.pages = self.pages.filter(|pages| *pages > 0);
        if self.is_empty() { None } else { Some(self) }
    }
}

fn clean_list(values: Vec<String>) -> Vec<String> {
    let mut ret = vec![];
    for value in values {
        let value = clean(&value);
        if !value.is_empty() && !ret.contains(&value) {
            ret.push(value);
        }
    }
    ret
}

/// A source that did not answer, or answered with data that could not be parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFailure {
    pub source: Source,
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_pages_are_ignored() {
        let mut book = Book::new(Source::Dnb);
        book.title = "A title".to_string();
        book.pages = Some(0);
        assert_eq!(book.finish().map(|book| book.pages), Some(None));
    }
}
