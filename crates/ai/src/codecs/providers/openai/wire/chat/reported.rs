use serde_json::{Map, Value};

use crate::codecs::completion::Cost;
use crate::codecs::json_utils::Lenient;
use crate::codecs::message::{Source, SourceLocation};
use crate::codecs::wire::WireCitation;

pub(super) const REPLY_FIELDS: [&str; 5] = [
    "citations",
    "search_results",
    "web_search",
    "venice_parameters",
    "cost",
];

const TICKS_PER_USD: f64 = 1e10;

fn url_source(url: &str, title: Option<&str>, quoted: Option<&str>) -> Source {
    let mut source = Source::new(SourceLocation::Url {
        url: url.to_owned(),
    });
    if let Some(title) = title.filter(|title| !title.is_empty()) {
        source = source.title(title);
    }
    if let Some(quoted) = quoted.filter(|quoted| !quoted.is_empty()) {
        source = source.cited_text(quoted);
    }
    source
}

pub(super) fn annotation(annotation: &Value) -> Option<WireCitation> {
    if annotation.str("type") != Some("url_citation") {
        return None;
    }
    let cited = annotation.get("url_citation").unwrap_or(annotation);
    let url = cited.str("url").filter(|url| !url.is_empty())?;
    let source = url_source(url, cited.str("title"), cited.str("content"));
    Some(WireCitation::new(None, vec![source]))
}

pub(super) fn reference(part: &Value) -> Option<WireCitation> {
    if part.str("type") != Some("reference") {
        return None;
    }
    let sources: Vec<Source> = part
        .arr("reference_ids")
        .iter()
        .map(|id| match id {
            Value::String(id) => id.clone(),
            id => id.to_string(),
        })
        .map(|id| {
            Source::new(SourceLocation::Document {
                index: None,
                id: Some(id),
                within: None,
            })
        })
        .collect();
    (!sources.is_empty()).then(|| WireCitation::new(None, sources))
}

pub(super) fn listed(fields: &Map<String, Value>) -> Vec<WireCitation> {
    let field = |key: &str| fields.get(key).and_then(Value::as_array);
    let results = field("search_results").map_or(&[][..], Vec::as_slice);
    let title = |url: &str| {
        results
            .iter()
            .find(|result| result.str("url") == Some(url))
            .and_then(|result| result.str("title"))
    };
    let urls = field("citations")
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(|url| url_source(url, title(url), None));
    let venice = fields
        .get("venice_parameters")
        .map_or(&[][..], |parameters| parameters.arr("web_search_citations"))
        .iter()
        .filter_map(|result| {
            let url = result.str("url")?;
            Some(url_source(url, result.str("title"), result.str("content")))
        });
    let zai = field("web_search")
        .into_iter()
        .flatten()
        .filter_map(|result| {
            let url = result.str("link")?;
            Some(url_source(url, result.str("title"), result.str("content")))
        });
    urls.chain(venice)
        .chain(zai)
        .filter(|source| !matches!(&source.location, SourceLocation::Url { url } if url.is_empty()))
        .map(|source| WireCitation::new(None, vec![source]))
        .collect()
}

pub(super) fn cost(usage: Option<&Value>, fields: &Map<String, Value>) -> Option<Cost> {
    let finite = |value: f64| value.is_finite().then_some(value);
    let reported = usage.and_then(|usage| match usage.get("cost") {
        Some(parts @ Value::Object(_)) => {
            let part = |key: &str| parts.f64(key).and_then(finite);
            let (input, output) = (part("input_tokens_cost"), part("output_tokens_cost"));
            let total = part("total_cost")
                .or_else(|| Some(input? + output? + part("request_cost").unwrap_or(0.0)))?;
            Some(Cost::from_total(total).input(input).output(output))
        }
        Some(_) => usage.f64("cost").and_then(finite).map(Cost::from_total),
        None => None,
    });
    let ticks = || {
        usage
            .and_then(|usage| usage.f64("cost_in_usd_ticks"))
            .and_then(finite)
            .map(|ticks| Cost::from_total(ticks / TICKS_PER_USD))
    };
    let top = || match fields.get("cost") {
        Some(cost @ Value::Object(_)) => cost.f64("usd"),
        Some(Value::Number(cost)) => cost.as_f64(),
        Some(Value::String(cost)) => cost.trim().parse().ok(),
        _ => None,
    };
    reported
        .or_else(ticks)
        .or_else(|| top().and_then(finite).map(Cost::from_total))
}
