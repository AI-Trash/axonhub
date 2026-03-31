use crate::handlers;
use crate::middleware::{
    admin_auth, api_key_auth, gemini_auth, http_metrics, request_context, service_api_key_auth,
};
use crate::state::{HttpMetricsCapability, HttpState};
use actix_web::dev::{ServiceFactory, ServiceRequest, ServiceResponse};
use actix_web::http::Method;
use actix_web::web::{self, ServiceConfig};
use actix_web::{App, HttpRequest};

async fn explicit_v1_not_implemented_boundary(req: HttpRequest) -> actix_web::HttpResponse {
    crate::errors::not_implemented_response(
        "/v1/*",
        Method::from(req.method().clone()),
        req.uri().clone(),
        None,
    )
    .into_response()
}

fn configure_admin_public(cfg: &mut ServiceConfig) {
    cfg.service(
        web::resource("/system/status").route(web::get().to(handlers::admin::system_status)),
    )
    .service(
        web::resource("/system/initialize").route(web::post().to(handlers::admin::initialize_system)),
    )
    .service(web::resource("/auth/signin").route(web::post().to(handlers::admin::sign_in)));
}

fn configure_admin_protected(cfg: &mut ServiceConfig) {
    cfg.service(web::resource("/debug/context").route(web::get().to(handlers::debug_context)))
        .service(
            web::resource("/playground")
                .route(web::get().to(handlers::graphql::admin_graphql_playground)),
        )
        .service(
            web::resource("/playground/chat").route(web::post().to(handlers::admin::playground_chat)),
        )
        .service(web::resource("/graphql").route(web::post().to(handlers::graphql::admin_graphql)))
        .service(
            web::resource("/codex/oauth/start")
                .route(web::post().to(handlers::provider_edge::start_codex_oauth)),
        )
        .service(
            web::resource("/codex/oauth/exchange")
                .route(web::post().to(handlers::provider_edge::exchange_codex_oauth)),
        )
        .service(
            web::resource("/claudecode/oauth/start")
                .route(web::post().to(handlers::provider_edge::start_claudecode_oauth)),
        )
        .service(
            web::resource("/claudecode/oauth/exchange")
                .route(web::post().to(handlers::provider_edge::exchange_claudecode_oauth)),
        )
        .service(
            web::resource("/antigravity/oauth/start")
                .route(web::post().to(handlers::provider_edge::start_antigravity_oauth)),
        )
        .service(
            web::resource("/antigravity/oauth/exchange")
                .route(web::post().to(handlers::provider_edge::exchange_antigravity_oauth)),
        )
        .service(
            web::resource("/copilot/oauth/start")
                .route(web::post().to(handlers::provider_edge::start_copilot_oauth)),
        )
        .service(
            web::resource("/copilot/oauth/poll")
                .route(web::post().to(handlers::provider_edge::poll_copilot_oauth)),
        )
        .service(
            web::resource("/requests/{request_id}/content")
                .route(web::get().to(handlers::admin::download_request_content)),
        )
        .default_service(web::route().to(handlers::not_found));
}

fn configure_openai_v1(cfg: &mut ServiceConfig) {
    cfg.service(web::resource("/debug/context").route(web::to(handlers::debug_context)))
        .service(web::resource("/models").route(web::get().to(handlers::openai_v1::list_openai_models)))
        .service(
            web::resource("/chat/completions")
                .route(web::post().to(handlers::openai_v1::openai_chat_completions)),
        )
        .service(
            web::resource("/responses").route(web::post().to(handlers::openai_v1::openai_responses)),
        )
        .service(
            web::resource("/embeddings").route(web::post().to(handlers::openai_v1::openai_embeddings)),
        )
        .service(
            web::resource("/images/generations")
                .route(web::post().to(handlers::openai_v1::openai_images_generations)),
        )
        .service(web::resource("/images/edits").route(web::to(explicit_v1_not_implemented_boundary)))
        .service(web::resource("/images/variations").route(web::to(explicit_v1_not_implemented_boundary)))
        .service(
            web::resource("/videos").route(web::post().to(handlers::openai_v1::openai_videos_create)),
        )
        .service(
            web::resource("/videos/{id}")
                .route(web::get().to(handlers::openai_v1::openai_videos_get))
                .route(web::delete().to(handlers::openai_v1::openai_videos_delete)),
        )
        .service(web::resource("/rerank").route(web::post().to(handlers::jina::jina_rerank)))
        .service(
            web::resource("/messages").route(web::post().to(handlers::anthropic::anthropic_messages)),
        )
        .service(web::resource("/realtime").route(web::to(explicit_v1_not_implemented_boundary)))
        .service(web::resource("/").route(web::to(handlers::not_found)))
        .default_service(web::route().to(handlers::not_found));
}

fn configure_jina(cfg: &mut ServiceConfig) {
    cfg.service(web::resource("/debug/context").route(web::to(handlers::debug_context)))
        .service(web::resource("/embeddings").route(web::post().to(handlers::jina::jina_embeddings)))
        .service(web::resource("/rerank").route(web::post().to(handlers::jina::jina_rerank)))
        .default_service(web::route().to(handlers::not_found));
}

fn configure_anthropic(cfg: &mut ServiceConfig) {
    cfg.service(web::resource("/debug/context").route(web::to(handlers::debug_context)))
        .service(
            web::resource("/messages").route(web::post().to(handlers::anthropic::anthropic_messages)),
        )
        .service(
            web::resource("/models").route(web::get().to(handlers::anthropic::list_anthropic_models)),
        )
        .default_service(web::route().to(handlers::not_found));
}

fn configure_v1beta(cfg: &mut ServiceConfig) {
    cfg.service(web::resource("/models").route(web::get().to(handlers::gemini::list_gemini_models)))
        .service(
            web::resource("/models/{action:.*}")
                .route(web::post().to(handlers::gemini::gemini_generate_content)),
        )
        .default_service(web::route().to(handlers::not_found));
}

fn configure_openapi(cfg: &mut ServiceConfig) {
    cfg.service(web::resource("/debug/context").route(web::to(handlers::debug_context)))
        .service(
            web::resource("/v1/playground")
                .route(web::get().to(handlers::graphql::openapi_graphql_playground)),
        )
        .service(
            web::resource("/v1/graphql").route(web::post().to(handlers::graphql::openapi_graphql)),
        )
        .default_service(web::route().to(handlers::not_found));
}

fn configure_doubao(cfg: &mut ServiceConfig) {
    cfg.service(
        web::scope("/doubao/v3")
            .service(
                web::resource("/debug/context")
                    .wrap(request_context())
                    .wrap(api_key_auth())
                    .route(web::to(handlers::debug_context)),
            )
            .service(
                web::resource("/contents/generations/tasks")
                    .wrap(request_context())
                    .wrap(api_key_auth())
                    .route(web::post().to(handlers::doubao::doubao_create_task)),
            )
            .service(
                web::resource("/contents/generations/tasks/{id}")
                    .wrap(request_context())
                    .wrap(api_key_auth())
                    .route(web::get().to(handlers::doubao::doubao_get_task))
                    .route(web::delete().to(handlers::doubao::doubao_delete_task)),
            )
            .default_service(web::route().wrap(request_context()).wrap(api_key_auth()).to(
                |req: HttpRequest| async move {
                    crate::errors::not_implemented_response(
                        "/*",
                        Method::from(req.method().clone()),
                        req.uri().clone(),
                        None,
                    )
                    .into_response()
                },
            )),
    );
}

fn configure_gemini(cfg: &mut ServiceConfig) {
    cfg.service(
        web::scope("/gemini/{gemini_api_version}")
            .service(
                web::resource("/debug/context")
                    .wrap(request_context())
                    .wrap(gemini_auth())
                    .route(web::to(handlers::debug_context)),
            )
            .service(
                web::resource("/models")
                    .wrap(request_context())
                    .wrap(gemini_auth())
                    .route(web::get().to(handlers::gemini::list_gemini_models)),
            )
            .service(
                web::resource("/models/{action:.*}")
                    .wrap(request_context())
                    .wrap(gemini_auth())
                    .route(web::post().to(handlers::gemini::gemini_generate_content)),
            )
            .default_service(
                web::route()
                    .wrap(request_context())
                    .wrap(gemini_auth())
                    .to(handlers::not_found),
            ),
    );
}

fn configure_http_routes(cfg: &mut ServiceConfig) {
    cfg.service(web::resource("/favicon").route(web::get().to(handlers::static_files::favicon)))
        .service(web::resource("/health").route(web::get().to(handlers::health)))
        .service(
            web::scope("/admin")
                .configure(configure_admin_public)
                .service(
                    web::scope("")
                        .wrap(request_context())
                        .wrap(admin_auth())
                        .configure(configure_admin_protected),
                ),
        )
        .service(
            web::scope("/v1")
                .wrap(request_context())
                .wrap(api_key_auth())
                .configure(configure_openai_v1),
        )
        .service(
            web::scope("/jina/v1")
                .wrap(request_context())
                .wrap(api_key_auth())
                .configure(configure_jina),
        )
        .service(
            web::scope("/anthropic/v1")
                .wrap(request_context())
                .wrap(api_key_auth())
                .configure(configure_anthropic),
        )
        .configure(configure_doubao)
        .configure(configure_gemini)
        .service(
            web::scope("/v1beta")
                .wrap(request_context())
                .wrap(gemini_auth())
                .configure(configure_v1beta),
        )
        .service(
            web::scope("/openapi")
                .wrap(request_context())
                .wrap(service_api_key_auth())
                .configure(configure_openapi),
        )
        .default_service(web::route().to(handlers::static_files::serve_embedded_or_not_implemented));
}

pub fn router(
    state: HttpState,
) -> App<
    impl ServiceFactory<
        ServiceRequest,
        Config = (),
        Response = ServiceResponse,
        Error = actix_web::Error,
        InitError = (),
    >,
> {
    router_with_metrics(state, HttpMetricsCapability::Disabled)
}

pub fn router_with_metrics(
    state: HttpState,
    http_metrics_capability: HttpMetricsCapability,
) -> App<
    impl ServiceFactory<
        ServiceRequest,
        Config = (),
        Response = ServiceResponse,
        Error = actix_web::Error,
        InitError = (),
    >,
> {
    router_with_metrics_and_base_path(state, http_metrics_capability, "/")
}

pub fn router_with_metrics_and_base_path(
    state: HttpState,
    http_metrics_capability: HttpMetricsCapability,
    base_path: &str,
) -> App<
    impl ServiceFactory<
        ServiceRequest,
        Config = (),
        Response = ServiceResponse,
        Error = actix_web::Error,
        InitError = (),
    >,
> {
    let normalized = base_path.trim();
    let prefixed = format!("/{}", normalized.trim_matches('/'));
    let static_files_config = web::Data::new(handlers::static_files::StaticFilesConfig {
        base_path: if normalized.is_empty() {
            "/".to_owned()
        } else {
            prefixed.clone()
        },
    });

    App::new()
        .app_data(web::Data::new(state))
        .app_data(static_files_config)
        .wrap(http_metrics(http_metrics_capability))
        .configure(move |cfg| {
            if normalized.is_empty() || normalized == "/" {
                configure_http_routes(cfg);
            } else {
                cfg.service(web::scope(&prefixed).configure(configure_http_routes));
            }
        })
}
