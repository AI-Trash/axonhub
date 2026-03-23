use crate::handlers;
use crate::middleware::{
    apply_request_context, require_admin_jwt, require_api_key_or_no_auth, require_gemini_key,
    require_service_api_key,
};
use crate::state::HttpState;
use axum::middleware::from_fn_with_state;
use axum::routing::{any, delete, get, post};
use axum::Router;

pub(crate) fn admin_public_routes() -> Router<HttpState> {
    Router::new()
        .route("/system/status", get(handlers::admin::system_status))
        .route(
            "/system/initialize",
            post(handlers::admin::initialize_system),
        )
        .route("/auth/signin", post(handlers::admin::sign_in))
}

pub(crate) fn admin_protected_routes(state: &HttpState) -> Router<HttpState> {
    Router::new()
        .route("/debug/context", get(handlers::debug_context))
        .route(
            "/playground",
            get(handlers::graphql::admin_graphql_playground),
        )
        .route("/graphql", post(handlers::graphql::admin_graphql))
        .route(
            "/codex/oauth/start",
            post(handlers::provider_edge::start_codex_oauth),
        )
        .route(
            "/codex/oauth/exchange",
            post(handlers::provider_edge::exchange_codex_oauth),
        )
        .route(
            "/claudecode/oauth/start",
            post(handlers::provider_edge::start_claudecode_oauth),
        )
        .route(
            "/claudecode/oauth/exchange",
            post(handlers::provider_edge::exchange_claudecode_oauth),
        )
        .route(
            "/antigravity/oauth/start",
            post(handlers::provider_edge::start_antigravity_oauth),
        )
        .route(
            "/antigravity/oauth/exchange",
            post(handlers::provider_edge::exchange_antigravity_oauth),
        )
        .route(
            "/copilot/oauth/start",
            post(handlers::provider_edge::start_copilot_oauth),
        )
        .route(
            "/copilot/oauth/poll",
            post(handlers::provider_edge::poll_copilot_oauth),
        )
        .route(
            "/requests/:request_id/content",
            get(handlers::admin::download_request_content),
        )
        .route("/", any(handlers::unported::unported_admin))
        .fallback(handlers::unported::unported_admin)
        .layer(from_fn_with_state(state.clone(), apply_request_context))
        .layer(from_fn_with_state(state.clone(), require_admin_jwt))
}

pub(crate) fn openai_v1_routes(state: &HttpState) -> Router<HttpState> {
    Router::new()
        .route("/debug/context", any(handlers::debug_context))
        .route("/models", get(handlers::openai_v1::list_openai_models))
        .route(
            "/chat/completions",
            post(handlers::openai_v1::openai_chat_completions),
        )
        .route("/responses", post(handlers::openai_v1::openai_responses))
        .route("/embeddings", post(handlers::openai_v1::openai_embeddings))
        .route("/", any(handlers::unported::unported_v1))
        .fallback(handlers::unported::unported_v1)
        .layer(from_fn_with_state(state.clone(), apply_request_context))
        .layer(from_fn_with_state(
            state.clone(),
            require_api_key_or_no_auth,
        ))
}

pub(crate) fn jina_routes(state: &HttpState) -> Router<HttpState> {
    Router::new()
        .route("/debug/context", any(handlers::debug_context))
        .route("/embeddings", post(handlers::jina::jina_embeddings))
        .route("/rerank", post(handlers::jina::jina_rerank))
        .route("/", any(handlers::unported::unported_jina_v1))
        .fallback(handlers::unported::unported_jina_v1)
        .layer(from_fn_with_state(state.clone(), apply_request_context))
        .layer(from_fn_with_state(
            state.clone(),
            require_api_key_or_no_auth,
        ))
}

pub(crate) fn anthropic_routes(state: &HttpState) -> Router<HttpState> {
    Router::new()
        .route("/debug/context", any(handlers::debug_context))
        .route("/messages", post(handlers::anthropic::anthropic_messages))
        .route("/models", get(handlers::anthropic::list_anthropic_models))
        .route("/", any(handlers::unported::unported_anthropic_v1))
        .fallback(handlers::unported::unported_anthropic_v1)
        .layer(from_fn_with_state(state.clone(), apply_request_context))
        .layer(from_fn_with_state(
            state.clone(),
            require_api_key_or_no_auth,
        ))
}

pub(crate) fn v1beta_routes(state: &HttpState) -> Router<HttpState> {
    Router::new()
        .route("/debug/context", any(handlers::debug_context))
        .route("/models", get(handlers::gemini::list_gemini_models))
        .route(
            "/models/*action",
            post(handlers::gemini::gemini_generate_content),
        )
        .route("/", any(handlers::unported::unported_v1beta))
        .fallback(handlers::unported::unported_v1beta)
        .layer(from_fn_with_state(state.clone(), apply_request_context))
        .layer(from_fn_with_state(state.clone(), require_gemini_key))
}

pub(crate) fn openapi_routes(state: &HttpState) -> Router<HttpState> {
    Router::new()
        .route("/debug/context", any(handlers::debug_context))
        .route(
            "/v1/playground",
            get(handlers::graphql::openapi_graphql_playground),
        )
        .route("/v1/graphql", post(handlers::graphql::openapi_graphql))
        .route("/", any(handlers::unported::unported_openapi))
        .fallback(handlers::unported::unported_openapi)
        .layer(from_fn_with_state(state.clone(), apply_request_context))
        .layer(from_fn_with_state(state.clone(), require_service_api_key))
}

pub(crate) fn doubao_routes(state: &HttpState) -> Router<HttpState> {
    Router::new()
        .route(
            "/doubao/v3/debug/context",
            any(handlers::debug_context)
                .layer(from_fn_with_state(state.clone(), apply_request_context))
                .layer(from_fn_with_state(
                    state.clone(),
                    require_api_key_or_no_auth,
                )),
        )
        .route(
            "/doubao/v3/contents/generations/tasks",
            post(handlers::doubao::doubao_create_task)
                .layer(from_fn_with_state(state.clone(), apply_request_context))
                .layer(from_fn_with_state(
                    state.clone(),
                    require_api_key_or_no_auth,
                )),
        )
        .route(
            "/doubao/v3/contents/generations/tasks/:id",
            get(handlers::doubao::doubao_get_task)
                .layer(from_fn_with_state(state.clone(), apply_request_context))
                .layer(from_fn_with_state(
                    state.clone(),
                    require_api_key_or_no_auth,
                )),
        )
        .route(
            "/doubao/v3/contents/generations/tasks/:id",
            delete(handlers::doubao::doubao_delete_task)
                .layer(from_fn_with_state(state.clone(), apply_request_context))
                .layer(from_fn_with_state(
                    state.clone(),
                    require_api_key_or_no_auth,
                )),
        )
        .route(
            "/doubao/v3/",
            any(handlers::unported::unported_doubao_v3)
                .layer(from_fn_with_state(state.clone(), apply_request_context))
                .layer(from_fn_with_state(
                    state.clone(),
                    require_api_key_or_no_auth,
                )),
        )
        .route(
            "/doubao/v3/*rest",
            any(handlers::unported::unported_doubao_v3)
                .layer(from_fn_with_state(state.clone(), apply_request_context))
                .layer(from_fn_with_state(
                    state.clone(),
                    require_api_key_or_no_auth,
                )),
        )
}

pub(crate) fn gemini_routes(state: &HttpState) -> Router<HttpState> {
    Router::new()
        .route(
            "/gemini/v1/debug/context",
            any(handlers::debug_context)
                .layer(from_fn_with_state(state.clone(), apply_request_context))
                .layer(from_fn_with_state(state.clone(), require_gemini_key)),
        )
        .route(
            "/gemini/v1/models",
            get(handlers::gemini::list_gemini_models)
                .layer(from_fn_with_state(state.clone(), apply_request_context))
                .layer(from_fn_with_state(state.clone(), require_gemini_key)),
        )
        .route(
            "/gemini/v1/models/*action",
            post(handlers::gemini::gemini_generate_content)
                .layer(from_fn_with_state(state.clone(), apply_request_context))
                .layer(from_fn_with_state(state.clone(), require_gemini_key)),
        )
        .route(
            "/gemini/{gemini_api_version}/debug/context",
            any(handlers::debug_context)
                .layer(from_fn_with_state(state.clone(), apply_request_context))
                .layer(from_fn_with_state(state.clone(), require_gemini_key)),
        )
        .route(
            "/gemini/{gemini_api_version}/models",
            get(handlers::gemini::list_gemini_models)
                .layer(from_fn_with_state(state.clone(), apply_request_context))
                .layer(from_fn_with_state(state.clone(), require_gemini_key)),
        )
        .route(
            "/gemini/{gemini_api_version}/models/*action",
            post(handlers::gemini::gemini_generate_content)
                .layer(from_fn_with_state(state.clone(), apply_request_context))
                .layer(from_fn_with_state(state.clone(), require_gemini_key)),
        )
        .route(
            "/gemini/{gemini_api_version}/",
            any(handlers::unported::unported_gemini)
                .layer(from_fn_with_state(state.clone(), apply_request_context))
                .layer(from_fn_with_state(state.clone(), require_gemini_key)),
        )
        .route(
            "/gemini/{gemini_api_version}/*rest",
            any(handlers::unported::unported_gemini)
                .layer(from_fn_with_state(state.clone(), apply_request_context))
                .layer(from_fn_with_state(state.clone(), require_gemini_key)),
        )
        .route(
            "/gemini/v1/",
            any(handlers::unported::unported_gemini)
                .layer(from_fn_with_state(state.clone(), apply_request_context))
                .layer(from_fn_with_state(state.clone(), require_gemini_key)),
        )
        .route(
            "/gemini/v1/*rest",
            any(handlers::unported::unported_gemini)
                .layer(from_fn_with_state(state.clone(), apply_request_context))
                .layer(from_fn_with_state(state.clone(), require_gemini_key)),
        )
}

pub fn router(state: HttpState) -> Router {
    Router::new()
        .route("/health", get(handlers::health))
        .nest(
            "/admin",
            admin_public_routes().merge(admin_protected_routes(&state)),
        )
        .nest("/v1", openai_v1_routes(&state))
        .nest("/jina/v1", jina_routes(&state))
        .nest("/anthropic/v1", anthropic_routes(&state))
        .merge(doubao_routes(&state))
        .merge(gemini_routes(&state))
        .nest("/v1beta", v1beta_routes(&state))
        .nest("/openapi", openapi_routes(&state))
        .with_state(state)
}
