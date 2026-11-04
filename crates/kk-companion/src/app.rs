use crate::security::{PairError, Security, SessionInfo};
use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Request, State};
use axum::http::{header, HeaderMap, HeaderName, HeaderValue, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

pub const BODY_LIMIT: usize = 1 << 20;
pub const CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data:; font-src 'self'; connect-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

#[derive(Clone)]
pub struct AppState {
    pub sec: Arc<Security>,
    pub addr: SocketAddr,
    pub ui_dir: Option<PathBuf>,
    pub extra: Arc<dyn Extension>,
}

pub trait Extension: Send + Sync {
    fn routes(&self, state: AppState) -> Router<AppState>;
}

pub struct NoExtension;

impl Extension for NoExtension {
    fn routes(&self, _state: AppState) -> Router<AppState> {
        Router::new()
    }
}

impl AppState {
    pub fn origin(&self) -> String {
        format!("http://127.0.0.1:{}", self.addr.port())
    }

    pub fn cookie_name(&self) -> String {
        format!("kk_session_{}", self.addr.port())
    }
}

pub fn error(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        Json(json!({"error": {"code": code, "message": message}})),
    )
        .into_response()
}

fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    for h in headers.get_all(header::COOKIE) {
        for part in h.to_str().ok()?.split(';') {
            let part = part.trim();
            if let Some(v) = part.strip_prefix(&format!("{name}=")) {
                return Some(v.to_string());
            }
        }
    }
    None
}

fn bearer(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .map(str::to_string)
}

#[derive(Clone)]
pub struct Authenticated {
    pub session_token: String,
    pub info: SessionInfo,
}

fn secure(mut res: Response) -> Response {
    let h = res.headers_mut();
    let set = |h: &mut HeaderMap, k: &'static str, v: &str| {
        h.insert(
            HeaderName::from_static(k),
            HeaderValue::from_str(v).expect("static header value"),
        );
    };
    set(h, "content-security-policy", CSP);
    set(h, "x-content-type-options", "nosniff");
    set(h, "referrer-policy", "no-referrer");
    set(h, "cache-control", "no-store");
    set(h, "cross-origin-opener-policy", "same-origin");
    set(h, "cross-origin-resource-policy", "same-origin");
    set(h, "x-frame-options", "DENY");
    set(
        h,
        "permissions-policy",
        "camera=(), microphone=(), geolocation=(), payment=(), usb=()",
    );
    res
}

async fn guard(State(state): State<AppState>, mut req: Request, next: Next) -> Response {
    let expected_host = format!("127.0.0.1:{}", state.addr.port());
    let host_ok = req
        .headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        == Some(expected_host.as_str());
    if !host_ok {
        return secure(error(
            StatusCode::MISDIRECTED_REQUEST,
            "BAD_HOST",
            "this service answers only on its own loopback address",
        ));
    }
    let method = req.method().clone();
    let safe_method = matches!(method, Method::GET | Method::HEAD);
    let origin = req
        .headers()
        .get(header::ORIGIN)
        .and_then(|h| h.to_str().ok())
        .map(str::to_string);
    if let Some(site) = req
        .headers()
        .get("sec-fetch-site")
        .and_then(|h| h.to_str().ok())
        .filter(|_| !req.uri().path().starts_with("/ext/"))
    {
        if !matches!(site, "same-origin" | "none") {
            return secure(error(
                StatusCode::FORBIDDEN,
                "CROSS_SITE",
                "cross-site requests are refused",
            ));
        }
    }
    let path = req.uri().path().to_string();
    let is_admin = path.starts_with("/admin/");
    if path.starts_with("/ext/") {
        let from_extension = origin.as_deref().is_some_and(|o| {
            o.starts_with("chrome-extension://") || o.starts_with("moz-extension://")
        });
        if req.headers().contains_key(header::COOKIE)
            || (origin.is_some() && !from_extension)
            || (origin.is_none() && !safe_method)
        {
            return secure(error(
                StatusCode::FORBIDDEN,
                "EXTENSION_ONLY",
                "this interface answers only the paired browser extension",
            ));
        }
        return secure(next.run(req).await);
    }
    if is_admin {
        if origin.is_some() || req.headers().contains_key(header::COOKIE) {
            return secure(error(
                StatusCode::FORBIDDEN,
                "ADMIN_NOT_FOR_BROWSERS",
                "the admin interface is for local tools, not for web pages",
            ));
        }
        let ok = bearer(req.headers()).is_some_and(|t| state.sec.admin_matches(&t));
        if !ok {
            return secure(error(
                StatusCode::UNAUTHORIZED,
                "ADMIN_AUTH",
                "a valid admin token is required",
            ));
        }
    } else {
        match &origin {
            Some(o) if *o != state.origin() => {
                return secure(error(
                    StatusCode::FORBIDDEN,
                    "BAD_ORIGIN",
                    "requests must come from this page",
                ));
            }
            None if !safe_method => {
                return secure(error(
                    StatusCode::FORBIDDEN,
                    "ORIGIN_REQUIRED",
                    "state-changing requests must carry the page's origin",
                ));
            }
            _ => {}
        }
        let open_api = matches!(path.as_str(), "/api/health" | "/api/pair" | "/api/session");
        if path.starts_with("/api/") && !open_api {
            let Some(token) = cookie_value(req.headers(), &state.cookie_name()) else {
                return secure(error(
                    StatusCode::UNAUTHORIZED,
                    "NO_SESSION",
                    "pair this browser first",
                ));
            };
            let Some(info) = state.sec.session(&token) else {
                return secure(error(
                    StatusCode::UNAUTHORIZED,
                    "NO_SESSION",
                    "the session has ended; pair again",
                ));
            };
            if !safe_method {
                let presented = req
                    .headers()
                    .get("x-kk-csrf")
                    .and_then(|h| h.to_str().ok())
                    .unwrap_or("");
                if !state.sec.csrf_matches(&token, presented) {
                    return secure(error(
                        StatusCode::FORBIDDEN,
                        "CSRF",
                        "missing or wrong CSRF token",
                    ));
                }
            }
            req.extensions_mut().insert(Authenticated {
                session_token: token,
                info,
            });
        }
    }
    secure(next.run(req).await)
}

#[derive(Deserialize)]
struct PairBody {
    code: String,
}

async fn health() -> Json<Value> {
    Json(json!({"ok": true, "service": "kakurega-companion"}))
}

async fn pair(State(state): State<AppState>, Json(body): Json<PairBody>) -> Response {
    match state.sec.pair(&body.code) {
        Ok((token, info)) => {
            let cookie = format!(
                "{}={}; HttpOnly; SameSite=Strict; Path=/; Max-Age=28800",
                state.cookie_name(),
                token
            );
            let mut res = Json(json!({"csrf": info.csrf})).into_response();
            res.headers_mut().insert(
                header::SET_COOKIE,
                HeaderValue::from_str(&cookie).expect("cookie is ascii"),
            );
            res
        }
        Err(PairError::Locked) => error(
            StatusCode::TOO_MANY_REQUESTS,
            "LOCKED",
            "too many wrong codes; wait a minute",
        ),
        Err(PairError::Invalid) => error(
            StatusCode::UNAUTHORIZED,
            "BAD_CODE",
            "that code is wrong, used or expired",
        ),
    }
}

async fn session(State(state): State<AppState>, req: Request) -> Response {
    let info =
        cookie_value(req.headers(), &state.cookie_name()).and_then(|t| state.sec.session(&t));
    match info {
        Some(i) => Json(json!({"paired": true, "csrf": i.csrf})).into_response(),
        None => Json(json!({"paired": false})).into_response(),
    }
}

fn clear_cookie(state: &AppState) -> HeaderValue {
    HeaderValue::from_str(&format!(
        "{}=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0",
        state.cookie_name()
    ))
    .expect("cookie is ascii")
}

async fn whoami() -> Json<Value> {
    Json(json!({"ok": true}))
}

async fn logout(State(state): State<AppState>, req: Request) -> Response {
    let auth = req
        .extensions()
        .get::<Authenticated>()
        .cloned()
        .expect("guard set the session");
    state.sec.revoke(&auth.session_token);
    let mut res = StatusCode::NO_CONTENT.into_response();
    res.headers_mut()
        .insert(header::SET_COOKIE, clear_cookie(&state));
    res
}

async fn revoke_all(State(state): State<AppState>) -> Response {
    state.sec.revoke_all();
    let mut res = StatusCode::NO_CONTENT.into_response();
    res.headers_mut()
        .insert(header::SET_COOKIE, clear_cookie(&state));
    res
}

async fn admin_ping() -> Json<Value> {
    Json(json!({"ok": true}))
}

async fn admin_pairing(State(state): State<AppState>) -> Json<Value> {
    Json(
        json!({"code": state.sec.issue_pairing_code(), "url": format!("{}/#pair=", state.origin())}),
    )
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "woff2" => "font/woff2",
        "ico" => "image/x-icon",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

pub fn safe_join(root: &Path, request_path: &str) -> Option<PathBuf> {
    let rel = request_path.trim_start_matches('/');
    let mut out = root.to_path_buf();
    for c in Path::new(rel).components() {
        match c {
            Component::Normal(p) => {
                let s = p.to_str()?;
                if s.contains('\\') || s.starts_with('.') {
                    return None;
                }
                out.push(p);
            }
            Component::CurDir => {}
            _ => return None,
        }
    }
    Some(out)
}

async fn static_files(State(state): State<AppState>, req: Request) -> Response {
    let Some(root) = state.ui_dir.clone() else {
        return error(
            StatusCode::NOT_FOUND,
            "NO_UI",
            "no web interface is installed",
        );
    };
    if req.uri().path().starts_with("/api/") || req.uri().path().starts_with("/admin/") {
        return error(StatusCode::NOT_FOUND, "NOT_FOUND", "no such endpoint");
    }
    let decoded = req.uri().path();
    let candidate = if decoded == "/" {
        root.join("index.html")
    } else {
        safe_join(&root, decoded).unwrap_or_else(|| root.join("\0invalid"))
    };
    let target = match std::fs::symlink_metadata(&candidate) {
        Ok(m) if m.is_file() => candidate,
        _ => {
            if decoded.contains('.') {
                return error(StatusCode::NOT_FOUND, "NOT_FOUND", "no such file");
            }
            root.join("index.html")
        }
    };
    match std::fs::read(&target) {
        Ok(bytes) => {
            let mut res = Response::new(Body::from(bytes));
            res.headers_mut().insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static(content_type(&target)),
            );
            res
        }
        Err(_) => error(StatusCode::NOT_FOUND, "NOT_FOUND", "no such file"),
    }
}

pub fn router(state: AppState) -> Router {
    let extra = state.extra.routes(state.clone());
    Router::new()
        .route("/api/health", get(health))
        .route("/api/pair", post(pair))
        .route("/api/session", get(session))
        .route("/api/whoami", get(whoami))
        .route("/api/logout", post(logout))
        .route("/api/sessions/revoke-all", post(revoke_all))
        .route("/admin/ping", get(admin_ping))
        .route("/admin/pairing", post(admin_pairing))
        .merge(extra)
        .fallback(static_files)
        .layer(DefaultBodyLimit::max(BODY_LIMIT))
        .layer(middleware::from_fn_with_state(state.clone(), guard))
        .with_state(state)
}
