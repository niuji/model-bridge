use crate::{admin::provider_svc, config::AccessType, state::AppState};
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

fn error(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({"error":message}))).into_response()
}

// Admin requests may arrive through the Vite same-origin development proxy.
// Compare Origin with Host, never use either as the OAuth success redirect.
fn check_origin(headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get("origin") else {
        return true;
    };
    let Ok(origin) = origin.to_str() else {
        return false;
    };
    let Ok(url) = reqwest::Url::parse(origin) else {
        return false;
    };
    let authority = origin
        .strip_prefix(&format!("{}://", url.scheme()))
        .unwrap_or("");
    matches!(url.scheme(), "http" | "https")
        && headers.get("host").and_then(|h| h.to_str().ok()) == Some(authority)
}

fn validate(state: &AppState, id: &str, headers: &HeaderMap) -> Result<(), Box<Response>> {
    if !check_origin(headers) {
        return Err(Box::new(error(
            StatusCode::FORBIDDEN,
            "cross-origin admin request rejected",
        )));
    }
    let Some(def) = state.provider_defs.iter().find(|d| d.id == id) else {
        return Err(Box::new(error(StatusCode::NOT_FOUND, "provider not found")));
    };
    if def.access_type != AccessType::Subscription || def.config_error.is_some() {
        return Err(Box::new(error(
            StatusCode::BAD_REQUEST,
            "provider is not a valid subscription provider",
        )));
    }
    Ok(())
}

pub(super) async fn decorate(state: &AppState, value: &mut Value) -> anyhow::Result<()> {
    let Some(id) = value.get("id").and_then(Value::as_str).map(str::to_owned) else {
        return Ok(());
    };
    let Some(def) = state.provider_defs.iter().find(|d| d.id == id) else {
        return Ok(());
    };
    value["access_type"] = serde_json::to_value(def.access_type)?;
    if def.access_type == AccessType::Subscription {
        let mut auth = serde_json::to_value(state.subscription.summary(&id).await?)?;
        auth["login_available"] = json!(state.encryption_key.is_some());
        value["auth"] = auth;
        // A previous definition with the same ID must not expose its key here.
        if let Some(object) = value.as_object_mut() {
            object.remove("api_key");
            object.remove("workspace_id");
        }
        value["has_cost_api_key"] = json!(false);
        value["balance"] = Value::Null;
        value["usage"] = Value::Null;
        if let Some(channels) = value["channels"].as_array_mut() {
            for ch in channels {
                ch["models_endpoint"] = json!("subscription");
            }
        }
    }
    Ok(())
}

pub async fn login(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Response {
    if let Err(r) = validate(&state, &id, &headers) {
        return *r;
    }
    match state
        .subscription
        .start_login(&id, &state.admin_base_url)
        .await
    {
        Ok(result) => {
            let watch_state = state.clone();
            let session = result.session_id.clone();
            // Automatic callbacks must activate the saved account even after the UI closes.
            // The service owns expiry; this observer is bounded separately for shutdown.
            tokio::spawn(async move {
                let _ = tokio::time::timeout(std::time::Duration::from_secs(601), async move {
                    loop {
                        match watch_state.subscription.login_status(&id, &session).await {
                            Ok(status) if status.status == "succeeded" => {
                                if provider_svc::refresh_routes(&watch_state).await.is_err() {
                                    tracing::error!(
                                        "Subscription account saved but route refresh failed"
                                    );
                                }
                                break;
                            }
                            Ok(status)
                                if matches!(status.status.as_str(), "pending" | "exchanging") => {}
                            _ => break,
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    }
                })
                .await;
            });
            Json(result).into_response()
        }
        Err(e) => error(StatusCode::BAD_REQUEST, &e.to_string()),
    }
}

pub async fn status(
    State(state): State<Arc<AppState>>,
    Path((id, session)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    if let Err(r) = validate(&state, &id, &headers) {
        return *r;
    }
    match state.subscription.login_status(&id, &session).await {
        Ok(result) => {
            if result.status == "succeeded" && provider_svc::refresh_routes(&state).await.is_err() {
                return error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "account saved but route refresh failed",
                );
            }
            Json(result).into_response()
        }
        Err(e) => error(StatusCode::BAD_REQUEST, &e.to_string()),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Callback {
    url: String,
}
pub async fn callback(
    State(state): State<Arc<AppState>>,
    Path((id, session)): Path<(String, String)>,
    headers: HeaderMap,
    Json(body): Json<Callback>,
) -> Response {
    if let Err(r) = validate(&state, &id, &headers) {
        return *r;
    }
    match state
        .subscription
        .submit_callback(&id, &session, &body.url)
        .await
    {
        Ok(result) => {
            if provider_svc::refresh_routes(&state).await.is_err() {
                return error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "account saved but route refresh failed",
                );
            }
            Json(result).into_response()
        }
        Err(e) => error(StatusCode::BAD_REQUEST, &e.to_string()),
    }
}
pub async fn cancel(
    State(state): State<Arc<AppState>>,
    Path((id, session)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    if let Err(r) = validate(&state, &id, &headers) {
        return *r;
    }
    match state.subscription.cancel_login(&id, &session).await {
        Ok(()) => Json(json!({"status":"cancelled"})).into_response(),
        Err(e) => error(StatusCode::BAD_REQUEST, &e.to_string()),
    }
}
pub async fn logout(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Response {
    if let Err(r) = validate(&state, &id, &headers) {
        return *r;
    }
    if state.subscription.logout(&id).await.is_err() {
        return error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to remove subscription account",
        );
    }
    if provider_svc::refresh_routes(&state).await.is_err() {
        return error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "account removed but route refresh failed",
        );
    }
    Json(json!({"status":"unconfigured"})).into_response()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelQuery {
    channel: String,
    #[serde(default)]
    api_key: Option<String>,
    #[serde(default)]
    workspace_id: Option<String>,
}
pub async fn models(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<ModelQuery>,
) -> Response {
    if !check_origin(&headers) {
        return error(StatusCode::FORBIDDEN, "cross-origin admin request rejected");
    }
    let Some(def) = state.provider_defs.iter().find(|d| d.id == id) else {
        return error(StatusCode::NOT_FOUND, "provider not found");
    };
    if def.config_error.is_some() || !def.channels.iter().any(|c| c.channel_type == body.channel) {
        return error(StatusCode::BAD_REQUEST, "invalid provider channel");
    }
    let result = if def.access_type == AccessType::Subscription {
        if body.api_key.as_deref().is_some_and(|key| !key.is_empty())
            || body
                .workspace_id
                .as_deref()
                .is_some_and(|workspace| !workspace.is_empty())
        {
            return error(
                StatusCode::BAD_REQUEST,
                "subscription credentials are managed by the server",
            );
        }
        state.subscription.models(&id).await
    } else {
        let saved = match provider_svc::get_provider_config(&state.db, &id).await {
            Ok(c) => c,
            Err(_) => {
                return error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "failed to read credentials",
                )
            }
        };
        let key = body
            .api_key
            .as_deref()
            .or_else(|| saved.as_ref().map(|c| c.api_key.as_str()))
            .unwrap_or("");
        let workspace = body
            .workspace_id
            .as_deref()
            .or_else(|| saved.as_ref().map(|c| c.workspace_id.as_str()))
            .unwrap_or("");
        provider_svc::fetch_models_from_api(
            &state.client,
            &state.provider_defs,
            &id,
            &body.channel,
            key,
            workspace,
        )
        .await
    };
    match result {
        Ok(models) => Json(json!({"models":models.into_iter().map(|(id,name)|json!({"model_id":id,"model_name":name})).collect::<Vec<_>>()})).into_response(),
        Err(e) => error(StatusCode::BAD_GATEWAY, &crate::crypto::credential_error_message(&e).unwrap_or_else(|| "model discovery failed; check account authentication and upstream connectivity".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tokio::sync::RwLock;

    #[test]
    fn subscription_origin_accepts_same_host_and_local_tools() {
        let mut headers = HeaderMap::new();
        assert!(check_origin(&headers));
        for host in ["localhost:10020", "localhost:3000", "[::1]:10020"] {
            headers.insert("host", host.parse().unwrap());
            headers.insert("origin", format!("http://{host}").parse().unwrap());
            assert!(check_origin(&headers));
        }
    }

    #[test]
    fn subscription_origin_rejects_cross_origin_and_malformed_values() {
        let mut headers = HeaderMap::new();
        headers.insert("host", "localhost:10020".parse().unwrap());
        for origin in [
            "https://example.com",
            "http://localhost:3000",
            "null",
            "file://localhost:10020",
            "http://localhost:10020/path",
            "http://localhost:10020?x=y",
            "http://user@localhost:10020",
            "http://localhost:10020#fragment",
        ] {
            headers.insert("origin", origin.parse().unwrap());
            assert!(!check_origin(&headers), "accepted {origin}");
        }
        headers.remove("host");
        headers.insert("origin", "http://localhost:10020".parse().unwrap());
        assert!(!check_origin(&headers));
    }

    async fn state() -> Arc<AppState> {
        let db = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        crate::db::schema::run_migrations(&db).await.unwrap();
        let defs = serde_json::from_value(json!([{
            "id":"sub", "name":"Subscription", "access_type":"subscription", "adapter":"openai_chatgpt",
            "channels":[{"type":"openai_responses", "base_url":"https://chatgpt.com/backend-api/codex"}]
        }])).unwrap();
        let client = reqwest::Client::new();
        Arc::new(AppState {
            subscription: Arc::new(
                crate::providers::openai_subscription::SubscriptionService::new(
                    db.clone(),
                    client.clone(),
                    None,
                ),
            ),
            admin_base_url: "http://localhost:10020".into(),
            updates: Arc::new(crate::update::Manager::default()),
            usage_tasks: tokio_util::task::TaskTracker::new(),
            request_log_enabled: RwLock::new(false),
            request_log_dir: std::env::temp_dir(),
            openai_chat_routes: Arc::new(RwLock::new(HashMap::new())),
            openai_responses_routes: Arc::new(RwLock::new(HashMap::new())),
            anthropic_routes: Arc::new(RwLock::new(HashMap::new())),
            provider_defs: defs,
            db,
            client,
            api_key_cache: Arc::new(RwLock::new(HashMap::new())),
            encryption_key: None,
            proxy_base_url: "http://localhost:10010".into(),
        })
    }

    async fn server() -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/api/admin", listener.local_addr().unwrap());
        let app = crate::router::create_admin_router(state().await);
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (base, task)
    }

    #[tokio::test]
    async fn subscription_router_rejects_cross_origin_for_every_new_endpoint() {
        let (base, task) = server().await;
        let client = reqwest::Client::new();
        for (method, path, body) in [
            (
                reqwest::Method::POST,
                "/providers/sub/subscription/login",
                json!({}),
            ),
            (
                reqwest::Method::GET,
                "/providers/sub/subscription/login/session",
                json!({}),
            ),
            (
                reqwest::Method::DELETE,
                "/providers/sub/subscription/login/session",
                json!({}),
            ),
            (
                reqwest::Method::POST,
                "/providers/sub/subscription/login/session/callback",
                json!({"url":"http://localhost:1455/auth/callback?code=secret"}),
            ),
            (
                reqwest::Method::DELETE,
                "/providers/sub/subscription/account",
                json!({}),
            ),
            (
                reqwest::Method::POST,
                "/providers/sub/models/query",
                json!({"channel":"openai_responses"}),
            ),
        ] {
            let response = client
                .request(method, format!("{base}{path}"))
                .header("origin", "https://attacker.example")
                .json(&body)
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::FORBIDDEN, "{path}");
            assert_eq!(
                response.json::<Value>().await.unwrap()["error"],
                "cross-origin admin request rejected"
            );
        }
        task.abort();
    }

    #[tokio::test]
    async fn subscription_router_exposes_safe_summary_and_encryption_requirement() {
        let (base, task) = server().await;
        let client = reqwest::Client::new();
        let response = client
            .get(format!("{base}/providers/sub"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let detail: Value = response.json().await.unwrap();
        assert_eq!(detail["access_type"], "subscription");
        assert_eq!(detail["auth"]["status"], "unconfigured");
        assert_eq!(detail["auth"]["login_available"], false);
        assert!(detail.get("api_key").is_none());
        assert!(detail.get("workspace_id").is_none());
        let response = client
            .post(format!("{base}/providers/sub/subscription/login"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert!(response.json::<Value>().await.unwrap()["error"]
            .as_str()
            .unwrap()
            .contains("encryption_key"));
        let response = client
            .delete(format!("{base}/providers/sub/subscription/account"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.json::<Value>().await.unwrap()["status"],
            "unconfigured"
        );
        task.abort();
    }

    #[tokio::test]
    async fn subscription_models_reject_all_api_key_credentials() {
        let (base, task) = server().await;
        let client = reqwest::Client::new();
        for field in ["api_key", "workspace_id"] {
            let mut body = json!({"channel":"openai_responses"});
            body[field] = json!("should-not-be-used");
            let response = client
                .post(format!("{base}/providers/sub/models/query"))
                .json(&body)
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{field}");
        }
        task.abort();
    }
}
