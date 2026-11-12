use bookmeta::{BookSources, Isbn};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), bookmeta::BookError> {
    let sources = BookSources::with_default_client(
        "bookmeta-live-test/0.1 (https://github.com/magnusmanske/bookmeta)",
    )?;
    for input in std::env::args().skip(1) {
        let isbn = Isbn::parse(&input)?;
        let lookup = sources.lookup(&isbn).await;
        println!("=== {input} ===");
        for book in &lookup.books {
            println!(
                "[{}] title={:?} subtitle={:?} authors={:?} publisher={:?} places={:?} year={:?} lang={:?} pages={:?} ids={:?}",
                book.source.label(), book.title, book.subtitle, book.authors,
                book.publisher, book.places, book.year, book.language, book.pages, book.ids
            );
        }
        for item in &lookup.wikidata {
            println!("[wikidata] {} {:?}", item.id, item.label);
        }
        for failure in &lookup.failures {
            println!("[fail] {}: {}", failure.source.label(), failure.message);
        }
    }
    Ok(())
}
