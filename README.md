# bookmeta

Book metadata by ISBN, from several public catalogues, as plain Rust records.
No web framework or wiki markup: callers map the [`Book`] to a citation, a
Wikidata statement, or anything else.

## Sources

| Source | Service | What it usually provides |
|---|---|---|
| Google Books | legacy GData feed, or the v1 API with a key | title, authors, publisher, year, pages, language |
| DNB | SRU, MARCXML | title, authors, publisher, place, year, pages, language, DNB ID |
| K10plus | SRU, MARCXML | title, authors, publisher, place, year, pages, language, K10plus ID |
| Library of Congress | SRU, MODS | title, authors, publisher, place, year, pages, language |
| Open Library | `jscmd=data`, edition JSON fallback | title, authors, publisher, place, year, pages, Open Library ID |
| BnF | SRU, Dublin Core | title, authors, publisher, place, year, pages, language, BnF ID |
| BNE | Alma SRU, MARCXML | title, authors, publisher, place, year, pages, language, BNE ID |
| KB | SRU, Dublin Core | title, authors, publisher, place, year, pages, language, PPN |
| Crossref | REST API, `type:book` | title, publisher, place, year, DOI, language |
| Goodreads | `book/isbn/<isbn13>`, embedded `__NEXT_DATA__` JSON | title, authors, publisher, year, pages, language, Goodreads ID |
| Wikidata | SPARQL, `P212`/`P957` | existing items for the ISBN |

All sources are queried concurrently. One failing source does not discard the
others' records; [`Lookup::failures`] says which did not answer.

## Use

```rust
use bookmeta::{BookSources, Isbn};

# async fn example() -> Result<(), bookmeta::BookError> {
let sources = BookSources::with_default_client("my-tool/1.0")?;
let isbn = Isbn::parse("978-3-423-30666-9")?;
let lookup = sources.lookup(&isbn).await;

for book in &lookup.books {
    println!("{}: {}", book.source.label(), book.display_title());
}
if let Some(merged) = lookup.merged() {
    println!("merged: {}", merged.display_title());
}
# Ok(())
# }
```

`Isbn` validates the check digit and converts between ISBN-10 and ISBN-13. The
sources use the digits-only ISBN-13; the display form keeps what the user
typed (without `ISBN` or other text).

### Goodreads

Goodreads retired its public API in 2020, so the source reads the book page
that `https://www.goodreads.com/book/isbn/<isbn13>` redirects to. The page is
a Next.js app that embeds the catalogue record as JSON in a `__NEXT_DATA__`
script tag; that JSON is read directly, without an HTML parser. Scraping is
inherently more brittle than an API, and the ISBN-10 form of the route answers
`404` for books the ISBN-13 form finds, so only the ISBN-13 is queried. Enable
it like any other source; drop it with `.without(bookmeta::Source::Goodreads)`
if the page layout changes.

### Google Books API key

Without a key, the legacy GData feed is used. The v1 API needs a key, because
keyless requests share a quota and frequently answer `429`:

```rust
let sources = BookSources::with_default_client("my-tool/1.0")?
    .with_google_books_api_key(std::env::var("GOOGLE_BOOKS_API_KEY").ok());
```

### Disabling a source

Library of Congress' ISBN index is unreliable; an empty answer is treated as
"no result". To skip a source entirely:

```rust
let sources = BookSources::with_default_client("my-tool/1.0")?
    .without(bookmeta::Source::Loc);
// or
let sources = sources.without_all([bookmeta::Source::Loc, bookmeta::Source::Crossref]);
```

### Merging

`Book::merge` starts from the most complete record (national library data
wins ties), then fills only fields that record lacks. The original records
stay available, so conflicting values are not hidden.

## License

MIT OR Apache-2.0.
