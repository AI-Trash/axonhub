use crate::errors::{error_response, execute_oauth_provider_admin_request};
use crate::handlers::oauth_provider_admin_port;
use crate::state::{HttpState, RequestAuthContext, RequestContextState};
use actix_web::http::{Method, StatusCode, Uri};
use actix_web::{HttpMessage, HttpRequest, HttpResponse, web};
use bytes::Bytes;
use std::sync::Arc;

const WRITE_CHANNELS_SCOPE: &str = "write_channels";

fn require_oauth_provider_admin_write_channels_scope(
    request: &HttpRequest,
) -> Result<(), HttpResponse> {
    let extensions = request.extensions();
    let user = extensions
        .get::<RequestContextState>()
        .and_then(|context| context.auth.as_ref())
        .and_then(|auth| match auth {
            RequestAuthContext::Admin(user) => Some(user),
            RequestAuthContext::ApiKey(_) => None,
        })
        .ok_or_else(|| error_response(StatusCode::UNAUTHORIZED, "Unauthorized", "Invalid token"))?;

    if user.has_system_scope(WRITE_CHANNELS_SCOPE) {
        return Ok(());
    }

    Err(error_response(
        StatusCode::FORBIDDEN,
        "Forbidden",
        "permission denied",
    ))
}

fn unsupported_oauth_provider_admin_response<'a>(
    state: &'a HttpState,
    route_family: &'static str,
    method: Method,
    uri: Uri,
) -> Result<&'a Arc<dyn crate::ports::OauthProviderAdminPort>, HttpResponse> {
    match oauth_provider_admin_port(state) {
        Ok(oauth_provider_admin) => Ok(oauth_provider_admin),
        Err(message) => Err(
            crate::errors::not_implemented_response(route_family, method, uri, None)
                .with_message(&message),
        ),
    }
}

pub async fn start_codex_oauth(
    state: web::Data<HttpState>,
    http_request: HttpRequest,
    body: Bytes,
) -> HttpResponse {
    if let Err(response) = require_oauth_provider_admin_write_channels_scope(&http_request) {
        return response;
    }

    let request_payload: crate::models::StartPkceOAuthRequest = match serde_json::from_slice(&body) {
        Ok(payload) => payload,
        Err(_) => {
            return error_response(StatusCode::BAD_REQUEST, "Bad Request", "invalid request format")
        }
    };

    let oauth_provider_admin = match unsupported_oauth_provider_admin_response(
        &state,
        "/admin/codex/oauth/start",
        Method::POST,
        http_request.uri().clone(),
    ) {
        Ok(oauth_provider_admin) => oauth_provider_admin,
        Err(response) => return response,
    };

    execute_oauth_provider_admin_request(Arc::clone(oauth_provider_admin), move |oauth_provider_admin| {
        oauth_provider_admin.start_codex_oauth(&request_payload)
    })
    .await
}

pub(crate) async fn exchange_codex_oauth(
    state: web::Data<HttpState>,
    http_request: HttpRequest,
    body: Bytes,
) -> HttpResponse {
    if let Err(response) = require_oauth_provider_admin_write_channels_scope(&http_request) {
        return response;
    }

    let request_payload: crate::models::ExchangeCallbackOAuthRequest = match serde_json::from_slice(&body) {
        Ok(payload) => payload,
        Err(_) => {
            return error_response(StatusCode::BAD_REQUEST, "Bad Request", "invalid request format")
        }
    };

    let oauth_provider_admin = match unsupported_oauth_provider_admin_response(
        &state,
        "/admin/codex/oauth/exchange",
        Method::POST,
        http_request.uri().clone(),
    ) {
        Ok(oauth_provider_admin) => oauth_provider_admin,
        Err(response) => return response,
    };

    execute_oauth_provider_admin_request(Arc::clone(oauth_provider_admin), move |oauth_provider_admin| {
        oauth_provider_admin.exchange_codex_oauth(&request_payload)
    })
    .await
}

pub async fn start_claudecode_oauth(
    state: web::Data<HttpState>,
    http_request: HttpRequest,
    body: Bytes,
) -> HttpResponse {
    if let Err(response) = require_oauth_provider_admin_write_channels_scope(&http_request) {
        return response;
    }

    let request_payload: crate::models::StartPkceOAuthRequest = match serde_json::from_slice(&body) {
        Ok(payload) => payload,
        Err(_) => {
            return error_response(StatusCode::BAD_REQUEST, "Bad Request", "invalid request format")
        }
    };

    let oauth_provider_admin = match unsupported_oauth_provider_admin_response(
        &state,
        "/admin/claudecode/oauth/start",
        Method::POST,
        http_request.uri().clone(),
    ) {
        Ok(oauth_provider_admin) => oauth_provider_admin,
        Err(response) => return response,
    };

    execute_oauth_provider_admin_request(Arc::clone(oauth_provider_admin), move |oauth_provider_admin| {
        oauth_provider_admin.start_claudecode_oauth(&request_payload)
    })
    .await
}

pub(crate) async fn exchange_claudecode_oauth(
    state: web::Data<HttpState>,
    http_request: HttpRequest,
    body: Bytes,
) -> HttpResponse {
    if let Err(response) = require_oauth_provider_admin_write_channels_scope(&http_request) {
        return response;
    }

    let request_payload: crate::models::ExchangeCallbackOAuthRequest = match serde_json::from_slice(&body) {
        Ok(payload) => payload,
        Err(_) => {
            return error_response(StatusCode::BAD_REQUEST, "Bad Request", "invalid request format")
        }
    };

    let oauth_provider_admin = match unsupported_oauth_provider_admin_response(
        &state,
        "/admin/claudecode/oauth/exchange",
        Method::POST,
        http_request.uri().clone(),
    ) {
        Ok(oauth_provider_admin) => oauth_provider_admin,
        Err(response) => return response,
    };

    execute_oauth_provider_admin_request(Arc::clone(oauth_provider_admin), move |oauth_provider_admin| {
        oauth_provider_admin.exchange_claudecode_oauth(&request_payload)
    })
    .await
}

pub async fn start_antigravity_oauth(
    state: web::Data<HttpState>,
    http_request: HttpRequest,
    body: Bytes,
) -> HttpResponse {
    if let Err(response) = require_oauth_provider_admin_write_channels_scope(&http_request) {
        return response;
    }

    let request_payload: crate::models::StartAntigravityOAuthRequest = match serde_json::from_slice(&body) {
        Ok(payload) => payload,
        Err(_) => {
            return error_response(StatusCode::BAD_REQUEST, "Bad Request", "invalid request format")
        }
    };

    let oauth_provider_admin = match unsupported_oauth_provider_admin_response(
        &state,
        "/admin/antigravity/oauth/start",
        Method::POST,
        http_request.uri().clone(),
    ) {
        Ok(oauth_provider_admin) => oauth_provider_admin,
        Err(response) => return response,
    };

    execute_oauth_provider_admin_request(Arc::clone(oauth_provider_admin), move |oauth_provider_admin| {
        oauth_provider_admin.start_antigravity_oauth(&request_payload)
    })
    .await
}

pub(crate) async fn exchange_antigravity_oauth(
    state: web::Data<HttpState>,
    http_request: HttpRequest,
    body: Bytes,
) -> HttpResponse {
    if let Err(response) = require_oauth_provider_admin_write_channels_scope(&http_request) {
        return response;
    }

    let request_payload: crate::models::ExchangeCallbackOAuthRequest = match serde_json::from_slice(&body) {
        Ok(payload) => payload,
        Err(_) => {
            return error_response(StatusCode::BAD_REQUEST, "Bad Request", "invalid request format")
        }
    };

    let oauth_provider_admin = match unsupported_oauth_provider_admin_response(
        &state,
        "/admin/antigravity/oauth/exchange",
        Method::POST,
        http_request.uri().clone(),
    ) {
        Ok(oauth_provider_admin) => oauth_provider_admin,
        Err(response) => return response,
    };

    execute_oauth_provider_admin_request(Arc::clone(oauth_provider_admin), move |oauth_provider_admin| {
        oauth_provider_admin.exchange_antigravity_oauth(&request_payload)
    })
    .await
}

pub async fn start_copilot_oauth(
    state: web::Data<HttpState>,
    http_request: HttpRequest,
    body: Bytes,
) -> HttpResponse {
    if let Err(response) = require_oauth_provider_admin_write_channels_scope(&http_request) {
        return response;
    }

    let request_payload = if body.is_empty() {
        crate::models::StartCopilotOAuthRequest {}
    } else {
        match serde_json::from_slice(&body) {
            Ok(payload) => payload,
            Err(_) => {
                return error_response(
                    StatusCode::BAD_REQUEST,
                    "Bad Request",
                    "invalid request format",
                )
            }
        }
    };

    let oauth_provider_admin = match unsupported_oauth_provider_admin_response(
        &state,
        "/admin/copilot/oauth/start",
        Method::POST,
        http_request.uri().clone(),
    ) {
        Ok(oauth_provider_admin) => oauth_provider_admin,
        Err(response) => return response,
    };

    execute_oauth_provider_admin_request(Arc::clone(oauth_provider_admin), move |oauth_provider_admin| {
        oauth_provider_admin.start_copilot_oauth(&request_payload)
    })
    .await
}

pub(crate) async fn poll_copilot_oauth(
    state: web::Data<HttpState>,
    http_request: HttpRequest,
    body: Bytes,
) -> HttpResponse {
    if let Err(response) = require_oauth_provider_admin_write_channels_scope(&http_request) {
        return response;
    }

    let request_payload: crate::models::PollCopilotOAuthRequest = match serde_json::from_slice(&body) {
        Ok(payload) => payload,
        Err(_) => {
            return error_response(StatusCode::BAD_REQUEST, "Bad Request", "invalid request format")
        }
    };

    let oauth_provider_admin = match unsupported_oauth_provider_admin_response(
        &state,
        "/admin/copilot/oauth/poll",
        Method::POST,
        http_request.uri().clone(),
    ) {
        Ok(oauth_provider_admin) => oauth_provider_admin,
        Err(response) => return response,
    };

    execute_oauth_provider_admin_request(Arc::clone(oauth_provider_admin), move |oauth_provider_admin| {
        oauth_provider_admin.poll_copilot_oauth(&request_payload)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{
        AuthUserContext, ExchangeOAuthResponse, PollCopilotOAuthResponse,
        StartCopilotOAuthResponse, StartPkceOAuthResponse, TraceConfig,
    };
    use crate::ports::{OauthProviderAdminError, OauthProviderAdminPort};
    use crate::state::{
        AdminCapability, AdminGraphqlCapability, IdentityCapability, OpenAiV1Capability,
        OauthProviderAdminCapability, OpenApiGraphqlCapability, RequestContextCapability,
        RequestContextState, RequestAuthContext, SystemBootstrapCapability,
    };
    use actix_web::http::StatusCode;
    use actix_web::test::TestRequest;
    use serde_json::json;

    #[derive(Default)]
    struct RecordingOauthProviderAdminPort;

    impl OauthProviderAdminPort for RecordingOauthProviderAdminPort {
        fn start_codex_oauth(
            &self,
            _request: &crate::models::StartPkceOAuthRequest,
        ) -> Result<StartPkceOAuthResponse, OauthProviderAdminError> {
            Ok(StartPkceOAuthResponse {
                session_id: "codex-session".to_owned(),
                auth_url: "https://example.test/codex/start".to_owned(),
            })
        }

        fn exchange_codex_oauth(
            &self,
            _request: &crate::models::ExchangeCallbackOAuthRequest,
        ) -> Result<ExchangeOAuthResponse, OauthProviderAdminError> {
            Ok(ExchangeOAuthResponse {
                credentials: "codex-credentials".to_owned(),
            })
        }

        fn start_claudecode_oauth(
            &self,
            _request: &crate::models::StartPkceOAuthRequest,
        ) -> Result<StartPkceOAuthResponse, OauthProviderAdminError> {
            Ok(StartPkceOAuthResponse {
                session_id: "claudecode-session".to_owned(),
                auth_url: "https://example.test/claudecode/start".to_owned(),
            })
        }

        fn exchange_claudecode_oauth(
            &self,
            _request: &crate::models::ExchangeCallbackOAuthRequest,
        ) -> Result<ExchangeOAuthResponse, OauthProviderAdminError> {
            Err(OauthProviderAdminError::BadGateway {
                message: "claudecode upstream failure".to_owned(),
            })
        }

        fn start_antigravity_oauth(
            &self,
            _request: &crate::models::StartAntigravityOAuthRequest,
        ) -> Result<StartPkceOAuthResponse, OauthProviderAdminError> {
            Ok(StartPkceOAuthResponse {
                session_id: "antigravity-session".to_owned(),
                auth_url: "https://example.test/antigravity/start".to_owned(),
            })
        }

        fn exchange_antigravity_oauth(
            &self,
            _request: &crate::models::ExchangeCallbackOAuthRequest,
        ) -> Result<ExchangeOAuthResponse, OauthProviderAdminError> {
            Ok(ExchangeOAuthResponse {
                credentials: "refresh-token|project-123".to_owned(),
            })
        }

        fn start_copilot_oauth(
            &self,
            _request: &crate::models::StartCopilotOAuthRequest,
        ) -> Result<StartCopilotOAuthResponse, OauthProviderAdminError> {
            Ok(StartCopilotOAuthResponse {
                session_id: "copilot-session".to_owned(),
                user_code: "ABCD-EFGH".to_owned(),
                verification_uri: "https://github.com/login/device".to_owned(),
                expires_in: 900,
                interval: 5,
            })
        }

        fn poll_copilot_oauth(
            &self,
            _request: &crate::models::PollCopilotOAuthRequest,
        ) -> Result<PollCopilotOAuthResponse, OauthProviderAdminError> {
            Ok(PollCopilotOAuthResponse {
                access_token: Some("gho_token".to_owned()),
                token_type: Some("bearer".to_owned()),
                scope: Some("read:user".to_owned()),
                status: "complete".to_owned(),
                message: Some("Authorization complete".to_owned()),
            })
        }
    }

    fn test_state() -> HttpState {
        HttpState {
            service_name: "AxonHub".to_owned(),
            version: "v0.9.20".to_owned(),
            config_source: None,
            system_bootstrap: SystemBootstrapCapability::Unsupported {
                message: "unused".to_owned(),
            },
            identity: IdentityCapability::Unsupported {
                message: "unused".to_owned(),
            },
            request_context: RequestContextCapability::Unsupported {
                message: "unused".to_owned(),
            },
            openai_v1: OpenAiV1Capability::Unsupported {
                message: "unused".to_owned(),
            },
            admin: AdminCapability::Unsupported {
                message: "unused".to_owned(),
            },
            admin_graphql: AdminGraphqlCapability::Unsupported {
                message: "unused".to_owned(),
            },
            openapi_graphql: OpenApiGraphqlCapability::Unsupported {
                message: "unused".to_owned(),
            },
            oauth_provider_admin: OauthProviderAdminCapability::Available {
                oauth_provider_admin: Arc::new(RecordingOauthProviderAdminPort),
            },
            allow_no_auth: false,
            cors: crate::state::HttpCorsSettings::default(),
            trace_config: TraceConfig {
                thread_header: Some("AH-Thread-Id".to_owned()),
                trace_header: Some("AH-Trace-Id".to_owned()),
                request_header: Some("X-Request-Id".to_owned()),
                extra_trace_headers: Vec::new(),
                extra_trace_body_fields: Vec::new(),
                claude_code_trace_enabled: false,
                codex_trace_enabled: false,
            },
        }
    }

    fn authorized_request(uri: &str, body: serde_json::Value) -> (web::Data<HttpState>, HttpRequest, Bytes) {
        let state = web::Data::new(test_state());
        let request = TestRequest::post().uri(uri).to_http_request();
        request.extensions_mut().insert(RequestContextState::default().with_auth(
            RequestAuthContext::Admin(AuthUserContext {
                id: 1,
                email: "owner@example.com".to_owned(),
                first_name: "System".to_owned(),
                last_name: "Owner".to_owned(),
                is_owner: true,
                prefer_language: "en".to_owned(),
                avatar: Some(String::new()),
                scopes: Vec::new(),
                roles: Vec::new(),
                projects: Vec::new(),
            }),
        ));
        (state, request, Bytes::from(body.to_string()))
    }

    #[actix_web::test]
    async fn start_routes_cover_codex_claudecode_antigravity_and_copilot() {
        let (state, request, body) = authorized_request("/admin/codex/oauth/start", json!({}));
        let response = start_codex_oauth(state, request, body).await;
        assert_eq!(response.status(), StatusCode::OK);

        let (state, request, body) =
            authorized_request("/admin/claudecode/oauth/start", json!({}));
        let response = start_claudecode_oauth(state, request, body).await;
        assert_eq!(response.status(), StatusCode::OK);

        let (state, request, body) = authorized_request(
            "/admin/antigravity/oauth/start",
            json!({"project_id":"project-123"}),
        );
        let response = start_antigravity_oauth(state, request, body).await;
        assert_eq!(response.status(), StatusCode::OK);

        let (state, request, body) = authorized_request("/admin/copilot/oauth/start", json!({}));
        let response = start_copilot_oauth(state, request, body).await;
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[actix_web::test]
    async fn exchange_and_poll_routes_cover_provider_specific_success_and_failure_shapes() {
        let (state, request, body) = authorized_request(
            "/admin/codex/oauth/exchange",
            json!({
                "session_id":"codex-session",
                "callback_url":"http://localhost/callback?code=test&state=codex-session"
            }),
        );
        let response = exchange_codex_oauth(state, request, body).await;
        assert_eq!(response.status(), StatusCode::OK);

        let (state, request, body) = authorized_request(
            "/admin/claudecode/oauth/exchange",
            json!({
                "session_id":"claudecode-session",
                "callback_url":"http://localhost/callback?code=test#claudecode-session"
            }),
        );
        let response = exchange_claudecode_oauth(state, request, body).await;
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);

        let (state, request, body) = authorized_request(
            "/admin/antigravity/oauth/exchange",
            json!({
                "session_id":"antigravity-session",
                "callback_url":"http://localhost/callback?code=test&state=antigravity-session"
            }),
        );
        let response = exchange_antigravity_oauth(state, request, body).await;
        assert_eq!(response.status(), StatusCode::OK);

        let (state, request, body) = authorized_request(
            "/admin/copilot/oauth/poll",
            json!({"session_id":"copilot-session"}),
        );
        let response = poll_copilot_oauth(state, request, body).await;
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[actix_web::test]
    async fn provider_edge_routes_reject_invalid_json_with_structured_bad_request() {
        let state = web::Data::new(test_state());
        let request = TestRequest::post().uri("/admin/copilot/oauth/poll").to_http_request();
        request.extensions_mut().insert(RequestContextState::default().with_auth(
            RequestAuthContext::Admin(AuthUserContext {
                id: 1,
                email: "owner@example.com".to_owned(),
                first_name: "System".to_owned(),
                last_name: "Owner".to_owned(),
                is_owner: true,
                prefer_language: "en".to_owned(),
                avatar: Some(String::new()),
                scopes: Vec::new(),
                roles: Vec::new(),
                projects: Vec::new(),
            }),
        ));

        let response = poll_copilot_oauth(state, request, Bytes::from_static(b"{" )).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}
