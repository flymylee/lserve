use crate::{
    config::{Config, route_selector_host, route_selector_path},
    logging::Logger,
    response::{self, BoxError, HttpResponse},
};
use anyhow::{Context, Result, bail};
use http_body_util::BodyExt;
use hyper::{
    HeaderMap, Method, Request, StatusCode, Uri,
    client::conn::http1,
    header::{self, HeaderName, HeaderValue},
    http::uri::Authority,
};
use hyper_util::rt::TokioIo;
use std::{collections::BTreeMap, net::SocketAddr};
use tokio::{
    io::copy_bidirectional,
    net::{TcpStream, lookup_host},
};

#[derive(Debug, Clone)]
struct Upstream {
    host: String,
    port: u16,
}

#[derive(Debug, Clone)]
struct RouteEntry {
    host: String,
    path_prefix: String,
    upstream: Upstream,
}

#[derive(Debug, Clone, Default)]
pub struct ProxyRouter {
    routes: BTreeMap<String, RouteEntry>,
}

impl ProxyRouter {
    pub fn from_config(config: &Config) -> Result<Self> {
        let mut routes = BTreeMap::new();
        for (selector, target) in &config.routes {
            routes.insert(
                selector.clone(),
                RouteEntry {
                    host: route_selector_host(selector).to_string(),
                    path_prefix: route_selector_path(selector).to_string(),
                    upstream: parse_upstream(target)?,
                },
            );
        }
        Ok(Self { routes })
    }

    pub fn route_for_request(&self, request: &Request<hyper::body::Incoming>) -> Option<String> {
        let host = request_host(request)?;
        let path = request.uri().path();
        self.routes
            .iter()
            .filter(|(_, route)| {
                route.host == host && path_prefix_matches(path, &route.path_prefix)
            })
            .max_by_key(|(_, route)| route.path_prefix.len())
            .map(|(selector, _)| selector.clone())
    }

    pub async fn proxy(
        &self,
        selector: &str,
        mut request: Request<hyper::body::Incoming>,
        peer: SocketAddr,
        client_scheme: &'static str,
        client_port: u16,
        logger: Logger,
    ) -> Result<HttpResponse> {
        if request.method() == Method::TRACE || request.method() == Method::CONNECT {
            let mut response =
                response::text(StatusCode::METHOD_NOT_ALLOWED, "405 Method Not Allowed\n");
            response.headers_mut().insert(
                header::ALLOW,
                HeaderValue::from_static("GET, HEAD, OPTIONS, POST, PUT, PATCH, DELETE"),
            );
            return Ok(response);
        }

        let route = self
            .routes
            .get(selector)
            .with_context(|| format!("proxy route disappeared for {selector}"))?;
        let upstream = &route.upstream;
        let destination = resolve_loopback(upstream).await?;
        let stream = TcpStream::connect(destination)
            .await
            .with_context(|| format!("failed to connect to loopback upstream {destination}"))?;
        let io = TokioIo::new(stream);
        let (mut sender, connection) = http1::handshake::<_, hyper::body::Incoming>(io)
            .await
            .context("upstream HTTP/1 handshake failed")?;

        let connection_logger = logger.clone();
        tokio::spawn(async move {
            if let Err(error) = connection.with_upgrades().await {
                connection_logger.error(format!("proxy connection: {error}"));
            }
        });

        let websocket = is_upgrade(request.headers());
        let client_upgrade = websocket.then(|| hyper::upgrade::on(&mut request));
        let original_host = request
            .headers()
            .get(header::HOST)
            .and_then(|v| v.to_str().ok())
            .unwrap_or(&route.host)
            .to_string();

        sanitize_request_headers(request.headers_mut(), websocket);
        if !request.headers().contains_key(header::HOST) {
            request.headers_mut().insert(
                header::HOST,
                HeaderValue::from_str(&original_host).context("invalid proxy Host header")?,
            );
        }
        set_forwarding_headers(
            request.headers_mut(),
            &original_host,
            peer,
            client_scheme,
            client_port,
        )?;

        // We connect to a validated SocketAddr, not to the user-facing route host.
        // Keep the original Host header for virtual-host aware local apps while using
        // an origin-form request URI on the direct HTTP/1 connection.
        let path_and_query = request
            .uri()
            .path_and_query()
            .map(|pq| pq.as_str())
            .unwrap_or("/")
            .to_string();
        *request.uri_mut() = path_and_query
            .parse::<Uri>()
            .context("invalid request path")?;

        let mut upstream_response = sender
            .send_request(request)
            .await
            .context("upstream request failed")?;
        let status = upstream_response.status();
        let upstream_upgrade = (websocket && status == StatusCode::SWITCHING_PROTOCOLS)
            .then(|| hyper::upgrade::on(&mut upstream_response));

        sanitize_response_headers(upstream_response.headers_mut(), upstream_upgrade.is_some());

        if let (Some(client_upgrade), Some(upstream_upgrade)) = (client_upgrade, upstream_upgrade) {
            let tunnel_logger = logger.clone();
            tokio::spawn(async move {
                let result = async {
                    let client = client_upgrade
                        .await
                        .context("client WebSocket upgrade failed")?;
                    let upstream = upstream_upgrade
                        .await
                        .context("upstream WebSocket upgrade failed")?;
                    let mut client = TokioIo::new(client);
                    let mut upstream = TokioIo::new(upstream);
                    copy_bidirectional(&mut client, &mut upstream)
                        .await
                        .context("WebSocket tunnel copy failed")?;
                    Ok::<(), anyhow::Error>(())
                }
                .await;
                if let Err(error) = result {
                    tunnel_logger.error(format!("WebSocket tunnel: {error:#}"));
                }
            });
        }

        Ok(upstream_response.map(|body| {
            body.map_err(|error| -> BoxError { Box::new(error) })
                .boxed_unsync()
        }))
    }
}

fn path_prefix_matches(path: &str, prefix: &str) -> bool {
    if prefix == "/" {
        return true;
    }
    if path == prefix {
        return true;
    }
    path.strip_prefix(prefix)
        .is_some_and(|rest| rest.starts_with('/'))
}

fn parse_upstream(target: &str) -> Result<Upstream> {
    let uri: Uri = target
        .parse()
        .with_context(|| format!("invalid proxy upstream URL {target:?}"))?;
    if uri.scheme_str() != Some("http") {
        bail!("proxy upstream must use http://: {target}");
    }
    let authority = uri
        .authority()
        .with_context(|| format!("proxy upstream has no host: {target}"))?;
    let path = uri.path();
    if path != "/" || uri.query().is_some() {
        bail!("proxy upstream must be an origin without path/query: {target}");
    }
    let host = authority.host().to_string();
    let port = authority.port_u16().unwrap_or(80);
    Ok(Upstream { host, port })
}

async fn resolve_loopback(upstream: &Upstream) -> Result<SocketAddr> {
    let addresses = lookup_host((upstream.host.as_str(), upstream.port))
        .await
        .with_context(|| format!("failed to resolve proxy upstream {}", upstream.host))?;
    let mut saw_non_loopback = false;
    for address in addresses {
        if address.ip().is_loopback() {
            return Ok(address);
        }
        saw_non_loopback = true;
    }
    if saw_non_loopback {
        bail!(
            "proxy upstream {}:{} resolved outside loopback; lserve refuses remote/private-LAN upstreams",
            upstream.host,
            upstream.port
        );
    }
    bail!(
        "proxy upstream {}:{} resolved to no usable address",
        upstream.host,
        upstream.port
    )
}

pub fn request_host(request: &Request<hyper::body::Incoming>) -> Option<String> {
    let raw = request.headers().get(header::HOST)?.to_str().ok()?;
    let authority: Authority = raw.parse().ok()?;
    Some(authority.host().trim_end_matches('.').to_ascii_lowercase())
}

fn set_forwarding_headers(
    headers: &mut HeaderMap,
    original_host: &str,
    peer: SocketAddr,
    scheme: &str,
    port: u16,
) -> Result<()> {
    // Do not trust client-supplied forwarding headers. Upstream development apps
    // frequently trust these values, so lserve replaces them with the connection
    // facts it actually observed instead of appending spoofable input.
    headers.insert(
        HeaderName::from_static("x-forwarded-host"),
        HeaderValue::from_str(original_host).context("invalid original Host header")?,
    );
    headers.insert(
        HeaderName::from_static("x-forwarded-proto"),
        HeaderValue::from_str(scheme).context("invalid forwarded scheme")?,
    );
    headers.insert(
        HeaderName::from_static("x-forwarded-for"),
        HeaderValue::from_str(&peer.ip().to_string())?,
    );
    headers.insert(
        HeaderName::from_static("x-forwarded-port"),
        HeaderValue::from_str(&port.to_string())?,
    );
    Ok(())
}

fn is_upgrade(headers: &HeaderMap) -> bool {
    headers.contains_key(header::UPGRADE)
        && connection_tokens(headers).any(|token| token.eq_ignore_ascii_case("upgrade"))
}

fn sanitize_request_headers(headers: &mut HeaderMap, preserve_upgrade: bool) {
    strip_hop_by_hop(headers, preserve_upgrade);
}

fn sanitize_response_headers(headers: &mut HeaderMap, preserve_upgrade: bool) {
    strip_hop_by_hop(headers, preserve_upgrade);
}

fn strip_hop_by_hop(headers: &mut HeaderMap, preserve_upgrade: bool) {
    let connection_named: Vec<HeaderName> = connection_tokens(headers)
        .filter_map(|token| HeaderName::from_bytes(token.trim().as_bytes()).ok())
        .collect();
    for name in connection_named {
        if !(preserve_upgrade && name == header::UPGRADE) {
            headers.remove(name);
        }
    }

    headers.remove("proxy-connection");
    headers.remove("keep-alive");
    headers.remove(header::PROXY_AUTHENTICATE);
    headers.remove(header::PROXY_AUTHORIZATION);
    headers.remove(header::TE);
    headers.remove(header::TRAILER);
    headers.remove(header::TRANSFER_ENCODING);

    if preserve_upgrade {
        headers.insert(header::CONNECTION, HeaderValue::from_static("upgrade"));
    } else {
        headers.remove(header::CONNECTION);
        headers.remove(header::UPGRADE);
    }
}

fn connection_tokens(headers: &HeaderMap) -> impl Iterator<Item = &str> {
    headers
        .get_all(header::CONNECTION)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .map(str::trim)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upstream_url_has_tight_scope() {
        let ok = parse_upstream("http://localhost:3000").unwrap();
        assert_eq!(ok.host, "localhost");
        assert_eq!(ok.port, 3000);
        assert!(parse_upstream("https://localhost:3000").is_err());
        assert!(parse_upstream("http://localhost:3000/api").is_err());
    }

    #[test]
    fn path_prefixes_match_segment_boundaries() {
        assert!(path_prefix_matches("/api", "/api"));
        assert!(path_prefix_matches("/api/users", "/api"));
        assert!(!path_prefix_matches("/apix", "/api"));
        assert!(path_prefix_matches("/anything", "/"));
    }

    #[test]
    fn forwarding_scheme_is_a_dynamic_header_value() {
        let mut headers = HeaderMap::new();
        set_forwarding_headers(
            &mut headers,
            "local.example",
            "127.0.0.1:12345".parse().unwrap(),
            "https",
            8443,
        )
        .unwrap();
        assert_eq!(headers["x-forwarded-proto"], "https");
        assert_eq!(headers["x-forwarded-host"], "local.example");
    }
}
