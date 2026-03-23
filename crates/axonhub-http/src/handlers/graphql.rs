use crate::errors::{error_response, not_implemented_response};
use crate::handlers::{execute_graphql_request, graphql_playground_html};
use crate::state::{
    AdminGraphqlCapability, HttpState, OpenApiGraphqlCapability, RequestAuthContext,
    RequestContextState,
};
use axum::extract::{OriginalUri, Request, State};
use axum::http::{Method, StatusCode};
use axum::response::{IntoResponse, Response};

pub(crate) async fn admin_graphql_playground() -> impl IntoResponse {
    (
        StatusCode::OK,
        [("content-type", "text/html; charset=utf-8")],
        graphql_playground_html("/admin/graphql"),
    )
}

pub(crate) async fn openapi_graphql_playground() -> impl IntoResponse {
    (
        StatusCode::OK,
        [("content-type", "text/html; charset=utf-8")],
        graphql_playground_html("/openapi/v1/graphql"),
    )
}

pub(crate) async fn admin_graphql(
    State(state): State<HttpState>,
    OriginalUri(original_uri): OriginalUri,
    request: Request,
) -> Response {
    let graphql = match &state.admin_graphql {
        AdminGraphqlCapability::Unsupported { message } => {
            return not_implemented_response("/admin/graphql", Method::POST, original_uri, None)
                .with_message(message)
        }
        AdminGraphqlCapability::Available { graphql } => graphql,
    };

    let project_id = request
        .extensions()
        .get::<RequestContextState>()
        .and_then(|context| context.project.as_ref())
        .map(|project| project.id);

    let user = request
        .extensions()
        .get::<RequestContextState>()
        .and_then(|context| context.auth.as_ref())
        .and_then(|auth| match auth {
            RequestAuthContext::Admin(user) => Some(user.clone()),
            RequestAuthContext::ApiKey(_) => None,
        });
    let user = match user {
        Some(user) => user,
        None => return error_response(StatusCode::UNAUTHORIZED, "Unauthorized", "Invalid token"),
    };

    execute_graphql_request(request, |payload| graphql.execute_graphql(payload, project_id, user)).await
}

pub(crate) async fn openapi_graphql(
    State(state): State<HttpState>,
    OriginalUri(original_uri): OriginalUri,
    request: Request,
) -> Response {
    let graphql = match &state.openapi_graphql {
        OpenApiGraphqlCapability::Unsupported { message } => {
            return not_implemented_response("/openapi/v1/graphql", Method::POST, original_uri, None)
                .with_message(message)
        }
        OpenApiGraphqlCapability::Available { graphql } => graphql,
    };

    let owner_api_key = request
        .extensions()
        .get::<RequestContextState>()
        .and_then(|context| context.auth.as_ref())
        .and_then(|auth| match auth {
            RequestAuthContext::ApiKey(key) => Some(key.clone()),
            RequestAuthContext::Admin(_) => None,
        });
    let owner_api_key = match owner_api_key {
        Some(owner_api_key) => owner_api_key,
        None => {
            return error_response(
                StatusCode::UNAUTHORIZED,
                "Unauthorized",
                "Invalid API key",
            )
        }
    };

    execute_graphql_request(request, |payload| graphql.execute_graphql(payload, owner_api_key)).await
}
