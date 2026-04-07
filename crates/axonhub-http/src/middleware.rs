use crate::state::transport::{
    HttpMetricRecord, HttpRejection, TransportHeaders, authenticate_admin_request,
    authenticate_api_key_request, authenticate_gemini_request,
    authenticate_service_api_key_request, enrich_request_context, resolve_http_metric_path,
};
use crate::state::{
    GeminiQueryKey, HttpMetricsCapability, HttpState, RequestAuthContext, RequestContextState,
};
use actix_web::body::{BoxBody, MessageBody};
use actix_web::dev::{Service, ServiceRequest, ServiceResponse, Transform};
use actix_web::http::StatusCode;
use actix_web::{Error, FromRequest, HttpMessage, HttpResponse, web};
use opentelemetry::propagation::{Extractor, TextMapPropagator};
use opentelemetry::trace::TraceContextExt;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use std::future::{Future, Ready as StdReady, ready};
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll};
use std::time::Instant;
use tracing::{field, Instrument, Level, Span};
use tracing_opentelemetry::OpenTelemetrySpanExt;

pub(crate) fn request_context() -> RequestContextMiddleware {
    RequestContextMiddleware
}

pub(crate) fn admin_auth() -> AdminAuthMiddleware {
    AdminAuthMiddleware
}

pub(crate) fn api_key_auth() -> ApiKeyAuthMiddleware {
    ApiKeyAuthMiddleware
}

pub(crate) fn service_api_key_auth() -> ServiceApiKeyAuthMiddleware {
    ServiceApiKeyAuthMiddleware
}

pub(crate) fn gemini_auth() -> GeminiAuthMiddleware {
    GeminiAuthMiddleware
}

pub(crate) fn http_metrics(http_metrics: HttpMetricsCapability) -> HttpMetricsMiddleware {
    HttpMetricsMiddleware { http_metrics }
}

fn request_span(req: &ServiceRequest) -> Span {
    tracing::span!(
        Level::INFO,
        "http.request",
        http.method = %req.method(),
        http.route = %req.match_pattern().unwrap_or_else(|| req.path().to_owned()),
        http.target = %req.uri(),
        http.status_code = field::Empty,
        auth.mode = field::Empty,
        auth.subject = field::Empty,
        request.bound = field::Empty,
        trace.bound = field::Empty,
        thread.bound = field::Empty,
        project.bound = field::Empty,
        trace.remote_parent = field::Empty,
    )
}

fn record_request_context(span: &Span, context: &RequestContextState) {
    if context.request_id.is_some() {
        span.record("request.bound", true);
    }
    if context.project.is_some() {
        span.record("project.bound", true);
    }
    if context.thread.is_some() {
        span.record("thread.bound", true);
    }
    if context.trace.is_some() {
        span.record("trace.bound", true);
    }
    if let Some(auth) = context.auth.as_ref() {
        match auth {
            RequestAuthContext::Admin(_) => {
                span.record("auth.mode", "jwt");
                span.record("auth.subject", "admin");
            }
            RequestAuthContext::ApiKey(key) => {
                span.record(
                    "auth.mode",
                    match key.key_type {
                        crate::models::ApiKeyType::NoAuth => "noauth",
                        _ => "api_key",
                    },
                );
                span.record(
                    "auth.subject",
                    match key.key_type {
                        crate::models::ApiKeyType::User => "user_api_key",
                        crate::models::ApiKeyType::ServiceAccount => "service_api_key",
                        crate::models::ApiKeyType::NoAuth => "system_noauth",
                    },
                );
            }
        }
    }
}

fn apply_inbound_trace_parent(span: &Span, headers: &TransportHeaders) {
    let propagator = TraceContextPropagator::new();
    let parent = propagator.extract(headers);
    if parent.span().span_context().is_valid() {
        span.record("trace.remote_parent", true);
        let _ = span.set_parent(parent);
    }
}

pub(crate) struct RequestContextMiddleware;

impl<S, B> Transform<S, ServiceRequest> for RequestContextMiddleware
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    B: MessageBody + 'static,
{
    type Response = ServiceResponse<BoxBody>;
    type Error = Error;
    type Transform = RequestContextMiddlewareService<S>;
    type InitError = ();
    type Future = StdReady<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(RequestContextMiddlewareService {
            service: Rc::new(service),
        }))
    }
}

pub(crate) struct RequestContextMiddlewareService<S> {
    service: Rc<S>,
}

impl<S, B> Service<ServiceRequest> for RequestContextMiddlewareService<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    B: MessageBody + 'static,
{
    type Response = ServiceResponse<BoxBody>;
    type Error = Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>>>>;

    fn poll_ready(&self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.service.poll_ready(cx)
    }

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let service = Rc::clone(&self.service);
        let request_span = Span::current();
        Box::pin(async move {
            let state = match req.app_data::<web::Data<HttpState>>() {
                Some(state) => state.clone(),
                None => {
                    tracing::error!("HTTP state is not configured");
                    return Ok(req.into_response(
                        crate::errors::internal_error_response(
                            "HTTP state is not configured".to_owned(),
                        )
                        .map_into_boxed_body(),
                    ))
                }
            };

            let headers = transport_headers(req.headers());
            let context = req
                .extensions_mut()
                .remove::<RequestContextState>()
                .unwrap_or_default();
            let context = match enrich_request_context(
                &state.request_context,
                &headers,
                &state.trace_config,
                context,
            ) {
                Ok(context) => context,
                Err(rejection) => {
                    tracing::warn!(rejection = ?rejection, "request context enrichment rejected");
                    return Ok(req.into_response(http_rejection_response(rejection).map_into_boxed_body()))
                }
            };
            record_request_context(&request_span, &context);
            req.extensions_mut().insert(context);

            let response = service.call(req).await?.map_into_boxed_body();
            Ok(response)
        })
    }
}

pub(crate) struct AdminAuthMiddleware;

impl<S, B> Transform<S, ServiceRequest> for AdminAuthMiddleware
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    B: MessageBody + 'static,
{
    type Response = ServiceResponse<BoxBody>;
    type Error = Error;
    type Transform = AdminAuthMiddlewareService<S>;
    type InitError = ();
    type Future = StdReady<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(AdminAuthMiddlewareService {
            service: Rc::new(service),
        }))
    }
}

pub(crate) struct AdminAuthMiddlewareService<S> {
    service: Rc<S>,
}

impl<S, B> Service<ServiceRequest> for AdminAuthMiddlewareService<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    B: MessageBody + 'static,
{
    type Response = ServiceResponse<BoxBody>;
    type Error = Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>>>>;

    fn poll_ready(&self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.service.poll_ready(cx)
    }

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let service = Rc::clone(&self.service);
        let request_span = Span::current();
        Box::pin(async move {
            let state = match req.app_data::<web::Data<HttpState>>() {
                Some(state) => state.clone(),
                None => {
                    tracing::error!("HTTP state is not configured");
                    return Ok(req.into_response(
                        crate::errors::internal_error_response(
                            "HTTP state is not configured".to_owned(),
                        )
                        .map_into_boxed_body(),
                    ))
                }
            };

            let headers = transport_headers(req.headers());
            let context = req
                .extensions_mut()
                .remove::<RequestContextState>()
                .unwrap_or_default();
            let context = match authenticate_admin_request(&state.identity, &headers, context) {
                Ok(context) => context,
                Err(rejection) => {
                    tracing::warn!(rejection = ?rejection, "admin auth rejected request");
                    return Ok(req.into_response(http_rejection_response(rejection).map_into_boxed_body()))
                }
            };
            record_request_context(&request_span, &context);
            req.extensions_mut().insert(context);

            let response = service.call(req).await?.map_into_boxed_body();
            Ok(response)
        })
    }
}

pub(crate) struct ApiKeyAuthMiddleware;

impl<S, B> Transform<S, ServiceRequest> for ApiKeyAuthMiddleware
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    B: MessageBody + 'static,
{
    type Response = ServiceResponse<BoxBody>;
    type Error = Error;
    type Transform = ApiKeyAuthMiddlewareService<S>;
    type InitError = ();
    type Future = StdReady<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(ApiKeyAuthMiddlewareService {
            service: Rc::new(service),
        }))
    }
}

pub(crate) struct ApiKeyAuthMiddlewareService<S> {
    service: Rc<S>,
}

impl<S, B> Service<ServiceRequest> for ApiKeyAuthMiddlewareService<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    B: MessageBody + 'static,
{
    type Response = ServiceResponse<BoxBody>;
    type Error = Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>>>>;

    fn poll_ready(&self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.service.poll_ready(cx)
    }

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let service = Rc::clone(&self.service);
        let request_span = Span::current();
        Box::pin(async move {
            let state = match req.app_data::<web::Data<HttpState>>() {
                Some(state) => state.clone(),
                None => {
                    tracing::error!("HTTP state is not configured");
                    return Ok(req.into_response(
                        crate::errors::internal_error_response(
                            "HTTP state is not configured".to_owned(),
                        )
                        .map_into_boxed_body(),
                    ))
                }
            };

            let headers = transport_headers(req.headers());
            let context = req
                .extensions_mut()
                .remove::<RequestContextState>()
                .unwrap_or_default();
            let context = match authenticate_api_key_request(
                &state.identity,
                &headers,
                state.allow_no_auth,
                context,
            ) {
                Ok(context) => context,
                Err(rejection) => {
                    tracing::warn!(rejection = ?rejection, "api key auth rejected request");
                    return Ok(req.into_response(http_rejection_response(rejection).map_into_boxed_body()))
                }
            };
            record_request_context(&request_span, &context);
            req.extensions_mut().insert(context);

            let response = service.call(req).await?.map_into_boxed_body();
            Ok(response)
        })
    }
}

pub(crate) struct ServiceApiKeyAuthMiddleware;

impl<S, B> Transform<S, ServiceRequest> for ServiceApiKeyAuthMiddleware
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    B: MessageBody + 'static,
{
    type Response = ServiceResponse<BoxBody>;
    type Error = Error;
    type Transform = ServiceApiKeyAuthMiddlewareService<S>;
    type InitError = ();
    type Future = StdReady<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(ServiceApiKeyAuthMiddlewareService {
            service: Rc::new(service),
        }))
    }
}

pub(crate) struct ServiceApiKeyAuthMiddlewareService<S> {
    service: Rc<S>,
}

impl<S, B> Service<ServiceRequest> for ServiceApiKeyAuthMiddlewareService<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    B: MessageBody + 'static,
{
    type Response = ServiceResponse<BoxBody>;
    type Error = Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>>>>;

    fn poll_ready(&self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.service.poll_ready(cx)
    }

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let service = Rc::clone(&self.service);
        let request_span = Span::current();
        Box::pin(async move {
            let state = match req.app_data::<web::Data<HttpState>>() {
                Some(state) => state.clone(),
                None => {
                    tracing::error!("HTTP state is not configured");
                    return Ok(req.into_response(
                        crate::errors::internal_error_response(
                            "HTTP state is not configured".to_owned(),
                        )
                        .map_into_boxed_body(),
                    ))
                }
            };

            let headers = transport_headers(req.headers());
            let context = req
                .extensions_mut()
                .remove::<RequestContextState>()
                .unwrap_or_default();
            let context = match authenticate_service_api_key_request(&state.identity, &headers, context) {
                Ok(context) => context,
                Err(rejection) => {
                    tracing::warn!(rejection = ?rejection, "service api key auth rejected request");
                    return Ok(req.into_response(http_rejection_response(rejection).map_into_boxed_body()))
                }
            };
            record_request_context(&request_span, &context);
            req.extensions_mut().insert(context);

            let response = service.call(req).await?.map_into_boxed_body();
            Ok(response)
        })
    }
}

pub(crate) struct GeminiAuthMiddleware;

impl<S, B> Transform<S, ServiceRequest> for GeminiAuthMiddleware
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    B: MessageBody + 'static,
{
    type Response = ServiceResponse<BoxBody>;
    type Error = Error;
    type Transform = GeminiAuthMiddlewareService<S>;
    type InitError = ();
    type Future = StdReady<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(GeminiAuthMiddlewareService {
            service: Rc::new(service),
        }))
    }
}

pub(crate) struct GeminiAuthMiddlewareService<S> {
    service: Rc<S>,
}

impl<S, B> Service<ServiceRequest> for GeminiAuthMiddlewareService<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    B: MessageBody + 'static,
{
    type Response = ServiceResponse<BoxBody>;
    type Error = Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>>>>;

    fn poll_ready(&self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.service.poll_ready(cx)
    }

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let service = Rc::clone(&self.service);
        let request_span = Span::current();
        Box::pin(async move {
            let state = match req.app_data::<web::Data<HttpState>>() {
                Some(state) => state.clone(),
                None => {
                    tracing::error!("HTTP state is not configured");
                    return Ok(req.into_response(
                        crate::errors::internal_error_response(
                            "HTTP state is not configured".to_owned(),
                        )
                        .map_into_boxed_body(),
                    ))
                }
            };

            let headers = transport_headers(req.headers());
            let query = match web::Query::<GeminiQueryKey>::from_query(req.query_string()) {
                Ok(query) => query.into_inner(),
                Err(_) => GeminiQueryKey { key: None },
            };
            let context = req
                .extensions_mut()
                .remove::<RequestContextState>()
                .unwrap_or_default();
            let context = match authenticate_gemini_request(
                &state.identity,
                &headers,
                query.key.as_deref(),
                context,
            ) {
                Ok(context) => context,
                Err(rejection) => {
                    tracing::warn!(rejection = ?rejection, "gemini auth rejected request");
                    return Ok(req.into_response(http_rejection_response(rejection).map_into_boxed_body()))
                }
            };
            record_request_context(&request_span, &context);
            req.extensions_mut().insert(context);

            let response = service.call(req).await?.map_into_boxed_body();
            Ok(response)
        })
    }
}

pub(crate) struct HttpMetricsMiddleware {
    http_metrics: HttpMetricsCapability,
}

impl<S, B> Transform<S, ServiceRequest> for HttpMetricsMiddleware
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    B: MessageBody + 'static,
{
    type Response = ServiceResponse<BoxBody>;
    type Error = Error;
    type Transform = HttpMetricsMiddlewareService<S>;
    type InitError = ();
    type Future = StdReady<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(HttpMetricsMiddlewareService {
            service: Rc::new(service),
            http_metrics: self.http_metrics.clone(),
        }))
    }
}

pub(crate) struct HttpMetricsMiddlewareService<S> {
    service: Rc<S>,
    http_metrics: HttpMetricsCapability,
}

impl<S, B> Service<ServiceRequest> for HttpMetricsMiddlewareService<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    B: MessageBody + 'static,
{
    type Response = ServiceResponse<BoxBody>;
    type Error = Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>>>>;

    fn poll_ready(&self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.service.poll_ready(cx)
    }

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let service = Rc::clone(&self.service);
        let http_metrics = self.http_metrics.clone();
        let span = request_span(&req);
        let headers = transport_headers(req.headers());
        apply_inbound_trace_parent(&span, &headers);
        let request_span = span.clone();
        Box::pin(
            async move {
            let HttpMetricsCapability::Available { recorder } = http_metrics else {
                let response = service.call(req).await?.map_into_boxed_body();
                request_span.record("http.status_code", response.status().as_u16());
                tracing::info!(
                    http.status_code = response.status().as_u16(),
                    "request completed"
                );
                return Ok(response);
            };

            let method = req.method().clone();
            let path = resolve_http_metric_path(
                req.match_pattern().as_deref(),
                req.request().path(),
            );
            let started_at = Instant::now();

            let response = service.call(req).await?.map_into_boxed_body();
            request_span.record("http.status_code", response.status().as_u16());
            let metric = HttpMetricRecord::new(method.as_str(), path, response.status().as_u16());
            recorder.record_http_request(
                &metric.method,
                &metric.path,
                metric.status_code,
                started_at.elapsed(),
            );
            tracing::info!(
                http.status_code = metric.status_code,
                duration_ms = started_at.elapsed().as_millis() as u64,
                "request completed"
            );

            Ok(response)
            }
            .instrument(span),
        )
    }
}

impl Extractor for TransportHeaders {
    fn get(&self, key: &str) -> Option<&str> {
        self.get(key)
    }

    fn keys(&self) -> Vec<&str> {
        Vec::new()
    }
}

fn transport_headers(headers: &actix_web::http::header::HeaderMap) -> TransportHeaders {
    let mut result = TransportHeaders::default();
    for (name, value) in headers {
        if let Ok(value) = value.to_str() {
            result.insert(name.as_str(), value);
        }
    }
    result
}

fn http_rejection_response(rejection: HttpRejection) -> HttpResponse {
    match rejection {
        HttpRejection::Error(error) => crate::errors::error_response(
            StatusCode::from_u16(error.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            error.kind,
            &error.message,
        ),
        HttpRejection::NotImplemented(route) => {
            crate::errors::NotImplementedJsonResponse::from_route(route).into_response()
        }
    }
}

pub(crate) struct ActixRequest(pub actix_web::HttpRequest);

impl FromRequest for ActixRequest {
    type Error = Error;
    type Future = StdReady<Result<Self, Self::Error>>;

    fn from_request(
        req: &actix_web::HttpRequest,
        _payload: &mut actix_web::dev::Payload,
    ) -> Self::Future {
        ready(Ok(Self(req.clone())))
    }
}
