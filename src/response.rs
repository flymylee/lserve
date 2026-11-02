use bytes::Bytes;
use http_body_util::{BodyExt, Full, combinators::UnsyncBoxBody};
use hyper::{Response, StatusCode, header};
use std::{convert::Infallible, error::Error};

pub type BoxError = Box<dyn Error + Send + Sync>;
pub type Body = UnsyncBoxBody<Bytes, BoxError>;
pub type HttpResponse = Response<Body>;

pub fn full(bytes: impl Into<Bytes>) -> Body {
    Full::new(bytes.into())
        .map_err(infallible_to_box)
        .boxed_unsync()
}

pub fn empty() -> Body {
    full(Bytes::new())
}

pub fn text(status: StatusCode, message: impl Into<String>) -> HttpResponse {
    let message = message.into();
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .header(header::CONTENT_LENGTH, message.len().to_string())
        .body(full(message))
        .expect("static response headers are valid")
}

pub fn without_body(response: HttpResponse) -> HttpResponse {
    let (parts, _) = response.into_parts();
    Response::from_parts(parts, empty())
}

pub fn internal_error(verbose: bool, detail: &str) -> HttpResponse {
    if verbose {
        text(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("500 Internal Server Error\n\n{detail}\n"),
        )
    } else {
        text(
            StatusCode::INTERNAL_SERVER_ERROR,
            "500 Internal Server Error\n",
        )
    }
}

pub fn bad_gateway(verbose: bool, detail: &str) -> HttpResponse {
    if verbose {
        text(
            StatusCode::BAD_GATEWAY,
            format!("502 Bad Gateway\n\n{detail}\n"),
        )
    } else {
        text(StatusCode::BAD_GATEWAY, "502 Bad Gateway\n")
    }
}

fn infallible_to_box(never: Infallible) -> BoxError {
    match never {}
}
