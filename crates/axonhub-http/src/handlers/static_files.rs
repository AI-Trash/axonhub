use crate::errors::not_implemented_response;
use actix_web::http::header::{CACHE_CONTROL, CONTENT_TYPE, EXPIRES, HeaderValue, PRAGMA};
use actix_web::{web, HttpRequest, HttpResponse};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "../../frontend/dist"]
#[allow_missing = true]
struct FrontendAssets;

#[derive(Clone)]
pub(crate) struct StaticFilesConfig {
    pub(crate) base_path: String,
}

const INDEX_PATH: &str = "index.html";
const FAVICON_PATH: &str = "favicon.ico";
const SPA_CACHE_CONTROL_VALUE: &str = "no-cache, no-store, must-revalidate";
const FAVICON_CACHE_CONTROL_VALUE: &str = "public, max-age=3600";
const FALLBACK_INDEX_HTML: &str = r#"<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>AxonHub</title>
    <script type="module" src="/assets/index.js"></script>
  </head>
  <body>
    <div id="root"></div>
  </body>
</html>
"#;
const FALLBACK_FAVICON: &[u8] = &[0x00, 0x00, 0x01, 0x00];
const API_PREFIXES: &[&str] = &[
    "/admin",
    "/anthropic",
    "/doubao",
    "/gemini",
    "/health",
    "/jina",
    "/openapi",
    "/v1",
    "/v1beta",
];

pub(crate) async fn favicon() -> HttpResponse {
    serve_favicon_response().unwrap_or_else(|| {
        not_implemented_response(
            "/*",
            actix_web::http::Method::GET,
            "/favicon".parse().expect("valid favicon uri"),
            None,
        )
        .into_response()
    })
}

pub(crate) async fn serve_embedded_or_not_implemented(request: HttpRequest) -> HttpResponse {
    let request_path = strip_base_path(
        request.path(),
        request
            .app_data::<web::Data<StaticFilesConfig>>()
            .map(|config| config.base_path.as_str())
            .unwrap_or("/"),
    );

    if let Some(response) = embedded_response_for_request(request_path) {
        return response;
    }

    not_implemented_response("/*", request.method().clone(), request.uri().clone(), None)
        .into_response()
}

fn serve_favicon_response() -> Option<HttpResponse> {
    embedded_asset_bytes(FAVICON_PATH).map(|body| {
        let mut response = HttpResponse::Ok();
        response.insert_header((CONTENT_TYPE, mime_for_path(FAVICON_PATH)));
        response.insert_header((CACHE_CONTROL, HeaderValue::from_static(FAVICON_CACHE_CONTROL_VALUE)));
        response.body(body)
    })
}

fn embedded_response_for_request(path: &str) -> Option<HttpResponse> {
    if path == "/favicon" {
        return serve_favicon_response();
    }

    let asset_path = normalize_asset_path(path);
    if let Some(body) = embedded_asset_bytes(&asset_path) {
        return Some(response_for_asset(&asset_path, body));
    }

    if should_fallback_to_index(path) {
        return embedded_asset_bytes(INDEX_PATH).map(response_for_spa);
    }

    None
}

fn embedded_asset_bytes(path: &str) -> Option<Vec<u8>> {
    FrontendAssets::get(path)
        .map(|file| file.data.into_owned())
        .or_else(|| fallback_asset_bytes(path).map(ToOwned::to_owned))
}

fn fallback_asset_bytes(path: &str) -> Option<&'static [u8]> {
    match path {
        INDEX_PATH => Some(FALLBACK_INDEX_HTML.as_bytes()),
        FAVICON_PATH => Some(FALLBACK_FAVICON),
        _ => None,
    }
}

fn normalize_asset_path(path: &str) -> String {
    match path.trim_start_matches('/') {
        "" => INDEX_PATH.to_owned(),
        value => value.to_owned(),
    }
}

fn should_fallback_to_index(path: &str) -> bool {
    let trimmed = path.trim_start_matches('/');
    !trimmed.is_empty() && !is_api_like_path(path)
}

fn is_api_like_path(path: &str) -> bool {
    API_PREFIXES
        .iter()
        .any(|prefix| path == *prefix || path.starts_with(&format!("{prefix}/")))
}

fn strip_base_path<'a>(path: &'a str, base_path: &str) -> &'a str {
    let normalized = match base_path.trim() {
        "" | "/" => return path,
        value => value.trim_end_matches('/'),
    };

    if path == normalized {
        "/"
    } else {
        path.strip_prefix(normalized).unwrap_or(path)
    }
}

fn response_for_asset(path: &str, body: Vec<u8>) -> HttpResponse {
    let mut response = HttpResponse::Ok();
    response.insert_header((CONTENT_TYPE, mime_for_path(path)));
    response.body(body)
}

fn response_for_spa(body: Vec<u8>) -> HttpResponse {
    let mut response = HttpResponse::Ok();
    response.insert_header((CONTENT_TYPE, mime_for_path(INDEX_PATH)));
    response.insert_header((CACHE_CONTROL, HeaderValue::from_static(SPA_CACHE_CONTROL_VALUE)));
    response.insert_header((PRAGMA, HeaderValue::from_static("no-cache")));
    response.insert_header((EXPIRES, HeaderValue::from_static("0")));
    response.body(body)
}

fn mime_for_path(path: &str) -> HeaderValue {
    let mime = match path.rsplit('.').next().unwrap_or_default() {
        "css" => "text/css; charset=utf-8",
        "gif" => "image/gif",
        "htm" | "html" => "text/html; charset=utf-8",
        "ico" => "image/x-icon",
        "jpeg" | "jpg" => "image/jpeg",
        "js" => "text/javascript; charset=utf-8",
        "json" => "application/json",
        "mjs" => "text/javascript; charset=utf-8",
        "png" => "image/png",
        "svg" => "image/svg+xml",
        "txt" => "text/plain; charset=utf-8",
        "webp" => "image/webp",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    };

    HeaderValue::from_static(mime)
}
