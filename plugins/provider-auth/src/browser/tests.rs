use super::*;
use tokio::sync::oneshot;
use tokio::task::AbortHandle;

struct Fixture {
    origin: String,
    request: oneshot::Receiver<String>,
    release: Arc<Notify>,
    task: AbortHandle,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Fixture {
    fn options(&self) -> BrowserOptions {
        BrowserOptions {
            authorization_url: format!("{}/authorize", self.origin),
            token_url: format!("{}/token", self.origin),
            callback_port: 0,
            timeout: Duration::from_secs(5),
        }
    }
}

async fn fixture(body: String, held: bool) -> Fixture {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let (sender, request) = oneshot::channel();
    let release = Arc::new(Notify::new());
    let release_task = release.clone();
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 512];
        let request = loop {
            let count = socket.read(&mut buffer).await.unwrap();
            assert_ne!(count, 0);
            bytes.extend_from_slice(&buffer[..count]);
            assert!(bytes.len() <= 16_384);
            if let Some(end) = bytes.windows(4).position(|v| v == b"\r\n\r\n") {
                let header = std::str::from_utf8(&bytes[..end]).unwrap();
                let length = header
                    .lines()
                    .find_map(|line| {
                        let (key, value) = line.split_once(':')?;
                        key.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().unwrap())
                    })
                    .unwrap_or(0);
                if bytes.len() >= end + 4 + length {
                    break String::from_utf8(bytes).unwrap();
                }
            }
        };
        let _ = sender.send(request);
        if held {
            release_task.notified().await;
        }
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = socket.write_all(response.as_bytes()).await;
        let _ = socket.shutdown().await;
    });
    Fixture {
        origin,
        request,
        release,
        task: task.abort_handle(),
    }
}

fn callback(login: &BrowserLogin, code: &str) -> String {
    let mut url = Url::parse(&login.instructions.callback_url).unwrap();
    url.query_pairs_mut()
        .append_pair("code", code)
        .append_pair("state", login.shared.csrf.secret());
    url.into()
}

async fn result(login: &BrowserLogin) -> Result<Credential> {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if let Some(value) = login.poll().await? {
                return Ok(value);
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap()
}

fn address(login: &BrowserLogin) -> SocketAddr {
    SocketAddr::from((Ipv4Addr::LOCALHOST, login.shared.redirect.port().unwrap()))
}

async fn assert_closed(address: SocketAddr) {
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if TcpStream::connect(address).await.is_err() {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn public_instructions_contain_pkce_challenge_but_no_private_verifier() {
    let fixture = fixture(r#"{"key":"unused"}"#.into(), false).await;
    let login = begin_browser_with_options(Provider::OpenRouter, fixture.options())
        .await
        .unwrap();
    let url = Url::parse(&login.instructions.authorization_url).unwrap();
    let query = url
        .query_pairs()
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(query["callback_url"], login.instructions.callback_url);
    assert_eq!(query["code_challenge_method"], "S256");
    assert_eq!(query["key_label"], "Swarmie");
    assert_ne!(
        query["code_challenge"],
        login
            .shared
            .verifier
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .as_str()
    );
    assert!(login.instructions.expires_at_ms > now_ms());
    assert_eq!(login.shared.redirect.host_str(), Some("localhost"));
    assert!(
        !format!("{login:?}").contains(
            login
                .shared
                .verifier
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .as_str()
        )
    );
    assert!(login.poll().await.unwrap().is_none());
    let address = address(&login);
    drop(login);
    assert_closed(address).await;
}

#[tokio::test]
async fn wrong_routes_and_wrong_state_do_not_consume_the_callback() {
    let fixture = fixture(r#"{"key":"fixture-openrouter-key"}"#.into(), false).await;
    let login = begin_browser_with_options(Provider::OpenRouter, fixture.options())
        .await
        .unwrap();
    let client = Client::new();
    let wrong_path = login
        .instructions
        .callback_url
        .replace("/auth/callback", "/not-callback");
    assert_eq!(client.get(wrong_path).send().await.unwrap().status(), 404);
    let wrong_state = format!(
        "{}?code=unrelated&state=wrong",
        login.instructions.callback_url
    );
    let response = client.get(wrong_state).send().await.unwrap();
    assert_eq!(response.status(), 400);
    assert!(!response.text().await.unwrap().contains("unrelated"));
    assert!(login.poll().await.unwrap().is_none());
    let response = client
        .get(callback(&login, "fixture-code"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()["cache-control"], "no-store");
    let body = response.text().await.unwrap();
    assert!(body.contains("Return to Swarmie"));
    assert!(!body.contains("fixture-code"));
    let credential = result(&login).await.unwrap();
    assert_eq!(credential.provider, Provider::OpenRouter);
    assert_eq!(credential.access_token.as_str(), "fixture-openrouter-key");
    assert!(credential.refresh_token.is_empty());
    assert_eq!(credential.expires_at_ms, u64::MAX);
    assert_eq!(login.poll().await.unwrap_err(), AuthError::Consumed);
}

#[tokio::test]
async fn openrouter_exchanges_exact_code_with_matching_s256_verifier_once() {
    let mut fixture = fixture(r#"{"key":"fixture-openrouter-key"}"#.into(), false).await;
    let login = begin_browser_with_options(Provider::OpenRouter, fixture.options())
        .await
        .unwrap();
    let authorization = Url::parse(&login.instructions.authorization_url).unwrap();
    let expected_challenge = authorization
        .query_pairs()
        .find(|(key, _)| key == "code_challenge")
        .unwrap()
        .1
        .into_owned();
    let redirect = callback(&login, "fixture-code");
    let credential = login.submit_redirect(&redirect).await.unwrap();
    assert_eq!(credential.access_token.as_str(), "fixture-openrouter-key");
    let request = (&mut fixture.request).await.unwrap();
    let (headers, body) = request.split_once("\r\n\r\n").unwrap();
    assert!(headers.starts_with("POST /token HTTP/1.1"));
    let body: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(body["code"], "fixture-code");
    assert_eq!(body["code_challenge_method"], "S256");
    let verifier = oauth2::PkceCodeVerifier::new(body["code_verifier"].as_str().unwrap().into());
    assert_eq!(
        PkceCodeChallenge::from_code_verifier_sha256(&verifier).as_str(),
        expected_challenge
    );
    assert_eq!(
        login.submit_redirect(&redirect).await.unwrap_err(),
        AuthError::Consumed
    );
}

#[tokio::test]
async fn xai_starts_an_ephemeral_ip_callback_with_minimal_scopes_and_random_nonce() {
    let login = begin_browser(Provider::Xai).await.unwrap();
    let other = begin_browser(Provider::Xai).await.unwrap();
    let authorization = Url::parse(&login.instructions.authorization_url).unwrap();
    let other_authorization = Url::parse(&other.instructions.authorization_url).unwrap();
    let fields = authorization
        .query_pairs()
        .collect::<std::collections::BTreeMap<_, _>>();
    let other_fields = other_authorization
        .query_pairs()
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(
        authorization.origin().ascii_serialization(),
        "https://auth.x.ai"
    );
    assert_eq!(authorization.path(), "/oauth2/authorize");
    assert_eq!(fields["client_id"], XAI_CLIENT);
    assert_eq!(fields["response_type"], "code");
    assert_eq!(fields["redirect_uri"], login.instructions.callback_url);
    assert_eq!(fields["scope"], XAI_SCOPE);
    assert_eq!(fields["referrer"], "swarmie");
    assert_eq!(fields["code_challenge_method"], "S256");
    assert!(!fields["nonce"].is_empty());
    assert_ne!(fields["nonce"], fields["state"]);
    assert_ne!(fields["nonce"], other_fields["nonce"]);
    assert_ne!(fields["state"], other_fields["state"]);
    assert_ne!(fields["code_challenge"], other_fields["code_challenge"]);
    assert!(!fields["scope"].contains("conversations:"));
    assert!(!fields["scope"].contains("workspaces:"));
    assert_eq!(login.shared.redirect.host_str(), Some("127.0.0.1"));
    assert_eq!(login.shared.redirect.path(), "/callback");
    assert_ne!(login.shared.redirect.port(), Some(0));
    let address = address(&login);
    login.cancel();
    assert_eq!(login.poll().await.unwrap_err(), AuthError::Cancelled);
    assert_closed(address).await;
}

#[tokio::test]
async fn xai_http_callback_exchanges_pkce_and_never_trusts_unverified_id_token_claims() {
    let mut fixture = fixture(
        serde_json::json!({
            "access_token": "fixture-xai-access", "refresh_token": "fixture-xai-refresh",
            "expires_in": 3600, "token_type": "Bearer", "id_token": "not-a-verified-id-token",
        })
        .to_string(),
        false,
    )
    .await;
    let login = begin_browser_with_options(Provider::Xai, fixture.options())
        .await
        .unwrap();
    let authorization = Url::parse(&login.instructions.authorization_url).unwrap();
    let fields = authorization
        .query_pairs()
        .collect::<std::collections::BTreeMap<_, _>>();
    let client = Client::new();
    let wrong_state = format!(
        "{}?code=unrelated&state=wrong",
        login.instructions.callback_url
    );
    assert_eq!(client.get(wrong_state).send().await.unwrap().status(), 400);
    assert!(login.poll().await.unwrap().is_none());
    let response = client
        .get(callback(&login, "fixture-xai-code"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert!(!response.text().await.unwrap().contains("fixture-xai-code"));
    let credential = result(&login).await.unwrap();
    assert_eq!(credential.provider, Provider::Xai);
    assert_eq!(credential.access_token.as_str(), "fixture-xai-access");
    assert_eq!(credential.refresh_token.as_str(), "fixture-xai-refresh");
    assert_eq!(credential.endpoint, "https://api.x.ai/v1/");
    assert!(credential.chatgpt.is_none());
    let request = (&mut fixture.request).await.unwrap();
    let (headers, body) = request.split_once("\r\n\r\n").unwrap();
    assert!(headers.starts_with("POST /token HTTP/1.1"));
    assert!(!headers.contains("GrokBuild"));
    let form = oauth2::url::form_urlencoded::parse(body.as_bytes())
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(form["client_id"], XAI_CLIENT);
    assert_eq!(form["grant_type"], "authorization_code");
    assert_eq!(form["code"], "fixture-xai-code");
    assert_eq!(form["redirect_uri"], login.instructions.callback_url);
    let verifier = oauth2::PkceCodeVerifier::new(form["code_verifier"].to_string());
    assert_eq!(
        PkceCodeChallenge::from_code_verifier_sha256(&verifier).as_str(),
        fields["code_challenge"]
    );
    assert_eq!(login.poll().await.unwrap_err(), AuthError::Consumed);
    assert_closed(address(&login)).await;
}

#[tokio::test]
async fn xai_manual_callback_rejects_host_substitution_and_cancelled_exchange() {
    let mut fixture = fixture(
        serde_json::json!({
            "access_token": "fixture-xai-access", "refresh_token": "fixture-xai-refresh",
            "expires_in": 3600, "token_type": "Bearer",
        })
        .to_string(),
        true,
    )
    .await;
    let login = Arc::new(
        begin_browser_with_options(Provider::Xai, fixture.options())
            .await
            .unwrap(),
    );
    let redirect = callback(&login, "fixture-xai-code");
    let wrong_host = redirect.replace("127.0.0.1:", "localhost:");
    assert_eq!(
        login.submit_redirect(&wrong_host).await.unwrap_err(),
        AuthError::Invalid
    );
    assert!(login.poll().await.unwrap().is_none());
    let task_login = login.clone();
    let task = tokio::spawn(async move { task_login.submit_redirect(&redirect).await });
    let _ = (&mut fixture.request).await.unwrap();
    login.cancel();
    fixture.release.notify_one();
    assert_eq!(task.await.unwrap().unwrap_err(), AuthError::Cancelled);
    assert_eq!(login.poll().await.unwrap_err(), AuthError::Cancelled);
    assert_closed(address(&login)).await;
}

#[tokio::test]
async fn manual_redirect_rejects_unbound_or_ambiguous_inputs_without_consuming_grant() {
    let fixture = fixture(r#"{"key":"fixture-key"}"#.into(), false).await;
    let login = begin_browser_with_options(Provider::OpenRouter, fixture.options())
        .await
        .unwrap();
    let valid = callback(&login, "fixture-code");
    let wrong_host = valid.replace("localhost:", "127.0.0.1:");
    let wrong_path = valid.replace("/auth/callback", "/other");
    let duplicate_code = format!("{valid}&code=second-code");
    let duplicate_state = format!("{valid}&state=second-state");
    let fragment = format!("{valid}#fragment");
    for input in [
        "fixture-code",
        &wrong_host,
        &wrong_path,
        &duplicate_code,
        &duplicate_state,
        &fragment,
    ] {
        assert_eq!(
            login.submit_redirect(input).await.unwrap_err(),
            AuthError::Invalid
        );
        assert!(login.poll().await.unwrap().is_none());
    }
    assert_eq!(
        login
            .submit_redirect(&valid)
            .await
            .unwrap()
            .access_token
            .as_str(),
        "fixture-key"
    );
}

#[tokio::test]
async fn cancellation_closes_listener_and_prevents_delivery_after_exchange() {
    let mut fixture = fixture(r#"{"key":"fixture-key"}"#.into(), true).await;
    let login = Arc::new(
        begin_browser_with_options(Provider::OpenRouter, fixture.options())
            .await
            .unwrap(),
    );
    let redirect = callback(&login, "fixture-code");
    let task_login = login.clone();
    let task = tokio::spawn(async move { task_login.submit_redirect(&redirect).await });
    let _ = (&mut fixture.request).await.unwrap();
    login.cancel();
    fixture.release.notify_one();
    assert_eq!(task.await.unwrap().unwrap_err(), AuthError::Cancelled);
    assert_eq!(login.poll().await.unwrap_err(), AuthError::Cancelled);
    assert_closed(address(&login)).await;
    login.cancel();
}

#[tokio::test]
async fn cancelling_a_waiting_caller_does_not_lose_owned_exchange_result() {
    let mut fixture = fixture(r#"{"key":"fixture-key"}"#.into(), true).await;
    let login = Arc::new(
        begin_browser_with_options(Provider::OpenRouter, fixture.options())
            .await
            .unwrap(),
    );
    let redirect = callback(&login, "fixture-code");
    let task_login = login.clone();
    let task = tokio::spawn(async move { task_login.submit_redirect(&redirect).await });
    let _ = (&mut fixture.request).await.unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    fixture.release.notify_one();
    assert_eq!(
        result(&login).await.unwrap().access_token.as_str(),
        "fixture-key"
    );
}

#[tokio::test]
async fn denied_callback_terminates_without_token_exchange() {
    let fixture = fixture(r#"{"key":"unused"}"#.into(), false).await;
    let login = begin_browser_with_options(Provider::OpenRouter, fixture.options())
        .await
        .unwrap();
    let mut denied = Url::parse(&login.instructions.callback_url).unwrap();
    denied
        .query_pairs_mut()
        .append_pair("error", "access_denied")
        .append_pair("state", login.shared.csrf.secret());
    assert_eq!(
        login.submit_redirect(denied.as_str()).await.unwrap_err(),
        AuthError::Denied
    );
    assert_eq!(login.poll().await.unwrap_err(), AuthError::Denied);
    assert_closed(address(&login)).await;
}

#[tokio::test]
async fn expiry_clears_private_grant_and_closes_listener_without_polling() {
    let fixture = fixture(r#"{"key":"unused"}"#.into(), false).await;
    let mut options = fixture.options();
    options.timeout = Duration::from_millis(20);
    let login = begin_browser_with_options(Provider::OpenRouter, options)
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(40)).await;
    assert_eq!(login.poll().await.unwrap_err(), AuthError::Expired);
    assert!(login.shared.verifier.lock().unwrap().is_none());
    assert_closed(address(&login)).await;
}

#[tokio::test]
async fn binding_failure_and_invalid_configuration_are_honest() {
    let fixture = fixture(r#"{"key":"unused"}"#.into(), false).await;
    let occupied = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let mut options = fixture.options();
    options.callback_port = occupied.local_addr().unwrap().port();
    assert_eq!(
        begin_browser_with_options(Provider::OpenRouter, options)
            .await
            .unwrap_err(),
        AuthError::CallbackUnavailable
    );
    for endpoint in [
        "http://example.com/token",
        "https://user:pass@example.com/token",
        "https://example.com/token#fragment",
    ] {
        let mut options = fixture.options();
        options.token_url = endpoint.into();
        assert_eq!(
            begin_browser_with_options(Provider::OpenRouter, options)
                .await
                .unwrap_err(),
            AuthError::Invalid
        );
    }
    assert_eq!(
        begin_browser(Provider::Kimi).await.unwrap_err(),
        AuthError::Invalid
    );
}

#[tokio::test]
async fn callback_header_limit_does_not_consume_grant_or_echo_input() {
    let fixture = fixture(r#"{"key":"fixture-key"}"#.into(), false).await;
    let login = begin_browser_with_options(Provider::OpenRouter, fixture.options())
        .await
        .unwrap();
    let mut stream = TcpStream::connect(address(&login)).await.unwrap();
    let large = format!(
        "GET /auth/callback HTTP/1.1\r\nHost: localhost:{}\r\nX-Large: {}\r\n\r\n",
        address(&login).port(),
        "x".repeat(HEADER_LIMIT)
    );
    stream.write_all(large.as_bytes()).await.unwrap();
    let mut response = Vec::new();
    let _ = stream.read_to_end(&mut response).await;
    assert!(
        String::from_utf8(response)
            .unwrap()
            .starts_with("HTTP/1.1 431")
    );
    assert!(login.poll().await.unwrap().is_none());
    assert!(
        login
            .submit_redirect(&callback(&login, "fixture-code"))
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn openrouter_accepts_its_state_less_callback_only_at_the_owned_random_path() {
    let fixture = fixture(r#"{"key":"fixture-openrouter-key"}"#.into(), false).await;
    let login = begin_browser_with_options(Provider::OpenRouter, fixture.options())
        .await
        .unwrap();
    let callback_url = Url::parse(&login.instructions.callback_url).unwrap();
    let mut callback_url = callback_url;
    callback_url
        .query_pairs_mut()
        .append_pair("code", "fixture-code");
    let response = reqwest::Client::new()
        .get(callback_url)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(result(&login).await.unwrap().provider, Provider::OpenRouter);
    let authorization = Url::parse(&login.instructions.authorization_url).unwrap();
    assert!(!authorization.query_pairs().any(|(key, _)| key == "state"));
    assert!(login.shared.redirect.path().starts_with("/auth/callback/"));
    assert!(login.shared.redirect.path().len() > 30);
}

#[tokio::test]
async fn close_waits_for_owned_exchange_cancellation_and_is_repeatable() {
    let mut fixture = fixture(r#"{"key":"fixture-key"}"#.into(), true).await;
    let login = Arc::new(
        begin_browser_with_options(Provider::OpenRouter, fixture.options())
            .await
            .unwrap(),
    );
    let redirect = callback(&login, "fixture-code");
    let waiting = login.clone();
    let submitted = tokio::spawn(async move { waiting.submit_redirect(&redirect).await });
    let _ = (&mut fixture.request).await.unwrap();
    tokio::join!(login.close(), login.close());
    assert!(login.shared.exchange_task.lock().unwrap().is_none());
    assert_eq!(submitted.await.unwrap().unwrap_err(), AuthError::Cancelled);
    assert_closed(address(&login)).await;
    login.close().await;
}
