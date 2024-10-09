use std::error::Error;

use reqwest::header::{HeaderMap, AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;
use tokio::sync::Mutex;

const APP_ID: &str = "android-id-id";
const APP_SECRET: &str = "android-id-oYOAkxPCU46_E04WxtwfOYatrUI";

#[derive(Deserialize, Debug, Clone)]
pub struct Device {
    pub device: DeviceInfo,
}

#[derive(Deserialize, Debug, Clone)]
pub struct DeviceInfo {
    pub product_name: String,
    pub dsn: String,
    pub model: String,
    pub oem_model: String,
    pub sw_version: String,
}

#[derive(Deserialize, Debug)]
pub struct Property {
    pub property: PropertyInfo,
}

#[derive(Deserialize, Debug)]
pub struct PropertyInfo {
    pub name: String,
    pub base_type: String,
    pub read_only: bool,
    pub value: Value,
}

#[derive(Serialize)]
struct Datapoint {
    datapoint: DatapointValue,
}

#[derive(Serialize)]
struct DatapointValue {
    metadata: Value,
    value: Value,
}

#[derive(Deserialize, Debug, Clone)]
pub struct AuthResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: u64,
}

#[derive(Serialize)]
struct LoginRequest {
    user: LoginUser,
}

#[derive(Serialize)]
struct LoginUser {
    email: String,
    password: String,
    application: Application,
}

#[derive(Serialize)]
struct Application {
    app_id: String,
    app_secret: String,
}

pub struct Session {
    access_token: String,
    refresh_token: String,
    expires_at: u64, // Epoch time when the token expires
}

#[derive(Serialize)]
struct RefreshTokenRequest {
    user: RefreshTokenUser,
}

#[derive(Serialize)]
struct RefreshTokenUser {
    refresh_token: String,
}

fn get_current_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

pub async fn login(username: &str, password: &str) -> Result<Session, Box<dyn Error>> {
    let client = reqwest::Client::new();
    let url = "https://user-field.aylanetworks.com/users/sign_in.json";

    let login_request = LoginRequest {
        user: LoginUser {
            email: username.to_string(),
            password: password.to_string(),
            application: Application {
                app_id: APP_ID.to_string(),
                app_secret: APP_SECRET.to_string(),
            },
        },
    };

    let response = client
        .post(url)
        .header("Content-Type", "application/json")
        .json(&login_request)
        .send()
        .await?;

    if response.status().is_success() {
        let auth_response: AuthResponse = response.json().await?;
        let expires_at = get_current_epoch() + auth_response.expires_in;

        Ok(Session {
            access_token: auth_response.access_token,
            refresh_token: auth_response.refresh_token,
            expires_at,
        })
    } else {
        Err(format!("Login failed with status: {}", response.status()).into())
    }
}

pub async fn ensure_session_valid(session: &Arc<Mutex<Session>>) -> Result<(), Box<dyn Error>> {
    let mut tokens = session.lock().await;
    let current_time = get_current_epoch();

    if current_time >= tokens.expires_at {
        // Token has expired, refresh it
        refresh_token(&mut tokens).await?;
    }

    Ok(())
}

pub async fn refresh_token(session: &mut Session) -> Result<(), Box<dyn Error>> {
    let client = reqwest::Client::new();
    let url = "https://user-field.aylanetworks.com/users/refresh_token.json";

    let refresh_request = RefreshTokenRequest {
        user: RefreshTokenUser {
            refresh_token: session.refresh_token.clone(),
        },
    };

    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        format!("auth_token {}", session.access_token)
            .parse()
            .unwrap(),
    );
    headers.insert("Content-Type", "application/json".parse().unwrap());

    let response = client
        .post(url)
        .headers(headers)
        .json(&refresh_request)
        .send()
        .await?;

    if response.status().is_success() {
        let auth_response: AuthResponse = response.json().await?;
        let expires_at = get_current_epoch() + auth_response.expires_in;

        session.access_token = auth_response.access_token;
        session.refresh_token = auth_response.refresh_token;
        session.expires_at = expires_at;

        Ok(())
    } else {
        Err(format!("Token refresh failed with status: {}", response.status()).into())
    }
}

pub async fn get_auth_headers(session: &Arc<Mutex<Session>>) -> HeaderMap {
    let mut headers = HeaderMap::new();
    let session = session.lock().await;
    headers.insert(
        AUTHORIZATION,
        format!("auth_token {}", session.access_token)
            .parse()
            .unwrap(),
    );
    headers
}

pub async fn fetch_devices(
    session: &Arc<Mutex<Session>>,
) -> Result<Vec<DeviceInfo>, Box<dyn Error>> {
    ensure_session_valid(session).await?;
    let client = reqwest::Client::new();
    let response = client
        .get("https://ads-field.aylanetworks.com/apiv1/devices.json")
        .headers(get_auth_headers(session).await)
        .send()
        .await?;

    let devices: Vec<Device> = response.json().await?;
    Ok(devices.into_iter().map(|d| d.device).collect())
}

pub async fn fetch_device_properties(
    session: &Arc<Mutex<Session>>,
    dsn: &str,
) -> Result<Vec<PropertyInfo>, Box<dyn Error>> {
    let client = reqwest::Client::new();
    let url = format!(
        "https://ads-field.aylanetworks.com/apiv1/dsns/{}/properties.json",
        dsn
    );
    let response = client
        .get(&url)
        .headers(get_auth_headers(session).await)
        .send()
        .await?;

    let properties: Vec<Property> = response.json().await?;
    Ok(properties.into_iter().map(|p| p.property).collect())
}

pub async fn set_device_power_state(
    session: &Arc<Mutex<Session>>,
    dsn: &str,
    state: bool,
) -> Result<(), Box<dyn Error>> {
    let client = reqwest::Client::new();
    let url = format!(
        "https://ads-field.aylanetworks.com/apiv1/dsns/{}/properties/set_power_state/datapoints.json",
        dsn
    );
    let payload = Datapoint {
        datapoint: DatapointValue {
            metadata: Value::Object(serde_json::Map::new()),
            value: Value::from(if state { 1 } else { 0 }),
        },
    };

    let mut headers = get_auth_headers(session).await;
    headers.insert(CONTENT_TYPE, "application/json".parse().unwrap());

    client
        .post(&url)
        .headers(headers)
        .json(&payload)
        .send()
        .await?;

    Ok(())
}

pub async fn set_device_intensity(
    session: &Arc<Mutex<Session>>,
    dsn: &str,
    intensity: u8,
) -> Result<(), Box<dyn Error>> {
    if intensity < 1 || intensity > 5 {
        return Err(format!("Intensity must be between 1 and 5").into());
    }

    let client = reqwest::Client::new();
    let url = format!(
        "https://ads-field.aylanetworks.com/apiv1/dsns/{}/properties/set_intensity_manual/datapoints.json",
        dsn
    );
    let payload = Datapoint {
        datapoint: DatapointValue {
            metadata: Value::Object(serde_json::Map::new()),
            value: Value::from(intensity),
        },
    };

    let mut headers = get_auth_headers(session).await;
    headers.insert(CONTENT_TYPE, "application/json".parse().unwrap());

    client
        .post(&url)
        .headers(headers)
        .json(&payload)
        .send()
        .await?;

    Ok(())
}
