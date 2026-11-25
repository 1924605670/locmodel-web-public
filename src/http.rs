use crate::{dialog, dispatch, shutdown, Ctx, Host};
use axum::{
    body::Body,
    extract::{DefaultBodyLimit, Multipart, Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    routing::{get, post},
    Json, Router,
};
use include_dir::{include_dir, Dir};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    convert::Infallible,
    fs,
    net::SocketAddr,
    path::{Component, PathBuf},
    sync::Arc,
};
use tokio::net::TcpListener;
use tokio_stream::{wrappers::BroadcastStream, StreamExt};
use tokio_util::io::ReaderStream;

static FRONTEND: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/frontend");

struct App {
    ctx: Arc<Ctx>,
    host: Host,
    token: Option<String>,
}

#[derive(Deserialize)]
struct InvokeBody {
    cmd: String,
    #[serde(default)]
    args: Value,
}

#[derive(Deserialize)]
struct DialogBody {
    id: u64,
    #[serde(default)]
    paths: Vec<String>,
}

#[derive(Deserialize)]
struct TokenQuery {
    token: Option<String>,
    path: Option<String>,
}

fn authorized(app: &App, headers: &HeaderMap, query_token: Option<&str>) -> bool {
    let Some(expected) = &app.token else {
        return true;
    };
    let header = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim);
    match header.or(query_token) {
        Some(got) if got == expected => true,
        _ => false,
    }
}

fn deny() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({"ok": false, "error": "需要访问口令"})),
    )
        .into_response()
}

async fn session(State(app): State<Arc<App>>, headers: HeaderMap, Query(q): Query<TokenQuery>) -> Json<Value> {
    let required = app.token.is_some();
    Json(json!({
        "required": required,
        "ok": !required || authorized(&app, &headers, q.token.as_deref()),
    }))
}

async fn health() -> Json<Value> {
    Json(json!({"ok": true, "service": "locmodel-web"}))
}

async fn invoke(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(body): Json<InvokeBody>,
) -> Response {
    if !authorized(&app, &headers, None) {
        return deny();
    }
    let ctx = Arc::clone(&app.ctx);
    let host = app.host.clone();
    let result = tokio::task::spawn_blocking(move || dispatch(&body.cmd, &body.args, &ctx, &host)).await;
    match result {
        Ok(Ok(value)) => Json(json!({"ok": true, "value": value})).into_response(),
        Ok(Err(error)) => Json(json!({"ok": false, "error": error})).into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"ok": false, "error": error.to_string()})),
        )
            .into_response(),
    }
}

async fn dialog_resolve(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(body): Json<DialogBody>,
) -> Response {
    if !authorized(&app, &headers, None) {
        return deny();
    }
    let paths = body.paths.into_iter().map(PathBuf::from).collect();
    match dialog::resolve(body.id, paths) {
        Ok(()) => Json(json!({"ok": true})).into_response(),
        Err(error) => Json(json!({"ok": false, "error": error})).into_response(),
    }
}

fn safe_rel(name: &str) -> Result<PathBuf, String> {
    let mut out = PathBuf::new();
    if name.trim().is_empty() {
        return Err("文件名无效".into());
    }
    for component in PathBuf::from(name).components() {
        match component {
            Component::Normal(part) => out.push(part),
            _ => return Err("文件名无效".into()),
        }
    }
    if out.as_os_str().is_empty() {
        return Err("文件名无效".into());
    }
    Ok(out)
}

async fn upload(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Response {
    if !authorized(&app, &headers, None) {
        return deny();
    }
    let batch = app.ctx.root.join("uploads").join(uuid());
    if let Err(error) = fs::create_dir_all(&batch) {
        return Json(json!({"ok": false, "error": error.to_string()})).into_response();
    }
    let mut rels = Vec::new();
    loop {
        let field = match multipart.next_field().await {
            Ok(Some(field)) => field,
            Ok(None) => break,
            Err(error) => {
                return Json(json!({"ok": false, "error": error.to_string()})).into_response()
            }
        };
        if field.name() != Some("file") {
            continue;
        }
        let file_name = field.file_name().unwrap_or("file").to_string();
        let rel = match safe_rel(&file_name) {
            Ok(path) => path,
            Err(error) => return Json(json!({"ok": false, "error": error})).into_response(),
        };
        let dest = batch.join(&rel);
        if let Some(parent) = dest.parent() {
            if let Err(error) = fs::create_dir_all(parent) {
                return Json(json!({"ok": false, "error": error.to_string()})).into_response();
            }
        }
        let bytes = match field.bytes().await {
            Ok(bytes) => bytes,
            Err(error) => {
                return Json(json!({"ok": false, "error": error.to_string()})).into_response()
            }
        };
        if let Err(error) = tokio::fs::write(&dest, &bytes).await {
            return Json(json!({"ok": false, "error": error.to_string()})).into_response();
        }
        rels.push(rel);
    }
    if rels.is_empty() {
        return Json(json!({"ok": false, "error": "没有收到文件"})).into_response();
    }
    let paths: Vec<_> = rels.iter().map(|rel| batch.join(rel)).collect::<Vec<_>>();
    let dir = upload_dir(&batch, &rels);
    Json(json!({"ok": true, "paths": paths, "dir": dir})).into_response()
}

fn upload_dir(batch: &std::path::Path, rels: &[PathBuf]) -> PathBuf {
    let mut tops = Vec::new();
    for rel in rels {
        if let Some(top) = rel.components().next() {
            let text = top.as_os_str().to_string_lossy().to_string();
            if !tops.contains(&text) {
                tops.push(text);
            }
        }
    }
    if tops.len() == 1 {
        let top = batch.join(&tops[0]);
        if top.is_dir() {
            return top;
        }
    }
    batch.to_path_buf()
}

fn uuid() -> String {
    let mut bytes = [0u8; 16];
    getrandom::getrandom(&mut bytes).expect("系统随机源不可用");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

async fn events(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Query(q): Query<TokenQuery>,
) -> Response {
    if !authorized(&app, &headers, q.token.as_deref()) {
        return deny();
    }
    let rx = app.host.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|item| match item {
        Ok(message) => Some(Ok::<Event, Infallible>(Event::default().data(message))),
        Err(_) => None,
    });
    Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response()
}

async fn raw_file(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Query(q): Query<TokenQuery>,
) -> Response {
    if !authorized(&app, &headers, q.token.as_deref()) {
        return deny();
    }
    let Some(path) = q.path.as_deref() else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"ok": false, "error": "缺少路径"})),
        )
            .into_response();
    };
    let path = PathBuf::from(path);
    if !path.is_file() {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({"ok": false, "error": "文件不存在"})),
        )
            .into_response();
    }
    let file = match tokio::fs::File::open(&path).await {
        Ok(file) => file,
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"ok": false, "error": error.to_string()})),
            )
                .into_response()
        }
    };
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("download");
    let body = Body::from_stream(ReaderStream::new(file));
    Response::builder()
        .header(header::CONTENT_TYPE, content_type(name))
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{}\"", name.replace('"', "")),
        )
        .body(body)
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

async fn asset(Path(path): Path<String>) -> Response {
    let path = path.trim_start_matches('/');
    if path.is_empty() {
        return index().await;
    }
    if path.split('/').any(|part| part == "..") {
        return StatusCode::NOT_FOUND.into_response();
    }
    match FRONTEND.get_file(path) {
        Some(file) => Response::builder()
            .header(header::CONTENT_TYPE, content_type(path))
            .header(header::CACHE_CONTROL, "no-cache")
            .body(Body::from(file.contents().to_vec()))
            .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response()),
        None => index().await,
    }
}

async fn index() -> Response {
    let file = FRONTEND.get_file("index.html").expect("缺少前端页面");
    Response::builder()
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .header(header::CACHE_CONTROL, "no-cache")
        .body(Body::from(file.contents().to_vec()))
        .unwrap()
}

fn engine_client() -> &'static reqwest::Client {
    static CLIENT: std::sync::LazyLock<reqwest::Client> = std::sync::LazyLock::new(|| {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(86_400))
            .connect_timeout(std::time::Duration::from_secs(10))
            .no_proxy()
            .build()
            .expect("无法创建引擎转发客户端")
    });
    &CLIENT
}

fn engine_url(uri: &axum::http::Uri, port: u16) -> Result<String, Response> {
    let path = uri.path().strip_prefix("/engine").unwrap_or("");
    if !path.starts_with('/') || path.contains("..") || path.contains('\\') {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"ok": false, "error": "引擎路径无效"})),
        )
            .into_response());
    }
    let query = uri
        .query()
        .map(|q| {
            q.split('&')
                .filter(|part| !part.is_empty() && !part.starts_with("token="))
                .collect::<Vec<_>>()
                .join("&")
        })
        .filter(|q| !q.is_empty());
    Ok(match query {
        Some(q) => format!("http://127.0.0.1:{port}{path}?{q}"),
        None => format!("http://127.0.0.1:{port}{path}"),
    })
}

async fn engine_proxy(
    State(app): State<Arc<App>>,
    req: axum::extract::Request,
) -> Response {
    let token = req.uri().query().and_then(|q| {
        q.split('&').find_map(|part| part.strip_prefix("token="))
    });
    if !authorized(&app, req.headers(), token) {
        return deny();
    }
    let Some(port) = crate::engine::running_port() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"ok": false, "error": "模型还没有启动"})),
        )
            .into_response();
    };
    let url = match engine_url(req.uri(), port) {
        Ok(url) => url,
        Err(response) => return response,
    };
    let method = reqwest::Method::from_bytes(req.method().as_str().as_bytes())
        .unwrap_or(reqwest::Method::GET);
    let content_type = req
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let accept = req
        .headers()
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let bytes = match axum::body::to_bytes(req.into_body(), 512 * 1024 * 1024).await {
        Ok(bytes) => bytes,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"ok": false, "error": error.to_string()})),
            )
                .into_response()
        }
    };
    let mut outbound = engine_client().request(method, &url);
    if let Some(content_type) = content_type {
        outbound = outbound.header(header::CONTENT_TYPE, content_type);
    }
    if let Some(accept) = accept {
        outbound = outbound.header(header::ACCEPT, accept);
    }
    if !bytes.is_empty() {
        outbound = outbound.body(bytes);
    }
    let upstream = match outbound.send().await {
        Ok(upstream) => upstream,
        Err(error) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(json!({"ok": false, "error": format!("连不上推理引擎：{error}")})),
            )
                .into_response()
        }
    };
    let status = upstream.status();
    let upstream_type = upstream
        .headers()
        .get(header::CONTENT_TYPE)
        .cloned();
    let stream = upstream.bytes_stream().map(|chunk| {
        chunk.map_err(|error| -> Box<dyn std::error::Error + Send + Sync> { Box::new(error) })
    });
    let mut response = Response::builder()
        .status(status)
        .header("X-Accel-Buffering", "no")
        .header(header::CACHE_CONTROL, "no-cache");
    if let Some(content_type) = upstream_type {
        response = response.header(header::CONTENT_TYPE, content_type);
    }
    response
        .body(Body::from_stream(stream))
        .unwrap_or_else(|_| StatusCode::BAD_GATEWAY.into_response())
}

fn content_type(path: &str) -> &'static str {
    match path.rsplit('.').next() {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("json") => "application/json",
        Some("ico") => "image/x-icon",
        Some("webm") => "video/webm",
        Some("mp4") => "video/mp4",
        _ => "application/octet-stream",
    }
}

fn router(app: Arc<App>) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/session", get(session))
        .route("/api/invoke", post(invoke))
        .route("/api/dialog", post(dialog_resolve))
        .route("/api/upload", post(upload))
        .route("/api/events", get(events))
        .route("/api/raw", get(raw_file))
        .route("/engine/{*path}", axum::routing::any(engine_proxy))
        .route("/", get(index))
        .route("/{*path}", get(asset))
        .layer(DefaultBodyLimit::max(512 * 1024 * 1024))
        .with_state(app)
}

pub async fn serve_listener(listener: TcpListener, ctx: Arc<Ctx>, host: Host, token: Option<String>) -> Result<(), String> {
    let app = Arc::new(App { ctx, host, token });
    axum::serve(listener, router(app))
        .await
        .map_err(|e| e.to_string())
}

pub fn run() {
    let root = std::env::var_os("LOCMODEL_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")).join("data"));
    let ctx = Arc::new(Ctx::new(root.clone()).expect("无法创建数据目录"));
    let _ = fs::create_dir_all(ctx.root.join("exports"));
    let _ = fs::create_dir_all(ctx.root.join("uploads"));
    let host = Host::new(root.clone()).install();
    let bind = std::env::var("LOCMODEL_BIND").unwrap_or_else(|_| "0.0.0.0:8787".into());
    let addr: SocketAddr = bind.parse().expect("LOCMODEL_BIND 必须是 host:port");
    let loopback = addr.ip().is_loopback();
    let token = load_token(&root, !loopback || std::env::var_os("LOCMODEL_TOKEN").is_some());
    println!("LocModel 网页版已启动");
    println!("本机访问: http://127.0.0.1:{}", addr.port());
    if !loopback {
        println!("服务器访问: http://{}:{}", addr.ip(), addr.port());
    }
    println!("数据目录: {}", root.display());
    if let Some(token) = &token {
        println!("访问口令: {token}");
        println!("口令文件: {}", root.join("access-token.txt").display());
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("无法启动服务");
    runtime
        .block_on(async move {
            let listener = TcpListener::bind(addr).await.expect("端口被占用");
            let app = Arc::new(App { ctx, host, token });
            axum::serve(listener, router(app))
                .with_graceful_shutdown(async {
                    let _ = tokio::signal::ctrl_c().await;
                    shutdown();
                })
                .await
                .expect("服务已退出");
        });
}

fn load_token(root: &std::path::Path, required: bool) -> Option<String> {
    if let Ok(token) = std::env::var("LOCMODEL_TOKEN") {
        let token = token.trim().to_string();
        if !token.is_empty() {
            return Some(token);
        }
    }
    let path = root.join("access-token.txt");
    if let Ok(saved) = fs::read_to_string(&path) {
        let saved = saved.trim().to_string();
        if !saved.is_empty() {
            return Some(saved);
        }
    }
    if !required {
        return None;
    }
    let token = uuid();
    fs::write(&path, &token).expect("无法写入访问口令");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
    }
    Some(token)
}

pub fn test_server(root: PathBuf) -> String {
    let std_listener = std::net::TcpListener::bind("127.0.0.1:0").expect("测试端口");
    std_listener.set_nonblocking(true).unwrap();
    let addr = std_listener.local_addr().unwrap();
    let ctx = Arc::new(Ctx::new(root.clone()).unwrap());
    let host = Host::new(root).install();
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async move {
            let listener = TcpListener::from_std(std_listener).unwrap();
            let _ = serve_listener(listener, ctx, host, None).await;
        });
    });
    format!("http://{addr}")
}

