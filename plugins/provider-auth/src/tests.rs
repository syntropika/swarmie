use crate::*;
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;
use std::{
    collections::VecDeque,
    io::{Read, Write},
    net::TcpListener,
    thread,
};

struct Fixture {
    url: String,
    requests: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Fixture {
    fn new(replies: Vec<(u16, Value)>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let observed = requests.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let worker = thread::spawn(move || {
            let mut replies = VecDeque::from(replies);
            while !stopping.load(Ordering::Acquire) {
                let Ok((mut socket, _)) = listener.accept() else {
                    thread::sleep(Duration::from_millis(1));
                    continue;
                };
                socket
                    .set_read_timeout(Some(Duration::from_secs(1)))
                    .unwrap();
                let mut data = Vec::new();
                loop {
                    let mut buf = [0; 4096];
                    let count = socket.read(&mut buf).unwrap();
                    if count == 0 {
                        break;
                    }
                    data.extend_from_slice(&buf[..count]);
                    if let Some(end) = data.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&data[..end]);
                        let length = headers
                            .lines()
                            .find_map(|line| {
                                line.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .and_then(|v| v.trim().parse::<usize>().ok())
                            })
                            .unwrap_or(0);
                        if data.len() >= end + 4 + length {
                            break;
                        }
                    }
                }
                observed
                    .lock()
                    .unwrap()
                    .push(String::from_utf8(data).unwrap());
                let (status, body) = replies.pop_front().expect("unexpected provider request");
                if let Some(delay) = body.get("__fixture_delay").and_then(Value::as_u64) {
                    thread::sleep(Duration::from_millis(delay));
                }
                let body = body.to_string();
                let _ = socket.write_all(format!("HTTP/1.1 {status} Fixture\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",body.len()).as_bytes());
            }
        });
        Self {
            url,
            requests,
            stop,
            worker: Some(worker),
        }
    }
    fn client(&self) -> ProviderClient {
        ProviderClient::with_endpoints(Endpoints {
            chatgpt_token: format!("{}/token", self.url),
            chatgpt_jwks: format!("{}/jwks", self.url),
            kimi_auth: self.url.clone(),
            github_auth: self.url.clone(),
            copilot_exchange: format!("{}/copilot_internal/v2/token", self.url),
            xai_auth: self.url.clone(),
        })
        .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.worker.take().unwrap().join().unwrap();
    }
}

fn github_device() -> Value {
    json!({"device_code":"private-code","user_code":"PUBLIC","verification_uri":"https://github.com/login/device","interval":1,"expires_in":900})
}

#[test]
fn credential_envelope_keeps_byok_and_redacts_secrets() {
    let credential = Credential {
        provider: Provider::Kimi,
        access_token: zeroize::Zeroizing::new("private-access".into()),
        refresh_token: zeroize::Zeroizing::new("private-refresh".into()),
        expires_at_ms: now_ms() + 60000,
        chatgpt: None,
        endpoint: "https://api.kimi.ai/coding/v1/".into(),
    };
    assert!(decode("ordinary-api-key").unwrap().is_none());
    assert!(decode("swarmie-oauth-v1:invalid").is_err());
    let encoded = encode(&credential).unwrap();
    let decoded = decode(&encoded).unwrap().unwrap();
    assert_eq!(decoded.access_token.as_str(), "private-access");
    assert!(!format!("{decoded:?}").contains("private"));
    for endpoint in [
        "https://api.githubcopilot.com.evil.test",
        "http://api.githubcopilot.com",
        "https://user@api.githubcopilot.com",
        "https://api.githubcopilot.com/steal",
    ] {
        assert!(trusted_copilot_endpoint(endpoint).is_err());
    }
}

#[tokio::test]
async fn github_pi_source_grant_is_exchanged_for_copilot_bearer_and_renewed() {
    let fixture = Fixture::new(vec![
        (200, github_device()),
        (200, json!({"error":"authorization_pending"})),
        (200, json!({"access_token":"github-source"})),
        (
            200,
            json!({"token":"copilot-token","expires_at":now_ms()/1000+3600,"endpoints":{"api":"https://api.business.githubcopilot.com"}}),
        ),
        (
            200,
            json!({"token":"copilot-renewed","expires_at":now_ms()/1000+7200}),
        ),
    ]);
    let client = fixture.client();
    let (device, _) = client
        .begin(Provider::Copilot, "github-copilot".into())
        .await
        .unwrap();
    assert!(matches!(
        client.poll(&device).await.unwrap(),
        Outcome::Pending
    ));
    let Outcome::Authorized(credential) = client.poll(&device).await.unwrap() else {
        panic!("expected credential");
    };
    assert_eq!(credential.access_token.as_str(), "copilot-token");
    assert_eq!(credential.refresh_token.as_str(), "github-source");
    assert_eq!(
        credential.endpoint,
        "https://api.business.githubcopilot.com"
    );
    let renewed = client.refresh(&credential).await.unwrap();
    assert_eq!(renewed.access_token.as_str(), "copilot-renewed");
    assert_eq!(renewed.refresh_token.as_str(), "github-source");
    let seen = fixture.requests.lock().unwrap();
    let form = seen[0].split("\r\n\r\n").nth(1).unwrap();
    let fields: std::collections::BTreeMap<_, _> =
        oauth2::url::form_urlencoded::parse(form.as_bytes())
            .into_owned()
            .collect();
    assert_eq!(fields.get("scope").map(String::as_str), Some("read:user"));
    assert_eq!(
        fields.get("client_id").map(String::as_str),
        Some("Iv1.b507a08c87ecfe98")
    );
    assert!(seen[3].contains("Bearer github-source"));
    assert!(!seen.iter().any(|request| request.contains("vscode/")));
}

#[tokio::test]
async fn kimi_pi_device_flow_preserves_rotated_and_omitted_refresh_tokens() {
    let fixture = Fixture::new(vec![
        (
            200,
            json!({"device_code":"private-device","user_code":"PUBLIC","verification_uri_complete":"https://auth.kimi.ai/authorize?user_code=PUBLIC","interval":1}),
        ),
        (400, json!({"error":"slow_down","interval":8})),
        (
            200,
            json!({"access_token":"access-one","refresh_token":"refresh-one","expires_in":3600}),
        ),
        (
            200,
            json!({"access_token":"access-two","refresh_token":"refresh-two","expires_in":3600}),
        ),
        (
            200,
            json!({"access_token":"access-three","expires_in":3600}),
        ),
    ]);
    let client = fixture.client();
    let (device, instructions) = client.begin(Provider::Kimi, "kimi".into()).await.unwrap();
    assert_eq!(
        instructions.verification_uri_complete.as_deref(),
        Some("https://auth.kimi.ai/authorize?user_code=PUBLIC")
    );
    assert!(matches!(
        client.poll(&device).await.unwrap(),
        Outcome::SlowDown(8000)
    ));
    let Outcome::Authorized(credential) = client.poll(&device).await.unwrap() else {
        panic!("expected credential");
    };
    let renewed = client.refresh(&credential).await.unwrap();
    assert_eq!(renewed.refresh_token.as_str(), "refresh-two");
    let again = client.refresh(&renewed).await.unwrap();
    assert_eq!(again.refresh_token.as_str(), "refresh-two");
    assert_eq!(again.access_token.as_str(), "access-three");
    let seen = fixture.requests.lock().unwrap();
    assert!(seen.iter().all(|request| {
        request
            .to_ascii_lowercase()
            .contains("x-msh-platform: swarmie")
    }));
}

#[test]
fn malformed_token_responses_are_rejected_without_exposing_provider_bodies() {
    for value in [
        json!({}),
        json!({"access_token":"PRIVATE","expires_in":3600}),
        json!({"access_token":"PRIVATE","refresh_token":"PRIVATE","expires_in":0}),
        json!({"access_token":"PRIVATE","refresh_token":"PRIVATE","expires_in":3600,"token_type":"mac"}),
    ] {
        let result = crate::protocol::parse_token(&value, Provider::Kimi, None);
        assert!(result.is_err());
        assert!(!format!("{}", result.unwrap_err()).contains("PRIVATE"));
    }
    let value =
        json!({"access_token":"not-an-account-jwt","refresh_token":"refresh","expires_in":3600});
    assert!(crate::protocol::parse_token(&value, Provider::Chatgpt, None).is_err());
}

#[tokio::test]
async fn openrouter_nonexpiring_key_is_not_a_renewable_subscription_token() {
    let credential = Credential {
        provider: Provider::OpenRouter,
        access_token: zeroize::Zeroizing::new("openrouter-key".into()),
        refresh_token: zeroize::Zeroizing::new(String::new()),
        expires_at_ms: u64::MAX,
        chatgpt: None,
        endpoint: "https://openrouter.ai/api/v1/".into(),
    };
    let encoded = encode(&credential).unwrap();
    assert!(decode(&encoded).unwrap().is_some());
    assert_eq!(
        refreshed(&credential).await.unwrap().access_token.as_str(),
        "openrouter-key"
    );
    let mut invalid = credential;
    invalid.provider = Provider::Kimi;
    assert!(decode(&encode(&invalid).unwrap()).is_err());
}

#[tokio::test]
async fn revoked_refresh_is_typed_and_keeps_the_existing_credential_untouched() {
    let fixture = Fixture::new(vec![
        (
            200,
            json!({"device_code":"private-device","user_code":"PUBLIC","verification_uri":"https://auth.kimi.ai/authorize","interval":1,"expires_in":900}),
        ),
        (
            200,
            json!({"access_token":"access-kept","refresh_token":"refresh-kept","expires_in":3600}),
        ),
        (
            401,
            json!({"error":"invalid_grant","error_description":"PRIVATE-RESPONSE-BODY"}),
        ),
    ]);
    let client = fixture.client();
    let (device, _) = client.begin(Provider::Kimi, "kimi".into()).await.unwrap();
    let Outcome::Authorized(credential) = client.poll(&device).await.unwrap() else {
        panic!("expected credential");
    };
    let error = client.refresh(&credential).await.unwrap_err();
    assert_eq!(error, AuthError::Denied);
    assert!(!error.to_string().contains("PRIVATE"));
    assert_eq!(credential.access_token.as_str(), "access-kept");
    assert_eq!(credential.refresh_token.as_str(), "refresh-kept");
    assert_eq!(fixture.requests.lock().unwrap().len(), 3);
}

#[tokio::test]
async fn provider_verification_uri_cannot_target_an_unrelated_origin() {
    let fixture = Fixture::new(vec![(
        200,
        json!({"device_code":"private-device","user_code":"PUBLIC","verification_uri":"https://unrelated.example/authorize","interval":1,"expires_in":900}),
    )]);
    let error = match fixture
        .client()
        .begin(Provider::Copilot, "github-copilot".into())
        .await
    {
        Ok(_) => panic!("expected invalid origin"),
        Err(error) => error,
    };
    assert_eq!(error, AuthError::Invalid);
    assert!(!error.to_string().contains("private-device"));
}

#[tokio::test]
async fn xai_device_flow_uses_minimal_scopes_and_preserves_refresh_rotation() {
    let fixture = Fixture::new(vec![
        (
            200,
            json!({"device_code":"private-device","user_code":"PUBLIC","verification_uri":"https://accounts.x.ai/device","verification_uri_complete":"https://accounts.x.ai/device?user_code=PUBLIC","expires_in":120}),
        ),
        (400, json!({"error":"authorization_pending"})),
        (400, json!({"error":"slow_down","interval":10})),
        (
            200,
            json!({"access_token":"access-one","refresh_token":"refresh-one","expires_in":600,"id_token":"unverified-identity"}),
        ),
        (200, json!({"access_token":"access-two","expires_in":900})),
        (
            200,
            json!({"access_token":"access-three","refresh_token":"refresh-three"}),
        ),
    ]);
    let client = fixture.client();
    let before = now_ms();
    let (device, instructions) = client.begin(Provider::Xai, "xai".into()).await.unwrap();
    assert_eq!(instructions.interval_ms, 5000);
    assert!(instructions.expires_at_ms >= before + 120_000);
    assert!(instructions.expires_at_ms <= now_ms() + 120_000);
    assert!(
        !serde_json::to_string(&instructions)
            .unwrap()
            .contains("private-device")
    );
    assert!(matches!(
        client.poll(&device).await.unwrap(),
        Outcome::Pending
    ));
    assert!(matches!(
        client.poll(&device).await.unwrap(),
        Outcome::SlowDown(10000)
    ));
    let before_token = now_ms();
    let Outcome::Authorized(credential) = client.poll(&device).await.unwrap() else {
        panic!("expected credential");
    };
    assert_eq!(credential.provider, Provider::Xai);
    assert_eq!(credential.endpoint, "https://api.x.ai/v1/");
    assert!(credential.chatgpt.is_none());
    assert!(credential.expires_at_ms >= before_token + 600_000);
    assert!(credential.expires_at_ms <= now_ms() + 600_000);
    let renewed = client.refresh(&credential).await.unwrap();
    assert_eq!(renewed.access_token.as_str(), "access-two");
    assert_eq!(renewed.refresh_token.as_str(), "refresh-one");
    let fallback_start = now_ms();
    let rotated = client.refresh(&renewed).await.unwrap();
    assert_eq!(rotated.access_token.as_str(), "access-three");
    assert_eq!(rotated.refresh_token.as_str(), "refresh-three");
    assert!(rotated.expires_at_ms >= fallback_start + 3_600_000);
    assert!(rotated.expires_at_ms <= now_ms() + 3_600_000);
    let stored = decode(&encode(&rotated).unwrap()).unwrap().unwrap();
    assert_eq!(stored.provider, Provider::Xai);
    assert_eq!(stored.refresh_token.as_str(), "refresh-three");
    let seen = fixture.requests.lock().unwrap();
    assert!(seen[0].starts_with("POST /oauth2/device/code "));
    let form = seen[0].split("\r\n\r\n").nth(1).unwrap();
    let fields: std::collections::BTreeMap<_, _> =
        oauth2::url::form_urlencoded::parse(form.as_bytes())
            .into_owned()
            .collect();
    assert_eq!(
        fields.get("client_id").map(String::as_str),
        Some("b1a00492-073a-47ea-816f-4c329264a828")
    );
    assert_eq!(
        fields.get("scope").map(String::as_str),
        Some("openid profile email offline_access grok-cli:access api:access")
    );
    assert_eq!(fields.get("referrer").map(String::as_str), Some("swarmie"));
    assert!(seen.iter().all(|request| {
        request
            .to_ascii_lowercase()
            .contains("x-grok-client-surface: ui")
    }));
    assert!(seen.iter().all(|request| {
        !request
            .to_ascii_lowercase()
            .contains("x-grok-client-version:")
    }));
    assert!(seen[4].starts_with("POST /oauth2/token "));
    assert!(seen[4].contains("refresh_token=refresh-one"));
    assert!(seen[5].contains("refresh_token=refresh-one"));
}

#[tokio::test]
async fn xai_expiry_denial_and_invalid_token_are_terminal_protocol_results() {
    let device = json!({"device_code":"private-device","user_code":"PUBLIC","verification_uri":"https://accounts.x.ai/device","interval":2,"expires_in":120});
    let fixture = Fixture::new(vec![
        (200, device),
        (
            400,
            json!({"error":"authorization_denied","error_description":"PRIVATE-DETAIL"}),
        ),
        (400, json!({"error":"expired_token"})),
        (
            200,
            json!({"access_token":"private-access","expires_in":3600}),
        ),
    ]);
    let client = fixture.client();
    let (grant, _) = client.begin(Provider::Xai, "xai".into()).await.unwrap();
    assert!(matches!(
        client.poll(&grant).await.unwrap(),
        Outcome::Denied
    ));
    assert!(matches!(
        client.poll(&grant).await.unwrap(),
        Outcome::Expired
    ));
    let error = match client.poll(&grant).await {
        Err(error) => error,
        Ok(_) => panic!("missing refresh token must fail"),
    };
    assert_eq!(error, AuthError::Invalid);
    assert!(!error.to_string().contains("private-access"));
}

#[tokio::test]
async fn xai_refresh_retries_transient_failures_with_a_bounded_budget() {
    let fixture = Fixture::new(vec![
        (503, json!({"error":"unavailable"})),
        (429, json!({"error":"rate_limit_exceeded"})),
        (200, json!({"access_token":"renewed","expires_in":3600})),
    ]);
    let previous = Credential {
        provider: Provider::Xai,
        access_token: zeroize::Zeroizing::new("previous-access".into()),
        refresh_token: zeroize::Zeroizing::new("previous-refresh".into()),
        expires_at_ms: now_ms(),
        chatgpt: None,
        endpoint: "https://api.x.ai/v1/".into(),
    };
    let renewed = fixture.client().refresh(&previous).await.unwrap();
    assert_eq!(renewed.access_token.as_str(), "renewed");
    assert_eq!(renewed.refresh_token.as_str(), "previous-refresh");
    assert_eq!(fixture.requests.lock().unwrap().len(), 3);
    assert_eq!(previous.access_token.as_str(), "previous-access");
}

#[tokio::test]
async fn xai_invalid_device_expiry_is_not_extended_locally() {
    let fixture = Fixture::new(vec![(
        200,
        json!({"device_code":"private-device","user_code":"PUBLIC","verification_uri":"https://accounts.x.ai/device","expires_in":0}),
    )]);
    let error = match fixture.client().begin(Provider::Xai, "xai".into()).await {
        Err(error) => error,
        Ok(_) => panic!("expired device grant must be rejected"),
    };
    assert_eq!(error, AuthError::Invalid);
}
