use axum::{
    body::Body,
    extract::ConnectInfo,
    http::{HeaderMap, Method, Request},
};
use mihomo_web_panel::{
    AppState, auth,
    config::{self, Config},
    helper::{HelperRequest, Operation, fixed_args},
    mihomo::{Proxies, Traffic, parse_connections},
    security,
};
use serde_json::json;
use std::net::SocketAddr;

#[test]
fn passwords_use_argon2id_and_unique_salts() {
    let first = auth::hash_password("correct horse battery staple").unwrap();
    let second = auth::hash_password("correct horse battery staple").unwrap();
    assert!(first.starts_with("$argon2id$v=19$m=19456,t=2,p=1$"));
    assert_ne!(first, second);
    assert!(auth::verify_password(
        "correct horse battery staple",
        &first
    ));
    assert!(!auth::verify_password("wrong password", &first));
    assert!(!auth::verify_password("anything", "broken hash"));
    assert!(auth::validate_password("short", "short").is_err());
    assert!(auth::validate_password("long enough password", "different password").is_err());
}

#[test]
fn tokens_have_256_bits_and_domain_separated_csrf() {
    use base64::Engine;
    let first = auth::random_token();
    let second = auth::random_token();
    assert_ne!(first, second);
    assert_eq!(
        base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(&first)
            .unwrap()
            .len(),
        32
    );
    assert_ne!(first, auth::digest(&first));
    assert_ne!(auth::digest(&first), auth::csrf_token(&first));
}

#[test]
fn controller_policy_rejects_ssrf_vectors() {
    for url in ["http://127.0.0.1:9090", "http://[::1]:9090"] {
        assert!(config::validate_controller(url, &[]).is_ok(), "{url}");
    }
    for url in [
        "http://example.com:9090",
        "http://localhost:9090",
        "http://8.8.8.8",
        "http://0.0.0.0",
        "http://[::]",
        "http://169.254.169.254",
        "http://100.100.100.200",
        "http://224.0.0.1",
        "http://[fe80::1]",
        "http://[::ffff:127.0.0.1]",
        "file:///etc/passwd",
        "http://127.0.0.1/private",
        "http://secret@127.0.0.1",
        "http://127.0.0.1/?url=x",
        "http://127.0.0.1/#fragment",
        "http://10.0.0.8",
    ] {
        assert!(config::validate_controller(url, &[]).is_err(), "{url}");
    }
    let trusted = [
        "10.42.0.8/32".parse().unwrap(),
        "fd12::8/128".parse().unwrap(),
    ];
    assert!(config::validate_controller("http://10.42.0.8:9090", &trusted).is_ok());
    assert!(config::validate_controller("http://[fd12::8]:9090", &trusted).is_ok());
    assert!(config::validate_controller("http://10.42.0.9:9090", &trusted).is_err());
    assert!(
        config::validate_controller("http://[fd00:ec2::254]", &["fd00::/8".parse().unwrap()])
            .is_err()
    );
    let config = Config {
        trusted_backend_networks: vec!["0.0.0.0/0".parse().unwrap()],
        ..Config::default()
    };
    assert!(config.validate().is_err());
}

#[test]
fn configuration_roundtrip_and_public_https_policy() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    config::atomic_private_write(&path, include_bytes!("../config.example.toml")).unwrap();
    let mut config = Config::load(&path).unwrap();
    assert_eq!(
        config.listen,
        "127.0.0.1:8787".parse::<SocketAddr>().unwrap()
    );
    config.listen = "[::]:8787".parse().unwrap();
    assert!(config.validate().is_err());
    config.public_origin = "https://panel.example.com".into();
    config.secure_cookies = true;
    assert!(config.validate().is_ok());
    config::atomic_private_write(&path, b"listen = 'not-an-address'").unwrap();
    assert!(Config::load(&path).is_err());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

#[test]
fn cookie_flags_and_duplicate_cookie_rejection() {
    let token = auth::random_token();
    let cookie = auth::cookie(&token, true, 86400);
    let text = cookie.to_str().unwrap();
    for attribute in [
        "HttpOnly",
        "Secure",
        "SameSite=Strict",
        "Path=/",
        "Max-Age=86400",
    ] {
        assert!(text.contains(attribute));
    }
    let mut headers = HeaderMap::new();
    headers.insert("cookie", format!("mwp_session={token}").parse().unwrap());
    assert_eq!(auth::read_cookie(&headers).unwrap(), token);
    headers.append("cookie", format!("mwp_session={token}").parse().unwrap());
    assert!(auth::read_cookie(&headers).is_none());
    assert!(
        auth::clear_cookie(true)
            .to_str()
            .unwrap()
            .contains("Max-Age=0")
    );
}

#[test]
fn origin_host_and_websocket_rules() {
    let mut headers = HeaderMap::new();
    headers.insert("host", "panel.example.com".parse().unwrap());
    let origin = "https://panel.example.com";
    assert!(security::validate_origin(&headers, &Method::GET, origin, false).is_ok());
    assert!(security::validate_origin(&headers, &Method::POST, origin, false).is_err());
    assert!(security::validate_origin(&headers, &Method::GET, origin, true).is_err());
    headers.insert("origin", origin.parse().unwrap());
    assert!(security::validate_origin(&headers, &Method::POST, origin, false).is_ok());
    headers.insert("host", "attacker.example.com".parse().unwrap());
    assert!(security::validate_origin(&headers, &Method::GET, origin, false).is_err());
    headers.insert("host", "panel.example.com".parse().unwrap());
    headers.insert("origin", "https://attacker.example.com".parse().unwrap());
    assert!(security::validate_origin(&headers, &Method::GET, origin, false).is_err());
}

#[test]
fn brute_force_limits_both_ip_and_username() {
    let limiter = security::RateLimiter::default();
    let ip = "192.0.2.10".parse().unwrap();
    let first = limiter.login_attempt(ip, "admin");
    assert!(first.is_ok());
    let early = limiter.login_failed(ip, "admin");
    let mut last = early;
    for _ in 0..7 {
        last = limiter.login_failed(ip, "admin");
    }
    assert!(last > early);
    assert!(limiter.login_attempt(ip, "different-account").is_err());
    assert!(
        limiter
            .login_attempt("192.0.2.11".parse().unwrap(), "ADMIN")
            .is_err()
    );
}

#[test]
fn helper_has_a_fixed_command_whitelist() {
    assert!(mihomo_web_panel::helper::caller_allowed(1001, 1001));
    assert!(!mihomo_web_panel::helper::caller_allowed(1001, 0));
    assert!(!mihomo_web_panel::helper::caller_allowed(1001, 1002));
    assert!(!mihomo_web_panel::helper::caller_allowed(0, 0));
    for operation in [Operation::Enable, Operation::Disable, Operation::Status] {
        assert_eq!(fixed_args(operation)[1], "--");
        assert_eq!(fixed_args(operation)[2], "mihomo-web.service");
    }
    for payload in [
        r#"{"operation":"restart"}"#,
        r#"{"operation":"enable; touch /tmp/pwned"}"#,
        r#"{"operation":"enable","unit":"ssh.service"}"#,
        r#"{"operation":"enable","command":"bash"}"#,
    ] {
        assert!(serde_json::from_str::<HelperRequest>(payload).is_err());
    }
}

#[test]
fn official_response_shapes_and_older_traffic_are_supported() {
    let proxies: Proxies = serde_json::from_value(json!({"proxies": {
        "PROXY": {"type": "Selector", "all": ["DIRECT", "node"], "now": "node", "history": []},
        "node": {"type": "Shadowsocks", "history": [{"time": "2026-11-21T00:00:00Z", "delay": 42}], "udp": true},
        "DIRECT": {"type": "Direct"}, "REJECT": {"type": "Reject"}
    }})).unwrap();
    assert_eq!(proxies.proxies["node"].history[0].delay, 42);
    let older: Traffic = serde_json::from_value(json!({"up": 12, "down": 34})).unwrap();
    assert_eq!(older.up_total, None);
    let latest: Traffic =
        serde_json::from_value(json!({"up": 12, "down": 34, "upTotal": 100, "downTotal": 200}))
            .unwrap();
    assert_eq!(latest.down_total, Some(200));
    let connections = parse_connections(json!({"downloadTotal": 1000, "uploadTotal": 100, "connections": [{
        "id": "abc", "metadata": {"network": "tcp", "sourceIP": "192.168.1.2", "sourcePort": 5555, "host": "example.com", "destinationPort": "443"},
        "chains": ["node", "PROXY"], "upload": 10, "download": 20, "start": "2026-11-21T00:00:00Z"
    }]})).unwrap();
    assert_eq!(connections.connections[0].source, "192.168.1.2:5555");
    assert_eq!(connections.connections[0].destination, "example.com:443");
    assert!(parse_connections(json!({"unexpected": "format"})).is_err());
    assert!(
        parse_connections(json!({"downloadTotal": 1, "uploadTotal": 1, "connections": [{}]}))
            .is_err()
    );
}

#[tokio::test]
async fn sessions_store_digests_expire_and_revoke() {
    let dir = tempfile::tempdir().unwrap();
    let state = AppState::new(Config {
        data_dir: dir.path().to_path_buf(),
        ..Config::default()
    })
    .await
    .unwrap();
    let token = auth::create_session(&state.pool, "192.0.2.1".parse().unwrap(), "test", 86400)
        .await
        .unwrap();
    let hash = auth::digest(&token);
    let stored: String = sqlx::query_scalar("SELECT token_hash FROM sessions")
        .fetch_one(&state.pool)
        .await
        .unwrap();
    assert_eq!(hash, stored);
    assert_ne!(token, stored);
    assert!(
        auth::session_by_hash(&state.pool, &hash, 1800, true)
            .await
            .is_ok()
    );
    sqlx::query("UPDATE sessions SET last_seen = ?")
        .bind(auth::now() - 1801)
        .execute(&state.pool)
        .await
        .unwrap();
    assert!(
        auth::session_by_hash(&state.pool, &hash, 1800, true)
            .await
            .is_err()
    );
    let token = auth::create_session(&state.pool, "192.0.2.1".parse().unwrap(), "test", -1)
        .await
        .unwrap();
    assert!(
        auth::session_by_hash(&state.pool, &auth::digest(&token), 1800, true)
            .await
            .is_err()
    );
    let token = auth::create_session(&state.pool, "192.0.2.1".parse().unwrap(), "test", 86400)
        .await
        .unwrap();
    sqlx::query("DELETE FROM sessions")
        .execute(&state.pool)
        .await
        .unwrap();
    assert!(
        auth::session_by_hash(&state.pool, &auth::digest(&token), 1800, true)
            .await
            .is_err()
    );
    state.pool.close().await;
}

#[tokio::test]
async fn bootstrap_persists_and_consumed_token_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let config = Config {
        data_dir: dir.path().to_path_buf(),
        ..Config::default()
    };
    let state = AppState::new(config.clone()).await.unwrap();
    let token = std::fs::read_to_string(dir.path().join("bootstrap-token")).unwrap();
    assert!(
        auth::check_bootstrap(&state.pool, token.trim())
            .await
            .is_ok()
    );
    assert!(
        auth::check_bootstrap(&state.pool, &auth::random_token())
            .await
            .is_err()
    );
    auth::prepare_bootstrap(&state).await.unwrap();
    assert_eq!(
        token,
        std::fs::read_to_string(dir.path().join("bootstrap-token")).unwrap()
    );
    sqlx::query("UPDATE bootstrap SET consumed_at = ?")
        .bind(auth::now())
        .execute(&state.pool)
        .await
        .unwrap();
    assert!(
        auth::check_bootstrap(&state.pool, token.trim())
            .await
            .is_err()
    );
    state.pool.close().await;
}

#[tokio::test]
async fn forwarded_ips_are_trusted_only_for_explicit_proxies() {
    let dir = tempfile::tempdir().unwrap();
    let state = AppState::new(Config {
        data_dir: dir.path().to_path_buf(),
        ..Config::default()
    })
    .await
    .unwrap();
    let mut request = Request::builder()
        .header("x-forwarded-for", "203.0.113.66")
        .body(Body::empty())
        .unwrap();
    request.extensions_mut().insert(ConnectInfo(
        "192.0.2.2:54321".parse::<SocketAddr>().unwrap(),
    ));
    assert_eq!(
        security::client_ip(&request, &state).unwrap().to_string(),
        "192.0.2.2"
    );
    state.pool.close().await;
    let mut config = Config {
        data_dir: dir.path().join("trusted"),
        ..Config::default()
    };
    config.trusted_proxies = vec!["127.0.0.1/32".parse().unwrap()];
    let state = AppState::new(config).await.unwrap();
    request.extensions_mut().insert(ConnectInfo(
        "127.0.0.1:54321".parse::<SocketAddr>().unwrap(),
    ));
    request.headers_mut().insert(
        "x-forwarded-for",
        "203.0.113.66, 192.0.2.4".parse().unwrap(),
    );
    assert_eq!(
        security::client_ip(&request, &state).unwrap().to_string(),
        "192.0.2.4"
    );
    state.pool.close().await;
}
