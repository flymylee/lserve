use crate::{
    certs,
    config::Config,
    logging::Logger,
    markdown::MarkdownRenderer,
    proxy::ProxyRouter,
    response::{self, HttpResponse},
    static_files::StaticServer,
};
use anyhow::{Context, Result};
use hyper::{
    Request, Response, StatusCode,
    header::{self, HeaderValue},
    http::uri::Authority,
    server::conn::http1,
    service::service_fn,
};
use hyper_util::rt::TokioIo;
use std::{
    convert::Infallible,
    fs::File,
    io::BufReader,
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::Instant,
};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::{TlsAcceptor, rustls::ServerConfig as RustlsServerConfig};

#[derive(Clone)]
struct App {
    config: Arc<Config>,
    logger: Logger,
    static_server: StaticServer,
    proxy: ProxyRouter,
}

pub async fn run(config: Config) -> Result<()> {
    let config = Arc::new(config);
    let logger = Logger::new(&config).await?;
    let markdown = if config.markdown {
        Some(Arc::new(MarkdownRenderer::new(&config.root)?))
    } else {
        None
    };
    let static_server = StaticServer::new(Arc::clone(&config), markdown);
    let proxy = ProxyRouter::from_config(&config)?;
    let app = App {
        config: Arc::clone(&config),
        logger: logger.clone(),
        static_server,
        proxy,
    };

    let mut tasks = tokio::task::JoinSet::new();
    if config.http_only {
        // Acquire ALL sockets before serving; a partial bind is a startup error.
        let listeners = bind_all(&config.binds, config.http_port).await?;
        for (address, listener) in listeners {
            logger.access(format!("server http listening on http://{address}"));
            tasks.spawn(serve_http(listener, app.clone(), false));
        }
    } else {
        let cert_paths = certs::ensure_for_serve(&config)?;
        let tls = load_tls_config(&cert_paths.cert, &cert_paths.key)?;
        let redirects = bind_all(&config.binds, config.http_port).await?;
        let https = bind_all(&config.binds, config.https_port).await?;
        for (address, listener) in redirects {
            logger.access(format!("server http redirect listening on http://{address}"));
            tasks.spawn(serve_redirect(listener, config.https_port, logger.clone()));
        }
        for (address, listener) in https {
            logger.access(format!("server https listening on https://{address}"));
            tasks.spawn(serve_https(listener, app.clone(), Arc::clone(&tls)));
        }
    }

    let result = tokio::select! {
        done = tasks.join_next() => match done {
            Some(Ok(Ok(()))) => anyhow::bail!("listener stopped unexpectedly"),
            Some(Ok(Err(error))) => Err(error),
            Some(Err(error)) => Err(error).context("listener task failed"),
            None => anyhow::bail!("no listeners running"),
        },
        shutdown = tokio::signal::ctrl_c() => {
            shutdown.context("failed to listen for shutdown signal")?;
            logger.access("server shutdown requested");
            Ok(())
        }
    };
    tasks.abort_all();
    result
}

async fn bind_all(ips: &[IpAddr], port: u16) -> Result<Vec<(SocketAddr, TcpListener)>> {
    let mut listeners = Vec::with_capacity(ips.len());
    for ip in ips {
        let address = SocketAddr::new(*ip, port);
        let listener = TcpListener::bind(address)
            .await
            .with_context(|| format!("failed to bind {address}"))?;
        listeners.push((address, listener));
    }
    Ok(listeners)
}

async fn serve_http(listener: TcpListener, app: App, tls: bool) -> Result<()> {
    loop {
        let (stream, peer) = listener.accept().await.context("HTTP accept failed")?;
        let app = app.clone();
        tokio::spawn(async move {
            if let Err(error) = serve_http_connection(stream, peer, app.clone(), tls).await {
                app.logger.error(format!("connection {peer}: {error:#}"));
            }
        });
    }
}

async fn serve_http_connection(
    stream: TcpStream,
    peer: SocketAddr,
    app: App,
    tls: bool,
) -> Result<()> {
    let io = TokioIo::new(stream);
    let service = service_fn(move |request| {
        let app = app.clone();
        async move { Ok::<_, Infallible>(handle_request(request, peer, app, tls).await) }
    });
    http1::Builder::new()
        .serve_connection(io, service)
        .with_upgrades()
        .await
        .context("HTTP connection failed")
}

async fn serve_https(
    listener: TcpListener,
    app: App,
    tls_config: Arc<RustlsServerConfig>,
) -> Result<()> {
    let acceptor = TlsAcceptor::from(tls_config);

    loop {
        let (stream, peer) = listener.accept().await.context("HTTPS accept failed")?;
        let acceptor = acceptor.clone();
        let app = app.clone();
        tokio::spawn(async move {
            let tls = match acceptor.accept(stream).await {
                Ok(tls) => tls,
                Err(error) => {
                    app.logger.error(format!("TLS accept {peer}: {error}"));
                    return;
                }
            };
            let io = TokioIo::new(tls);
            let connection_logger = app.logger.clone();
            let service = service_fn(move |request| {
                let app = app.clone();
                async move { Ok::<_, Infallible>(handle_request(request, peer, app, true).await) }
            });
            if let Err(error) = http1::Builder::new()
                .serve_connection(io, service)
                .with_upgrades()
                .await
            {
                connection_logger.error(format!("HTTPS connection {peer}: {error}"));
            }
        });
    }
}

async fn handle_request(
    request: Request<hyper::body::Incoming>,
    peer: SocketAddr,
    app: App,
    tls: bool,
) -> HttpResponse {
    let started = Instant::now();
    let method = request.method().clone();
    let target = request
        .uri()
        .path_and_query()
        .map(|value| value.as_str())
        .unwrap_or("/")
        .to_string();
    let route = app.proxy.route_for_request(&request);
    let source = if route.is_some() { "proxy" } else { "static" };

    let response = if let Some(route_selector) = route {
        let scheme = if tls { "https" } else { "http" };
        let port = if tls {
            app.config.https_port
        } else {
            app.config.http_port
        };
        match app
            .proxy
            .proxy(
                &route_selector,
                request,
                peer,
                scheme,
                port,
                app.logger.clone(),
            )
            .await
        {
            Ok(response) => response,
            Err(error) => {
                app.logger.error(format!(
                    "proxy {method} {target} route={route_selector}: {error:#}"
                ));
                response::bad_gateway(app.config.verbose_errors, &format!("{error:#}"))
            }
        }
    } else {
        match app.static_server.serve(request).await {
            Ok(response) => response,
            Err(error) => {
                app.logger
                    .error(format!("static {method} {target}: {error:#}"));
                let mut response =
                    response::internal_error(app.config.verbose_errors, &format!("{error:#}"));
                crate::static_files::apply_static_cors(&mut response);
                response
            }
        }
    };

    // HEAD is bodyless regardless of whether the response came from static
    // serving, an upstream application, or an error path. Preserve headers
    // (including Content-Length) while discarding any body an implementation
    // accidentally supplied.
    let response = if method == hyper::Method::HEAD {
        response::without_body(response)
    } else {
        response
    };

    let status = response.status();
    let bytes = response
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("-");
    app.logger.access(format!(
        "{} {} {} {} {} {}ms {}",
        peer.ip(),
        method,
        target,
        status.as_u16(),
        bytes,
        started.elapsed().as_millis(),
        source
    ));
    response
}

async fn serve_redirect(listener: TcpListener, https_port: u16, logger: Logger) -> Result<()> {
    loop {
        let (stream, peer) = listener.accept().await.context("redirect accept failed")?;
        let logger = logger.clone();
        tokio::spawn(async move {
            let io = TokioIo::new(stream);
            let logger_for_service = logger.clone();
            let service = service_fn(move |request: Request<hyper::body::Incoming>| {
                let logger = logger_for_service.clone();
                async move {
                    let started = Instant::now();
                    let method = request.method().clone();
                    let target = request
                        .uri()
                        .path_and_query()
                        .map(|pq| pq.as_str())
                        .unwrap_or("/")
                        .to_string();
                    let response = redirect_response(&request, https_port);
                    logger.access(format!(
                        "{} {} {} {} 0 {}ms redirect",
                        peer.ip(),
                        method,
                        target,
                        response.status().as_u16(),
                        started.elapsed().as_millis()
                    ));
                    Ok::<_, Infallible>(response)
                }
            });
            if let Err(error) = http1::Builder::new().serve_connection(io, service).await {
                logger.error(format!("redirect connection {peer}: {error}"));
            }
        });
    }
}

fn redirect_response(request: &Request<hyper::body::Incoming>, https_port: u16) -> HttpResponse {
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<Authority>().ok())
        .map(|authority| authority.host().to_string());

    let Some(host) = host else {
        return response::text(StatusCode::BAD_REQUEST, "400 Bad Request\n");
    };
    let host = format_host_for_url(&host);
    let port = if https_port == 443 {
        String::new()
    } else {
        format!(":{https_port}")
    };
    let target = request
        .uri()
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or("/");
    let location = format!("https://{host}{port}{target}");

    match HeaderValue::from_str(&location) {
        Ok(location) => Response::builder()
            .status(StatusCode::PERMANENT_REDIRECT)
            .header(header::LOCATION, location)
            .header(header::CONTENT_LENGTH, "0")
            .body(response::empty())
            .expect("redirect response headers are valid"),
        Err(_) => response::text(StatusCode::BAD_REQUEST, "400 Bad Request\n"),
    }
}

fn format_host_for_url(host: &str) -> String {
    if host.parse::<IpAddr>().is_ok_and(|ip| ip.is_ipv6()) {
        format!("[{host}]")
    } else {
        host.to_string()
    }
}

fn load_tls_config(
    cert_path: &std::path::Path,
    key_path: &std::path::Path,
) -> Result<Arc<RustlsServerConfig>> {
    let cert_file = File::open(cert_path)
        .with_context(|| format!("failed to open certificate {}", cert_path.display()))?;
    let mut cert_reader = BufReader::new(cert_file);
    let certs = rustls_pemfile::certs(&mut cert_reader)
        .collect::<std::result::Result<Vec<_>, _>>()
        .with_context(|| format!("failed to read certificate {}", cert_path.display()))?;
    if certs.is_empty() {
        anyhow::bail!(
            "certificate file contains no certificate: {}",
            cert_path.display()
        );
    }

    let key_file = File::open(key_path)
        .with_context(|| format!("failed to open private key {}", key_path.display()))?;
    let mut key_reader = BufReader::new(key_file);
    let key = rustls_pemfile::private_key(&mut key_reader)
        .with_context(|| format!("failed to read private key {}", key_path.display()))?
        .with_context(|| format!("private key file contains no key: {}", key_path.display()))?;

    let mut config = RustlsServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .context("certificate/private key are not a usable TLS pair")?;
    // lserve's HTTP stack is deliberately HTTP/1.1-only in v1.0.0. This also
    // keeps WebSocket Upgrade handling straightforward and avoids advertising h2.
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(Arc::new(config))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipv6_hosts_are_bracketed_for_redirects() {
        assert_eq!(format_host_for_url("::1"), "[::1]");
        assert_eq!(format_host_for_url("localhost"), "localhost");
    }
}
