use std::{
    collections::HashSet,
    sync::{Arc, OnceLock},
    time::Duration,
};

use axum::{
    Json, Router,
    body::{Body, Bytes},
    extract::{Path, Query, Request, State, rejection::JsonRejection, ws::WebSocketUpgrade},
    http::{HeaderMap, HeaderValue, Method, Response, StatusCode, header},
    middleware::{self, Next},
    response::IntoResponse,
    routing::{delete, get, put},
};
use serde::{Deserialize, Serialize};
use tower_http::{cors::CorsLayer, set_header::SetResponseHeaderLayer, trace::TraceLayer};

use crate::{
    config::{ApplicationProfile, GameConfig, LocalApplication},
    drivers, platform,
    service::{BatteryReading, BridgeService},
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ApiResult {
    ok: bool,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AutostartPayload {
    enabled: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProfilesPayload {
    profiles: Vec<ApplicationProfile>,
}

#[derive(Debug, Deserialize)]
struct LocalApplicationsPayload {
    applications: Vec<LocalApplication>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeSettingsPayload {
    brand: String,
    dpi: Option<u32>,
    polling_rate_hz: Option<u32>,
}

pub fn router(service: BridgeService, origins: &[String]) -> Router {
    let allowed: Vec<HeaderValue> = origins
        .iter()
        .filter_map(|origin| origin.parse().ok())
        .collect();
    let request_origins = Arc::new(origins.iter().cloned().collect::<HashSet<_>>());
    let hid_origins = Arc::clone(&request_origins);
    let hid_route = Router::new().route(
        "/v1/hid",
        get(move |websocket, headers| hid_upgrade(websocket, headers, Arc::clone(&hid_origins))),
    );
    let cors = CorsLayer::new()
        .allow_origin(allowed)
        .allow_methods([Method::GET, Method::PUT, Method::DELETE])
        .allow_headers([axum::http::header::CONTENT_TYPE])
        .max_age(Duration::from_secs(3600));
    Router::new()
        .route("/v1/status", get(status))
        .route("/v1/handshake", put(handshake))
        .route("/v1/games", get(games))
        .route(
            "/v1/custom-games",
            get(custom_games).put(replace_custom_games),
        )
        .route(
            "/v1/custom-games/image",
            get(custom_game_image).put(set_custom_game_image),
        )
        .route("/v1/custom-games/image-from-url", get(image_from_link))
        .route(
            "/v1/local-applications",
            get(local_applications).put(replace_local_applications),
        )
        .route("/v1/applications", get(applications))
        .route("/v1/applications/{icon_id}/icon", get(application_icon))
        .route("/v1/running-applications", get(running_applications))
        .route("/v1/profiles", get(profiles).put(replace_profiles))
        .route(
            "/v1/default-profile",
            get(default_profile).put(set_default_profile),
        )
        .route("/v1/native/settings", put(apply_native_settings))
        .route("/v1/battery", put(record_battery))
        .route("/v1/battery/{device_id}", delete(forget_battery))
        .route("/v1/autostart", put(set_autostart))
        .route("/v1/panel", put(show_panel))
        .merge(hid_route)
        .layer(SetResponseHeaderLayer::if_not_present(
            axum::http::HeaderName::from_static("access-control-allow-private-network"),
            HeaderValue::from_static("true"),
        ))
        .layer(cors)
        .layer(middleware::from_fn(move |request: Request, next: Next| {
            request_gate(request, next, Arc::clone(&request_origins))
        }))
        .layer(TraceLayer::new_for_http())
        .with_state(service)
}

fn single_header(headers: &HeaderMap, name: header::HeaderName) -> Option<&str> {
    let mut values = headers.get_all(name).iter();
    let value = values.next()?.to_str().ok()?;
    if values.next().is_some() {
        return None;
    }
    Some(value)
}

fn loopback_authority(host: &str) -> bool {
    let Ok(authority) = host.parse::<axum::http::uri::Authority>() else {
        return false;
    };
    let name = authority.host();
    if !matches!(name, "localhost" | "127.0.0.1" | "[::1]") {
        return false;
    }
    if host == name {
        return true;
    }
    let Some(port) = host
        .strip_prefix(name)
        .and_then(|value| value.strip_prefix(':'))
    else {
        return false;
    };
    !port.is_empty()
        && port.bytes().all(|value| value.is_ascii_digit())
        && port.parse::<u16>().is_ok_and(|value| value != 0)
}

async fn request_gate(
    request: Request,
    next: Next,
    allowed: Arc<HashSet<String>>,
) -> axum::response::Response {
    let headers = request.headers();
    let Some(host) = single_header(headers, header::HOST).filter(|host| loopback_authority(host))
    else {
        return StatusCode::FORBIDDEN.into_response();
    };
    if request
        .uri()
        .authority()
        .is_some_and(|authority| authority.as_str() != host)
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    if headers.contains_key(header::ORIGIN)
        && !single_header(headers, header::ORIGIN)
            .is_some_and(|origin| origin != "null" && allowed.contains(origin))
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    // Covers every route, the HID WebSocket included (audit N3).
    #[cfg(target_os = "linux")]
    if !peer_is_this_user(
        request
            .extensions()
            .get::<axum::extract::ConnectInfo<crate::peer::Connection>>()
            .map(|info| info.0.clone()),
    )
    .await
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    next.run(request).await
}

/// Linux: the client socket must belong to this account, checked once per
/// connection and reused by its later requests. Requests without
/// connection info (a server not started through [`make_service`]) are
/// refused, except in this crate's unit tests, which drive the router
/// directly.
#[cfg(target_os = "linux")]
async fn peer_is_this_user(connection: Option<crate::peer::Connection>) -> bool {
    match connection {
        Some(connection) => connection.same_user().await,
        None => cfg!(test),
    }
}

/// The router as a service that records each connection's addresses, which
/// the request gate needs. Serve the API through this.
pub fn make_service(
    router: Router,
) -> axum::extract::connect_info::IntoMakeServiceWithConnectInfo<Router, crate::peer::Connection> {
    router.into_make_service_with_connect_info::<crate::peer::Connection>()
}

async fn hid_upgrade(
    websocket: WebSocketUpgrade,
    headers: HeaderMap,
    allowed_origins: Arc<HashSet<String>>,
) -> Result<Response<Body>, StatusCode> {
    let Some(origin) = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
    else {
        tracing::warn!("Rejected native HID WebSocket without an Origin header");
        return Err(StatusCode::FORBIDDEN);
    };
    if !allowed_origins.contains(origin) {
        tracing::warn!(%origin, "Rejected native HID WebSocket from an unapproved origin");
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(websocket
        .max_message_size(64 * 1024)
        .max_frame_size(64 * 1024)
        .on_upgrade(crate::hid::serve))
}

async fn status(State(service): State<BridgeService>) -> Json<crate::service::BridgeSnapshot> {
    Json(service.snapshot().await)
}

async fn show_panel(State(service): State<BridgeService>) -> Json<ApiResult> {
    service.request_show_panel();
    Json(ApiResult { ok: true })
}

async fn handshake(State(service): State<BridgeService>) -> Json<ApiResult> {
    service.record_client_heartbeat().await;
    Json(ApiResult { ok: true })
}

async fn applications(
    State(service): State<BridgeService>,
) -> Json<Vec<crate::applications::ApplicationInfo>> {
    Json(service.applications().await)
}

async fn running_applications(
    State(service): State<BridgeService>,
) -> Json<Vec<crate::applications::ApplicationInfo>> {
    Json(service.running_applications().await)
}

async fn application_icon(
    State(service): State<BridgeService>,
    Path(icon_id): Path<String>,
) -> Result<Response<Body>, StatusCode> {
    let icon = service
        .application_icon(&icon_id)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;
    Response::builder()
        .header(header::CONTENT_TYPE, "image/png")
        .header(header::CACHE_CONTROL, "private, max-age=86400")
        .body(Body::from(icon))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn profiles(State(service): State<BridgeService>) -> Json<Vec<ApplicationProfile>> {
    Json(service.profiles().await)
}

async fn games(State(service): State<BridgeService>) -> Json<Vec<serde_json::Value>> {
    Json(service.games().await)
}

async fn custom_games(State(service): State<BridgeService>) -> Json<Vec<GameConfig>> {
    Json(service.custom_games().await)
}

async fn replace_custom_games(
    State(service): State<BridgeService>,
    Json(games): Json<Vec<GameConfig>>,
) -> Result<Json<Vec<GameConfig>>, (StatusCode, String)> {
    service
        .set_custom_games(games)
        .await
        .map(Json)
        .map_err(internal_error)
}

#[derive(Debug, Deserialize)]
struct GameImageQuery {
    name: String,
}

async fn custom_game_image(
    State(service): State<BridgeService>,
    Query(query): Query<GameImageQuery>,
) -> Result<Response<Body>, StatusCode> {
    let (image, kind) = service
        .custom_game_image(&query.name)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;
    Response::builder()
        .header(header::CONTENT_TYPE, kind)
        // Revalidated every time: an image can be replaced under the same name.
        .header(header::CACHE_CONTROL, "private, no-cache")
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .body(Body::from(image))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

#[derive(Debug, Deserialize)]
struct ImageLinkQuery {
    url: String,
}

async fn image_from_link(
    Query(query): Query<ImageLinkQuery>,
) -> Result<Response<Body>, (StatusCode, String)> {
    let (image, kind) = crate::remote_image::fetch(&query.url)
        .await
        .map_err(|error| (StatusCode::BAD_REQUEST, error.to_string()))?;
    Response::builder()
        .header(header::CONTENT_TYPE, kind)
        .header(header::CACHE_CONTROL, "no-store")
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .body(Body::from(image))
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))
}

async fn set_custom_game_image(
    State(service): State<BridgeService>,
    Query(query): Query<GameImageQuery>,
    body: Bytes,
) -> Result<Json<ApiResult>, (StatusCode, String)> {
    service
        .set_custom_game_image(&query.name, &body)
        .await
        .map_err(|error| (StatusCode::BAD_REQUEST, error.to_string()))?;
    Ok(Json(ApiResult { ok: true }))
}

/// A saved local application as the API serves it, with the id its icon is
/// served under. The id is never read back from a saved list.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LocalApplicationEntry {
    #[serde(flatten)]
    application: LocalApplication,
    icon_id: String,
}

fn local_application_entries(applications: Vec<LocalApplication>) -> Vec<LocalApplicationEntry> {
    applications
        .into_iter()
        .map(|application| LocalApplicationEntry {
            icon_id: crate::applications::icon_id(&application.path),
            application,
        })
        .collect()
}

async fn local_applications(
    State(service): State<BridgeService>,
) -> Json<Vec<LocalApplicationEntry>> {
    Json(local_application_entries(
        service.local_applications().await,
    ))
}

async fn replace_local_applications(
    State(service): State<BridgeService>,
    payload: Result<Json<LocalApplicationsPayload>, JsonRejection>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let Json(payload) = payload.map_err(invalid_json)?;
    let applications = service
        .replace_local_applications(payload.applications)
        .await
        .map_err(profile_error)?;
    Ok(Json(
        serde_json::json!({"ok":true,"applications":local_application_entries(applications)}),
    ))
}

async fn replace_profiles(
    State(service): State<BridgeService>,
    payload: Result<Json<ProfilesPayload>, JsonRejection>,
) -> Result<Json<ApiResult>, (StatusCode, String)> {
    let Json(payload) = payload.map_err(invalid_json)?;
    service
        .replace_profiles(payload.profiles)
        .await
        .map_err(profile_error)?;
    Ok(Json(ApiResult { ok: true }))
}

async fn default_profile(State(service): State<BridgeService>) -> Json<Option<ApplicationProfile>> {
    Json(service.default_profile().await)
}

async fn set_default_profile(
    State(service): State<BridgeService>,
    payload: Result<Json<ApplicationProfile>, JsonRejection>,
) -> Result<Json<ApiResult>, (StatusCode, String)> {
    let Json(profile) = payload.map_err(invalid_json)?;
    service
        .set_default_profile(profile)
        .await
        .map_err(profile_error)?;
    Ok(Json(ApiResult { ok: true }))
}

async fn record_battery(
    State(service): State<BridgeService>,
    Json(reading): Json<BatteryReading>,
) -> Result<Json<ApiResult>, (StatusCode, String)> {
    service
        .record_panel_battery(reading)
        .await
        .map_err(profile_error)?;
    Ok(Json(ApiResult { ok: true }))
}

async fn forget_battery(
    State(service): State<BridgeService>,
    Path(device_id): Path<String>,
) -> Json<ApiResult> {
    service.forget_battery(&device_id).await;
    Json(ApiResult { ok: true })
}

async fn set_autostart(
    Json(payload): Json<AutostartPayload>,
) -> Result<Json<ApiResult>, (StatusCode, String)> {
    platform::set_autostart(payload.enabled).map_err(internal_error)?;
    Ok(Json(ApiResult { ok: true }))
}

static NATIVE_SETTINGS_ADMISSION: OnceLock<Arc<tokio::sync::Semaphore>> = OnceLock::new();

async fn apply_native_settings(
    Json(payload): Json<NativeSettingsPayload>,
) -> Result<Json<ApiResult>, (StatusCode, String)> {
    if payload.dpi.is_none() && payload.polling_rate_hz.is_none() {
        return Err((
            StatusCode::BAD_REQUEST,
            "at least one of dpi or pollingRateHz is required".into(),
        ));
    }

    let permit = NATIVE_SETTINGS_ADMISSION
        .get_or_init(|| Arc::new(tokio::sync::Semaphore::new(1)))
        .clone()
        .try_acquire_owned()
        .map_err(|_| {
            (
                StatusCode::TOO_MANY_REQUESTS,
                "native settings are busy".into(),
            )
        })?;
    if crate::hid::client_session_active() {
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            "native HID is owned by an active WebSocket session".into(),
        ));
    }
    let NativeSettingsPayload {
        brand,
        dpi,
        polling_rate_hz,
    } = payload;
    let error_brand = brand.clone();
    let applied = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        drivers::apply_settings(&brand, dpi, polling_rate_hz)
    })
    .await
    .map_err(|error| internal_error(anyhow::anyhow!(error)))?
    .map_err(|error| {
        if error
            .downcast_ref::<std::io::Error>()
            .is_some_and(|error| error.kind() == std::io::ErrorKind::WouldBlock)
        {
            (StatusCode::TOO_MANY_REQUESTS, error.to_string())
        } else {
            internal_error(error)
        }
    })?;

    if !applied {
        return Err((
            StatusCode::NOT_FOUND,
            format!("no native driver is registered for {error_brand}"),
        ));
    }
    Ok(Json(ApiResult { ok: true }))
}

fn invalid_json(error: JsonRejection) -> (StatusCode, String) {
    (StatusCode::BAD_REQUEST, error.body_text())
}

fn profile_error(error: anyhow::Error) -> (StatusCode, String) {
    if error
        .downcast_ref::<crate::local_targets::InvalidInput>()
        .is_some()
    {
        (StatusCode::BAD_REQUEST, error.to_string())
    } else {
        internal_error(error)
    }
}

fn internal_error(error: anyhow::Error) -> (StatusCode, String) {
    tracing::error!(%error, "Bridge request failed");
    (StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
}

#[cfg(test)]
mod tests {
    use axum::{
        body::to_bytes,
        http::{Method, Request},
    };
    use tower::ServiceExt;

    use super::*;
    use crate::{config::BridgeConfig, remote_image::PNG_SIGNATURE};

    fn app() -> (Router, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "bridge-api-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let service = BridgeService::new(BridgeConfig::default(), dir.join("config.json"));
        (router(service, &[]), dir)
    }

    async fn call(
        app: &Router,
        method: Method,
        uri: &str,
        content_type: &str,
        body: Vec<u8>,
    ) -> (StatusCode, Vec<u8>, Option<String>) {
        let request = Request::builder()
            .method(method)
            .uri(uri)
            // The request gate only serves loopback hosts.
            .header(header::HOST, "127.0.0.1:43821")
            .header(header::CONTENT_TYPE, content_type)
            .body(Body::from(body))
            .expect("request");
        let response = app.clone().oneshot(request).await.expect("response");
        let status = response.status();
        let kind = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body")
            .to_vec();
        (status, bytes, kind)
    }

    /// Linux: a request whose client socket is not ours (here: not in
    /// /proc/net/tcp at all) is refused, HID WebSocket included.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn unknown_peer_socket_is_refused() {
        let (app, dir) = app();
        for uri in ["/v1/status", "/v1/hid"] {
            // A fresh connection per route, so each one reads the table.
            let unknown = crate::peer::Connection::new(
                std::net::SocketAddr::from(([127, 0, 0, 1], 1)),
                Some(std::net::SocketAddr::from(([127, 0, 0, 1], 9))),
            );
            let request = Request::builder()
                .uri(uri)
                .header(header::HOST, "127.0.0.1:17846")
                .extension(axum::extract::ConnectInfo(unknown))
                .body(Body::empty())
                .expect("request");
            let response = app.clone().oneshot(request).await.expect("response");
            assert_eq!(response.status(), StatusCode::FORBIDDEN, "{uri}");
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn saved_local_applications_carry_their_icon_id() {
        let (app, dir) = app();
        let path = r"C:\Apps\Editor.exe";
        let icon_id = crate::applications::icon_id(path);
        let entry = serde_json::json!({"id":"local:editor","name":"Editor","executable":"Editor.exe","path":path,"kind":"app"});
        let (status, saved, _) = call(
            &app,
            Method::PUT,
            "/v1/local-applications",
            "application/json",
            serde_json::to_vec(&serde_json::json!({"applications":[entry]})).unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let saved: serde_json::Value = serde_json::from_slice(&saved).unwrap();
        assert_eq!(saved["applications"][0]["iconId"], icon_id.as_str());

        let (status, listed, _) =
            call(&app, Method::GET, "/v1/local-applications", "", vec![]).await;
        assert_eq!(status, StatusCode::OK);
        let listed: serde_json::Value = serde_json::from_slice(&listed).unwrap();
        assert_eq!(listed[0]["iconId"], icon_id.as_str());

        // A client may send back the list it was served, icon ids included.
        let (status, ..) = call(
            &app,
            Method::PUT,
            "/v1/local-applications",
            "application/json",
            serde_json::to_vec(&serde_json::json!({"applications":listed})).unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[tokio::test]
    async fn a_custom_game_keeps_the_image_saved_for_it() {
        let (app, dir) = app();
        let games = br#"[{"name":"My Game","executables":["MyGame.exe"]}]"#.to_vec();
        let (status, saved, _) = call(
            &app,
            Method::PUT,
            "/v1/custom-games",
            "application/json",
            games,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            String::from_utf8_lossy(&saved).contains("mygame.exe"),
            "the list comes back normalized"
        );

        let mut png = PNG_SIGNATURE.to_vec();
        png.extend_from_slice(&[1, 2, 3]);
        let (status, ..) = call(
            &app,
            Method::PUT,
            "/v1/custom-games/image?name=My%20Game",
            "image/png",
            png.clone(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (status, bytes, kind) = call(
            &app,
            Method::GET,
            "/v1/custom-games/image?name=my%20game",
            "",
            vec![],
        )
        .await;
        assert_eq!(
            (status, bytes, kind.as_deref()),
            (StatusCode::OK, png, Some("image/png"))
        );
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[tokio::test]
    async fn an_image_is_refused_for_an_unknown_game_or_a_file_that_is_not_an_image() {
        let (app, dir) = app();
        let (status, ..) = call(
            &app,
            Method::PUT,
            "/v1/custom-games/image?name=Nope",
            "image/png",
            PNG_SIGNATURE.to_vec(),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "the game must exist");
        let games = br#"[{"name":"Game","executables":["game.exe"]}]"#.to_vec();
        call(
            &app,
            Method::PUT,
            "/v1/custom-games",
            "application/json",
            games,
        )
        .await;
        let (status, ..) = call(
            &app,
            Method::PUT,
            "/v1/custom-games/image?name=Game",
            "image/png",
            b"<html>".to_vec(),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "only real images are kept");
        let (status, ..) = call(
            &app,
            Method::GET,
            "/v1/custom-games/image?name=Game",
            "",
            vec![],
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[tokio::test]
    async fn a_link_into_the_local_network_is_refused() {
        let (app, dir) = app();
        let (status, message, _) = call(
            &app,
            Method::GET,
            "/v1/custom-games/image-from-url?url=http%3A%2F%2F192.168.1.1%2Flogo.png",
            "",
            vec![],
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(String::from_utf8_lossy(&message).contains("cannot be reached"));
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    fn profile_with(buttons: serde_json::Value) -> serde_json::Value {
        serde_json::json!({
            "application": {"name": "Apex", "executable": "r5apex.exe", "path": ""},
            "device": {"id": "Logitech:PRO X", "name": "PRO X"},
            "settings": {"dpi": null, "pollingRateHz": null, "snapshot": {"hostButtons": buttons}},
        })
    }

    /// Assignments from a web page become key presses. The panel sends every
    /// profile at once and may record keys this OS cannot press, so a save
    /// with entries off Bridge's allow-list still goes through (refusing it
    /// would lose the whole save); `Assignments::from_profile` never presses
    /// them.
    #[tokio::test]
    async fn profiles_with_assignments_off_the_allow_list_still_save() {
        let (app, dir) = app();
        for buttons in [
            serde_json::json!({"8": {"key": 4, "keys": [5, 6, 7]}}),
            serde_json::json!({"8": {"text": "hello"}}),
            serde_json::json!({"16": {"key": 4, "repeat": 100}}),
            serde_json::json!({"8": {"key": 255}}),
            serde_json::json!({"8": {"consumer": 0x194}}),
            serde_json::json!({"4": {"key": 4}}),
        ] {
            let (status, message, _) = call(
                &app,
                Method::PUT,
                "/v1/profiles",
                "application/json",
                serde_json::to_vec(
                    &serde_json::json!({"profiles": [profile_with(buttons.clone())]}),
                )
                .unwrap(),
            )
            .await;
            assert_eq!(
                status,
                StatusCode::OK,
                "{buttons}: {}",
                String::from_utf8_lossy(&message)
            );
            let (status, ..) = call(
                &app,
                Method::PUT,
                "/v1/default-profile",
                "application/json",
                serde_json::to_vec(&profile_with(buttons.clone())).unwrap(),
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{buttons}");
        }

        let good = profile_with(
            serde_json::json!({"8": {"key": 0x68, "modifiers": 3}, "16": {"consumer": 233}}),
        );
        let (status, ..) = call(
            &app,
            Method::PUT,
            "/v1/profiles",
            "application/json",
            serde_json::to_vec(&serde_json::json!({"profiles": [good.clone()]})).unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (status, ..) = call(
            &app,
            Method::PUT,
            "/v1/default-profile",
            "application/json",
            serde_json::to_vec(&good).unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let _ = std::fs::remove_dir_all(dir);
    }
}
