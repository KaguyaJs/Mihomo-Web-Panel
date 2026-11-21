mod support;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode, header::SET_COOKIE},
};
use futures_util::StreamExt;
use mihomo_web_panel::{
    AppState, api, auth,
    config::Config,
    mihomo::{BackendConfig, MihomoClient},
};
use serde_json::{Value, json};
use std::{sync::atomic::Ordering, time::Duration};
use support::{NODE_A, NODE_B, SECRET};
use tower::ServiceExt;

const PASSWORD: &str = "test administrator password 2026";

struct Fixture {
    state: AppState,
    url: String,
    mock: support::MockState,
    task: tokio::task::JoinHandle<()>,
    dir: tempfile::TempDir,
}

impl Fixture {
    async fn new() -> Self {
        let (url, mock, task) = support::start_mock().await;
        let dir = tempfile::tempdir().unwrap();
        let config = Config {
            data_dir: dir.path().to_path_buf(),
            request_timeout_seconds: 1,
            ..Config::default()
        };
        let state = AppState::new(config).await.unwrap();
        Self {
            state,
            url,
            mock,
            task,
            dir,
        }
    }
    fn app(&self) -> Router {
        api::router(self.state.clone())
    }
    fn token(&self) -> String {
        std::fs::read_to_string(self.dir.path().join("bootstrap-token"))
            .unwrap()
            .trim()
            .to_string()
    }
    async fn request(
        &self,
        method: &str,
        path: &str,
        body: Value,
        session: Option<(&str, &str)>,
    ) -> (StatusCode, Value, Option<String>) {
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .header("host", "127.0.0.1:8787")
            .header("origin", "http://127.0.0.1:8787")
            .header("content-type", "application/json");
        if let Some((cookie, csrf)) = session {
            request = request
                .header("cookie", cookie)
                .header("x-csrf-token", csrf);
        }
        let response = self
            .app()
            .oneshot(request.body(Body::from(body.to_string())).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let cookie = response
            .headers()
            .get(SET_COOKIE)
            .map(|value| value.to_str().unwrap().to_string());
        let bytes = to_bytes(response.into_body(), 10 * 1024 * 1024)
            .await
            .unwrap();
        let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, value, cookie)
    }
    async fn setup(&self) {
        let (status, _, _) = self.request("POST", "/api/setup/complete", json!({"token": self.token(), "username": "admin", "password": PASSWORD, "confirm_password": PASSWORD, "controller_url": self.url, "secret": SECRET}), None).await;
        assert_eq!(status, StatusCode::CREATED);
    }
    async fn login(&self) -> (String, String) {
        let (status, user, cookie) = self
            .request(
                "POST",
                "/api/auth/login",
                json!({"username": "admin", "password": PASSWORD}),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{user}");
        (
            cookie.unwrap().split(';').next().unwrap().to_string(),
            user["csrf_token"].as_str().unwrap().to_string(),
        )
    }
    async fn finish(self) {
        self.state.shutdown.send_replace(true);
        self.task.abort();
        self.state.pool.close().await;
    }
}

#[tokio::test]
async fn unauthenticated_management_and_websockets_are_denied() {
    let fixture = Fixture::new().await;
    for path in [
        "/api/proxies",
        "/api/config",
        "/api/connections",
        "/api/settings",
        "/api/traffic",
        "/api/logs",
        "/api/settings/autostart",
    ] {
        assert_eq!(
            fixture.request("GET", path, Value::Null, None).await.0,
            StatusCode::UNAUTHORIZED,
            "{path}"
        );
    }
    fixture.finish().await;
}

#[tokio::test]
async fn initialization_is_atomic_token_bound_and_single_use() {
    let fixture = Fixture::new().await;
    let token = fixture.token();
    let (_, status, _) = fixture
        .request("GET", "/api/setup/status", Value::Null, None)
        .await;
    assert_eq!(status["required"], true);
    let mut input = json!({"token": "invalid", "username": "admin", "password": PASSWORD, "confirm_password": PASSWORD, "controller_url": fixture.url, "secret": SECRET});
    assert_eq!(
        fixture
            .request("POST", "/api/setup/complete", input.clone(), None)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    input["token"] = token.clone().into();
    *fixture.mock.behavior.lock().await = "offline".into();
    assert_eq!(
        fixture
            .request("POST", "/api/setup/complete", input.clone(), None)
            .await
            .0,
        StatusCode::BAD_GATEWAY
    );
    assert!(!auth::initialized(&fixture.state.pool).await.unwrap());
    assert!(
        auth::check_bootstrap(&fixture.state.pool, &token)
            .await
            .is_ok()
    );
    *fixture.mock.behavior.lock().await = "normal".into();
    assert_eq!(
        fixture
            .request("POST", "/api/setup/complete", input.clone(), None)
            .await
            .0,
        StatusCode::CREATED
    );
    assert_eq!(
        fixture
            .request("POST", "/api/setup/complete", input, None)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert!(!fixture.dir.path().join("bootstrap-token").exists());
    assert!(
        auth::check_bootstrap(&fixture.state.pool, &token)
            .await
            .is_err()
    );
    fixture.finish().await;
}

#[tokio::test]
async fn real_node_switch_delay_mode_and_connections() {
    let fixture = Fixture::new().await;
    fixture.setup().await;
    let (cookie, csrf) = fixture.login().await;
    let session = Some((cookie.as_str(), csrf.as_str()));
    let (status, proxies, _) = fixture
        .request("GET", "/api/proxies", Value::Null, session)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(proxies["proxies"]["PROXY"]["now"], NODE_A);
    let (status, switched, _) = fixture
        .request(
            "PUT",
            "/api/proxies/PROXY",
            json!({"name": NODE_B}),
            session,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(switched["now"], NODE_B);
    assert_eq!(*fixture.mock.selected.lock().await, NODE_B);
    assert_eq!(fixture.mock.switches.load(Ordering::SeqCst), 1);
    let escaped = url::form_urlencoded::byte_serialize(NODE_B.as_bytes())
        .collect::<String>()
        .replace('+', "%20");
    let (status, delay, _) = fixture
        .request(
            "POST",
            &format!("/api/proxies/{escaped}/delay"),
            json!({"timeout_ms": 1000}),
            session,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(delay["delay"], 67);
    assert_eq!(
        fixture
            .request(
                "PUT",
                "/api/proxies/%E8%87%AA%E5%8A%A8%E9%80%89%E6%8B%A9",
                json!({"name": NODE_B}),
                session
            )
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        fixture
            .request("PATCH", "/api/config", json!({"mode": "global"}), session)
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(*fixture.mock.mode.lock().await, "global");
    let (_, connections, _) = fixture
        .request("GET", "/api/connections", Value::Null, session)
        .await;
    assert_eq!(connections["connections"][0]["id"], "abc-123");
    assert_eq!(
        fixture
            .request("DELETE", "/api/connections/abc-123", Value::Null, session)
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        fixture
            .request(
                "DELETE",
                "/api/connections",
                json!({"confirm": false}),
                session
            )
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        fixture
            .request(
                "DELETE",
                "/api/connections",
                json!({"confirm": true}),
                session
            )
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    fixture.finish().await;
}

#[tokio::test]
async fn secret_address_and_unprojected_upstream_fields_never_escape() {
    let fixture = Fixture::new().await;
    fixture.setup().await;
    let (cookie, csrf) = fixture.login().await;
    let session = Some((cookie.as_str(), csrf.as_str()));
    for path in [
        "/api/proxies",
        "/api/config",
        "/api/settings/backend",
        "/api/status",
        "/api/connections",
    ] {
        let (status, body, _) = fixture.request("GET", path, Value::Null, session).await;
        assert_eq!(status, StatusCode::OK);
        let text = body.to_string();
        assert!(!text.contains(SECRET), "{path}");
        assert!(!text.contains(&fixture.url), "{path}");
        assert!(!text.contains("external-controller"));
        assert!(!text.contains("private-server"));
    }
    *fixture.mock.behavior.lock().await = "offline".into();
    let (_, body, _) = fixture
        .request("GET", "/api/status", Value::Null, session)
        .await;
    assert!(!body.to_string().contains(SECRET));
    assert!(!body.to_string().contains(&fixture.url));
    fixture.finish().await;
}

#[tokio::test]
async fn csrf_origin_host_and_logout_are_enforced() {
    let fixture = Fixture::new().await;
    fixture.setup().await;
    let (cookie, csrf) = fixture.login().await;
    assert_eq!(
        fixture
            .request(
                "PATCH",
                "/api/config",
                json!({"mode": "global"}),
                Some((&cookie, "wrong"))
            )
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    for (host, origin) in [
        ("evil.example", "http://127.0.0.1:8787"),
        ("127.0.0.1:8787", "https://evil.example"),
    ] {
        let request = Request::builder()
            .uri("/api/proxies")
            .header("host", host)
            .header("origin", origin)
            .header("cookie", &cookie)
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            fixture.app().oneshot(request).await.unwrap().status(),
            StatusCode::FORBIDDEN
        );
    }
    let (status, _, cleared) = fixture
        .request(
            "POST",
            "/api/auth/logout",
            Value::Null,
            Some((&cookie, &csrf)),
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(cleared.unwrap().contains("Max-Age=0"));
    assert_eq!(
        fixture
            .request("GET", "/api/proxies", Value::Null, Some((&cookie, &csrf)))
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    fixture.finish().await;
}

#[tokio::test]
async fn offline_timeout_invalid_json_and_redirects_are_safe() {
    let fixture = Fixture::new().await;
    let client = MihomoClient::new(
        &fixture.state.config,
        BackendConfig {
            controller_url: fixture.url.clone(),
            secret: SECRET.into(),
        },
    )
    .unwrap();
    for (behavior, code) in [
        ("offline", "backend_unavailable"),
        ("timeout", "backend_timeout"),
        ("invalid", "invalid_backend_response"),
        ("redirect", "backend_unavailable"),
    ] {
        *fixture.mock.behavior.lock().await = behavior.into();
        let error = client.version().await.unwrap_err();
        assert_eq!(error.code, code);
        assert!(!error.message.contains(SECRET));
        assert!(!error.message.contains(&fixture.url));
    }
    *fixture.mock.behavior.lock().await = "invalid-proxies".into();
    assert_eq!(
        client.proxies().await.unwrap_err().code,
        "invalid_backend_response"
    );
    fixture.finish().await;
}

#[tokio::test]
async fn password_change_rotates_current_session_revokes_others_and_persists() {
    let fixture = Fixture::new().await;
    fixture.setup().await;
    let (current, csrf) = fixture.login().await;
    let (other, other_csrf) = fixture.login().await;
    let new_password = "new administrator password 2026";
    let input = json!({"current_password": PASSWORD, "username": "renamed-admin", "new_password": new_password, "confirm_password": new_password});
    let (status, user, rotated) = fixture
        .request(
            "PATCH",
            "/api/settings/security",
            input,
            Some((&current, &csrf)),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(user["username"], "renamed-admin");
    assert_ne!(
        rotated.as_ref().unwrap().split(';').next().unwrap(),
        current
    );
    for (cookie, csrf) in [(&current, &csrf), (&other, &other_csrf)] {
        assert_eq!(
            fixture
                .request("GET", "/api/auth/me", Value::Null, Some((cookie, csrf)))
                .await
                .0,
            StatusCode::UNAUTHORIZED
        );
    }
    let (status, _, _) = fixture
        .request(
            "POST",
            "/api/auth/login",
            json!({"username": "renamed-admin", "password": new_password}),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let pool_config = (*fixture.state.config).clone();
    fixture.state.pool.close().await;
    let restarted = AppState::new(pool_config).await.unwrap();
    assert_eq!(
        auth::username(&restarted.pool).await.unwrap(),
        "renamed-admin"
    );
    assert!(restarted.client().await.unwrap().version().await.is_ok());
    restarted.pool.close().await;
    fixture.finish().await;
}

#[tokio::test]
async fn invalid_backend_never_overwrites_working_settings() {
    let fixture = Fixture::new().await;
    fixture.setup().await;
    let (cookie, csrf) = fixture.login().await;
    let (status, _, _) = fixture
        .request(
            "PUT",
            "/api/settings/backend",
            json!({"controller_url": "http://169.254.169.254", "secret": "bad"}),
            Some((&cookie, &csrf)),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (_, saved, _) = fixture
        .request(
            "GET",
            "/api/settings/backend",
            Value::Null,
            Some((&cookie, &csrf)),
        )
        .await;
    assert_eq!(saved["connected"], true);
    let (status, _, _) = fixture
        .request(
            "PUT",
            "/api/settings/backend",
            json!({"controller_url": fixture.url, "secret": "wrong-secret"}),
            Some((&cookie, &csrf)),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert!(
        fixture
            .state
            .client()
            .await
            .unwrap()
            .version()
            .await
            .is_ok()
    );
    fixture.finish().await;
}

#[tokio::test]
async fn web_settings_persist_and_listen_changes_only_stage_restart() {
    let fixture = Fixture::new().await;
    fixture.setup().await;
    let (cookie, csrf) = fixture.login().await;
    let mut settings = (*fixture.state.active_settings).clone();
    settings.traffic_interval_ms = 2000;
    settings.listen = "127.0.0.1:8899".parse().unwrap();
    settings.public_origin = "http://127.0.0.1:8899".into();
    let (status, saved, _) = fixture
        .request(
            "PUT",
            "/api/settings",
            serde_json::to_value(settings).unwrap(),
            Some((&cookie, &csrf)),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(saved["restart_required"], true);
    assert_eq!(fixture.state.active_settings.listen.port(), 8787);
    let reloaded = mihomo_web_panel::db::load_settings(&fixture.state.pool, &fixture.state.config)
        .await
        .unwrap();
    assert_eq!(reloaded.listen.port(), 8899);
    assert_eq!(reloaded.traffic_interval_ms, 2000);
    assert_eq!(
        fixture
            .request("GET", "/api/settings", Value::Null, Some((&cookie, &csrf)))
            .await
            .0,
        StatusCode::OK
    );
    fixture.finish().await;
}

#[tokio::test]
async fn bounded_batch_delay_and_invalid_commands() {
    let fixture = Fixture::new().await;
    fixture.setup().await;
    let (cookie, csrf) = fixture.login().await;
    let session = Some((cookie.as_str(), csrf.as_str()));
    let (status, results, _) = fixture
        .request(
            "POST",
            "/api/delay/batch",
            json!({"names": [NODE_A, NODE_B, "DIRECT"], "timeout_ms": 1000}),
            session,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(results["results"].as_array().unwrap().len(), 3);
    assert!(
        results["results"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["name"] == "DIRECT" && entry["status"] == "timeout")
    );
    assert!(fixture.mock.delay_max.load(Ordering::SeqCst) <= 4);
    assert_eq!(
        fixture
            .request(
                "POST",
                "/api/settings/autostart",
                json!({"operation": "enable; rm -rf /"}),
                session
            )
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        fixture
            .request(
                "POST",
                "/api/settings/autostart",
                json!({"operation": "enable", "unit": "ssh.service"}),
                session
            )
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        fixture
            .request(
                "POST",
                "/api/proxy",
                json!({"url": "http://169.254.169.254"}),
                session
            )
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    fixture.finish().await;
}

#[tokio::test]
async fn websocket_auth_shared_upstream_logs_redaction_and_revocation() {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    let fixture = Fixture::new().await;
    fixture.setup().await;
    let token = auth::create_session(
        &fixture.state.pool,
        "127.0.0.1".parse().unwrap(),
        "websocket test",
        86400,
    )
    .await
    .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = fixture.app();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .unwrap();
    });
    let connect = |path: &str, authenticated: bool| {
        let mut request = format!("ws://{address}{path}")
            .into_client_request()
            .unwrap();
        request
            .headers_mut()
            .insert("host", "127.0.0.1:8787".parse().unwrap());
        request
            .headers_mut()
            .insert("origin", "http://127.0.0.1:8787".parse().unwrap());
        if authenticated {
            request
                .headers_mut()
                .insert("cookie", format!("mwp_session={token}").parse().unwrap());
        }
        request
    };
    assert!(
        tokio_tungstenite::connect_async(connect("/api/traffic", false))
            .await
            .is_err()
    );
    let (mut first, _) = tokio_tungstenite::connect_async(connect("/api/traffic", true))
        .await
        .unwrap();
    let (mut second, _) = tokio_tungstenite::connect_async(connect("/api/traffic", true))
        .await
        .unwrap();
    async fn wait_for(
        socket: &mut tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        kind: &str,
    ) -> Value {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let message = socket.next().await.unwrap().unwrap();
                if let Ok(text) = message.to_text() {
                    let value: Value = serde_json::from_str(text).unwrap();
                    if value["type"] == kind {
                        return value;
                    }
                }
            }
        })
        .await
        .unwrap()
    }
    assert!(
        wait_for(&mut first, "traffic").await["data"]["down"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert!(
        wait_for(&mut second, "traffic").await["data"]["up"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert_eq!(fixture.mock.traffic_sockets.load(Ordering::SeqCst), 1);
    *fixture.mock.behavior.lock().await = "disconnect".into();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let value: Value =
                serde_json::from_str(first.next().await.unwrap().unwrap().to_text().unwrap())
                    .unwrap();
            if value["type"] == "status" && value["state"] == "offline" {
                break;
            }
        }
    })
    .await
    .unwrap();
    *fixture.mock.behavior.lock().await = "normal".into();
    assert!(
        wait_for(&mut first, "traffic").await["data"]["down"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert!(fixture.mock.traffic_sockets.load(Ordering::SeqCst) >= 2);
    let (mut logs, _) = tokio_tungstenite::connect_async(connect("/api/logs", true))
        .await
        .unwrap();
    let first_log = wait_for(&mut logs, "log").await;
    assert!(!first_log.to_string().contains(SECRET));
    let redacted = wait_for(&mut logs, "log").await.to_string();
    assert!(!redacted.contains(SECRET));
    assert!(!redacted.contains(&fixture.url));
    assert!(redacted.contains("[redacted]"));
    sqlx::query("DELETE FROM sessions")
        .execute(&fixture.state.pool)
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(7), async {
        loop {
            if matches!(
                first.next().await,
                Some(Ok(tokio_tungstenite::tungstenite::Message::Close(_))) | None | Some(Err(_))
            ) {
                break;
            }
        }
    })
    .await
    .unwrap();
    let _ = second.close(None).await;
    let _ = logs.close(None).await;
    server.abort();
    fixture.finish().await;
}

#[tokio::test]
async fn login_endpoint_locks_after_repeated_failures() {
    let fixture = Fixture::new().await;
    fixture.setup().await;
    for _ in 0..8 {
        let (status, _, _) = fixture
            .request(
                "POST",
                "/api/auth/login",
                json!({"username": "admin", "password": "wrong password"}),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }
    let (status, body, _) = fixture
        .request(
            "POST",
            "/api/auth/login",
            json!({"username": "admin", "password": PASSWORD}),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(body["error"], "rate_limited");
    assert_eq!(
        fixture
            .request(
                "POST",
                "/api/auth/login",
                json!({"username": "different-user", "password": "wrong password"}),
                None
            )
            .await
            .0,
        StatusCode::TOO_MANY_REQUESTS
    );
    fixture.finish().await;
}

#[tokio::test]
async fn request_limits_security_headers_and_malformed_json() {
    let fixture = Fixture::new().await;
    let request = Request::builder()
        .method("POST")
        .uri("/api/auth/login")
        .header("host", "127.0.0.1:8787")
        .header("origin", "http://127.0.0.1:8787")
        .header("content-type", "application/json")
        .body(Body::from("x".repeat(32769)))
        .unwrap();
    assert_eq!(
        fixture.app().oneshot(request).await.unwrap().status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
    let request = Request::builder()
        .uri("/")
        .header("host", "127.0.0.1:8787")
        .body(Body::empty())
        .unwrap();
    let response = fixture.app().oneshot(request).await.unwrap();
    assert!(
        response.headers()["content-security-policy"]
            .to_str()
            .unwrap()
            .contains("frame-ancestors 'none'")
    );
    assert_eq!(response.headers()["x-content-type-options"], "nosniff");
    let request = Request::builder()
        .method("POST")
        .uri("/api/auth/login")
        .header("host", "127.0.0.1:8787")
        .header("origin", "http://127.0.0.1:8787")
        .header("content-type", "application/json")
        .body(Body::from(format!("{{\"{SECRET}\":")))
        .unwrap();
    let response = fixture.app().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(response.into_body(), 1024).await.unwrap();
    assert!(!String::from_utf8_lossy(&body).contains(SECRET));
    fixture.finish().await;
}
