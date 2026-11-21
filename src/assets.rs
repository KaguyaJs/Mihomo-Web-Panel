use axum::{
    body::Body,
    http::{HeaderValue, StatusCode, Uri},
    response::{IntoResponse, Response},
};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "web/dist/"]
struct Assets;

pub async fn serve(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    if path.starts_with("api/") || path.contains("..") || path.contains('\\') {
        return StatusCode::NOT_FOUND.into_response();
    }
    let name = if path.is_empty() { "index.html" } else { path };
    let file = Assets::get(name).or_else(|| {
        if path.contains('.') {
            None
        } else {
            Assets::get("index.html")
        }
    });
    match file {
        Some(file) => {
            let mut response = Response::new(Body::from(file.data.into_owned()));
            let mime = if !path.contains('.') {
                "text/html".to_string()
            } else {
                mime_guess::from_path(name)
                    .first_or_octet_stream()
                    .to_string()
            };
            response
                .headers_mut()
                .insert("content-type", HeaderValue::from_str(&mime).unwrap());
            response
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}
