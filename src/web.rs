use crate::config::Config;
use axum::{
    Json, Router,
    extract::{FromRequestParts, State},
    http::{StatusCode, request::Parts},
    response::Html,
    routing::{get, post},
};
use axum_extra::extract::cookie::{Cookie, CookieJar};
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_http::services::ServeDir;
use tracing::info;

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    exp: usize,
}

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<RwLock<Config>>,
    pub jwt_secret: String,
}

struct AuthExtractor;

impl FromRequestParts<AppState> for AuthExtractor {
    type Rejection = StatusCode;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let cookie_jar = CookieJar::from_request_parts(parts, state)
            .await
            .map_err(|_| StatusCode::UNAUTHORIZED)?;

        let token = cookie_jar
            .get("token")
            .map(|cookie| cookie.value())
            .ok_or(StatusCode::UNAUTHORIZED)?;

        let decoding_key = DecodingKey::from_secret(state.jwt_secret.as_bytes());
        let validation = Validation::default();

        decode::<Claims>(token, &decoding_key, &validation)
            .map_err(|_| StatusCode::UNAUTHORIZED)?;

        Ok(AuthExtractor)
    }
}

pub async fn start_web_server(state: AppState) {
    let app = Router::new()
        .route("/", get(index))
        .route("/api/login", post(login))
        .route("/api/config", get(get_config))
        .route("/api/config", post(update_config))
        .route("/api/change-password", post(change_password))
        .route("/api/logout", post(logout_handler))
        .nest_service("/static", ServeDir::new("static"))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    info!("Web server listening on http://0.0.0.0:3000");
    axum::serve(listener, app).await.unwrap();
}

async fn index() -> Html<&'static str> {
    Html(include_str!("../static/index.html"))
}

#[derive(Deserialize)]
struct LoginRequest {
    password: String,
}

async fn login(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(payload): Json<LoginRequest>,
) -> Result<(CookieJar, StatusCode), StatusCode> {
    if Config::verify_password(&payload.password) {
        let expiration = chrono::Utc::now()
            .checked_add_signed(chrono::Duration::hours(24))
            .expect("valid timestamp")
            .timestamp() as usize;

        let claims = Claims { exp: expiration };
        let token = encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(state.jwt_secret.as_bytes()),
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        let cookie = Cookie::build(("token", token))
            .path("/")
            .http_only(true)
            .same_site(axum_extra::extract::cookie::SameSite::Lax)
            .build();

        Ok((jar.add(cookie), StatusCode::OK))
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

async fn logout_handler(jar: CookieJar) -> (CookieJar, StatusCode) {
    let cookie = Cookie::build(("token", ""))
        .path("/")
        .http_only(true)
        .same_site(axum_extra::extract::cookie::SameSite::Lax)
        .build();
    (jar.remove(cookie), StatusCode::OK)
}

#[derive(Deserialize)]
struct ChangePasswordRequest {
    new_password: String,
}

async fn change_password(
    _auth: AuthExtractor,
    Json(payload): Json<ChangePasswordRequest>,
) -> StatusCode {
    if payload.new_password.is_empty() {
        return StatusCode::BAD_REQUEST;
    }

    match Config::set_password(&payload.new_password) {
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

async fn get_config(_auth: AuthExtractor, State(state): State<AppState>) -> Json<Config> {
    let config = state.config.read().await;
    Json(config.clone())
}

async fn update_config(
    _auth: AuthExtractor,
    State(state): State<AppState>,
    Json(new_config): Json<Config>,
) -> Json<Config> {
    let mut config = state.config.write().await;

    *config = new_config.clone();
    if let Err(e) = config.save() {
        tracing::error!("Failed to save config: {}", e);
    } else {
        info!("Configuration updated and saved to file.");
    }
    Json(config.clone())
}
