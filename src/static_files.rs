use crate::{
    config::Config,
    markdown::MarkdownRenderer,
    response::{self, Body, BoxError, HttpResponse},
};
use anyhow::{Context, Result};
use futures_util::TryStreamExt;
use http_body_util::{BodyExt, StreamBody};
use hyper::{
    Method, Request, Response, StatusCode, Uri,
    body::Frame,
    header::{self, HeaderValue},
};
use percent_encoding::percent_decode_str;
use std::{
    ffi::OsStr,
    path::{Component, Path, PathBuf},
    sync::Arc,
};
use tokio::{
    fs::{self, File},
    io::{AsyncReadExt, AsyncSeekExt, SeekFrom},
};
use tokio_util::io::ReaderStream;

#[derive(Clone)]
pub struct StaticServer {
    config: Arc<Config>,
    markdown: Option<Arc<MarkdownRenderer>>,
}

impl StaticServer {
    pub fn new(config: Arc<Config>, markdown: Option<Arc<MarkdownRenderer>>) -> Self {
        Self { config, markdown }
    }

    pub async fn serve(&self, req: Request<hyper::body::Incoming>) -> Result<HttpResponse> {
        let method = req.method().clone();
        let uri = req.uri().clone();
        let request_path = match decode_request_path(uri.path()) {
            Ok(path) => path,
            Err(()) => {
                return Ok(self.static_response(response::text(
                    StatusCode::BAD_REQUEST,
                    "400 Bad Request\n",
                )));
            }
        };

        // Reserved/private server state must stay hidden even for OPTIONS probes.
        if self.is_hidden_path(&request_path).await {
            return Ok(
                self.static_response(response::text(StatusCode::NOT_FOUND, "404 Not Found\n"))
            );
        }

        if method == Method::OPTIONS {
            let mut response = Response::builder()
                .status(StatusCode::NO_CONTENT)
                .header(header::CONTENT_LENGTH, "0")
                .body(response::empty())?;
            self.apply_static_headers(&mut response);
            return Ok(response);
        }

        if method != Method::GET && method != Method::HEAD {
            let mut response =
                response::text(StatusCode::METHOD_NOT_ALLOWED, "405 Method Not Allowed\n");
            response.headers_mut().insert(
                header::ALLOW,
                HeaderValue::from_static("GET, HEAD, OPTIONS"),
            );
            return Ok(self.static_response(response));
        }

        let mut target = match self.resolve_existing(&request_path).await? {
            Some(path) => path,
            None => {
                return Ok(
                    self.static_response(response::text(StatusCode::NOT_FOUND, "404 Not Found\n"))
                );
            }
        };

        let metadata = fs::metadata(&target)
            .await
            .with_context(|| format!("failed to stat {}", target.display()))?;

        if metadata.is_dir() {
            if !uri.path().ends_with('/') {
                return Ok(self.static_response(directory_redirect(&uri)?));
            }
            target = match self.resolve_index(&request_path).await? {
                Some(path) => path,
                None => {
                    return Ok(self.static_response(response::text(
                        StatusCode::NOT_FOUND,
                        "404 Not Found\n",
                    )));
                }
            };
        }

        let metadata = fs::metadata(&target)
            .await
            .with_context(|| format!("failed to stat {}", target.display()))?;
        if !metadata.is_file() {
            return Ok(
                self.static_response(response::text(StatusCode::NOT_FOUND, "404 Not Found\n"))
            );
        }

        if self.config.markdown
            && target
                .extension()
                .and_then(OsStr::to_str)
                .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
            && !query_has_raw(uri.query())
        {
            return self.serve_markdown(&target, method == Method::HEAD).await;
        }

        self.serve_file(
            &target,
            req.headers().get(header::RANGE),
            method == Method::HEAD,
        )
        .await
    }

    async fn serve_markdown(&self, path: &Path, head: bool) -> Result<HttpResponse> {
        let source = fs::read_to_string(path)
            .await
            .with_context(|| format!("failed to read Markdown {}", path.display()))?;
        let title = path
            .file_name()
            .and_then(OsStr::to_str)
            .unwrap_or("Markdown")
            .to_string();
        let renderer = Arc::clone(
            self.markdown
                .as_ref()
                .expect("Markdown renderer exists when Markdown serving is enabled"),
        );
        let theme = self.config.markdown_theme;
        let html = tokio::task::spawn_blocking(move || renderer.render(&source, &title, theme))
            .await
            .context("Markdown rendering task failed")?;
        let len = html.len();

        let body = if head {
            response::empty()
        } else {
            response::full(html)
        };
        let mut response = Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
            .header(header::CONTENT_LENGTH, len.to_string())
            .header(
                hyper::header::HeaderName::from_static("content-security-policy"),
                "default-src 'self' data: https:; script-src 'none'; style-src 'unsafe-inline'; object-src 'none'; base-uri 'none'",
            )
            .body(body)?;
        self.apply_static_headers(&mut response);
        Ok(response)
    }

    async fn serve_file(
        &self,
        path: &Path,
        range_header: Option<&HeaderValue>,
        head: bool,
    ) -> Result<HttpResponse> {
        let metadata = fs::metadata(path)
            .await
            .with_context(|| format!("failed to stat {}", path.display()))?;
        let total = metadata.len();

        let range = match range_header {
            Some(value) => {
                let value = match value.to_str() {
                    Ok(value) => value,
                    Err(_) => return Ok(self.range_not_satisfiable(total, head)),
                };
                match parse_single_range(value, total) {
                    Some(range) => Some(range),
                    None => return Ok(self.range_not_satisfiable(total, head)),
                }
            }
            None => None,
        };

        let mime = if path
            .extension()
            .and_then(OsStr::to_str)
            .is_some_and(|e| e.eq_ignore_ascii_case("md"))
        {
            "text/markdown; charset=utf-8".to_string()
        } else {
            mime_guess::from_path(path)
                .first_or_octet_stream()
                .essence_str()
                .to_string()
        };

        let (status, start, length, content_range) = match range {
            Some((start, end)) => (
                StatusCode::PARTIAL_CONTENT,
                start,
                end - start + 1,
                Some(format!("bytes {start}-{end}/{total}")),
            ),
            None => (StatusCode::OK, 0, total, None),
        };

        let body: Body = if head || length == 0 {
            response::empty()
        } else {
            let mut file = File::open(path)
                .await
                .with_context(|| format!("failed to open {}", path.display()))?;
            if start != 0 {
                file.seek(SeekFrom::Start(start)).await?;
            }
            let reader = file.take(length);
            let stream = ReaderStream::new(reader)
                .map_ok(Frame::data)
                .map_err(|e| -> BoxError { Box::new(e) });
            StreamBody::new(stream).boxed_unsync()
        };

        let mut builder = Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, mime)
            .header(header::CONTENT_LENGTH, length.to_string())
            .header(header::ACCEPT_RANGES, "bytes");
        if let Some(value) = content_range {
            builder = builder.header(header::CONTENT_RANGE, value);
        }
        let mut response = builder.body(body)?;
        self.apply_static_headers(&mut response);
        Ok(response)
    }

    fn range_not_satisfiable(&self, total: u64, head: bool) -> HttpResponse {
        let message = "416 Range Not Satisfiable\n";
        let mut response = if head {
            Response::builder()
                .status(StatusCode::RANGE_NOT_SATISFIABLE)
                .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
                .header(header::CONTENT_LENGTH, message.len().to_string())
                .body(response::empty())
                .expect("static HEAD error response is valid")
        } else {
            response::text(StatusCode::RANGE_NOT_SATISFIABLE, message)
        };
        response.headers_mut().insert(
            header::CONTENT_RANGE,
            HeaderValue::from_str(&format!("bytes */{total}")).expect("numeric header is valid"),
        );
        response
            .headers_mut()
            .insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
        self.static_response(response)
    }

    fn static_response(&self, mut response: HttpResponse) -> HttpResponse {
        self.apply_static_headers(&mut response);
        response
    }

    fn apply_static_headers(&self, response: &mut HttpResponse) {
        apply_static_cors(response);
    }

    async fn resolve_existing(&self, components: &[String]) -> Result<Option<PathBuf>> {
        let lexical = join_components(&self.config.root, components);
        if !self.config.follow_symlinks && contains_symlink(&self.config.root, components).await? {
            return Ok(None);
        }

        let canonical = match fs::canonicalize(&lexical).await {
            Ok(path) => path,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) if e.kind() == std::io::ErrorKind::NotADirectory => return Ok(None),
            Err(e) => {
                return Err(e).with_context(|| format!("failed to resolve {}", lexical.display()));
            }
        };

        if path_contains_reserved_beyond_common_root(&self.config.root, &canonical)
            || self.matches_log_path(&canonical).await
        {
            return Ok(None);
        }
        Ok(Some(canonical))
    }

    async fn resolve_index(&self, components: &[String]) -> Result<Option<PathBuf>> {
        let mut with_index = components.to_vec();
        with_index.push("index.html".to_string());
        self.resolve_existing(&with_index).await
    }

    async fn is_hidden_path(&self, components: &[String]) -> bool {
        if components.first().is_some_and(|c| c == "__lserve")
            || components.iter().any(|c| is_reserved_name(OsStr::new(c)))
        {
            return true;
        }
        let lexical = join_components(&self.config.root, components);
        if self.matches_log_path(&lexical).await {
            return true;
        }
        if let Ok(canonical) = fs::canonicalize(&lexical).await
            && (path_contains_reserved_beyond_common_root(&self.config.root, &canonical)
                || self.matches_log_path(&canonical).await)
        {
            return true;
        }
        false
    }

    async fn matches_log_path(&self, candidate: &Path) -> bool {
        for configured in [&self.config.access_log, &self.config.error_log]
            .into_iter()
            .flatten()
        {
            if matches_log_name(candidate, configured) {
                return true;
            }

            if let (Ok(candidate), Ok(configured_parent)) = (
                fs::canonicalize(candidate).await,
                canonical_parent(configured).await,
            ) && candidate.parent() == Some(configured_parent.as_path())
                && rotated_or_active_filename(&candidate, configured)
            {
                return true;
            }
        }
        false
    }
}

pub fn apply_static_cors(response: &mut HttpResponse) {
    let headers = response.headers_mut();
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static("GET, HEAD, OPTIONS"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static("*"),
    );
    headers.insert(
        header::ACCESS_CONTROL_MAX_AGE,
        HeaderValue::from_static("86400"),
    );
}

fn decode_request_path(path: &str) -> std::result::Result<Vec<String>, ()> {
    let decoded = percent_decode_str(path).decode_utf8().map_err(|_| ())?;
    if decoded.contains('\0') {
        return Err(());
    }
    let mut components = Vec::new();
    for component in decoded.split('/') {
        match component {
            "" | "." => {}
            ".." => return Err(()),
            other => components.push(other.to_string()),
        }
    }
    Ok(components)
}

fn join_components(root: &Path, components: &[String]) -> PathBuf {
    let mut path = root.to_path_buf();
    for component in components {
        path.push(component);
    }
    path
}

async fn contains_symlink(root: &Path, components: &[String]) -> Result<bool> {
    let mut path = root.to_path_buf();
    for component in components {
        path.push(component);
        match fs::symlink_metadata(&path).await {
            Ok(meta) if meta.file_type().is_symlink() => return Ok(true),
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(e) if e.kind() == std::io::ErrorKind::NotADirectory => return Ok(false),
            Err(e) => {
                return Err(e).with_context(|| format!("failed to inspect {}", path.display()));
            }
        }
    }
    Ok(false)
}

fn path_contains_reserved_beyond_common_root(root: &Path, target: &Path) -> bool {
    // The document root itself may legally live below a directory whose name is
    // reserved (for example /home/me/.certs/demo). Ignore components shared with
    // the root, but inspect every component introduced by the requested path or
    // an escaping symlink target.
    let root_components: Vec<_> = root.components().collect();
    let target_components: Vec<_> = target.components().collect();
    let common = root_components
        .iter()
        .zip(&target_components)
        .take_while(|(left, right)| left == right)
        .count();

    target_components[common..]
        .iter()
        .any(|component| match component {
            Component::Normal(name) => is_reserved_name(name),
            _ => false,
        })
}

fn is_reserved_name(name: &OsStr) -> bool {
    name == OsStr::new(".lserve") || name == OsStr::new(".certs") || name == OsStr::new(".syntaxes")
}

fn matches_log_name(candidate: &Path, configured: &Path) -> bool {
    let candidate = lexical_normalize(candidate);
    let configured = lexical_normalize(configured);
    candidate.parent() == configured.parent() && rotated_or_active_filename(&candidate, &configured)
}

fn rotated_or_active_filename(candidate: &Path, configured: &Path) -> bool {
    let Some(candidate_name) = candidate.file_name().and_then(OsStr::to_str) else {
        return false;
    };
    let Some(base_name) = configured.file_name().and_then(OsStr::to_str) else {
        return false;
    };
    if candidate_name == base_name {
        return true;
    }
    let Some(suffix) = candidate_name.strip_prefix(&format!("{base_name}.")) else {
        return false;
    };
    let timestamp = suffix.split('.').next().unwrap_or(suffix);
    timestamp.len() == 15
        && timestamp.as_bytes().get(8) == Some(&b'_')
        && timestamp
            .bytes()
            .enumerate()
            .all(|(i, b)| i == 8 || b.is_ascii_digit())
}

async fn canonical_parent(path: &Path) -> Result<PathBuf> {
    let parent = path.parent().context("log path has no parent")?;
    Ok(fs::canonicalize(parent)
        .await
        .unwrap_or_else(|_| lexical_normalize(parent)))
}

fn lexical_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn query_has_raw(query: Option<&str>) -> bool {
    query.is_some_and(|query| {
        query.split('&').any(|part| {
            let key = part.split_once('=').map(|(key, _)| key).unwrap_or(part);
            key == "raw"
        })
    })
}

fn directory_redirect(uri: &Uri) -> Result<HttpResponse> {
    let mut location = uri.path().to_string();
    location.push('/');
    if let Some(query) = uri.query() {
        location.push('?');
        location.push_str(query);
    }
    Ok(Response::builder()
        .status(StatusCode::PERMANENT_REDIRECT)
        .header(header::LOCATION, location)
        .header(header::CONTENT_LENGTH, "0")
        .body(response::empty())?)
}

fn parse_single_range(header_value: &str, total: u64) -> Option<(u64, u64)> {
    let (unit, spec) = header_value.trim().split_once('=')?;
    if !unit.trim().eq_ignore_ascii_case("bytes") {
        return None;
    }
    let spec = spec.trim();
    if spec.contains(',') || total == 0 {
        return None;
    }
    let (start, end) = spec.split_once('-')?;
    if start.is_empty() {
        let suffix: u64 = end.parse().ok()?;
        if suffix == 0 {
            return None;
        }
        let length = suffix.min(total);
        return Some((total - length, total - 1));
    }

    let start: u64 = start.parse().ok()?;
    if start >= total {
        return None;
    }
    let end = if end.is_empty() {
        total - 1
    } else {
        end.parse::<u64>().ok()?.min(total - 1)
    };
    if end < start {
        return None;
    }
    Some((start, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traversal_is_rejected_after_percent_decode() {
        assert!(decode_request_path("/%2e%2e/secret").is_err());
        assert!(decode_request_path("/foo/../../secret").is_err());
    }

    #[test]
    fn ranges_cover_common_media_cases() {
        assert_eq!(parse_single_range("bytes=0-99", 1000), Some((0, 99)));
        assert_eq!(parse_single_range("bytes=900-", 1000), Some((900, 999)));
        assert_eq!(parse_single_range("bytes=-100", 1000), Some((900, 999)));
        assert_eq!(parse_single_range("Bytes=0-9", 1000), Some((0, 9)));
        assert_eq!(parse_single_range("bytes=1000-", 1000), None);
        assert_eq!(parse_single_range("bytes=0-1,4-5", 1000), None);
    }

    #[test]
    fn rotated_log_names_are_recognized() {
        let base = Path::new("/tmp/access.log");
        assert!(matches_log_name(Path::new("/tmp/access.log"), base));
        assert!(matches_log_name(
            Path::new("/tmp/access.log.20260924_191105"),
            base
        ));
        assert!(matches_log_name(
            Path::new("/tmp/access.log.20260924_191105.1"),
            base
        ));
        assert!(!matches_log_name(Path::new("/tmp/access.log.backup"), base));
    }

    #[test]
    fn reserved_ancestor_of_root_does_not_hide_entire_site() {
        let root = Path::new("/home/me/.certs/demo");
        assert!(!path_contains_reserved_beyond_common_root(
            root,
            Path::new("/home/me/.certs/demo/index.html"),
        ));
        assert!(path_contains_reserved_beyond_common_root(
            root,
            Path::new("/home/me/.certs/demo/foo/.certs/server.pem"),
        ));
        assert!(path_contains_reserved_beyond_common_root(
            Path::new("/home/me/project"),
            Path::new("/home/me/.certs/server.pem"),
        ));
    }
}
