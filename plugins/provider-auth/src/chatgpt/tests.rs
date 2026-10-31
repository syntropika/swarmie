use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use jsonwebtoken::{EncodingKey, Header, encode};
use rsa::{RsaPrivateKey, pkcs1::EncodeRsaPrivateKey, rand_core::OsRng, traits::PublicKeyParts};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    task::AbortHandle,
};

use super::*;
use crate::{
    AuthError, BrowserLogin, BrowserOptions, ChatgptBrowserOptions, Endpoints, ProviderClient,
    begin_chatgpt_browser_with_options,
};

struct Keys {
    signer: EncodingKey,
    unrelated: EncodingKey,
    jwks: Value,
}
fn keys() -> &'static Keys {
    static KEYS: OnceLock<Keys> = OnceLock::new();
    KEYS.get_or_init(||{
        let key=RsaPrivateKey::new(&mut OsRng,2048).unwrap();
        let other=RsaPrivateKey::new(&mut OsRng,2048).unwrap();
        Keys {signer:EncodingKey::from_rsa_der(key.to_pkcs1_der().unwrap().as_bytes()),unrelated:EncodingKey::from_rsa_der(other.to_pkcs1_der().unwrap().as_bytes()),jwks:json!({"keys":[{"kty":"RSA","kid":"fixture-signing-key","use":"sig","alg":"RS256","n":URL_SAFE_NO_PAD.encode(key.n().to_bytes_be()),"e":URL_SAFE_NO_PAD.encode(key.e().to_bytes_be())}]})}
    })
}

struct Fixture {
    origin: String,
    token: Arc<Mutex<Value>>,
    seen: Arc<Mutex<Vec<String>>>,
    task: AbortHandle,
}
impl Fixture {
    async fn new() -> Self {
        let _ = keys();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let token = Arc::new(Mutex::new(json!({})));
        let reply = token.clone();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let requests = seen.clone();
        let task = tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                let mut bytes = Vec::new();
                loop {
                    let mut buf = [0; 2048];
                    let count = socket.read(&mut buf).await.unwrap();
                    if count == 0 {
                        break;
                    }
                    bytes.extend_from_slice(&buf[..count]);
                    if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                        let header = String::from_utf8_lossy(&bytes[..end]);
                        let length = header
                            .lines()
                            .find_map(|line| {
                                let (key, value) = line.split_once(':')?;
                                key.eq_ignore_ascii_case("content-length")
                                    .then(|| value.trim().parse::<usize>().unwrap())
                            })
                            .unwrap_or(0);
                        if bytes.len() >= end + 4 + length {
                            break;
                        }
                    }
                }
                let request = String::from_utf8(bytes).unwrap();
                let body = if request.starts_with("GET /jwks ") {
                    keys().jwks.clone()
                } else {
                    reply.lock().unwrap().clone()
                }
                .to_string();
                requests.lock().unwrap().push(request);
                let _=socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await;
            }
        });
        Self {
            origin,
            token,
            seen,
            task: task.abort_handle(),
        }
    }
    fn options(&self) -> ChatgptBrowserOptions {
        ChatgptBrowserOptions {
            browser: BrowserOptions {
                authorization_url: format!("{}/authorize", self.origin),
                token_url: format!("{}/token", self.origin),
                callback_port: 0,
                timeout: Duration::from_secs(10),
            },
            jwks_url: format!("{}/jwks", self.origin),
        }
    }
    fn client(&self) -> ProviderClient {
        ProviderClient::with_endpoints(Endpoints {
            chatgpt_token: format!("{}/token", self.origin),
            chatgpt_jwks: format!("{}/jwks", self.origin),
            ..Endpoints::default()
        })
        .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn fields(login: &BrowserLogin) -> BTreeMap<String, String> {
    reqwest::Url::parse(&login.instructions().authorization_url)
        .unwrap()
        .query_pairs()
        .into_owned()
        .collect()
}
fn claims(login: &BrowserLogin) -> Value {
    json!({"iss":ISSUER,"aud":"oaiapp_fixture","sub":"verified-subject","exp":now_ms()/1000+3600,"iat":now_ms()/1000,"nonce":fields(login)["nonce"],"email":"person@example.test","email_verified":true})
}
fn signed(claims: &Value, wrong: bool) -> String {
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some("fixture-signing-key".into());
    encode(
        &header,
        claims,
        if wrong {
            &keys().unrelated
        } else {
            &keys().signer
        },
    )
    .unwrap()
}
fn token(id_token: Option<String>) -> Value {
    let mut token = json!({"access_token":"new-access","refresh_token":"new-refresh","expires_in":3600,"scope":SCOPE,"token_type":"Bearer"});
    if let Some(id) = id_token {
        token["id_token"] = json!(id);
    }
    token
}
fn callback(login: &BrowserLogin, client: Option<&str>) -> String {
    let mut url = reqwest::Url::parse(&login.instructions().callback_url).unwrap();
    url.query_pairs_mut()
        .append_pair("code", "single-use-code")
        .append_pair("state", &fields(login)["state"]);
    if let Some(client) = client {
        url.query_pairs_mut().append_pair("client_id", client);
    }
    url.into()
}
async fn completed(login: &BrowserLogin) -> Result<Credential> {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if let Some(credential) = login.poll().await? {
                return Ok(credential);
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap()
}
async fn connected(fixture: &Fixture, host: Uuid) -> Credential {
    let login = begin_chatgpt_browser_with_options(host, None, fixture.options())
        .await
        .unwrap();
    *fixture.token.lock().unwrap() = token(Some(signed(&claims(&login), false)));
    login
        .submit_redirect(&callback(&login, Some("oaiapp_fixture")))
        .await
        .unwrap()
}

#[tokio::test]
async fn dynamic_registration_real_callback_requires_signed_identity_and_issued_client() {
    let fixture = Fixture::new().await;
    let host = Uuid::new_v4();
    let login = begin_chatgpt_browser_with_options(host, None, fixture.options())
        .await
        .unwrap();
    let query = fields(&login);
    assert_eq!(query["client_id"], "dynamic_agent_client");
    assert_eq!(query["agent_name_hint"], "Swarmie");
    assert_eq!(query["ext_agent_host_id"], format!("urn:uuid:{host}"));
    assert_eq!(query["scope"], SCOPE);
    assert_eq!(query["resource"], RESOURCE);
    assert_eq!(
        reqwest::Url::parse(&login.instructions().callback_url)
            .unwrap()
            .host_str(),
        Some("127.0.0.1")
    );
    assert_eq!(
        login
            .submit_redirect(&callback(&login, None))
            .await
            .unwrap_err(),
        AuthError::Invalid
    );
    *fixture.token.lock().unwrap() = token(Some(signed(&claims(&login), false)));
    let response = Client::new()
        .get(callback(&login, Some("oaiapp_fixture")))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let credential = completed(&login).await.unwrap();
    assert_eq!(credential.endpoint, "https://api.openai.com/v1/");
    let registration = credential.chatgpt.as_ref().unwrap();
    assert_eq!(registration.client_id, "oaiapp_fixture");
    assert_eq!(registration.identity.subject, "verified-subject");
    assert_eq!(registration.identity.issuer, ISSUER);
    assert_eq!(
        registration.identity.email.as_deref(),
        Some("person@example.test")
    );
    assert_eq!(registration.host_id, format!("urn:uuid:{host}"));
    assert!(
        crate::decode(&crate::encode(&credential).unwrap())
            .unwrap()
            .is_some()
    );
    let requests = fixture.seen.lock().unwrap();
    let request = requests
        .iter()
        .find(|request| request.starts_with("POST /token "))
        .unwrap();
    let form: BTreeMap<_, _> =
        oauth2::url::form_urlencoded::parse(request.split("\r\n\r\n").nth(1).unwrap().as_bytes())
            .into_owned()
            .collect();
    assert_eq!(form["client_id"], "oaiapp_fixture");
    assert_eq!(form["resource"], RESOURCE);
    assert_eq!(form["redirect_uri"], login.instructions().callback_url);
    let verifier = oauth2::PkceCodeVerifier::new(form["code_verifier"].clone());
    assert_eq!(
        oauth2::PkceCodeChallenge::from_code_verifier_sha256(&verifier).as_str(),
        query["code_challenge"]
    );
}

#[tokio::test]
async fn signature_issuer_audience_expiry_nonce_subject_and_scope_are_enforced() {
    let fixture = Fixture::new().await;
    for fault in [
        "signature",
        "issuer",
        "audience",
        "expiry",
        "nonce",
        "missing_nonce",
        "subject",
        "scope",
        "missing_identity",
        "algorithm",
    ] {
        let login = begin_chatgpt_browser_with_options(Uuid::new_v4(), None, fixture.options())
            .await
            .unwrap();
        let mut claims = claims(&login);
        match fault {
            "issuer" => claims["iss"] = json!("https://unrelated.example"),
            "audience" => claims["aud"] = json!("another-client"),
            "expiry" => claims["exp"] = json!(now_ms() / 1000 - 1),
            "nonce" => claims["nonce"] = json!("wrong-nonce"),
            "missing_nonce" => {
                claims.as_object_mut().unwrap().remove("nonce");
            }
            "subject" => claims["sub"] = json!(""),
            _ => {}
        }
        let mut reply = token(Some(signed(&claims, fault == "signature")));
        if fault == "scope" {
            reply["scope"] = json!("openid email profile");
        }
        if fault == "missing_identity" {
            reply.as_object_mut().unwrap().remove("id_token");
        }
        if fault == "algorithm" {
            let mut header = Header::new(Algorithm::HS256);
            header.kid = Some("fixture-signing-key".into());
            reply["id_token"] = json!(
                encode(
                    &header,
                    &claims,
                    &EncodingKey::from_secret(b"fixture-untrusted-secret")
                )
                .unwrap()
            );
        }
        *fixture.token.lock().unwrap() = reply;
        assert_eq!(
            login
                .submit_redirect(&callback(&login, Some("oaiapp_fixture")))
                .await
                .unwrap_err(),
            AuthError::Invalid,
            "fault: {fault}"
        );
    }
}

#[tokio::test]
async fn returning_registration_reuses_client_and_refuses_account_substitution() {
    let fixture = Fixture::new().await;
    let host = Uuid::new_v4();
    let previous = connected(&fixture, host).await;
    let before = crate::encode(&previous).unwrap();
    let login = begin_chatgpt_browser_with_options(host, Some(&previous), fixture.options())
        .await
        .unwrap();
    let query = fields(&login);
    assert_eq!(query["client_id"], "oaiapp_fixture");
    assert!(!query.contains_key("agent_name_hint"));
    assert!(query.contains_key("id_token_hint"));
    assert!(!format!("{login:?}").contains(&query["id_token_hint"]));
    assert_eq!(
        login
            .submit_redirect(&callback(&login, Some("different-client")))
            .await
            .unwrap_err(),
        AuthError::Invalid
    );
    let mut switched = claims(&login);
    switched["sub"] = json!("different-subject");
    *fixture.token.lock().unwrap() = token(Some(signed(&switched, false)));
    assert_eq!(
        login
            .submit_redirect(&callback(&login, None))
            .await
            .unwrap_err(),
        AuthError::Invalid
    );
    assert_eq!(crate::encode(&previous).unwrap().as_str(), before.as_str());
    assert!(
        begin_chatgpt_browser_with_options(Uuid::new_v4(), Some(&previous), fixture.options())
            .await
            .is_err()
    );
    let login = begin_chatgpt_browser_with_options(host, Some(&previous), fixture.options())
        .await
        .unwrap();
    *fixture.token.lock().unwrap() = token(Some(signed(&claims(&login), false)));
    assert!(login.submit_redirect(&callback(&login, None)).await.is_ok());
}

#[tokio::test]
async fn refresh_uses_issued_registration_resource_and_rotates_only_valid_identity() {
    let fixture = Fixture::new().await;
    let previous = connected(&fixture, Uuid::new_v4()).await;
    let client = fixture.client();
    let mut reply = token(None);
    reply["access_token"] = json!("rotated-access");
    reply["refresh_token"] = json!("rotated-refresh");
    *fixture.token.lock().unwrap() = reply;
    let renewed = client.refresh(&previous).await.unwrap();
    assert_eq!(renewed.access_token.as_str(), "rotated-access");
    assert_eq!(renewed.refresh_token.as_str(), "rotated-refresh");
    assert_eq!(
        renewed.chatgpt.as_ref().unwrap().id_token.as_str(),
        previous.chatgpt.as_ref().unwrap().id_token.as_str()
    );
    let wrong_identity = json!({"iss":ISSUER,"aud":"oaiapp_fixture","sub":"different-subject","exp":now_ms()/1000+3600,"iat":now_ms()/1000});
    *fixture.token.lock().unwrap() = token(Some(signed(&wrong_identity, false)));
    assert_eq!(
        client.refresh(&renewed).await.unwrap_err(),
        AuthError::Invalid
    );
    assert_eq!(renewed.refresh_token.as_str(), "rotated-refresh");
    let requests = fixture.seen.lock().unwrap();
    let refresh = requests
        .iter()
        .find(|request| request.contains("grant_type=refresh_token"))
        .unwrap();
    assert!(refresh.contains("client_id=oaiapp_fixture"));
    assert!(refresh.contains("resource=https%3A%2F%2Fapi.openai.com%2Fv1"));
}

#[tokio::test]
async fn host_identity_and_complete_registration_are_required() {
    assert_eq!(
        crate::begin_browser(Provider::Chatgpt).await.unwrap_err(),
        AuthError::Invalid
    );
    let fixture = Fixture::new().await;
    assert!(
        begin_chatgpt_browser_with_options(Uuid::nil(), None, fixture.options())
            .await
            .is_err()
    );
    let credential = Credential {
        provider: Provider::Chatgpt,
        access_token: Zeroizing::new("access".into()),
        refresh_token: Zeroizing::new("refresh".into()),
        expires_at_ms: now_ms() + 60000,
        chatgpt: None,
        endpoint: "https://api.openai.com/v1/".into(),
    };
    assert!(crate::decode(&crate::encode(&credential).unwrap()).is_err());
    let production = ChatgptBrowserOptions::default();
    assert_eq!(production.browser.authorization_url, AUTHORIZE);
    assert_eq!(production.browser.token_url, TOKEN);
    assert_eq!(production.jwks_url, JWKS);
    assert_eq!(production.browser.callback_port, 1455);
}

#[tokio::test]
async fn close_releases_the_fixed_callback_port_before_the_next_login() {
    let fixture = Fixture::new().await;
    let reservation = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = reservation.local_addr().unwrap().port();
    drop(reservation);
    for _ in 0..5 {
        let mut options = fixture.options();
        options.browser.callback_port = port;
        let login = begin_chatgpt_browser_with_options(Uuid::new_v4(), None, options)
            .await
            .unwrap();
        assert!(std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).is_err());
        login.close().await;
        let released = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).unwrap();
        drop(released);
    }
}

#[cfg(feature = "ai")]
fn ai_identity() -> swarmie_ai::catalog::ModelIdentity {
    swarmie_ai::catalog::ModelIdentity {
        provider: "chatgpt".into(),
        adapter: "swarmie-ai.responses.chatgpt.v1".into(),
        endpoint: "https://api.openai.com/v1/responses".into(),
        deployment: None,
        model: "fixture-model".into(),
        revision: Some("fixture-revision".into()),
    }
}

#[cfg(feature = "ai")]
#[tokio::test]
async fn dynamic_login_and_rotation_feed_the_exact_ai_registration_without_more_network() {
    use swarmie_ai::CredentialSource;

    let fixture = Fixture::new().await;
    let credential = connected(&fixture, Uuid::new_v4()).await;
    let selected = credential.chatgpt.as_deref().unwrap().clone();
    let identity = ai_identity();
    let source =
        crate::ChatgptCredentialSource::new(identity.clone(), &selected, credential.clone())
            .unwrap();
    let before = fixture.seen.lock().unwrap().len();
    source.resolve(&identity).await.unwrap();
    assert_eq!(fixture.seen.lock().unwrap().len(), before);
    let mut other = identity.clone();
    other.deployment = Some("another-deployment".into());
    assert_eq!(
        source.resolve(&other).await.err().unwrap().failure,
        swarmie_ai::Failure::Credential
    );
    let mut other = identity.clone();
    other.model = "another-model".into();
    assert!(source.resolve(&other).await.is_err());
    other = identity.clone();
    other.revision = None;
    assert!(source.resolve(&other).await.is_err());

    let mut reply = token(None);
    reply["access_token"] = json!("rotated-for-ai");
    *fixture.token.lock().unwrap() = reply;
    let renewed = fixture.client().refresh(&credential).await.unwrap();
    let renewed =
        crate::ChatgptCredentialSource::new(identity.clone(), &selected, renewed).unwrap();
    renewed.resolve(&identity).await.unwrap();
    let requests = fixture.seen.lock().unwrap();
    let refreshes = requests
        .iter()
        .filter(|request| request.contains("grant_type=refresh_token"))
        .collect::<Vec<_>>();
    assert_eq!(refreshes.len(), 1);
    assert!(refreshes[0].contains("client_id=oaiapp_fixture"));
    assert!(!refreshes[0].contains("dynamic_agent_client"));
}

#[cfg(feature = "ai")]
#[tokio::test]
async fn ai_capture_refuses_other_registrations_expiry_gateway_and_api_key_profiles() {
    let fixture = Fixture::new().await;
    let credential = connected(&fixture, Uuid::new_v4()).await;
    let selected = credential.chatgpt.as_deref().unwrap().clone();
    for field in [
        "client",
        "host",
        "subject",
        "dynamic",
        "permission",
        "expired",
        "endpoint",
    ] {
        let mut other = credential.clone();
        let registration = other.chatgpt.as_deref_mut().unwrap();
        match field {
            "client" => registration.client_id = "oaiapp_other".into(),
            "host" => registration.host_id = format!("urn:uuid:{}", Uuid::new_v4()),
            "subject" => registration.identity.subject = "other-subject".into(),
            "dynamic" => registration.client_id = DYNAMIC_CLIENT.into(),
            "permission" => registration.scopes.clear(),
            "expired" => other.expires_at_ms = now_ms() - 1,
            "endpoint" => other.endpoint = "https://chatgpt.com/backend-api/codex/".into(),
            _ => unreachable!(),
        }
        assert!(
            crate::ChatgptCredentialSource::new(ai_identity(), &selected, other).is_err(),
            "{field}"
        );
    }
    for field in ["adapter", "provider", "endpoint"] {
        let mut identity = ai_identity();
        match field {
            "adapter" => identity.adapter = swarmie_ai::Protocol::Responses.adapter().into(),
            "provider" => identity.provider = "openai".into(),
            "endpoint" => identity.endpoint = "https://example.test/responses".into(),
            _ => unreachable!(),
        }
        assert!(
            crate::ChatgptCredentialSource::new(identity, &selected, credential.clone()).is_err(),
            "{field}"
        );
    }
}
