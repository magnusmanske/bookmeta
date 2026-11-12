//! Wikidata items that already have the ISBN, for linking and deduplication.

use super::{FetchOptions, WikidataItem, get_text_with};
use crate::{BookError, Isbn};
use reqwest::Client;
use serde::Deserialize;
use std::collections::{BTreeSet, HashMap};

const URL: &str = "https://query.wikidata.org/sparql";
const SPARQL_JSON: &str = "application/sparql-results+json";

pub(crate) async fn fetch(
    client: &Client,
    isbn: &Isbn,
    options: &FetchOptions<'_>,
) -> Result<Vec<WikidataItem>, BookError> {
    let query = query(isbn);
    let text = get_text_with(
        client,
        URL,
        &[("query", query), ("format", "json".to_string())],
        options.timeout,
        Some(SPARQL_JSON),
    )
    .await?;
    let response: SparqlResponse = serde_json::from_str(&text)?;
    let mut seen = BTreeSet::new();
    let mut items = vec![];
    for binding in response.results.bindings {
        let Some(item) = binding.get("item").and_then(|value| value.value.rsplit('/').next()) else {
            continue;
        };
        if !seen.insert(item.to_string()) {
            continue;
        }
        let label =
            binding.get("itemLabel").map(|value| value.value.trim().to_string()).filter(|label| !label.is_empty());
        items.push(WikidataItem { id: item.to_string(), label });
    }
    Ok(items)
}

fn query(isbn: &Isbn) -> String {
    let mut values13 = vec![isbn.isbn13_hyphenated(), isbn.isbn13_digits()];
    values13.sort();
    values13.dedup();
    let values13 = values13.into_iter().map(|value| format!("\"{value}\"")).collect::<Vec<_>>().join(" ");
    let p212 = format!("{{ VALUES ?isbn {{ {values13} }} ?item wdt:P212 ?isbn. }}");

    let p957 = isbn
        .isbn10_hyphenated()
        .zip(isbn.isbn10_digits())
        .map(|(hyphenated, digits)| {
            let mut values = vec![hyphenated, digits];
            values.sort();
            values.dedup();
            values.into_iter().map(|value| format!("\"{value}\"")).collect::<Vec<_>>().join(" ")
        })
        .map(|values| format!("{{ VALUES ?isbn {{ {values} }} ?item wdt:P957 ?isbn. }}"));

    let body = match p957 {
        Some(p957) => format!("{p212} UNION {p957}"),
        None => p212,
    };
    format!(
        "SELECT DISTINCT ?item ?itemLabel WHERE {{ {body} SERVICE wikibase:label {{ bd:serviceParam wikibase:language \"en,de,fr\". }} }} LIMIT 5"
    )
}

#[derive(Debug, Deserialize)]
struct SparqlResponse {
    results: SparqlResults,
}

#[derive(Debug, Deserialize)]
struct SparqlResults {
    #[serde(default)]
    bindings: Vec<HashMap<String, SparqlValue>>,
}

#[derive(Debug, Deserialize)]
struct SparqlValue {
    value: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_has_both_isbn_forms() {
        let isbn = Isbn::parse("9780140328721").unwrap();
        let query = query(&isbn);
        assert!(query.contains("978-0-14-032872-1"));
        assert!(query.contains("9780140328721"));
        assert!(query.contains("0140328726"));
        assert!(query.contains("P212"));
        assert!(query.contains("P957"));
    }
}
