// Copyright 2026 Syntropika
// SPDX-License-Identifier: Apache-2.0
//! Headless owner tests use fake first-party HTTP and an isolated vault, never personal credentials.
use async_trait::async_trait;
use axum::{
    Json,
    extract::State,
    routing::{get, post},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use swarmie_accounts::{AccountManager, Error, Result, vault::Vault};
use tokio::sync::Mutex;
#[derive(Default)]
struct MemoryVault {
    value: Mutex<Option<String>>,
    locked: AtomicBool,
    delete_failed: AtomicBool,
}
#[async_trait]
impl Vault for MemoryVault {
    async fn read(&self) -> Result<Option<String>> {
        if self.locked.load(Ordering::SeqCst) {
            Err(Error::Vault)
        } else {
            Ok(self.value.lock().await.clone())
        }
    }
    async fn write(&self, value: String) -> Result<()> {
        if self.locked.load(Ordering::SeqCst) {
            return Err(Error::Vault);
        }
        *self.value.lock().await = Some(value);
        Ok(())
    }
    async fn delete(&self) -> Result<()> {
        if self.delete_failed.load(Ordering::SeqCst) {
            return Err(Error::Vault);
        }
        *self.value.lock().await = None;
        Ok(())
    }
}
struct Fake {
    session_reads: AtomicUsize,
    revocations: AtomicUsize,
    deny_sessions: AtomicBool,
    fail_revocation: AtomicBool,
    origin: String,
    request: Mutex<Value>,
    renewals: AtomicUsize,
    reads: AtomicUsize,
    fail_refresh: AtomicBool,
    throttle_refresh: AtomicBool,
}
fn profile() -> Value {
    json!({"id":uuid::Uuid::nil(),"displayName":"Test","email":"test@example.com","emailVerified":true,"avatarUrl":null,"revision":0})
}
fn grant(expiry: i64) -> Value {
    json!({"accessToken":"private-access","refreshToken":"private-refresh","expiresAt":expiry,"profile":profile()})
}
async fn request(State(fake): State<Arc<Fake>>, Json(request): Json<Value>) -> Json<Value> {
    *fake.request.lock().await = request;
    Json(
        json!({"authorizationUrl":format!("{}/auth/native/authorize?request=one-use-browser-proof",fake.origin)}),
    )
}
async fn redeem(State(fake): State<Arc<Fake>>, Json(value): Json<Value>) -> Json<Value> {
    let request = fake.request.lock().await;
    assert_eq!(
        request["challenge"],
        URL_SAFE_NO_PAD.encode(Sha256::digest(
            value["verifier"].as_str().unwrap().as_bytes()
        ))
    );
    assert_eq!(request["callback"], value["callback"]);
    assert_eq!(request["installationId"], value["installationId"]);
    Json(grant(0)) // Deliberately expires before the next headless tick.
}
async fn refresh(
    State(fake): State<Arc<Fake>>,
    Json(value): Json<Value>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    fake.renewals.fetch_add(1, Ordering::SeqCst);
    assert_eq!(value["refreshToken"], "private-refresh");
    if fake.throttle_refresh.load(Ordering::SeqCst) {
        return (
            axum::http::StatusCode::TOO_MANY_REQUESTS,
            [(axum::http::header::RETRY_AFTER, "1")],
        )
            .into_response();
    }
    if fake.fail_refresh.load(Ordering::SeqCst) {
        return axum::http::StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    Json(grant(i64::MAX / 2)).into_response()
}
async fn read(State(fake): State<Arc<Fake>>) -> Json<Value> {
    fake.reads.fetch_add(1, Ordering::SeqCst);
    Json(profile())
}
async fn fixture() -> (
    Arc<Fake>,
    Arc<MemoryVault>,
    Arc<AccountManager>,
    tokio::task::JoinHandle<()>,
    uuid::Uuid,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let fake = Arc::new(Fake {
        session_reads: AtomicUsize::new(0),
        revocations: AtomicUsize::new(0),
        deny_sessions: AtomicBool::new(false),
        fail_revocation: AtomicBool::new(false),
        origin: origin.clone(),
        request: Mutex::new(Value::Null),
        renewals: AtomicUsize::new(0),
        reads: AtomicUsize::new(0),
        fail_refresh: AtomicBool::new(false),
        throttle_refresh: AtomicBool::new(false),
    });
    let router = axum::Router::new()
        .route("/auth/native/request", post(request))
        .route("/auth/native/redeem", post(redeem))
        .route("/auth/native/refresh", post(refresh))
        .route(
            "/auth/native/revoke",
            post(|| async { axum::http::StatusCode::NO_CONTENT }),
        )
        .route("/api/native/profile", get(read))
        .route("/api/native/sessions", get(sessions).delete(revoke_all))
        .route(
            "/api/native/sessions/{id}",
            axum::routing::delete(revoke_one),
        )
        .with_state(fake.clone());
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let vault = Arc::new(MemoryVault::default());
    let installation = uuid::Uuid::new_v4();
    let manager = Arc::new(
        AccountManager::new(&origin, installation, vault.clone())
            .await
            .unwrap(),
    );
    (fake, vault, manager, task, installation)
}
fn session_page() -> Value {
    let session = |id| json!({"id":uuid::Uuid::from_u128(id),"installationId":uuid::Uuid::nil(),"authenticatedAt":"2026-10-14T10:00:00Z","createdAt":"2026-10-14T10:00:00Z","lastUsedAt":"2026-10-14T11:00:00Z","expiresAt":"2026-11-13T10:00:00Z"});
    json!({"current":session(1),"others":[session(2)],"nextCursor":null})
}
async fn sessions(
    State(fake): State<Arc<Fake>>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    fake.session_reads.fetch_add(1, Ordering::SeqCst);
    assert_eq!(headers["authorization"], "Bearer private-access");
    if fake.deny_sessions.load(Ordering::SeqCst) {
        return axum::http::StatusCode::UNAUTHORIZED.into_response();
    }
    Json(session_page()).into_response()
}
async fn revoke(fake: Arc<Fake>, current: bool) -> axum::response::Response {
    use axum::response::IntoResponse;
    fake.revocations.fetch_add(1, Ordering::SeqCst);
    if fake.fail_revocation.load(Ordering::SeqCst) {
        return axum::http::StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    Json(json!({"currentSessionRevoked":current})).into_response()
}
async fn revoke_one(
    State(fake): State<Arc<Fake>>,
    axum::extract::Path(id): axum::extract::Path<uuid::Uuid>,
) -> axum::response::Response {
    revoke(fake, id == uuid::Uuid::from_u128(1)).await
}
async fn revoke_all(State(fake): State<Arc<Fake>>) -> axum::response::Response {
    revoke(fake, true).await
}

#[tokio::test]
async fn session_management_preserves_account_intent_and_clears_own_credentials() {
    let (fake, vault, manager, task, _) = fixture().await;
    sign_in(&fake, &manager, false).await;
    assert!(matches!(
        manager.sessions(uuid::Uuid::new_v4(), None).await,
        Err(Error::Unauthenticated)
    ));
    assert!(matches!(
        manager.revoke_sessions(uuid::Uuid::new_v4(), None).await,
        Err(Error::Unauthenticated)
    ));
    assert_eq!(fake.session_reads.load(Ordering::SeqCst), 0);
    assert_eq!(fake.revocations.load(Ordering::SeqCst), 0);
    let page = manager.sessions(uuid::Uuid::nil(), None).await.unwrap();
    let serialized = serde_json::to_string(&page).unwrap();
    assert!(!serialized.contains("private-"));
    manager
        .revoke_sessions(uuid::Uuid::nil(), Some(page.others[0].id))
        .await
        .unwrap();
    assert!(vault.value.lock().await.is_some());
    manager
        .revoke_sessions(uuid::Uuid::nil(), Some(page.current.id))
        .await
        .unwrap();
    assert!(vault.value.lock().await.is_none());
    assert_eq!(manager.retained_snapshot().await.status, "signedOut");
    task.abort();
}
#[tokio::test]
async fn failed_revocation_is_not_replayed_or_reported_as_success() {
    let (fake, vault, manager, task, _) = fixture().await;
    sign_in(&fake, &manager, false).await;
    manager.read().await.unwrap();
    let before = vault.value.lock().await.clone();
    fake.fail_revocation.store(true, Ordering::SeqCst);
    assert!(matches!(
        manager.revoke_sessions(uuid::Uuid::nil(), None).await,
        Err(Error::Unavailable)
    ));
    assert_eq!(fake.revocations.load(Ordering::SeqCst), 1);
    assert_eq!(*vault.value.lock().await, before);
    fake.fail_revocation.store(false, Ordering::SeqCst);
    assert_eq!(
        manager
            .revoke_sessions(uuid::Uuid::nil(), None)
            .await
            .unwrap()
            .status,
        "signedOut"
    );
    assert!(vault.value.lock().await.is_none());
    task.abort();
}
#[tokio::test]
async fn rejected_security_read_invalidates_restorable_credentials() {
    let (fake, vault, manager, task, _) = fixture().await;
    sign_in(&fake, &manager, false).await;
    fake.deny_sessions.store(true, Ordering::SeqCst);
    assert!(matches!(
        manager.sessions(uuid::Uuid::nil(), None).await,
        Err(Error::Unauthenticated)
    ));
    assert_eq!(manager.retained_snapshot().await.status, "signedOut");
    assert!(vault.value.lock().await.is_none());
    task.abort();
}
async fn sign_in(fake: &Fake, manager: &AccountManager, session_only: bool) {
    manager
        .start(
            "http://127.0.0.1:49000/account/callback".into(),
            session_only,
        )
        .await
        .unwrap();
    let nonce = fake.request.lock().await["state"]
        .as_str()
        .unwrap()
        .to_owned();
    manager.complete(&nonce, "approved-code").await.unwrap();
}
#[tokio::test]
async fn headless_tick_renews_once_and_restart_revalidates_without_ui() {
    let (fake, vault, manager, task, installation) = fixture().await;
    sign_in(&fake, &manager, false).await;
    manager.tick().await;
    assert_eq!(fake.renewals.load(Ordering::SeqCst), 1);
    let (a, b) = tokio::join!(manager.read(), manager.read());
    assert_eq!(a.unwrap().status, "signedIn");
    assert_eq!(b.unwrap().status, "signedIn");
    assert_eq!(fake.renewals.load(Ordering::SeqCst), 1);
    let snapshot = serde_json::to_string(&manager.read().await.unwrap()).unwrap();
    assert!(!snapshot.contains("private-access"));
    assert!(!snapshot.contains("private-refresh"));
    drop(manager);
    let restored = AccountManager::new(&fake.origin, installation, vault.clone())
        .await
        .unwrap();
    let before = fake.reads.load(Ordering::SeqCst);
    assert_eq!(restored.read().await.unwrap().status, "signedIn");
    assert_eq!(fake.reads.load(Ordering::SeqCst), before + 1);
    restored.logout().await.unwrap();
    assert!(vault.value.lock().await.is_none());
    task.abort();
}
#[tokio::test]
async fn locked_restored_vault_recovers_after_unlock_without_a_new_login() {
    let (fake, vault, manager, task, installation) = fixture().await;
    sign_in(&fake, &manager, false).await;
    drop(manager);
    vault.locked.store(true, Ordering::SeqCst);
    let restored = AccountManager::new(&fake.origin, installation, vault.clone())
        .await
        .unwrap();
    assert_eq!(restored.read().await.unwrap().status, "storageUnavailable");
    assert_eq!(fake.renewals.load(Ordering::SeqCst), 0);
    vault.locked.store(false, Ordering::SeqCst);
    assert_eq!(restored.read().await.unwrap().status, "signedIn");
    assert_eq!(fake.renewals.load(Ordering::SeqCst), 1);
    let before = fake.reads.load(Ordering::SeqCst);
    assert_eq!(restored.retained_snapshot().await.status, "signedIn");
    assert_eq!(fake.reads.load(Ordering::SeqCst), before);
    assert_eq!(fake.renewals.load(Ordering::SeqCst), 1);
    restored.logout().await.unwrap();
    task.abort();
}
#[tokio::test]
async fn wrong_state_does_not_consume_login_and_explicit_cancel_does() {
    let (fake, _, manager, task, _) = fixture().await;
    let pending = manager
        .start("http://127.0.0.1:49000/account/callback".into(), false)
        .await
        .unwrap();
    assert!(matches!(
        manager.complete("wrong-state", "code").await,
        Err(Error::Unauthenticated)
    ));
    assert_eq!(manager.read().await.unwrap().status, "signingIn");
    let nonce = fake.request.lock().await["state"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(matches!(
        manager.cancel(uuid::Uuid::new_v4()).await,
        Err(Error::Invalid)
    ));
    manager
        .cancel(serde_json::from_value(pending["loginId"].clone()).unwrap())
        .await
        .unwrap();
    assert!(manager.complete(&nonce, "code").await.is_err());
    task.abort();
}
#[tokio::test]
async fn session_only_is_explicit_and_ambiguous_renewal_is_not_replayed() {
    let (fake, vault, manager, task, _) = fixture().await;
    vault.locked.store(true, Ordering::SeqCst);
    assert!(matches!(
        manager
            .start("http://127.0.0.1:49000/account/callback".into(), false)
            .await,
        Err(Error::Vault)
    ));
    sign_in(&fake, &manager, true).await;
    assert!(vault.value.lock().await.is_none());
    fake.fail_refresh.store(true, Ordering::SeqCst);
    assert!(matches!(manager.read().await, Err(Error::Unavailable)));
    assert_eq!(manager.read().await.unwrap().status, "signedOut");
    assert_eq!(fake.renewals.load(Ordering::SeqCst), 1);
    task.abort();
}
#[test]
fn account_origins_reject_credentials_paths_and_insecure_remote_hosts() {
    for value in [
        "http://remote.example",
        "https://user:secret@example.com",
        "https://example.com/path",
        "https://example.com/?token=secret",
        "https://example.com/#fragment",
    ] {
        assert!(swarmie_accounts::validate_origin(value).is_err());
    }
    assert!(swarmie_accounts::validate_origin("https://accounts.example.com").is_ok());
}

#[tokio::test]
async fn rejected_session_only_login_cannot_erase_an_active_durable_login() {
    let (fake, vault, manager, task, _) = fixture().await;
    sign_in(&fake, &manager, false).await;
    let stored = vault.value.lock().await.clone();
    assert!(matches!(
        manager
            .start("http://127.0.0.1:49000/account/callback".into(), true)
            .await,
        Err(Error::Busy)
    ));
    assert_eq!(*vault.value.lock().await, stored);
    task.abort();
}
#[tokio::test]
async fn local_invalidation_failure_never_claims_success_or_replays_renewal() {
    let (fake, vault, manager, task, _) = fixture().await;
    sign_in(&fake, &manager, false).await;
    fake.fail_refresh.store(true, Ordering::SeqCst);
    vault.delete_failed.store(true, Ordering::SeqCst);
    assert!(matches!(manager.read().await, Err(Error::Vault)));
    assert!(matches!(manager.read().await, Err(Error::Vault)));
    assert_eq!(fake.renewals.load(Ordering::SeqCst), 1);
    assert!(matches!(manager.logout().await, Err(Error::Vault)));
    vault.delete_failed.store(false, Ordering::SeqCst);
    assert_eq!(manager.read().await.unwrap().status, "signedOut");
    task.abort();
}

#[tokio::test]
async fn stale_account_profile_intent_is_rejected_before_mutation() {
    let (fake, _, manager, task, _) = fixture().await;
    sign_in(&fake, &manager, false).await;
    assert!(matches!(manager.update(json!({"accountId":uuid::Uuid::new_v4(),"expectedRevision":0,"requestId":uuid::Uuid::new_v4(),"displayName":"Wrong account"})).await,Err(Error::Unauthenticated)));
    task.abort();
}

#[tokio::test]
async fn rejected_renewal_preserves_the_vault_and_waits_before_another_attempt() {
    let (fake, vault, manager, task, _) = fixture().await;
    sign_in(&fake, &manager, false).await;
    let before = vault.value.lock().await.clone();
    fake.throttle_refresh.store(true, Ordering::SeqCst);
    assert!(matches!(manager.read().await, Err(Error::RateLimited(1))));
    manager.tick().await;
    assert_eq!(fake.renewals.load(Ordering::SeqCst), 1);
    assert_eq!(*vault.value.lock().await, before);
    assert_eq!(manager.retained_snapshot().await.status, "signedIn");
    fake.throttle_refresh.store(false, Ordering::SeqCst);
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    assert_eq!(manager.read().await.unwrap().status, "signedIn");
    assert_eq!(fake.renewals.load(Ordering::SeqCst), 2);
    manager.logout().await.unwrap();
    task.abort();
}
