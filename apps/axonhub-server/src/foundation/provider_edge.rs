use axonhub_http::{
    ExchangeCallbackOAuthRequest, ExchangeOAuthResponse, OAuthProxyConfig, OAuthProxyType,
    PollCopilotOAuthRequest, PollCopilotOAuthResponse, ProviderEdgeAdminError,
    ProviderEdgeAdminPort, StartAntigravityOAuthRequest, StartCopilotOAuthRequest,
    StartCopilotOAuthResponse, StartPkceOAuthRequest, StartPkceOAuthResponse,
};
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, CONTENT_TYPE, USER_AGENT};
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::env;
use std::sync::Arc;
use std::sync::{Mutex, OnceLock};

use super::shared::{
    current_unix_timestamp, format_unix_timestamp, PROVIDER_EDGE_COPILOT_COMPLETE_MESSAGE,
    PROVIDER_EDGE_COPILOT_DEVICE_GRANT_TYPE, PROVIDER_EDGE_COPILOT_PENDING_MESSAGE,
    PROVIDER_EDGE_COPILOT_SLOW_DOWN_MESSAGE, PROVIDER_EDGE_PKCE_SESSION_TTL_SECONDS,
};
use getrandom::fill as getrandom;
use hex::encode as hex_encode;

pub struct SqliteProviderEdgeAdminService {
    config: ProviderEdgeAdminConfig,
    sessions: Arc<Mutex<HashMap<String, ProviderEdgeSession>>>,
}

pub(crate) const PROVIDER_EDGE_REQUIRED_ENV_VARS: &[&str] = &[
    "AXONHUB_PROVIDER_EDGE_CODEX_AUTHORIZE_URL",
    "AXONHUB_PROVIDER_EDGE_CODEX_TOKEN_URL",
    "AXONHUB_PROVIDER_EDGE_CODEX_CLIENT_ID",
    "AXONHUB_PROVIDER_EDGE_CODEX_REDIRECT_URI",
    "AXONHUB_PROVIDER_EDGE_CODEX_SCOPES",
    "AXONHUB_PROVIDER_EDGE_CODEX_USER_AGENT",
    "AXONHUB_PROVIDER_EDGE_CLAUDECODE_AUTHORIZE_URL",
    "AXONHUB_PROVIDER_EDGE_CLAUDECODE_TOKEN_URL",
    "AXONHUB_PROVIDER_EDGE_CLAUDECODE_CLIENT_ID",
    "AXONHUB_PROVIDER_EDGE_CLAUDECODE_REDIRECT_URI",
    "AXONHUB_PROVIDER_EDGE_CLAUDECODE_SCOPES",
    "AXONHUB_PROVIDER_EDGE_CLAUDECODE_USER_AGENT",
    "AXONHUB_PROVIDER_EDGE_ANTIGRAVITY_AUTHORIZE_URL",
    "AXONHUB_PROVIDER_EDGE_ANTIGRAVITY_TOKEN_URL",
    "AXONHUB_PROVIDER_EDGE_ANTIGRAVITY_CLIENT_ID",
    "AXONHUB_PROVIDER_EDGE_ANTIGRAVITY_CLIENT_SECRET",
    "AXONHUB_PROVIDER_EDGE_ANTIGRAVITY_REDIRECT_URI",
    "AXONHUB_PROVIDER_EDGE_ANTIGRAVITY_SCOPES",
    "AXONHUB_PROVIDER_EDGE_ANTIGRAVITY_LOAD_ENDPOINTS",
    "AXONHUB_PROVIDER_EDGE_ANTIGRAVITY_USER_AGENT",
    "AXONHUB_PROVIDER_EDGE_ANTIGRAVITY_CLIENT_METADATA",
    "AXONHUB_PROVIDER_EDGE_COPILOT_DEVICE_CODE_URL",
    "AXONHUB_PROVIDER_EDGE_COPILOT_ACCESS_TOKEN_URL",
    "AXONHUB_PROVIDER_EDGE_COPILOT_CLIENT_ID",
    "AXONHUB_PROVIDER_EDGE_COPILOT_SCOPE",
];

#[derive(Debug, Clone)]
pub struct ProviderEdgeAdminConfig {
    codex_authorize_url: String,
    codex_token_url: String,
    codex_client_id: String,
    codex_redirect_uri: String,
    codex_scopes: String,
    codex_user_agent: String,
    claudecode_authorize_url: String,
    claudecode_token_url: String,
    claudecode_client_id: String,
    claudecode_redirect_uri: String,
    claudecode_scopes: String,
    claudecode_user_agent: String,
    antigravity_authorize_url: String,
    antigravity_token_url: String,
    antigravity_client_id: String,
    antigravity_client_secret: String,
    antigravity_redirect_uri: String,
    antigravity_scopes: String,
    antigravity_load_endpoints: Vec<String>,
    antigravity_user_agent: String,
    antigravity_client_metadata: String,
    copilot_device_code_url: String,
    copilot_access_token_url: String,
    copilot_client_id: String,
    copilot_scope: String,
}

#[derive(Debug, Clone)]
pub(crate) enum ProviderEdgeSession {
    Pkce {
        provider: PkceProvider,
        code_verifier: String,
        project_id: Option<String>,
        created_at: i64,
    },
    CopilotDevice {
        device_code: String,
        expires_in: i64,
        created_at: i64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PkceProvider {
    Codex,
    ClaudeCode,
    Antigravity,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct OAuthTokenResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    id_token: Option<String>,
    expires_in: Option<i64>,
    token_type: Option<String>,
    scope: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct CopilotDeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    expires_in: i64,
    interval: i64,
}

impl ProviderEdgeAdminConfig {
    fn from_env() -> Option<Self> {
        Some(Self {
            codex_authorize_url: required_env("AXONHUB_PROVIDER_EDGE_CODEX_AUTHORIZE_URL")?,
            codex_token_url: required_env("AXONHUB_PROVIDER_EDGE_CODEX_TOKEN_URL")?,
            codex_client_id: required_env("AXONHUB_PROVIDER_EDGE_CODEX_CLIENT_ID")?,
            codex_redirect_uri: required_env("AXONHUB_PROVIDER_EDGE_CODEX_REDIRECT_URI")?,
            codex_scopes: required_env("AXONHUB_PROVIDER_EDGE_CODEX_SCOPES")?,
            codex_user_agent: required_env("AXONHUB_PROVIDER_EDGE_CODEX_USER_AGENT")?,
            claudecode_authorize_url: required_env(
                "AXONHUB_PROVIDER_EDGE_CLAUDECODE_AUTHORIZE_URL",
            )?,
            claudecode_token_url: required_env("AXONHUB_PROVIDER_EDGE_CLAUDECODE_TOKEN_URL")?,
            claudecode_client_id: required_env("AXONHUB_PROVIDER_EDGE_CLAUDECODE_CLIENT_ID")?,
            claudecode_redirect_uri: required_env("AXONHUB_PROVIDER_EDGE_CLAUDECODE_REDIRECT_URI")?,
            claudecode_scopes: required_env("AXONHUB_PROVIDER_EDGE_CLAUDECODE_SCOPES")?,
            claudecode_user_agent: required_env("AXONHUB_PROVIDER_EDGE_CLAUDECODE_USER_AGENT")?,
            antigravity_authorize_url: required_env(
                "AXONHUB_PROVIDER_EDGE_ANTIGRAVITY_AUTHORIZE_URL",
            )?,
            antigravity_token_url: required_env("AXONHUB_PROVIDER_EDGE_ANTIGRAVITY_TOKEN_URL")?,
            antigravity_client_id: required_env("AXONHUB_PROVIDER_EDGE_ANTIGRAVITY_CLIENT_ID")?,
            antigravity_client_secret: required_env(
                "AXONHUB_PROVIDER_EDGE_ANTIGRAVITY_CLIENT_SECRET",
            )?,
            antigravity_redirect_uri: required_env(
                "AXONHUB_PROVIDER_EDGE_ANTIGRAVITY_REDIRECT_URI",
            )?,
            antigravity_scopes: required_env("AXONHUB_PROVIDER_EDGE_ANTIGRAVITY_SCOPES")?,
            antigravity_load_endpoints: required_env_list(
                "AXONHUB_PROVIDER_EDGE_ANTIGRAVITY_LOAD_ENDPOINTS",
            )?,
            antigravity_user_agent: required_env("AXONHUB_PROVIDER_EDGE_ANTIGRAVITY_USER_AGENT")?,
            antigravity_client_metadata: required_env(
                "AXONHUB_PROVIDER_EDGE_ANTIGRAVITY_CLIENT_METADATA",
            )?,
            copilot_device_code_url: required_env("AXONHUB_PROVIDER_EDGE_COPILOT_DEVICE_CODE_URL")?,
            copilot_access_token_url: required_env(
                "AXONHUB_PROVIDER_EDGE_COPILOT_ACCESS_TOKEN_URL",
            )?,
            copilot_client_id: required_env("AXONHUB_PROVIDER_EDGE_COPILOT_CLIENT_ID")?,
            copilot_scope: required_env("AXONHUB_PROVIDER_EDGE_COPILOT_SCOPE")?,
        })
    }
}

impl SqliteProviderEdgeAdminService {
    pub fn new(config: ProviderEdgeAdminConfig) -> Self {
        Self {
            config,
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn from_env() -> Option<Self> {
        ProviderEdgeAdminConfig::from_env().map(Self::new)
    }

    fn http_client(&self) -> &reqwest::blocking::Client {
        provider_edge_default_http_client()
    }

    fn codex_exchange_http_client(
        &self,
        proxy: Option<&OAuthProxyConfig>,
    ) -> Result<CodexExchangeHttpClient<'_>, ProviderEdgeAdminError> {
        match proxy {
            Some(proxy) => match build_codex_exchange_http_client(proxy)? {
                Some(client) => Ok(CodexExchangeHttpClient::Owned(client)),
                None => Ok(CodexExchangeHttpClient::Borrowed(self.http_client())),
            },
            None => Ok(CodexExchangeHttpClient::Borrowed(self.http_client())),
        }
    }

    fn run_copilot_http_task<T, Task>(&self, task: Task) -> Result<T, ProviderEdgeAdminError>
    where
        T: Send + 'static,
        Task:
            FnOnce(reqwest::blocking::Client) -> Result<T, ProviderEdgeAdminError> + Send + 'static,
    {
        std::thread::spawn(move || {
            let client = reqwest::blocking::Client::new();
            task(client)
        })
        .join()
        .map_err(|_| provider_edge_internal_error("copilot upstream task panicked"))?
    }

    fn start_pkce_flow(
        &self,
        provider: PkceProvider,
        project_id: Option<String>,
    ) -> Result<StartPkceOAuthResponse, ProviderEdgeAdminError> {
        let session_id = generate_provider_edge_session_id()?;
        let code_verifier = generate_provider_edge_code_verifier()?;
        let auth_url =
            self.provider_authorize_url(provider, session_id.as_str(), code_verifier.as_str());

        self.sessions
            .lock()
            .map_err(|_| provider_edge_internal_error("failed to lock provider-edge sessions"))?
            .insert(
                session_id.clone(),
                ProviderEdgeSession::Pkce {
                    provider,
                    code_verifier,
                    project_id,
                    created_at: current_unix_timestamp(),
                },
            );

        Ok(StartPkceOAuthResponse {
            session_id,
            auth_url,
        })
    }

    fn exchange_pkce_flow(
        &self,
        provider: PkceProvider,
        request: &ExchangeCallbackOAuthRequest,
    ) -> Result<ExchangeOAuthResponse, ProviderEdgeAdminError> {
        if request.session_id.trim().is_empty() || request.callback_url.trim().is_empty() {
            return Err(provider_edge_invalid_request(
                "session_id and callback_url are required",
            ));
        }

        let session = self.take_session(request.session_id.as_str())?;
        let (code_verifier, project_id) = match session {
            ProviderEdgeSession::Pkce {
                provider: stored_provider,
                code_verifier,
                project_id,
                created_at,
            } => {
                if stored_provider != provider {
                    return Err(provider_edge_invalid_request(
                        "invalid or expired oauth session",
                    ));
                }
                if current_unix_timestamp().saturating_sub(created_at)
                    > PROVIDER_EDGE_PKCE_SESSION_TTL_SECONDS
                {
                    return Err(provider_edge_invalid_request(
                        "invalid or expired oauth session",
                    ));
                }
                (code_verifier, project_id)
            }
            ProviderEdgeSession::CopilotDevice { .. } => {
                return Err(provider_edge_invalid_request(
                    "invalid or expired oauth session",
                ))
            }
        };

        let callback = parse_callback(provider, request.callback_url.as_str())?;
        if callback.state != request.session_id {
            return Err(provider_edge_invalid_request("oauth state mismatch"));
        }

        let codex_client = if provider == PkceProvider::Codex {
            Some(self.codex_exchange_http_client(request.proxy.as_ref())?)
        } else {
            None
        };

        let token = self.exchange_provider_token(
            provider,
            callback.code.as_str(),
            callback.state.as_str(),
            code_verifier.as_str(),
            codex_client
                .as_ref()
                .map(CodexExchangeHttpClient::as_client),
        )?;

        let credentials = match provider {
            PkceProvider::Antigravity => {
                let refresh_token = token.refresh_token.clone().unwrap_or_default();
                let project_id = match project_id {
                    Some(project_id) if !project_id.trim().is_empty() => project_id,
                    _ => self.resolve_antigravity_project_id(
                        token.access_token.as_deref().unwrap_or_default(),
                    )?,
                };
                format!("{refresh_token}|{project_id}")
            }
            _ => oauth_credentials_json(&token, self.provider_client_id(provider).to_owned()),
        };

        Ok(ExchangeOAuthResponse { credentials })
    }

    fn take_session(
        &self,
        session_id: &str,
    ) -> Result<ProviderEdgeSession, ProviderEdgeAdminError> {
        self.sessions
            .lock()
            .map_err(|_| provider_edge_internal_error("failed to lock provider-edge sessions"))?
            .remove(session_id)
            .ok_or_else(|| provider_edge_invalid_request("invalid or expired oauth session"))
    }

    fn load_session(
        &self,
        session_id: &str,
    ) -> Result<ProviderEdgeSession, ProviderEdgeAdminError> {
        self.sessions
            .lock()
            .map_err(|_| provider_edge_internal_error("failed to lock provider-edge sessions"))?
            .get(session_id)
            .cloned()
            .ok_or_else(|| provider_edge_invalid_request("invalid or expired session"))
    }

    fn delete_session(&self, session_id: &str) -> Result<(), ProviderEdgeAdminError> {
        self.sessions
            .lock()
            .map_err(|_| provider_edge_internal_error("failed to lock provider-edge sessions"))?
            .remove(session_id);
        Ok(())
    }

    fn provider_authorize_url(
        &self,
        provider: PkceProvider,
        state: &str,
        code_verifier: &str,
    ) -> String {
        let mut params = vec![
            ("response_type", "code".to_owned()),
            ("client_id", self.provider_client_id(provider).to_owned()),
            (
                "redirect_uri",
                self.provider_redirect_uri(provider).to_owned(),
            ),
            ("scope", self.provider_scopes(provider).to_owned()),
            (
                "code_challenge",
                provider_edge_code_challenge(code_verifier),
            ),
            ("code_challenge_method", "S256".to_owned()),
            ("state", state.to_owned()),
        ];

        match provider {
            PkceProvider::Codex => {
                params.push(("id_token_add_organizations", "true".to_owned()));
                params.push(("codex_cli_simplified_flow", "true".to_owned()));
            }
            PkceProvider::Antigravity => {
                params.push(("access_type", "offline".to_owned()));
                params.push(("prompt", "consent".to_owned()));
            }
            PkceProvider::ClaudeCode => {}
        }

        format!(
            "{}?{}",
            self.provider_authorize_endpoint(provider),
            form_urlencode(params),
        )
    }

    fn provider_authorize_endpoint(&self, provider: PkceProvider) -> &str {
        match provider {
            PkceProvider::Codex => self.config.codex_authorize_url.as_str(),
            PkceProvider::ClaudeCode => self.config.claudecode_authorize_url.as_str(),
            PkceProvider::Antigravity => self.config.antigravity_authorize_url.as_str(),
        }
    }

    fn provider_client_id(&self, provider: PkceProvider) -> &str {
        match provider {
            PkceProvider::Codex => self.config.codex_client_id.as_str(),
            PkceProvider::ClaudeCode => self.config.claudecode_client_id.as_str(),
            PkceProvider::Antigravity => self.config.antigravity_client_id.as_str(),
        }
    }

    fn provider_redirect_uri(&self, provider: PkceProvider) -> &str {
        match provider {
            PkceProvider::Codex => self.config.codex_redirect_uri.as_str(),
            PkceProvider::ClaudeCode => self.config.claudecode_redirect_uri.as_str(),
            PkceProvider::Antigravity => self.config.antigravity_redirect_uri.as_str(),
        }
    }

    fn provider_scopes(&self, provider: PkceProvider) -> &str {
        match provider {
            PkceProvider::Codex => self.config.codex_scopes.as_str(),
            PkceProvider::ClaudeCode => self.config.claudecode_scopes.as_str(),
            PkceProvider::Antigravity => self.config.antigravity_scopes.as_str(),
        }
    }

    fn provider_user_agent(&self, provider: PkceProvider) -> Option<&str> {
        match provider {
            PkceProvider::Codex => Some(self.config.codex_user_agent.as_str()),
            PkceProvider::ClaudeCode => Some(self.config.claudecode_user_agent.as_str()),
            PkceProvider::Antigravity => Some(self.config.antigravity_user_agent.as_str()),
        }
    }

    fn provider_token_endpoint(&self, provider: PkceProvider) -> &str {
        match provider {
            PkceProvider::Codex => self.config.codex_token_url.as_str(),
            PkceProvider::ClaudeCode => self.config.claudecode_token_url.as_str(),
            PkceProvider::Antigravity => self.config.antigravity_token_url.as_str(),
        }
    }

    fn exchange_provider_token(
        &self,
        provider: PkceProvider,
        code: &str,
        state: &str,
        code_verifier: &str,
        codex_client: Option<&reqwest::blocking::Client>,
    ) -> Result<OAuthTokenResponse, ProviderEdgeAdminError> {
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        if let Some(user_agent) = self.provider_user_agent(provider) {
            headers.insert(
                USER_AGENT,
                HeaderValue::from_str(user_agent).map_err(|error| {
                    provider_edge_internal_error(format!("invalid user agent header: {error}"))
                })?,
            );
        }

        let response = match provider {
            PkceProvider::ClaudeCode => {
                headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
                let mut body = serde_json::Map::new();
                body.insert(
                    "grant_type".to_owned(),
                    Value::String("authorization_code".to_owned()),
                );
                body.insert("code".to_owned(), Value::String(code.to_owned()));
                body.insert(
                    "client_id".to_owned(),
                    Value::String(self.provider_client_id(provider).to_owned()),
                );
                body.insert(
                    "redirect_uri".to_owned(),
                    Value::String(self.provider_redirect_uri(provider).to_owned()),
                );
                body.insert(
                    "code_verifier".to_owned(),
                    Value::String(code_verifier.to_owned()),
                );
                body.insert("state".to_owned(), Value::String(state.to_owned()));
                self.http_client()
                    .post(self.provider_token_endpoint(provider))
                    .headers(headers)
                    .json(&Value::Object(body))
                    .send()
                    .map_err(|error| {
                        provider_edge_bad_gateway(format!("token exchange failed: {error}"))
                    })?
            }
            PkceProvider::Codex | PkceProvider::Antigravity => {
                headers.insert(
                    CONTENT_TYPE,
                    HeaderValue::from_static("application/x-www-form-urlencoded"),
                );
                let mut params = vec![
                    ("grant_type", "authorization_code".to_owned()),
                    ("client_id", self.provider_client_id(provider).to_owned()),
                    ("code", code.to_owned()),
                    (
                        "redirect_uri",
                        self.provider_redirect_uri(provider).to_owned(),
                    ),
                    ("code_verifier", code_verifier.to_owned()),
                ];
                if provider == PkceProvider::Antigravity {
                    params.push((
                        "client_secret",
                        self.config.antigravity_client_secret.clone(),
                    ));
                }
                self.token_exchange_http_client(provider, codex_client)
                    .post(self.provider_token_endpoint(provider))
                    .headers(headers)
                    .body(form_urlencode(params))
                    .send()
                    .map_err(|error| {
                        provider_edge_bad_gateway(format!("token exchange failed: {error}"))
                    })?
            }
        };

        let status = response.status();
        let body = response.text().map_err(|error| {
            provider_edge_bad_gateway(format!("token exchange failed: {error}"))
        })?;
        if !status.is_success() {
            return Err(provider_edge_bad_gateway(format!(
                "token exchange failed: upstream status {}: {body}",
                status.as_u16()
            )));
        }

        let token: OAuthTokenResponse = serde_json::from_str(body.as_str()).map_err(|error| {
            provider_edge_bad_gateway(format!("token exchange failed: {error}"))
        })?;
        if let Some(error) = token.error.as_ref() {
            let description = token.error_description.clone().unwrap_or_default();
            return Err(provider_edge_bad_gateway(format!(
                "token exchange failed: {error} - {description}"
            )));
        }
        if token.access_token.as_deref().unwrap_or_default().is_empty() {
            return Err(provider_edge_bad_gateway(
                "token exchange failed: token response missing access_token",
            ));
        }
        Ok(token)
    }

    fn token_exchange_http_client<'a>(
        &'a self,
        provider: PkceProvider,
        codex_client: Option<&'a reqwest::blocking::Client>,
    ) -> &'a reqwest::blocking::Client {
        match provider {
            PkceProvider::Codex => codex_client.unwrap_or_else(|| self.http_client()),
            PkceProvider::ClaudeCode | PkceProvider::Antigravity => self.http_client(),
        }
    }

    fn resolve_antigravity_project_id(
        &self,
        access_token: &str,
    ) -> Result<String, ProviderEdgeAdminError> {
        if self.config.antigravity_load_endpoints.is_empty() {
            return Err(provider_edge_bad_gateway(
                "failed to resolve project id and none provided: no load endpoints configured",
            ));
        }

        let mut last_error = None;
        let mut default_tier_id = "FREE".to_owned();
        for endpoint in &self.config.antigravity_load_endpoints {
            let url = format!("{endpoint}/v1internal:loadCodeAssist");
            let response = self
                .http_client()
                .post(url)
                .header(AUTHORIZATION, format!("Bearer {access_token}"))
                .header(CONTENT_TYPE, "application/json")
                .header(USER_AGENT, self.config.antigravity_user_agent.as_str())
                .header(
                    "X-Goog-Api-Client",
                    "google-cloud-sdk vscode_cloudshelleditor/0.1",
                )
                .header(
                    "Client-Metadata",
                    self.config.antigravity_client_metadata.as_str(),
                )
                .json(&serde_json::json!({
                    "metadata": {
                        "ideType": "ANTIGRAVITY",
                        "platform": "PLATFORM_UNSPECIFIED",
                        "pluginType": "GEMINI"
                    }
                }))
                .send();

            let response = match response {
                Ok(response) => response,
                Err(error) => {
                    last_error = Some(error.to_string());
                    continue;
                }
            };
            if !response.status().is_success() {
                last_error = Some(format!("status {}", response.status().as_u16()));
                continue;
            }

            let body: Value = response.json().map_err(|error| {
                provider_edge_bad_gateway(format!(
                    "failed to resolve project id and none provided: {error}"
                ))
            })?;
            if let Some(project_id) = extract_antigravity_project_id(&body) {
                return Ok(project_id);
            }

            if let Some(tier_id) = extract_antigravity_default_tier(&body) {
                default_tier_id = tier_id;
            }
            match self.onboard_antigravity_user(
                endpoint.as_str(),
                access_token,
                default_tier_id.as_str(),
            ) {
                Ok(project_id) if !project_id.is_empty() => return Ok(project_id),
                Ok(_) => {}
                Err(error) => {
                    last_error = Some(error.clone());
                }
            }
        }

        Err(provider_edge_bad_gateway(format!(
            "failed to resolve project id and none provided: {}",
            last_error.unwrap_or_else(|| "unknown error".to_owned())
        )))
    }

    fn onboard_antigravity_user(
        &self,
        endpoint: &str,
        access_token: &str,
        tier_id: &str,
    ) -> Result<String, String> {
        let url = format!("{endpoint}/v1internal:onboardUser");
        for _ in 0..3 {
            let response = self
                .http_client()
                .post(url.as_str())
                .header(AUTHORIZATION, format!("Bearer {access_token}"))
                .header(CONTENT_TYPE, "application/json")
                .header(USER_AGENT, self.config.antigravity_user_agent.as_str())
                .header(
                    "X-Goog-Api-Client",
                    "google-cloud-sdk vscode_cloudshelleditor/0.1",
                )
                .header(
                    "Client-Metadata",
                    self.config.antigravity_client_metadata.as_str(),
                )
                .json(&serde_json::json!({
                    "tierId": tier_id,
                    "metadata": {
                        "ideType": "ANTIGRAVITY",
                        "platform": "PLATFORM_UNSPECIFIED",
                        "pluginType": "GEMINI"
                    }
                }))
                .send();

            let response = match response {
                Ok(response) => response,
                Err(error) => return Err(error.to_string()),
            };
            if !response.status().is_success() {
                continue;
            }
            let body: Value = response.json().map_err(|error| error.to_string())?;
            if body.get("done").and_then(Value::as_bool) == Some(true) {
                if let Some(project_id) = body
                    .get("response")
                    .and_then(|value| value.get("cloudaicompanionProject"))
                    .and_then(|value| value.get("id"))
                    .and_then(Value::as_str)
                {
                    return Ok(project_id.to_owned());
                }
            }
        }
        Err("failed to onboard user after retries".to_owned())
    }

    fn request_copilot_device_code(
        &self,
    ) -> Result<CopilotDeviceCodeResponse, ProviderEdgeAdminError> {
        let url = self.config.copilot_device_code_url.clone();
        let client_id = self.config.copilot_client_id.clone();
        let scope = self.config.copilot_scope.clone();

        self.run_copilot_http_task(move |client| {
            let response = client
                .post(url.as_str())
                .header(ACCEPT, "application/json")
                .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(form_urlencode(vec![
                    ("client_id", client_id),
                    ("scope", scope),
                ]))
                .send()
                .map_err(|error| {
                    provider_edge_bad_gateway(format!("failed to request device code: {error}"))
                })?;
            let status = response.status();
            let body = response.text().map_err(|error| {
                provider_edge_bad_gateway(format!("failed to request device code: {error}"))
            })?;
            if !status.is_success() {
                return Err(provider_edge_bad_gateway(format!(
                    "failed to request device code: device code request failed with status {}: {}",
                    status.as_u16(),
                    body
                )));
            }
            let device: CopilotDeviceCodeResponse =
                serde_json::from_str(body.as_str()).map_err(|error| {
                    provider_edge_bad_gateway(format!(
                    "failed to request device code: failed to parse device code response: {error}"
                ))
                })?;
            if device.device_code.trim().is_empty() {
                return Err(provider_edge_bad_gateway(
                    "failed to request device code: device code not received from GitHub",
                ));
            }
            Ok(device)
        })
    }

    fn poll_copilot_token_upstream(
        &self,
        device_code: &str,
    ) -> Result<CopilotPollResponse, ProviderEdgeAdminError> {
        let url = self.config.copilot_access_token_url.clone();
        let client_id = self.config.copilot_client_id.clone();
        let device_code = device_code.to_owned();

        self.run_copilot_http_task(move |client| {
            let response = client
                .post(url.as_str())
                .header(ACCEPT, "application/json")
                .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(form_urlencode(vec![
                    ("client_id", client_id),
                    ("device_code", device_code),
                    (
                        "grant_type",
                        PROVIDER_EDGE_COPILOT_DEVICE_GRANT_TYPE.to_owned(),
                    ),
                ]))
                .send()
                .map_err(|error| {
                    provider_edge_bad_gateway(format!("token poll failed: {error}"))
                })?;
            if !response.status().is_success() {
                return Err(provider_edge_bad_gateway(format!(
                    "token poll failed: access token request failed with status {}",
                    response.status().as_u16()
                )));
            }

            let content_type = response
                .headers()
                .get(CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_owned();
            let body = response.text().map_err(|error| {
                provider_edge_bad_gateway(format!("token poll failed: {error}"))
            })?;
            if content_type.contains("application/json") {
                serde_json::from_str(body.as_str()).map_err(|error| {
                    provider_edge_bad_gateway(format!(
                        "token poll failed: failed to parse access token JSON response: {error}"
                    ))
                })
            } else {
                parse_copilot_form_response(body.as_str())
            }
        })
    }
}

impl ProviderEdgeAdminPort for SqliteProviderEdgeAdminService {
    fn start_codex_oauth(
        &self,
        _request: &StartPkceOAuthRequest,
    ) -> Result<StartPkceOAuthResponse, ProviderEdgeAdminError> {
        self.start_pkce_flow(PkceProvider::Codex, None)
    }

    fn exchange_codex_oauth(
        &self,
        request: &ExchangeCallbackOAuthRequest,
    ) -> Result<ExchangeOAuthResponse, ProviderEdgeAdminError> {
        self.exchange_pkce_flow(PkceProvider::Codex, request)
    }

    fn start_claudecode_oauth(
        &self,
        _request: &StartPkceOAuthRequest,
    ) -> Result<StartPkceOAuthResponse, ProviderEdgeAdminError> {
        self.start_pkce_flow(PkceProvider::ClaudeCode, None)
    }

    fn exchange_claudecode_oauth(
        &self,
        request: &ExchangeCallbackOAuthRequest,
    ) -> Result<ExchangeOAuthResponse, ProviderEdgeAdminError> {
        self.exchange_pkce_flow(PkceProvider::ClaudeCode, request)
    }

    fn start_antigravity_oauth(
        &self,
        request: &StartAntigravityOAuthRequest,
    ) -> Result<StartPkceOAuthResponse, ProviderEdgeAdminError> {
        let project_id = request.project_id.trim();
        self.start_pkce_flow(
            PkceProvider::Antigravity,
            if project_id.is_empty() {
                None
            } else {
                Some(project_id.to_owned())
            },
        )
    }

    fn exchange_antigravity_oauth(
        &self,
        request: &ExchangeCallbackOAuthRequest,
    ) -> Result<ExchangeOAuthResponse, ProviderEdgeAdminError> {
        self.exchange_pkce_flow(PkceProvider::Antigravity, request)
    }

    fn start_copilot_oauth(
        &self,
        _request: &StartCopilotOAuthRequest,
    ) -> Result<StartCopilotOAuthResponse, ProviderEdgeAdminError> {
        let session_id = generate_provider_edge_session_id()?;
        let device = self.request_copilot_device_code()?;
        self.sessions
            .lock()
            .map_err(|_| provider_edge_internal_error("failed to lock provider-edge sessions"))?
            .insert(
                session_id.clone(),
                ProviderEdgeSession::CopilotDevice {
                    device_code: device.device_code.clone(),
                    expires_in: device.expires_in,
                    created_at: current_unix_timestamp(),
                },
            );
        Ok(StartCopilotOAuthResponse {
            session_id,
            user_code: device.user_code,
            verification_uri: device.verification_uri,
            expires_in: device.expires_in,
            interval: device.interval,
        })
    }

    fn poll_copilot_oauth(
        &self,
        request: &PollCopilotOAuthRequest,
    ) -> Result<PollCopilotOAuthResponse, ProviderEdgeAdminError> {
        let session = self.load_session(request.session_id.as_str())?;
        let (device_code, expires_in, created_at) = match session {
            ProviderEdgeSession::CopilotDevice {
                device_code,
                expires_in,
                created_at,
                ..
            } => (device_code, expires_in, created_at),
            ProviderEdgeSession::Pkce { .. } => {
                return Err(provider_edge_invalid_request("invalid or expired session"))
            }
        };
        if current_unix_timestamp() > created_at.saturating_add(expires_in) {
            self.delete_session(request.session_id.as_str())?;
            return Err(provider_edge_invalid_request("device code expired"));
        }

        let response = self.poll_copilot_token_upstream(device_code.as_str())?;
        if let Some(error) = response.error.as_deref() {
            return match error {
                "authorization_pending" => Ok(PollCopilotOAuthResponse {
                    access_token: None,
                    token_type: None,
                    scope: None,
                    status: "pending".to_owned(),
                    message: Some(PROVIDER_EDGE_COPILOT_PENDING_MESSAGE.to_owned()),
                }),
                "slow_down" => Ok(PollCopilotOAuthResponse {
                    access_token: None,
                    token_type: None,
                    scope: None,
                    status: "slow_down".to_owned(),
                    message: Some(PROVIDER_EDGE_COPILOT_SLOW_DOWN_MESSAGE.to_owned()),
                }),
                "expired_token" => {
                    self.delete_session(request.session_id.as_str())?;
                    Err(provider_edge_invalid_request("device code expired"))
                }
                "access_denied" => {
                    self.delete_session(request.session_id.as_str())?;
                    Err(provider_edge_invalid_request("access denied by user"))
                }
                other => Err(provider_edge_bad_gateway(format!(
                    "OAuth error: {other} - {}",
                    response.error_description.unwrap_or_default()
                ))),
            };
        }
        if let Some(access_token) = response.access_token {
            self.delete_session(request.session_id.as_str())?;
            return Ok(PollCopilotOAuthResponse {
                access_token: Some(access_token),
                token_type: response.token_type,
                scope: response.scope,
                status: "complete".to_owned(),
                message: Some(PROVIDER_EDGE_COPILOT_COMPLETE_MESSAGE.to_owned()),
            });
        }
        Err(provider_edge_internal_error(
            "unexpected response from GitHub",
        ))
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ParsedCallback {
    code: String,
    state: String,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct CopilotPollResponse {
    access_token: Option<String>,
    token_type: Option<String>,
    scope: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

pub(crate) fn provider_edge_invalid_request(message: impl Into<String>) -> ProviderEdgeAdminError {
    ProviderEdgeAdminError::InvalidRequest {
        message: message.into(),
    }
}

pub(crate) fn provider_edge_bad_gateway(message: impl Into<String>) -> ProviderEdgeAdminError {
    ProviderEdgeAdminError::BadGateway {
        message: message.into(),
    }
}

pub(crate) fn provider_edge_internal_error(message: impl Into<String>) -> ProviderEdgeAdminError {
    ProviderEdgeAdminError::Internal {
        message: message.into(),
    }
}

pub(crate) fn parse_callback(
    provider: PkceProvider,
    callback_url: &str,
) -> Result<ParsedCallback, ProviderEdgeAdminError> {
    let trimmed = callback_url.trim();
    if !trimmed.starts_with("http://") && !trimmed.starts_with("https://") {
        return Err(provider_edge_invalid_request(
            "callback_url must be a full URL",
        ));
    }
    let url = reqwest::Url::parse(trimmed)
        .map_err(|error| provider_edge_invalid_request(format!("invalid callback_url: {error}")))?;
    let code = url
        .query_pairs()
        .find(|(key, _)| key == "code")
        .map(|(_, value)| value.into_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| provider_edge_invalid_request("code parameter not found in callback_url"))?;
    let state = match provider {
        PkceProvider::ClaudeCode => {
            if !url.fragment().unwrap_or_default().trim().is_empty() {
                url.fragment().unwrap_or_default().to_owned()
            } else {
                url.query_pairs()
                    .find(|(key, _)| key == "state")
                    .map(|(_, value)| value.into_owned())
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| {
                        provider_edge_invalid_request(
                            "state parameter not found in callback_url (should be after # or in query)",
                        )
                    })?
            }
        }
        PkceProvider::Codex | PkceProvider::Antigravity => url
            .query_pairs()
            .find(|(key, _)| key == "state")
            .map(|(_, value)| value.into_owned())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                provider_edge_invalid_request("state parameter not found in callback_url")
            })?,
    };
    Ok(ParsedCallback { code, state })
}

pub(crate) fn generate_provider_edge_session_id() -> Result<String, ProviderEdgeAdminError> {
    let mut bytes = [0_u8; 32];
    getrandom(&mut bytes)
        .map(|_| hex_encode(bytes))
        .map_err(|error| {
            provider_edge_internal_error(format!("failed to generate oauth state: {error}"))
        })
}

pub(crate) fn generate_provider_edge_code_verifier() -> Result<String, ProviderEdgeAdminError> {
    let mut bytes = [0_u8; 64];
    getrandom(&mut bytes)
        .map(|_| hex_encode(bytes))
        .map_err(|error| {
            provider_edge_internal_error(format!("failed to generate code verifier: {error}"))
        })
}

pub(crate) fn provider_edge_code_challenge(code_verifier: &str) -> String {
    base64_url_no_padding(&sha256_digest(code_verifier.as_bytes()))
}

pub(crate) fn form_urlencode(params: Vec<(&str, String)>) -> String {
    let mut url = reqwest::Url::parse("http://localhost/").unwrap();
    {
        let mut query = url.query_pairs_mut();
        for (key, value) in params {
            query.append_pair(key, value.as_str());
        }
    }
    url.query().unwrap_or_default().to_owned()
}

pub(crate) fn oauth_credentials_json(token: &OAuthTokenResponse, client_id: String) -> String {
    let mut credentials = serde_json::Map::new();
    credentials.insert("client_id".to_owned(), Value::String(client_id));
    credentials.insert(
        "access_token".to_owned(),
        Value::String(token.access_token.clone().unwrap_or_default()),
    );
    if let Some(refresh_token) = token.refresh_token.clone() {
        credentials.insert("refresh_token".to_owned(), Value::String(refresh_token));
    }
    if let Some(id_token) = token.id_token.clone() {
        credentials.insert("id_token".to_owned(), Value::String(id_token));
    }
    if let Some(token_type) = token.token_type.clone() {
        credentials.insert("token_type".to_owned(), Value::String(token_type));
    }
    if let Some(scope) = token.scope.clone() {
        let scopes = scope
            .split_whitespace()
            .map(|value| Value::String(value.to_owned()))
            .collect::<Vec<_>>();
        credentials.insert("scopes".to_owned(), Value::Array(scopes));
    }
    if let Some(expires_in) = token.expires_in {
        credentials.insert(
            "expires_at".to_owned(),
            Value::String(format_unix_timestamp(
                current_unix_timestamp().saturating_add(expires_in),
            )),
        );
    }
    Value::Object(credentials).to_string()
}

pub(crate) fn extract_antigravity_project_id(body: &Value) -> Option<String> {
    if let Some(project_id) = body
        .get("cloudaicompanionProject")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    {
        return Some(project_id.to_owned());
    }
    body.get("cloudaicompanionProject")
        .and_then(|value| value.get("id"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

pub(crate) fn extract_antigravity_default_tier(body: &Value) -> Option<String> {
    let tiers = body.get("allowedTiers")?.as_array()?;
    let first = tiers
        .first()
        .and_then(|tier| tier.get("id"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    tiers
        .iter()
        .find(|tier| tier.get("isDefault").and_then(Value::as_bool) == Some(true))
        .and_then(|tier| tier.get("id"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .or(first)
}

pub(crate) fn parse_copilot_form_response(
    body: &str,
) -> Result<CopilotPollResponse, ProviderEdgeAdminError> {
    let url =
        reqwest::Url::parse(format!("http://localhost/?{body}").as_str()).map_err(|error| {
            provider_edge_bad_gateway(format!(
                "token poll failed: failed to parse access token form response: {error}"
            ))
        })?;
    let values = url
        .query_pairs()
        .into_owned()
        .collect::<HashMap<String, String>>();
    Ok(CopilotPollResponse {
        access_token: values
            .get("access_token")
            .cloned()
            .filter(|value| !value.is_empty()),
        token_type: values
            .get("token_type")
            .cloned()
            .filter(|value| !value.is_empty()),
        scope: values
            .get("scope")
            .cloned()
            .filter(|value| !value.is_empty()),
        error: values
            .get("error")
            .cloned()
            .filter(|value| !value.is_empty()),
        error_description: values
            .get("error_description")
            .cloned()
            .filter(|value| !value.is_empty()),
    })
}

pub(crate) fn provider_edge_default_http_client() -> &'static reqwest::blocking::Client {
    static CLIENT: OnceLock<reqwest::blocking::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        std::thread::spawn(reqwest::blocking::Client::new)
            .join()
            .unwrap_or_else(|_| reqwest::blocking::Client::new())
    })
}

enum CodexExchangeHttpClient<'a> {
    Borrowed(&'a reqwest::blocking::Client),
    Owned(reqwest::blocking::Client),
}

impl CodexExchangeHttpClient<'_> {
    fn as_client(&self) -> &reqwest::blocking::Client {
        match self {
            Self::Borrowed(client) => client,
            Self::Owned(client) => client,
        }
    }
}

fn build_codex_exchange_http_client(
    proxy: &OAuthProxyConfig,
) -> Result<Option<reqwest::blocking::Client>, ProviderEdgeAdminError> {
    let builder = reqwest::blocking::Client::builder();
    match proxy.proxy_type {
        OAuthProxyType::Disabled => {
            builder.no_proxy().build().map(Some).map_err(|error| {
                provider_edge_bad_gateway(format!("token exchange failed: {error}"))
            })
        }
        OAuthProxyType::Environment => builder
            .build()
            .map(Some)
            .map_err(|error| provider_edge_bad_gateway(format!("token exchange failed: {error}"))),
        OAuthProxyType::Url => {
            if proxy.url.trim().is_empty() {
                return Ok(None);
            }

            let mut reqwest_proxy = reqwest::Proxy::all(proxy.url.as_str()).map_err(|error| {
                provider_edge_bad_gateway(format!(
                    "token exchange failed: invalid proxy URL: {error}"
                ))
            })?;

            if !proxy.username.is_empty() && !proxy.password.is_empty() {
                reqwest_proxy =
                    reqwest_proxy.basic_auth(proxy.username.as_str(), proxy.password.as_str());
            }

            builder
                .proxy(reqwest_proxy)
                .build()
                .map(Some)
                .map_err(|error| {
                    provider_edge_bad_gateway(format!("token exchange failed: {error}"))
                })
        }
    }
}

fn required_env(key: &str) -> Option<String> {
    let value = env::var(key).ok()?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

fn required_env_list(key: &str) -> Option<Vec<String>> {
    let values = required_env(key)?
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();

    if values.is_empty() {
        None
    } else {
        Some(values)
    }
}

pub(crate) fn sha256_digest(input: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    let bit_len = (input.len() as u64) * 8;
    let mut data = input.to_vec();
    data.push(0x80);
    while data.len() % 64 != 56 {
        data.push(0);
    }
    data.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in data.chunks(64) {
        let mut w = [0_u32; 64];
        for (index, word) in w.iter_mut().enumerate().take(16) {
            let start = index * 4;
            *word = u32::from_be_bytes([
                chunk[start],
                chunk[start + 1],
                chunk[start + 2],
                chunk[start + 3],
            ]);
        }
        for index in 16..64 {
            let s0 = w[index - 15].rotate_right(7)
                ^ w[index - 15].rotate_right(18)
                ^ (w[index - 15] >> 3);
            let s1 = w[index - 2].rotate_right(17)
                ^ w[index - 2].rotate_right(19)
                ^ (w[index - 2] >> 10);
            w[index] = w[index - 16]
                .wrapping_add(s0)
                .wrapping_add(w[index - 7])
                .wrapping_add(s1);
        }

        let mut a = h[0];
        let mut b = h[1];
        let mut c = h[2];
        let mut d = h[3];
        let mut e = h[4];
        let mut f = h[5];
        let mut g = h[6];
        let mut hh = h[7];

        for index in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[index])
                .wrapping_add(w[index]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    let mut digest = [0_u8; 32];
    for (index, word) in h.iter().enumerate() {
        digest[index * 4..index * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    digest
}

pub(crate) fn base64_url_no_padding(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut encoded = String::new();
    let mut index = 0;
    while index + 3 <= input.len() {
        let chunk = &input[index..index + 3];
        let value = ((chunk[0] as u32) << 16) | ((chunk[1] as u32) << 8) | (chunk[2] as u32);
        encoded.push(TABLE[((value >> 18) & 0x3f) as usize] as char);
        encoded.push(TABLE[((value >> 12) & 0x3f) as usize] as char);
        encoded.push(TABLE[((value >> 6) & 0x3f) as usize] as char);
        encoded.push(TABLE[(value & 0x3f) as usize] as char);
        index += 3;
    }

    let remainder = input.len().saturating_sub(index);
    if remainder == 1 {
        let value = (input[index] as u32) << 16;
        encoded.push(TABLE[((value >> 18) & 0x3f) as usize] as char);
        encoded.push(TABLE[((value >> 12) & 0x3f) as usize] as char);
    } else if remainder == 2 {
        let value = ((input[index] as u32) << 16) | ((input[index + 1] as u32) << 8);
        encoded.push(TABLE[((value >> 18) & 0x3f) as usize] as char);
        encoded.push(TABLE[((value >> 12) & 0x3f) as usize] as char);
        encoded.push(TABLE[((value >> 6) & 0x3f) as usize] as char);
    }

    encoded
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};
    use std::thread;

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    struct ProviderEdgeEnvFixture {
        previous: Vec<(&'static str, Option<String>)>,
    }

    impl ProviderEdgeEnvFixture {
        fn new() -> Self {
            let previous = PROVIDER_EDGE_REQUIRED_ENV_VARS
                .iter()
                .map(|key| (*key, env::var(key).ok()))
                .collect::<Vec<_>>();

            for key in PROVIDER_EDGE_REQUIRED_ENV_VARS {
                env::remove_var(key);
            }

            Self { previous }
        }

        fn set_all(&self) {
            for (key, value) in provider_edge_env_values() {
                env::set_var(key, value);
            }
        }
    }

    impl Drop for ProviderEdgeEnvFixture {
        fn drop(&mut self) {
            for (key, value) in &self.previous {
                match value {
                    Some(value) => env::set_var(key, value),
                    None => env::remove_var(key),
                }
            }
        }
    }

    fn provider_edge_env_values() -> Vec<(&'static str, &'static str)> {
        vec![
            (
                "AXONHUB_PROVIDER_EDGE_CODEX_AUTHORIZE_URL",
                "https://example.test/codex/authorize",
            ),
            (
                "AXONHUB_PROVIDER_EDGE_CODEX_TOKEN_URL",
                "https://example.test/codex/token",
            ),
            ("AXONHUB_PROVIDER_EDGE_CODEX_CLIENT_ID", "codex-client-id"),
            (
                "AXONHUB_PROVIDER_EDGE_CODEX_REDIRECT_URI",
                "http://localhost:1455/auth/callback",
            ),
            (
                "AXONHUB_PROVIDER_EDGE_CODEX_SCOPES",
                "openid profile email offline_access",
            ),
            ("AXONHUB_PROVIDER_EDGE_CODEX_USER_AGENT", "codex-test-agent"),
            (
                "AXONHUB_PROVIDER_EDGE_CLAUDECODE_AUTHORIZE_URL",
                "https://example.test/claudecode/authorize",
            ),
            (
                "AXONHUB_PROVIDER_EDGE_CLAUDECODE_TOKEN_URL",
                "https://example.test/claudecode/token",
            ),
            (
                "AXONHUB_PROVIDER_EDGE_CLAUDECODE_CLIENT_ID",
                "claudecode-client-id",
            ),
            (
                "AXONHUB_PROVIDER_EDGE_CLAUDECODE_REDIRECT_URI",
                "http://localhost:54545/callback",
            ),
            (
                "AXONHUB_PROVIDER_EDGE_CLAUDECODE_SCOPES",
                "org:create_api_key user:profile user:inference",
            ),
            (
                "AXONHUB_PROVIDER_EDGE_CLAUDECODE_USER_AGENT",
                "claudecode-test-agent",
            ),
            (
                "AXONHUB_PROVIDER_EDGE_ANTIGRAVITY_AUTHORIZE_URL",
                "https://example.test/antigravity/authorize",
            ),
            (
                "AXONHUB_PROVIDER_EDGE_ANTIGRAVITY_TOKEN_URL",
                "https://example.test/antigravity/token",
            ),
            (
                "AXONHUB_PROVIDER_EDGE_ANTIGRAVITY_CLIENT_ID",
                "antigravity-client-id",
            ),
            (
                "AXONHUB_PROVIDER_EDGE_ANTIGRAVITY_CLIENT_SECRET",
                "antigravity-client-secret",
            ),
            (
                "AXONHUB_PROVIDER_EDGE_ANTIGRAVITY_REDIRECT_URI",
                "http://localhost:51121/oauth-callback",
            ),
            (
                "AXONHUB_PROVIDER_EDGE_ANTIGRAVITY_SCOPES",
                "scope-a scope-b",
            ),
            (
                "AXONHUB_PROVIDER_EDGE_ANTIGRAVITY_LOAD_ENDPOINTS",
                "https://example.test/load-a,https://example.test/load-b",
            ),
            (
                "AXONHUB_PROVIDER_EDGE_ANTIGRAVITY_USER_AGENT",
                "antigravity-test-agent",
            ),
            (
                "AXONHUB_PROVIDER_EDGE_ANTIGRAVITY_CLIENT_METADATA",
                r#"{"ideType":"ANTIGRAVITY"}"#,
            ),
            (
                "AXONHUB_PROVIDER_EDGE_COPILOT_DEVICE_CODE_URL",
                "https://example.test/copilot/device/code",
            ),
            (
                "AXONHUB_PROVIDER_EDGE_COPILOT_ACCESS_TOKEN_URL",
                "https://example.test/copilot/access/token",
            ),
            (
                "AXONHUB_PROVIDER_EDGE_COPILOT_CLIENT_ID",
                "copilot-client-id",
            ),
            ("AXONHUB_PROVIDER_EDGE_COPILOT_SCOPE", "read:user"),
        ]
    }

    #[test]
    fn provider_edge_config_requires_secure_runtime_env() {
        let _lock = env_lock().lock().unwrap();
        let fixture = ProviderEdgeEnvFixture::new();

        assert!(ProviderEdgeAdminConfig::from_env().is_none());

        fixture.set_all();

        let config =
            ProviderEdgeAdminConfig::from_env().expect("expected provider-edge env config");
        assert_eq!(config.codex_client_id, "codex-client-id");
        assert_eq!(
            config.antigravity_load_endpoints,
            vec![
                "https://example.test/load-a".to_owned(),
                "https://example.test/load-b".to_owned(),
            ]
        );
        assert_eq!(config.copilot_scope, "read:user");
    }

    #[test]
    fn sqlite_provider_edge_service_is_env_gated() {
        let _lock = env_lock().lock().unwrap();
        let fixture = ProviderEdgeEnvFixture::new();

        assert!(SqliteProviderEdgeAdminService::from_env().is_none());

        fixture.set_all();

        assert!(SqliteProviderEdgeAdminService::from_env().is_some());
    }

    fn read_http_request(stream: &mut std::net::TcpStream) -> (String, Vec<u8>) {
        let mut request_bytes = Vec::new();
        let mut buffer = [0_u8; 4096];
        let mut header_end = None;
        let mut expected_body_len = None;

        loop {
            let size = stream.read(&mut buffer).expect("read request");
            if size == 0 {
                break;
            }
            request_bytes.extend_from_slice(&buffer[..size]);

            if header_end.is_none() {
                header_end = request_bytes
                    .windows(4)
                    .position(|window| window == b"\r\n\r\n")
                    .map(|position| position + 4);
                if let Some(end) = header_end {
                    let headers = String::from_utf8_lossy(&request_bytes[..end]);
                    expected_body_len = headers
                        .lines()
                        .find_map(|line| {
                            line.split_once(':').and_then(|(name, value)| {
                                name.trim()
                                    .eq_ignore_ascii_case("content-length")
                                    .then(|| value.trim().parse::<usize>().ok())
                                    .flatten()
                            })
                        })
                        .or(Some(0));
                }
            }

            if let (Some(end), Some(body_len)) = (header_end, expected_body_len) {
                if request_bytes.len() >= end + body_len {
                    let raw = String::from_utf8_lossy(&request_bytes).to_string();
                    let body = request_bytes[end..end + body_len].to_vec();
                    return (raw, body);
                }
            }
        }

        (
            String::from_utf8_lossy(&request_bytes).to_string(),
            Vec::new(),
        )
    }

    fn start_http_server<F>(handler: F) -> String
    where
        F: Fn(String, Vec<u8>) -> String + Send + 'static,
    {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let address = listener.local_addr().expect("read local addr");
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept request");
            let (request, body) = read_http_request(&mut stream);
            let response = handler(request, body);
            stream
                .write_all(response.as_bytes())
                .expect("write response");
        });
        format!("http://{address}")
    }

    fn http_json_response(status_line: &str, body: &str) -> String {
        format!(
            "{status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    fn http_form_response(status_line: &str, body: &str) -> String {
        format!(
            "{status_line}\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    fn start_proxy_server<F>(handler: F) -> String
    where
        F: Fn(String, Vec<u8>) -> String + Send + 'static,
    {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind proxy server");
        let address = listener.local_addr().expect("read proxy local addr");
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept proxy request");
            let (request, body) = read_http_request(&mut stream);
            let response = handler(request, body);
            stream
                .write_all(response.as_bytes())
                .expect("write proxy response");
        });
        format!("http://{address}")
    }

    fn test_provider_edge_config() -> ProviderEdgeAdminConfig {
        ProviderEdgeAdminConfig {
            codex_authorize_url: "https://example.test/codex/authorize".to_owned(),
            codex_token_url: "https://example.test/codex/token".to_owned(),
            codex_client_id: "codex-client-id".to_owned(),
            codex_redirect_uri: "http://localhost:1455/auth/callback".to_owned(),
            codex_scopes: "openid profile email offline_access".to_owned(),
            codex_user_agent: "codex-test-agent".to_owned(),
            claudecode_authorize_url: "https://example.test/claudecode/authorize".to_owned(),
            claudecode_token_url: "https://example.test/claudecode/token".to_owned(),
            claudecode_client_id: "claudecode-client-id".to_owned(),
            claudecode_redirect_uri: "http://localhost:54545/callback".to_owned(),
            claudecode_scopes: "org:create_api_key user:profile user:inference".to_owned(),
            claudecode_user_agent: "claudecode-test-agent".to_owned(),
            antigravity_authorize_url: "https://example.test/antigravity/authorize".to_owned(),
            antigravity_token_url: "https://example.test/antigravity/token".to_owned(),
            antigravity_client_id: "antigravity-client-id".to_owned(),
            antigravity_client_secret: "antigravity-client-secret".to_owned(),
            antigravity_redirect_uri: "http://localhost:51121/oauth-callback".to_owned(),
            antigravity_scopes: "scope-a scope-b".to_owned(),
            antigravity_load_endpoints: vec!["https://example.test/load-a".to_owned()],
            antigravity_user_agent: "antigravity-test-agent".to_owned(),
            antigravity_client_metadata: r#"{"ideType":"ANTIGRAVITY"}"#.to_owned(),
            copilot_device_code_url: "https://example.test/copilot/device/code".to_owned(),
            copilot_access_token_url: "https://example.test/copilot/access/token".to_owned(),
            copilot_client_id: "copilot-client-id".to_owned(),
            copilot_scope: "read:user".to_owned(),
        }
    }

    #[test]
    fn parse_callback_supports_claudecode_fragment_state() {
        let parsed = parse_callback(
            PkceProvider::ClaudeCode,
            "http://localhost:54545/callback?code=test-code#test-state",
        )
        .expect("parse callback");

        assert_eq!(parsed.code, "test-code");
        assert_eq!(parsed.state, "test-state");
    }

    #[test]
    fn parse_callback_rejects_missing_claudecode_state() {
        let error = parse_callback(
            PkceProvider::ClaudeCode,
            "http://localhost:54545/callback?code=test-code",
        )
        .expect_err("missing state should fail");

        match error {
            ProviderEdgeAdminError::InvalidRequest { message } => {
                assert_eq!(
                    message,
                    "state parameter not found in callback_url (should be after # or in query)"
                );
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn codex_exchange_consumes_session_after_state_mismatch() {
        let service = SqliteProviderEdgeAdminService::new(test_provider_edge_config());
        let start = service
            .start_codex_oauth(&StartPkceOAuthRequest {})
            .expect("start codex oauth");

        let mismatch = service
            .exchange_codex_oauth(&ExchangeCallbackOAuthRequest {
                session_id: start.session_id.clone(),
                callback_url: "http://localhost:1455/auth/callback?code=test-code&state=mismatch"
                    .to_owned(),
                proxy: None,
            })
            .expect_err("state mismatch should fail");

        match mismatch {
            ProviderEdgeAdminError::InvalidRequest { message } => {
                assert_eq!(message, "oauth state mismatch");
            }
            other => panic!("unexpected error: {other:?}"),
        }

        let replay = service
            .exchange_codex_oauth(&ExchangeCallbackOAuthRequest {
                session_id: start.session_id,
                callback_url:
                    "http://localhost:1455/auth/callback?code=test-code&state=unused-session"
                        .to_owned(),
                proxy: None,
            })
            .expect_err("replay after mismatch should fail");

        match replay {
            ProviderEdgeAdminError::InvalidRequest { message } => {
                assert_eq!(message, "invalid or expired oauth session");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn codex_exchange_without_proxy_posts_form_and_returns_credentials_json() {
        let token_server_url = start_http_server(|request, body| {
            assert!(request.starts_with("POST /token HTTP/1.1"));
            assert!(
                request.contains("content-type: application/x-www-form-urlencoded")
                    || request.contains("Content-Type: application/x-www-form-urlencoded")
            );
            assert!(
                request.contains("user-agent: codex-test-agent")
                    || request.contains("User-Agent: codex-test-agent")
            );
            let body = String::from_utf8(body).expect("form body");
            assert!(body.contains("grant_type=authorization_code"));
            assert!(body.contains("client_id=codex-client-id"));
            assert!(body.contains("code=test-code"));
            assert!(body.contains("redirect_uri=http%3A%2F%2Flocalhost%3A1455%2Fauth%2Fcallback"));

            http_json_response(
                "HTTP/1.1 200 OK",
                r#"{"access_token":"codex-access","refresh_token":"codex-refresh","id_token":"codex-id","expires_in":3600,"token_type":"bearer","scope":"openid profile email offline_access"}"#,
            )
        });

        let mut config = test_provider_edge_config();
        config.codex_token_url = format!("{token_server_url}/token");
        let service = SqliteProviderEdgeAdminService::new(config);
        let start = service
            .start_codex_oauth(&StartPkceOAuthRequest {})
            .expect("start codex oauth");

        let response = service
            .exchange_codex_oauth(&ExchangeCallbackOAuthRequest {
                session_id: start.session_id.clone(),
                callback_url: format!(
                    "http://localhost:1455/auth/callback?code=test-code&state={}",
                    start.session_id
                ),
                proxy: None,
            })
            .expect("exchange codex oauth");

        let credentials: Value =
            serde_json::from_str(&response.credentials).expect("credentials json");
        assert_eq!(credentials["client_id"], "codex-client-id");
        assert_eq!(credentials["access_token"], "codex-access");
        assert_eq!(credentials["refresh_token"], "codex-refresh");
        assert_eq!(credentials["id_token"], "codex-id");
        assert_eq!(credentials["token_type"], "bearer");
        assert_eq!(
            credentials["scopes"],
            serde_json::json!(["openid", "profile", "email", "offline_access"])
        );
        assert!(credentials["expires_at"]
            .as_str()
            .is_some_and(|value| value.ends_with('Z')));
    }

    #[test]
    fn codex_exchange_uses_request_scoped_proxy_override_when_present() {
        let proxy_hits = Arc::new(Mutex::new(0_u32));
        let proxy_hits_for_server = Arc::clone(&proxy_hits);
        let proxy_url = start_proxy_server(move |request, body| {
            *proxy_hits_for_server.lock().unwrap() += 1;
            assert!(request.starts_with("POST http://codex-upstream.invalid/token HTTP/1.1"));
            assert!(
                request.contains("proxy-authorization: Basic dXNlcjpzZWNyZXQ=")
                    || request.contains("Proxy-Authorization: Basic dXNlcjpzZWNyZXQ=")
            );
            let body = String::from_utf8(body).expect("form body");
            assert!(body.contains("grant_type=authorization_code"));
            assert!(body.contains("client_id=codex-client-id"));
            assert!(body.contains("code=test-code"));

            http_json_response(
                "HTTP/1.1 200 OK",
                r#"{"access_token":"proxy-access","refresh_token":"proxy-refresh","expires_in":3600,"token_type":"bearer"}"#,
            )
        });

        let mut config = test_provider_edge_config();
        config.codex_token_url = "http://codex-upstream.invalid/token".to_owned();
        let service = SqliteProviderEdgeAdminService::new(config);
        let start = service
            .start_codex_oauth(&StartPkceOAuthRequest {})
            .expect("start codex oauth");

        let response = service
            .exchange_codex_oauth(&ExchangeCallbackOAuthRequest {
                session_id: start.session_id.clone(),
                callback_url: format!(
                    "http://localhost:1455/auth/callback?code=test-code&state={}",
                    start.session_id
                ),
                proxy: Some(OAuthProxyConfig {
                    proxy_type: OAuthProxyType::Url,
                    url: proxy_url,
                    username: "user".to_owned(),
                    password: "secret".to_owned(),
                }),
            })
            .expect("exchange codex oauth through proxy");

        let credentials: Value =
            serde_json::from_str(&response.credentials).expect("credentials json");
        assert_eq!(credentials["access_token"], "proxy-access");
        assert_eq!(credentials["refresh_token"], "proxy-refresh");
        assert_eq!(*proxy_hits.lock().unwrap(), 1);
    }

    #[test]
    fn claudecode_exchange_posts_json_and_returns_credentials_json() {
        let token_server_url = start_http_server(|request, body| {
            assert!(request.starts_with("POST /token HTTP/1.1"));
            assert!(
                request.contains("content-type: application/json")
                    || request.contains("Content-Type: application/json")
            );
            assert!(
                request.contains("user-agent: claudecode-test-agent")
                    || request.contains("User-Agent: claudecode-test-agent")
            );

            let payload: Value = serde_json::from_slice(&body).expect("json body");
            assert_eq!(payload["grant_type"], "authorization_code");
            assert_eq!(payload["code"], "test-code");
            assert_eq!(payload["client_id"], "claudecode-client-id");
            assert_eq!(payload["redirect_uri"], "http://localhost:54545/callback");
            assert!(payload["state"]
                .as_str()
                .is_some_and(|value| !value.is_empty()));
            assert!(payload["code_verifier"]
                .as_str()
                .is_some_and(|value| !value.is_empty()));

            http_json_response(
                "HTTP/1.1 200 OK",
                r#"{"access_token":"claude-access","refresh_token":"claude-refresh","id_token":"claude-id","expires_in":3600,"token_type":"bearer","scope":"org:create_api_key user:profile"}"#,
            )
        });

        let mut config = test_provider_edge_config();
        config.claudecode_token_url = format!("{token_server_url}/token");
        let service = SqliteProviderEdgeAdminService::new(config);
        let start = service
            .start_claudecode_oauth(&StartPkceOAuthRequest {})
            .expect("start claudecode oauth");
        let session_id = start.session_id.clone();

        let response = service
            .exchange_claudecode_oauth(&ExchangeCallbackOAuthRequest {
                session_id: session_id.clone(),
                callback_url: format!(
                    "http://localhost:54545/callback?code=test-code#{session_id}"
                ),
                proxy: None,
            })
            .expect("exchange claudecode oauth");

        let credentials: Value =
            serde_json::from_str(&response.credentials).expect("credentials json");
        assert_eq!(credentials["client_id"], "claudecode-client-id");
        assert_eq!(credentials["access_token"], "claude-access");
        assert_eq!(credentials["refresh_token"], "claude-refresh");
        assert_eq!(credentials["id_token"], "claude-id");
        assert_eq!(credentials["token_type"], "bearer");
        assert_eq!(
            credentials["scopes"],
            serde_json::json!(["org:create_api_key", "user:profile"])
        );
        assert!(credentials["expires_at"]
            .as_str()
            .is_some_and(|value| value.ends_with('Z')));
    }

    #[test]
    fn antigravity_exchange_returns_refresh_token_with_project_id_and_rejects_empty_load_endpoints()
    {
        let token_server_url = start_http_server(|request, body| {
            assert!(request.starts_with("POST /token HTTP/1.1"));
            assert!(
                request.contains("content-type: application/x-www-form-urlencoded")
                    || request.contains("Content-Type: application/x-www-form-urlencoded")
            );
            let body = String::from_utf8(body).expect("form body");
            assert!(body.contains("grant_type=authorization_code"));
            assert!(body.contains("client_id=antigravity-client-id"));
            assert!(body.contains("client_secret=antigravity-client-secret"));
            assert!(body.contains("code=test-code"));

            http_json_response(
                "HTTP/1.1 200 OK",
                r#"{"access_token":"ag-access","refresh_token":"ag-refresh","expires_in":3600,"token_type":"bearer"}"#,
            )
        });

        let mut config = test_provider_edge_config();
        config.antigravity_token_url = format!("{token_server_url}/token");
        config.antigravity_load_endpoints = Vec::new();
        let service = SqliteProviderEdgeAdminService::new(config.clone());

        let start_with_project = service
            .start_antigravity_oauth(&StartAntigravityOAuthRequest {
                project_id: "project-123".to_owned(),
            })
            .expect("start antigravity oauth");

        let response = service
            .exchange_antigravity_oauth(&ExchangeCallbackOAuthRequest {
                session_id: start_with_project.session_id.clone(),
                callback_url: format!(
                    "http://localhost:51121/oauth-callback?code=test-code&state={}",
                    start_with_project.session_id
                ),
                proxy: None,
            })
            .expect("exchange antigravity oauth");
        assert_eq!(response.credentials, "ag-refresh|project-123");

        let second_token_server_url = start_http_server(|_, _| {
            http_json_response(
                "HTTP/1.1 200 OK",
                r#"{"access_token":"ag-access-2","refresh_token":"ag-refresh-2","expires_in":3600,"token_type":"bearer"}"#,
            )
        });
        let mut config_without_project = config;
        config_without_project.antigravity_token_url = format!("{second_token_server_url}/token");
        let service_without_project = SqliteProviderEdgeAdminService::new(config_without_project);
        let start_without_project = service_without_project
            .start_antigravity_oauth(&StartAntigravityOAuthRequest {
                project_id: String::new(),
            })
            .expect("start antigravity oauth without project");

        let error = service_without_project
            .exchange_antigravity_oauth(&ExchangeCallbackOAuthRequest {
                session_id: start_without_project.session_id.clone(),
                callback_url: format!(
                    "http://localhost:51121/oauth-callback?code=test-code&state={}",
                    start_without_project.session_id
                ),
                proxy: None,
            })
            .expect_err("missing endpoints should fail");

        match error {
            ProviderEdgeAdminError::BadGateway { message } => {
                assert_eq!(
                    message,
                    "failed to resolve project id and none provided: no load endpoints configured"
                );
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn copilot_start_and_poll_cover_form_and_error_responses() {
        let device_server_url = start_http_server(|request, body| {
            assert!(request.starts_with("POST /device HTTP/1.1"));
            assert!(
                request.contains("content-type: application/x-www-form-urlencoded")
                    || request.contains("Content-Type: application/x-www-form-urlencoded")
            );
            let body = String::from_utf8(body).expect("form body");
            assert!(body.contains("client_id=copilot-client-id"));
            assert!(body.contains("scope=read%3Auser"));

            http_json_response(
                "HTTP/1.1 200 OK",
                r#"{"device_code":"device-code-123","user_code":"ABCD-EFGH","verification_uri":"https://github.com/login/device","expires_in":900,"interval":5}"#,
            )
        });
        let token_server_url = start_http_server(|request, body| {
            assert!(request.starts_with("POST /token HTTP/1.1"));
            let body = String::from_utf8(body).expect("form body");
            assert!(body.contains("client_id=copilot-client-id"));
            assert!(body.contains("device_code=device-code-123"));
            assert!(
                body.contains("grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Adevice_code")
            );

            http_form_response(
                "HTTP/1.1 200 OK",
                "access_token=gho_form_token&token_type=bearer&scope=read%3Auser",
            )
        });

        let mut config = test_provider_edge_config();
        config.copilot_device_code_url = format!("{device_server_url}/device");
        config.copilot_access_token_url = format!("{token_server_url}/token");
        let service = SqliteProviderEdgeAdminService::new(config);

        let start = service
            .start_copilot_oauth(&StartCopilotOAuthRequest {})
            .expect("start copilot oauth");
        assert_eq!(start.user_code, "ABCD-EFGH");
        assert_eq!(start.verification_uri, "https://github.com/login/device");
        assert_eq!(start.expires_in, 900);
        assert_eq!(start.interval, 5);

        let poll = service
            .poll_copilot_oauth(&PollCopilotOAuthRequest {
                session_id: start.session_id.clone(),
            })
            .expect("poll copilot oauth");
        assert_eq!(poll.status, "complete");
        assert_eq!(poll.access_token.as_deref(), Some("gho_form_token"));
        assert_eq!(poll.token_type.as_deref(), Some("bearer"));
        assert_eq!(poll.scope.as_deref(), Some("read:user"));

        let replay = service
            .poll_copilot_oauth(&PollCopilotOAuthRequest {
                session_id: start.session_id,
            })
            .expect_err("session should be deleted after success");
        match replay {
            ProviderEdgeAdminError::InvalidRequest { message } => {
                assert_eq!(message, "invalid or expired session");
            }
            other => panic!("unexpected error: {other:?}"),
        }

        let pending_token_url = start_http_server(|_, _| {
            http_json_response(
                "HTTP/1.1 200 OK",
                r#"{"error":"authorization_pending","error_description":"pending"}"#,
            )
        });
        let device_server_url = start_http_server(|_, _| {
            http_json_response(
                "HTTP/1.1 200 OK",
                r#"{"device_code":"device-code-456","user_code":"WXYZ-1234","verification_uri":"https://github.com/login/device","expires_in":900,"interval":5}"#,
            )
        });
        let mut config = test_provider_edge_config();
        config.copilot_device_code_url = format!("{device_server_url}/device");
        config.copilot_access_token_url = format!("{pending_token_url}/token");
        let pending_service = SqliteProviderEdgeAdminService::new(config);
        let pending_start = pending_service
            .start_copilot_oauth(&StartCopilotOAuthRequest {})
            .expect("start pending copilot oauth");
        let pending = pending_service
            .poll_copilot_oauth(&PollCopilotOAuthRequest {
                session_id: pending_start.session_id,
            })
            .expect("pending poll should succeed");
        assert_eq!(pending.status, "pending");
        assert_eq!(
            pending.message.as_deref(),
            Some(PROVIDER_EDGE_COPILOT_PENDING_MESSAGE)
        );

        let denied_token_url = start_http_server(|_, _| {
            http_json_response(
                "HTTP/1.1 200 OK",
                r#"{"error":"access_denied","error_description":"denied"}"#,
            )
        });
        let device_server_url = start_http_server(|_, _| {
            http_json_response(
                "HTTP/1.1 200 OK",
                r#"{"device_code":"device-code-789","user_code":"QWER-9876","verification_uri":"https://github.com/login/device","expires_in":900,"interval":5}"#,
            )
        });
        let mut config = test_provider_edge_config();
        config.copilot_device_code_url = format!("{device_server_url}/device");
        config.copilot_access_token_url = format!("{denied_token_url}/token");
        let denied_service = SqliteProviderEdgeAdminService::new(config);
        let denied_start = denied_service
            .start_copilot_oauth(&StartCopilotOAuthRequest {})
            .expect("start denied copilot oauth");
        let denied = denied_service
            .poll_copilot_oauth(&PollCopilotOAuthRequest {
                session_id: denied_start.session_id.clone(),
            })
            .expect_err("access denied should fail");
        match denied {
            ProviderEdgeAdminError::InvalidRequest { message } => {
                assert_eq!(message, "access denied by user");
            }
            other => panic!("unexpected error: {other:?}"),
        }
        let replay = denied_service
            .poll_copilot_oauth(&PollCopilotOAuthRequest {
                session_id: denied_start.session_id,
            })
            .expect_err("denied session should be deleted");
        match replay {
            ProviderEdgeAdminError::InvalidRequest { message } => {
                assert_eq!(message, "invalid or expired session");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }
}
