use crate::errors::not_implemented_response;
use crate::handlers::gemini_version_from_path;
use actix_web::http::Method;
use actix_web::{HttpRequest, HttpResponse};

pub(crate) async fn unported_v1(request: HttpRequest) -> HttpResponse {
    not_implemented_response(
        "/v1/*",
        Method::from(request.method().clone()),
        request.uri().clone(),
        None,
    )
    .into_response()
}

pub(crate) async fn unported_admin(request: HttpRequest) -> HttpResponse {
    not_implemented_response(
        "/admin/*",
        Method::from(request.method().clone()),
        request.uri().clone(),
        None,
    )
    .into_response()
}

pub(crate) async fn unported_gemini(request: HttpRequest) -> HttpResponse {
    let path = request.uri().path().to_owned();
    let route_family = if path.starts_with("/v1beta") {
        "/v1beta/*"
    } else {
        "/gemini/:gemini_api_version/*"
    };
    not_implemented_response(
        route_family,
        Method::from(request.method().clone()),
        request.uri().clone(),
        gemini_version_from_path(path.as_str()),
    )
    .into_response()
}
