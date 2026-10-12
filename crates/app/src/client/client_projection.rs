//! Verified session hydration owned by the native application rather than its views.
use crate::{client_submissions::ClientSubmissions, node_gateway::NodeGateway};
use serde_json::{Value, json};
use swarmie::{Error, Result, RuntimeIdentity};

pub async fn restore(
    nodes: &NodeGateway,
    submissions: &ClientSubmissions,
    node: &str,
    request: Value,
) -> Result<Value> {
    let target: RuntimeIdentity = serde_json::from_value(request["expectedRuntime"].clone())?;
    let session = request["params"]["sessionId"]
        .as_str()
        .ok_or_else(|| Error::Invalid("sessionId is required".into()))?;
    let mut after = match request["params"].get("afterSequence") {
        None => 0,
        Some(value) => value
            .as_u64()
            .ok_or_else(|| Error::Invalid("afterSequence must be an unsigned integer".into()))?,
    };
    let mut response = submissions.recover(node, request.clone()).await?;
    let end = response["result"]["snapshot"]["sequence"]
        .as_u64()
        .ok_or_else(|| Error::Provider("session projection has no sequence".into()))?;
    if after > end {
        return Err(Error::Invalid(
            "session projection sequence is ahead of its snapshot".into(),
        ));
    }
    let mut events = Vec::new();
    let mut bytes = response.to_string().len();
    if bytes > 48 * 1024 * 1024 {
        return Err(Error::Invalid(
            "session projection exceeds the client transfer limit".into(),
        ));
    }
    while after < end {
        let page = nodes.forward(node, json!({"jsonrpc":"2.0","id":uuid::Uuid::new_v4().to_string(),"method":"session/transcript",
            "expectedRuntime":target,"params":{"sessionId":session,"afterSequence":after,"limit":256}})).await;
        if let Some(error) = page.get("error") {
            return Err(Error::Provider(
                error["message"]
                    .as_str()
                    .unwrap_or("session projection read failed")
                    .into(),
            ));
        }
        let page = page["result"]
            .as_array()
            .ok_or_else(|| Error::Provider("invalid transcript page".into()))?;
        if page.is_empty() {
            return Err(Error::Provider(
                "session transcript ended before its snapshot sequence".into(),
            ));
        }
        let previous = after;
        for event in page {
            let sequence = event_sequence(event, session, after)?;
            if sequence > end {
                break;
            }
            bytes += event.to_string().len() + 2;
            if bytes > 48 * 1024 * 1024 {
                return Err(Error::Invalid(
                    "session projection exceeds the client transfer limit".into(),
                ));
            }
            after = sequence;
            events.push(event.clone());
        }
        if after >= end {
            break;
        }
        if after == previous || page.len() < 256 {
            return Err(Error::Provider(
                "session transcript has an incomplete sequence range".into(),
            ));
        }
    }
    response["result"]["events"] = events.into();
    if response.to_string().len() > 48 * 1024 * 1024 {
        return Err(Error::Invalid(
            "session projection exceeds the client transfer limit".into(),
        ));
    }
    Ok(response)
}

fn event_sequence(event: &Value, session: &str, previous: u64) -> Result<u64> {
    let sequence = event["sequence"]
        .as_u64()
        .ok_or_else(|| Error::Provider("invalid transcript sequence".into()))?;
    if event["sessionId"] != session || previous.checked_add(1) != Some(sequence) {
        return Err(Error::Provider(
            "transcript page changed session or has a sequence gap".into(),
        ));
    }
    Ok(sequence)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sequence_validation_rejects_gaps_duplicates_and_other_sessions() {
        assert_eq!(
            event_sequence(
                &json!({"sessionId":"session","sequence":257}),
                "session",
                256
            )
            .unwrap(),
            257
        );
        for event in [
            json!({"sessionId":"session","sequence":258}),
            json!({"sessionId":"session","sequence":256}),
            json!({"sessionId":"other","sequence":257}),
        ] {
            assert!(event_sequence(&event, "session", 256).is_err());
        }
    }
}
