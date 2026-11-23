//! Serving the MCP transport over HTTP/1.1, behind the token and origin checks.

use std::convert::Infallible;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use hyper::body::{Body, Bytes, Frame, SizeHint};
use hyper::header::{AUTHORIZATION, CONTENT_TYPE, HeaderValue, ORIGIN, WWW_AUTHENTICATE};
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use tokio::net::TcpListener;
use tokio::sync::watch;

use crate::tools::Tools;

/// The one path served.
pub const PATH: &str = "/mcp";
/// The largest request accepted: a file to write, mostly.
const MAX_REQUEST_BYTES: usize = 64 << 20;

/// Accepts connections until `stopped` turns true.
pub async fn serve(
    listener: std::net::TcpListener,
    tools: Tools,
    token: Arc<str>,
    mut stopped: watch::Receiver<bool>,
) {
    let Ok(listener) = TcpListener::from_std(listener) else {
        return;
    };
    let mut config = StreamableHttpServerConfig::default();
    // Each request on its own, answered with plain JSON: nothing to keep
    // between requests, and the simplest for clients.
    config.legacy_session_mode = false;
    config.json_response = true;
    config.sse_keep_alive = None;
    config.max_request_body_bytes = MAX_REQUEST_BYTES;
    // `Host` must be this machine (rmcp's default), against DNS rebinding.
    config.allowed_hosts = vec!["127.0.0.1".into(), "localhost".into()];
    let sessions = config.cancellation_token.clone();
    let service: StreamableHttpService<Tools, LocalSessionManager> =
        StreamableHttpService::new(move || Ok(tools.clone()), Arc::default(), config);

    loop {
        tokio::select! {
            _ = stopped.changed() => break,
            accepted = listener.accept() => {
                let Ok((stream, _)) = accepted else { continue };
                let service = service.clone();
                let token = Arc::clone(&token);
                tokio::spawn(async move {
                    let answer = hyper::service::service_fn(move |request| {
                        let service = service.clone();
                        let token = Arc::clone(&token);
                        async move {
                            let response = match refuse(&request, &token) {
                                Some(refusal) => refusal.map(|text| Reply::Text(Some(text))),
                                None => service.handle(request).await.map(Reply::Inner),
                            };
                            Ok::<_, Infallible>(response)
                        }
                    });
                    let _ = hyper::server::conn::http1::Builder::new()
                        .serve_connection(TokioIo::new(stream), answer)
                        .await;
                });
            }
        }
    }
    sessions.cancel();
}

/// Why a request isn't let through, as the response to send; `None` to let it.
pub fn refuse<B>(request: &Request<B>, token: &str) -> Option<Response<Bytes>> {
    if request.uri().path() != PATH {
        return Some(text(StatusCode::NOT_FOUND, "not found"));
    }
    // Browsers send an Origin with every cross-site request; agents don't.
    if request.headers().contains_key(ORIGIN) {
        return Some(text(
            StatusCode::FORBIDDEN,
            "web pages may not use this server",
        ));
    }
    let given = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::trim);
    match given {
        Some(given) if same(given.as_bytes(), token.as_bytes()) => None,
        _ => {
            let mut response = text(
                StatusCode::UNAUTHORIZED,
                "a token is needed: Authorization: Bearer <token>, from the IDE's AI Agents settings",
            );
            response
                .headers_mut()
                .insert(WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
            Some(response)
        }
    }
}

fn text(status: StatusCode, message: &'static str) -> Response<Bytes> {
    let mut response = Response::new(Bytes::from_static(message.as_bytes()));
    *response.status_mut() = status;
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    response
}

/// Compares in time that doesn't depend on where they differ.
fn same(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0_u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// A response's body: the transport's, or a refusal's text.
enum Reply<B> {
    Inner(B),
    Text(Option<Bytes>),
}

impl<B> Body for Reply<B>
where
    B: Body<Data = Bytes, Error = Infallible> + Unpin,
{
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Infallible>>> {
        match self.get_mut() {
            Reply::Inner(body) => Pin::new(body).poll_frame(cx),
            Reply::Text(text) => Poll::Ready(text.take().map(|bytes| Ok(Frame::data(bytes)))),
        }
    }

    fn is_end_stream(&self) -> bool {
        match self {
            Reply::Inner(body) => body.is_end_stream(),
            Reply::Text(text) => text.is_none(),
        }
    }

    fn size_hint(&self) -> SizeHint {
        match self {
            Reply::Inner(body) => body.size_hint(),
            Reply::Text(Some(bytes)) => SizeHint::with_exact(bytes.len() as u64),
            Reply::Text(None) => SizeHint::with_exact(0),
        }
    }
}
