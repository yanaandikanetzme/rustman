//! Local mock servers: a collection's saved examples answered over HTTP.
//!
//! Every saved example becomes a response for its request's method and
//! path. A request picks among several examples with `x-mock-response-name`
//! or `x-mock-response-code`, otherwise the first success answers. The
//! server is a small HTTP/1.1 responder on 127.0.0.1 (one request per
//! connection), enough for frontends, tests and curl.
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use crate::domain::{
    collection::SavedRequest,
    environment::{AppEnvironment, substitute},
    example::RequestExample,
};

#[derive(Debug, Clone, PartialEq)]
pub struct MockExample {
    pub name: String,
    pub status: u16,
    pub status_text: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq)]
enum Seg {
    Lit(String),
    Param,
}

/// One method + path the mock answers, with the examples it can answer with.
#[derive(Debug, Clone, PartialEq)]
pub struct Route {
    pub method: String,
    /// The path as shown: parameters as `:name`.
    pub path: String,
    segs: Vec<Seg>,
    /// The saved request it came from.
    pub request_name: String,
    pub examples: Vec<MockExample>,
}

/// Routes shared with a running server, so saved examples apply live.
pub type SharedRoutes = Arc<RwLock<Vec<Route>>>;

/// The path part of a request URL: scheme, host and a leading `{{base_url}}`
/// dropped, query and fragment cut, variables resolved where they can be.
pub fn request_path(url: &str, env: Option<&AppEnvironment>) -> String {
    let resolved = substitute(url.trim(), env);
    let mut s = resolved.as_str();
    s = s.split(['?', '#']).next().unwrap_or("");
    if let Some(i) = s.find("://") {
        let rest = &s[i + 3..];
        s = rest.find('/').map_or("", |j| &rest[j..]);
    } else if s.starts_with("{{") {
        // An unresolved base variable stands for scheme + host.
        s = s.find("}}").map_or("", |j| &s[j + 2..]);
    } else if !s.starts_with('/') {
        // `api.example.com/v1/x`: a host without a scheme.
        s = s.find('/').map_or("", |j| &s[j..]);
    }
    let trimmed = s.trim_end_matches('/');
    if trimmed.is_empty() {
        "/".into()
    } else if trimmed.starts_with('/') {
        trimmed.into()
    } else {
        format!("/{trimmed}")
    }
}

fn is_param(seg: &str) -> bool {
    seg.starts_with(':') || seg.contains("{{") || (seg.starts_with('{') && seg.ends_with('}'))
}

fn param_name(seg: &str) -> String {
    let name = seg.trim_start_matches(':').trim_matches(['{', '}']).trim();
    format!(":{}", if name.is_empty() { "param" } else { name })
}

fn parse_path(path: &str) -> (String, Vec<Seg>) {
    let mut shown = String::new();
    let mut segs = Vec::new();
    for seg in path.split('/').filter(|s| !s.is_empty()) {
        shown.push('/');
        if is_param(seg) {
            shown.push_str(&param_name(seg));
            segs.push(Seg::Param);
        } else {
            shown.push_str(seg);
            segs.push(Seg::Lit(seg.to_owned()));
        }
    }
    if shown.is_empty() {
        shown.push('/');
    }
    (shown, segs)
}

fn mock_example(example: &RequestExample, env: Option<&AppEnvironment>) -> MockExample {
    let resp = &example.response;
    let mut headers: Vec<(String, String)> = resp
        .headers
        .iter()
        .map(|(k, v)| (k.clone(), substitute(v, env)))
        .collect();
    headers.sort();
    MockExample {
        name: example.name.clone(),
        status: if resp.status == 0 { 200 } else { resp.status },
        status_text: resp.status_text.clone(),
        headers,
        body: substitute(&resp.body, env),
    }
}

/// The routes of a collection's requests. A request without examples still
/// gets a route, so a call to it explains what is missing.
pub fn build_routes(
    requests: &[SavedRequest],
    examples: &HashMap<String, Vec<RequestExample>>,
    env: Option<&AppEnvironment>,
) -> Vec<Route> {
    let mut routes: Vec<Route> = Vec::new();
    let mut add = |method: &str, url: &str, name: &str, example: Option<MockExample>| {
        let (path, segs) = parse_path(&request_path(url, env));
        let method = method.to_ascii_uppercase();
        let route = match routes
            .iter_mut()
            .position(|r| r.method == method && r.path == path)
        {
            Some(i) => &mut routes[i],
            None => {
                routes.push(Route {
                    method,
                    path,
                    segs,
                    request_name: name.to_owned(),
                    examples: Vec::new(),
                });
                routes.last_mut().expect("just pushed")
            }
        };
        route.examples.extend(example);
    };
    for request in requests {
        let saved = examples.get(&request.id).map(Vec::as_slice).unwrap_or(&[]);
        if saved.is_empty() {
            add(request.method.as_str(), &request.url, &request.name, None);
        }
        for example in saved {
            // An example keeps its own request snapshot (its own URL).
            let url = if example.request.url.trim().is_empty() {
                &request.url
            } else {
                &example.request.url
            };
            add(
                example.request.method.as_str(),
                url,
                &request.name,
                Some(mock_example(example, env)),
            );
        }
    }
    routes
}

/// What the mock sends back for one call.
#[derive(Debug, Clone, PartialEq)]
pub struct Answer {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: String,
    /// The example that answered.
    pub example: Option<String>,
    /// Why nothing matched, for the call log.
    pub note: Option<String>,
}

fn json_error(status: u16, message: &str, method: &str, path: &str, note: &str) -> Answer {
    let body = serde_json::json!({
        "error": message,
        "method": method,
        "path": path,
        "hint": "Save an example for this request in Rustman, or check the method and path.",
    });
    Answer {
        status,
        headers: vec![("Content-Type".into(), "application/json".into())],
        body: serde_json::to_string_pretty(&body).unwrap_or_default(),
        example: None,
        note: Some(note.into()),
    }
}

fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.trim())
}

fn segs_match(segs: &[Seg], path: &[&str]) -> Option<usize> {
    if segs.len() != path.len() {
        return None;
    }
    let mut literal = 0;
    for (seg, part) in segs.iter().zip(path) {
        match seg {
            Seg::Lit(lit) if lit == part => literal += 1,
            Seg::Lit(_) => return None,
            Seg::Param => {}
        }
    }
    Some(literal)
}

/// Picks the route and example for a call. `path` may carry a query.
pub fn answer(routes: &[Route], method: &str, path: &str, headers: &[(String, String)]) -> Answer {
    let method = method.to_ascii_uppercase();
    let bare = path.split(['?', '#']).next().unwrap_or("/");
    let parts: Vec<&str> = bare.split('/').filter(|s| !s.is_empty()).collect();
    // HEAD answers like GET; the server drops the body.
    let wanted = if method == "HEAD" { "GET" } else { method.as_str() };
    let best = routes
        .iter()
        .filter(|r| r.method == wanted)
        .filter_map(|r| segs_match(&r.segs, &parts).map(|score| (score, !r.examples.is_empty(), r)))
        .max_by_key(|(score, has, _)| (*has, *score))
        .map(|(_, _, r)| r);
    let Some(route) = best else {
        let other_method = routes
            .iter()
            .any(|r| segs_match(&r.segs, &parts).is_some());
        return if other_method {
            json_error(405, "Method not mocked for this path", &method, bare, "Method not mocked")
        } else {
            json_error(404, "No saved example matches this request", &method, bare, "No matching route")
        };
    };
    if route.examples.is_empty() {
        return json_error(
            404,
            &format!("\"{}\" has no saved example", route.request_name),
            &method,
            bare,
            "No saved example",
        );
    }
    let by_name = header(headers, "x-mock-response-name")
        .and_then(|n| route.examples.iter().find(|e| e.name.eq_ignore_ascii_case(n)));
    let by_code = header(headers, "x-mock-response-code")
        .and_then(|c| c.parse::<u16>().ok())
        .and_then(|c| route.examples.iter().find(|e| e.status == c));
    let first_ok = route.examples.iter().find(|e| (200..300).contains(&e.status));
    let example = by_name
        .or(by_code)
        .or(first_ok)
        .unwrap_or(&route.examples[0]);
    Answer {
        status: example.status,
        headers: example.headers.clone(),
        body: example.body.clone(),
        example: Some(example.name.clone()),
        note: None,
    }
}

// ── Server ───────────────────────────────────────────────────────────────────

/// One handled call, for the call log.
#[derive(Debug, Clone, PartialEq)]
pub struct CallLog {
    /// Milliseconds since the epoch.
    pub at_ms: i64,
    pub method: String,
    pub path: String,
    pub status: u16,
    pub example: Option<String>,
    pub note: Option<String>,
    pub duration_ms: u64,
}

#[derive(Debug, Clone)]
pub enum MockEvent {
    Started,
    Failed(String),
    Call(CallLog),
}

/// Request head cap; a bigger head is refused.
const MAX_HEAD: usize = 64 * 1024;
/// Request bodies are read (and dropped) up to this size.
const MAX_BODY: usize = 16 * 1024 * 1024;

/// Headers the server writes itself or that would lie about the body.
const SKIPPED: [&str; 4] = [
    "content-length",
    "transfer-encoding",
    "connection",
    "content-encoding",
];

struct Request {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
}

fn parse_head(head: &str) -> Option<Request> {
    let mut lines = head.split("\r\n");
    let mut first = lines.next()?.split_whitespace();
    let method = first.next()?.to_owned();
    let path = first.next()?.to_owned();
    let headers = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned()))
        .collect();
    Some(Request {
        method,
        path,
        headers,
    })
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        304 => "Not Modified",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        422 => "Unprocessable Entity",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        _ => "",
    }
}

/// The full HTTP response bytes for an answer.
fn response_bytes(answer: &Answer, head_only: bool, origin: Option<&str>) -> Vec<u8> {
    let no_body = head_only || answer.status == 204 || answer.status == 304;
    let mut out = format!("HTTP/1.1 {} {}\r\n", answer.status, reason(answer.status));
    let mut has_cors = false;
    for (k, v) in &answer.headers {
        let lower = k.to_ascii_lowercase();
        if SKIPPED.contains(&lower.as_str()) || k.contains(['\r', '\n']) || v.contains(['\r', '\n']) {
            continue;
        }
        has_cors |= lower == "access-control-allow-origin";
        out.push_str(&format!("{k}: {v}\r\n"));
    }
    if !has_cors {
        // Frontends call the mock from another origin.
        out.push_str(&format!(
            "Access-Control-Allow-Origin: {}\r\n",
            origin.unwrap_or("*")
        ));
    }
    let body = if no_body { "" } else { answer.body.as_str() };
    out.push_str(&format!(
        "Content-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    ));
    let mut bytes = out.into_bytes();
    bytes.extend_from_slice(body.as_bytes());
    bytes
}

fn preflight(req: &Request) -> Answer {
    let allow_headers = header(&req.headers, "access-control-request-headers")
        .unwrap_or("*")
        .to_owned();
    Answer {
        status: 204,
        headers: vec![
            (
                "Access-Control-Allow-Methods".into(),
                "GET, POST, PUT, PATCH, DELETE, HEAD, OPTIONS".into(),
            ),
            ("Access-Control-Allow-Headers".into(), allow_headers),
            ("Access-Control-Max-Age".into(), "600".into()),
        ],
        body: String::new(),
        example: None,
        note: Some("CORS preflight".into()),
    }
}

async fn handle(
    mut stream: tokio::net::TcpStream,
    routes: SharedRoutes,
    delay_ms: u64,
) -> Option<CallLog> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let started = std::time::Instant::now();
    let at_ms = chrono::Utc::now().timestamp_millis();
    let mut buf = Vec::with_capacity(4096);
    let mut chunk = [0u8; 8192];
    let head_end = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i;
        }
        if buf.len() > MAX_HEAD {
            let _ = stream
                .write_all(b"HTTP/1.1 431 Request Header Fields Too Large\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .await;
            return None;
        }
        let n = stream.read(&mut chunk).await.ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
    };
    let req = parse_head(&String::from_utf8_lossy(&buf[..head_end]))?;
    // Read the body so the client is not cut off mid-send; it is not used.
    let length = header(&req.headers, "content-length")
        .and_then(|l| l.parse::<usize>().ok())
        .unwrap_or(0)
        .min(MAX_BODY);
    let mut have = buf.len() - (head_end + 4);
    while have < length {
        match stream.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(n) => have += n,
        }
    }
    let answer = if req.method.eq_ignore_ascii_case("OPTIONS")
        && header(&req.headers, "access-control-request-method").is_some()
    {
        preflight(&req)
    } else {
        let routes = routes.read().map(|r| r.clone()).unwrap_or_default();
        answer(&routes, &req.method, &req.path, &req.headers)
    };
    if delay_ms > 0 {
        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
    }
    let bytes = response_bytes(
        &answer,
        req.method.eq_ignore_ascii_case("HEAD"),
        header(&req.headers, "origin"),
    );
    let _ = stream.write_all(&bytes).await;
    let _ = stream.shutdown().await;
    Some(CallLog {
        at_ms,
        method: req.method.to_ascii_uppercase(),
        path: req.path,
        status: answer.status,
        example: answer.example,
        note: answer.note,
        duration_ms: started.elapsed().as_millis() as u64,
    })
}

/// Serves until the returned stream is dropped (the mock is stopped).
pub fn serve(
    port: u16,
    routes: SharedRoutes,
    delay_ms: u64,
) -> impl iced::futures::Stream<Item = MockEvent> {
    iced::stream::channel(
        64,
        move |mut out: iced::futures::channel::mpsc::Sender<MockEvent>| async move {
            use futures_util::SinkExt as _;
            // A restart can race the previous listener closing: retry briefly.
            let mut bound = tokio::net::TcpListener::bind(("127.0.0.1", port)).await;
            for _ in 0..10 {
                match &bound {
                    Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                        bound = tokio::net::TcpListener::bind(("127.0.0.1", port)).await;
                    }
                    _ => break,
                }
            }
            let listener = match bound {
                Ok(l) => l,
                Err(e) => {
                    let msg = if e.kind() == std::io::ErrorKind::AddrInUse {
                        format!("Port {port} is already in use")
                    } else {
                        format!("Could not open port {port}: {e}")
                    };
                    let _ = out.send(MockEvent::Failed(msg)).await;
                    // Stay pending: ending the stream would restart it.
                    std::future::pending::<()>().await;
                    return;
                }
            };
            let _ = out.send(MockEvent::Started).await;
            let (log_tx, mut log_rx) = tokio::sync::mpsc::channel::<CallLog>(256);
            loop {
                tokio::select! {
                    accepted = listener.accept() => {
                        if let Ok((stream, _)) = accepted {
                            let routes = routes.clone();
                            let log_tx = log_tx.clone();
                            tokio::spawn(async move {
                                let call = tokio::time::timeout(
                                    std::time::Duration::from_secs(30 + delay_ms / 1000),
                                    handle(stream, routes, delay_ms),
                                )
                                .await;
                                if let Ok(Some(call)) = call {
                                    let _ = log_tx.send(call).await;
                                }
                            });
                        }
                    }
                    Some(call) = log_rx.recv() => {
                        if out.send(MockEvent::Call(call)).await.is_err() {
                            return;
                        }
                    }
                }
            }
        },
    )
}

/// Whether a port can be opened on 127.0.0.1 right now.
pub fn port_free(port: u16) -> bool {
    port != 0 && std::net::TcpListener::bind(("127.0.0.1", port)).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{request::HttpMethod, response::HttpResponse};

    fn req(id: &str, method: HttpMethod, url: &str) -> SavedRequest {
        let mut r = SavedRequest::new_in("c".into(), id.into());
        r.id = id.into();
        r.method = method;
        r.url = url.into();
        r
    }

    fn ex(request: &SavedRequest, name: &str, status: u16, body: &str) -> RequestExample {
        let mut e = RequestExample::new(
            request.clone(),
            Some(HttpResponse {
                status,
                body: body.into(),
                ..Default::default()
            }),
        );
        e.name = name.into();
        e
    }

    #[test]
    fn paths_drop_host_query_and_base_variable() {
        assert_eq!(request_path("{{base_url}}/v1/login?x=1", None), "/v1/login");
        assert_eq!(request_path("https://api.test/v1/users/", None), "/v1/users");
        assert_eq!(request_path("api.test/v1", None), "/v1");
        assert_eq!(request_path("https://api.test", None), "/");
        assert_eq!(parse_path("/v1/users/:id/{{x}}").0, "/v1/users/:id/:x");
    }

    fn routes() -> Vec<Route> {
        let login = req("login", HttpMethod::Post, "{{base_url}}/v1/login");
        let user = req("user", HttpMethod::Get, "{{base_url}}/v1/users/:id");
        let me = req("me", HttpMethod::Get, "{{base_url}}/v1/users/me");
        let logout = req("logout", HttpMethod::Delete, "{{base_url}}/v1/session");
        let mut examples = HashMap::new();
        examples.insert(
            "login".to_owned(),
            vec![
                ex(&login, "Invalid credentials", 401, "{\"error\":1}"),
                ex(&login, "Success", 200, "{\"token\":\"t\"}"),
            ],
        );
        examples.insert("user".to_owned(), vec![ex(&user, "User", 200, "user")]);
        examples.insert("me".to_owned(), vec![ex(&me, "Me", 200, "me")]);
        build_routes(&[login, user, me, logout], &examples, None)
    }

    #[test]
    fn first_success_answers_and_headers_pick_another() {
        let routes = routes();
        let a = answer(&routes, "post", "/v1/login", &[]);
        assert_eq!((a.status, a.example.as_deref()), (200, Some("Success")));
        let by_name = [("X-Mock-Response-Name".into(), "invalid credentials".into())];
        assert_eq!(answer(&routes, "POST", "/v1/login", &by_name).status, 401);
        let by_code = [("x-mock-response-code".into(), "401".into())];
        assert_eq!(answer(&routes, "POST", "/v1/login", &by_code).status, 401);
    }

    #[test]
    fn literal_segments_beat_parameters() {
        let routes = routes();
        assert_eq!(answer(&routes, "GET", "/v1/users/me", &[]).body, "me");
        assert_eq!(answer(&routes, "GET", "/v1/users/42?full=1", &[]).body, "user");
    }

    #[test]
    fn misses_explain_themselves() {
        let routes = routes();
        let none = answer(&routes, "DELETE", "/v1/session", &[]);
        assert_eq!((none.status, none.note.as_deref()), (404, Some("No saved example")));
        assert_eq!(answer(&routes, "GET", "/v1/login", &[]).status, 405);
        assert_eq!(answer(&routes, "GET", "/nope", &[]).status, 404);
    }

    #[test]
    fn responses_carry_length_and_cors() {
        let a = Answer {
            status: 200,
            headers: vec![
                ("Content-Type".into(), "application/json".into()),
                ("Content-Length".into(), "999".into()),
            ],
            body: "{}".into(),
            example: None,
            note: None,
        };
        let text = String::from_utf8(response_bytes(&a, false, None)).unwrap();
        assert!(text.starts_with("HTTP/1.1 200 OK\r\n"), "{text}");
        assert!(text.contains("Content-Length: 2\r\n") && !text.contains("999"));
        assert!(text.contains("Access-Control-Allow-Origin: *\r\n"));
        assert!(text.ends_with("\r\n\r\n{}"));
    }

    #[tokio::test]
    async fn serves_over_tcp() {
        use futures_util::StreamExt as _;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let port = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap().port()
        };
        let shared: SharedRoutes = Arc::new(RwLock::new(routes()));
        let mut events = Box::pin(serve(port, shared, 0));
        let server = tokio::spawn(async move {
            let mut seen = Vec::new();
            while let Some(e) = events.next().await {
                let done = matches!(e, MockEvent::Call(_));
                seen.push(e);
                if done {
                    break;
                }
            }
            seen
        });
        let mut stream = loop {
            if let Ok(s) = tokio::net::TcpStream::connect(("127.0.0.1", port)).await {
                break s;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        };
        stream
            .write_all(b"POST /v1/login HTTP/1.1\r\nHost: x\r\nContent-Length: 2\r\n\r\n{}")
            .await
            .unwrap();
        let mut reply = String::new();
        stream.read_to_string(&mut reply).await.unwrap();
        assert!(reply.starts_with("HTTP/1.1 200 OK"), "{reply}");
        assert!(reply.ends_with("{\"token\":\"t\"}"), "{reply}");
        let seen = server.await.unwrap();
        assert!(matches!(seen[0], MockEvent::Started));
        let MockEvent::Call(call) = &seen[1] else {
            panic!("a call is logged")
        };
        assert_eq!((call.method.as_str(), call.status), ("POST", 200));
        assert_eq!(call.example.as_deref(), Some("Success"));
    }
}
