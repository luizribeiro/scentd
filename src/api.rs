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

// API client with configurable base URLs for testing
pub struct ApiClient {
    user_base_url: String,
    device_base_url: String,
}

impl Default for ApiClient {
    fn default() -> Self {
        Self {
            user_base_url: "https://user-field.aylanetworks.com".to_string(),
            device_base_url: "https://ads-field.aylanetworks.com".to_string(),
        }
    }
}

impl ApiClient {
    #[cfg(test)]
    pub fn new(user_base_url: String, device_base_url: String) -> Self {
        Self {
            user_base_url,
            device_base_url,
        }
    }
}

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

#[derive(Debug)]
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

impl ApiClient {
    pub async fn login(&self, username: &str, password: &str) -> Result<Session, Box<dyn Error>> {
        let client = reqwest::Client::new();
        let url = format!("{}/users/sign_in.json", self.user_base_url);

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
            .post(&url)
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
}

// Keep the old function for backwards compatibility
pub async fn login(username: &str, password: &str) -> Result<Session, Box<dyn Error>> {
    let client = ApiClient::default();
    client.login(username, password).await
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

impl ApiClient {
    pub async fn refresh_token(&self, session: &mut Session) -> Result<(), Box<dyn Error>> {
        let client = reqwest::Client::new();
        let url = format!("{}/users/refresh_token.json", self.user_base_url);

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
            .post(&url)
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
}

pub async fn refresh_token(session: &mut Session) -> Result<(), Box<dyn Error>> {
    let client = ApiClient::default();
    client.refresh_token(session).await
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

impl ApiClient {
    pub async fn fetch_devices(
        &self,
        session: &Arc<Mutex<Session>>,
    ) -> Result<Vec<DeviceInfo>, Box<dyn Error>> {
        ensure_session_valid(session).await?;
        let client = reqwest::Client::new();
        let url = format!("{}/apiv1/devices.json", self.device_base_url);
        let response = client
            .get(&url)
            .headers(get_auth_headers(session).await)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(format!("Failed to fetch devices: {}", response.status()).into());
        }

        let devices: Vec<Device> = response.json().await?;
        Ok(devices.into_iter().map(|d| d.device).collect())
    }
}

pub async fn fetch_devices(
    session: &Arc<Mutex<Session>>,
) -> Result<Vec<DeviceInfo>, Box<dyn Error>> {
    let client = ApiClient::default();
    client.fetch_devices(session).await
}

impl ApiClient {
    pub async fn fetch_device_properties(
        &self,
        session: &Arc<Mutex<Session>>,
        dsn: &str,
    ) -> Result<Vec<PropertyInfo>, Box<dyn Error>> {
        ensure_session_valid(session).await?;
        let client = reqwest::Client::new();
        let url = format!(
            "{}/apiv1/dsns/{}/properties.json",
            self.device_base_url, dsn
        );
        let response = client
            .get(&url)
            .headers(get_auth_headers(session).await)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(format!("Failed to fetch device properties for {}: {}", dsn, response.status()).into());
        }

        let properties: Vec<Property> = response.json().await?;
        Ok(properties.into_iter().map(|p| p.property).collect())
    }
}

pub async fn fetch_device_properties(
    session: &Arc<Mutex<Session>>,
    dsn: &str,
) -> Result<Vec<PropertyInfo>, Box<dyn Error>> {
    let client = ApiClient::default();
    client.fetch_device_properties(session, dsn).await
}

impl ApiClient {
    pub async fn set_device_power_state(
        &self,
        session: &Arc<Mutex<Session>>,
        dsn: &str,
        state: bool,
    ) -> Result<(), Box<dyn Error>> {
        ensure_session_valid(session).await?;
        let client = reqwest::Client::new();
        let url = format!(
            "{}/apiv1/dsns/{}/properties/set_power_state/datapoints.json",
            self.device_base_url, dsn
        );
        let payload = Datapoint {
            datapoint: DatapointValue {
                metadata: Value::Object(serde_json::Map::new()),
                value: Value::from(if state { 1 } else { 0 }),
            },
        };

        let mut headers = get_auth_headers(session).await;
        headers.insert(CONTENT_TYPE, "application/json".parse().unwrap());

        let response = client
            .post(&url)
            .headers(headers)
            .json(&payload)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(format!("Failed to set power state for {}: {}", dsn, response.status()).into());
        }

        Ok(())
    }
}

pub async fn set_device_power_state(
    session: &Arc<Mutex<Session>>,
    dsn: &str,
    state: bool,
) -> Result<(), Box<dyn Error>> {
    let client = ApiClient::default();
    client.set_device_power_state(session, dsn, state).await
}

impl ApiClient {
    pub async fn set_device_intensity(
        &self,
        session: &Arc<Mutex<Session>>,
        dsn: &str,
        intensity: u8,
    ) -> Result<(), Box<dyn Error>> {
        if intensity < 1 || intensity > 5 {
            return Err(format!("Intensity must be between 1 and 5").into());
        }

        ensure_session_valid(session).await?;
        let client = reqwest::Client::new();
        let url = format!(
            "{}/apiv1/dsns/{}/properties/set_intensity_manual/datapoints.json",
            self.device_base_url, dsn
        );
        let payload = Datapoint {
            datapoint: DatapointValue {
                metadata: Value::Object(serde_json::Map::new()),
                value: Value::from(intensity),
            },
        };

        let mut headers = get_auth_headers(session).await;
        headers.insert(CONTENT_TYPE, "application/json".parse().unwrap());

        let response = client
            .post(&url)
            .headers(headers)
            .json(&payload)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(format!("Failed to set intensity for {}: {}", dsn, response.status()).into());
        }

        Ok(())
    }
}

pub async fn set_device_intensity(
    session: &Arc<Mutex<Session>>,
    dsn: &str,
    intensity: u8,
) -> Result<(), Box<dyn Error>> {
    let client = ApiClient::default();
    client.set_device_intensity(session, dsn, intensity).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::{Server, ServerGuard};
    use std::sync::Arc;
    use tokio::sync::Mutex;

    async fn create_mock_server() -> ServerGuard {
        Server::new_async().await
    }

    fn create_test_session(expires_at: u64) -> Session {
        Session {
            access_token: "test_access_token".to_string(),
            refresh_token: "test_refresh_token".to_string(),
            expires_at,
        }
    }

    #[tokio::test]
    async fn test_login_success() {
        let mut server = create_mock_server().await;

        let _mock = server.mock("POST", "/users/sign_in.json")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{
                "access_token": "new_access_token",
                "refresh_token": "new_refresh_token",
                "expires_in": 3600
            }"#)
            .create_async()
            .await;

        let client = ApiClient::new(server.url(), server.url());
        let result = client.login("test@example.com", "password123").await;

        assert!(result.is_ok());
        let session = result.unwrap();
        assert_eq!(session.access_token, "new_access_token");
        assert_eq!(session.refresh_token, "new_refresh_token");
        assert!(session.expires_at > get_current_epoch());
    }

    #[tokio::test]
    async fn test_login_failure() {
        let mut server = create_mock_server().await;

        let _mock = server.mock("POST", "/users/sign_in.json")
            .with_status(401)
            .with_body("Unauthorized")
            .create_async()
            .await;

        let client = ApiClient::new(server.url(), server.url());
        let result = client.login("test@example.com", "wrong_password").await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Login failed"));
    }

    #[tokio::test]
    async fn test_refresh_token_success() {
        let mut server = create_mock_server().await;

        let _mock = server.mock("POST", "/users/refresh_token.json")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{
                "access_token": "refreshed_access_token",
                "refresh_token": "refreshed_refresh_token",
                "expires_in": 7200
            }"#)
            .create_async()
            .await;

        let client = ApiClient::new(server.url(), server.url());
        let mut session = create_test_session(get_current_epoch() - 1);

        let result = client.refresh_token(&mut session).await;

        assert!(result.is_ok());
        assert_eq!(session.access_token, "refreshed_access_token");
        assert_eq!(session.refresh_token, "refreshed_refresh_token");
        assert!(session.expires_at > get_current_epoch());
    }

    #[tokio::test]
    async fn test_fetch_devices_success() {
        let mut server = create_mock_server().await;

        let _mock = server.mock("GET", "/apiv1/devices.json")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"[
                {
                    "device": {
                        "product_name": "Test Device 1",
                        "dsn": "DSN001",
                        "model": "MODEL_1",
                        "oem_model": "OEM_1",
                        "sw_version": "1.0"
                    }
                },
                {
                    "device": {
                        "product_name": "Test Device 2",
                        "dsn": "DSN002",
                        "model": "MODEL_2",
                        "oem_model": "OEM_2",
                        "sw_version": "2.0"
                    }
                }
            ]"#)
            .create_async()
            .await;

        let client = ApiClient::new(server.url(), server.url());
        let session = Arc::new(Mutex::new(create_test_session(get_current_epoch() + 3600)));

        let result = client.fetch_devices(&session).await;

        assert!(result.is_ok());
        let devices = result.unwrap();
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].product_name, "Test Device 1");
        assert_eq!(devices[0].dsn, "DSN001");
        assert_eq!(devices[1].product_name, "Test Device 2");
    }

    #[tokio::test]
    async fn test_fetch_device_properties_success() {
        let mut server = create_mock_server().await;

        let _mock = server.mock("GET", "/apiv1/dsns/DSN123/properties.json")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"[
                {
                    "property": {
                        "name": "set_power_state",
                        "base_type": "integer",
                        "read_only": false,
                        "value": 1
                    }
                },
                {
                    "property": {
                        "name": "set_intensity_manual",
                        "base_type": "integer",
                        "read_only": false,
                        "value": 3
                    }
                }
            ]"#)
            .create_async()
            .await;

        let client = ApiClient::new(server.url(), server.url());
        let session = Arc::new(Mutex::new(create_test_session(get_current_epoch() + 3600)));

        let result = client.fetch_device_properties(&session, "DSN123").await;

        assert!(result.is_ok());
        let properties = result.unwrap();
        assert_eq!(properties.len(), 2);
        assert_eq!(properties[0].name, "set_power_state");
        assert_eq!(properties[0].value.as_u64().unwrap(), 1);
        assert_eq!(properties[1].name, "set_intensity_manual");
        assert_eq!(properties[1].value.as_u64().unwrap(), 3);
    }

    #[tokio::test]
    async fn test_set_device_power_state_success() {
        let mut server = create_mock_server().await;

        let _mock = server.mock("POST", "/apiv1/dsns/DSN123/properties/set_power_state/datapoints.json")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body("{}")
            .create_async()
            .await;

        let client = ApiClient::new(server.url(), server.url());
        let session = Arc::new(Mutex::new(create_test_session(get_current_epoch() + 3600)));

        let result = client.set_device_power_state(&session, "DSN123", true).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_set_device_intensity_success() {
        let mut server = create_mock_server().await;

        let _mock = server.mock("POST", "/apiv1/dsns/DSN123/properties/set_intensity_manual/datapoints.json")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body("{}")
            .create_async()
            .await;

        let client = ApiClient::new(server.url(), server.url());
        let session = Arc::new(Mutex::new(create_test_session(get_current_epoch() + 3600)));

        let result = client.set_device_intensity(&session, "DSN123", 3).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_set_device_intensity_validates_range() {
        let client = ApiClient::default();
        let session = Arc::new(Mutex::new(create_test_session(get_current_epoch() + 3600)));

        // Test intensity below valid range
        let result = client.set_device_intensity(&session, "DSN123", 0).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("must be between 1 and 5"));

        // Test intensity above valid range
        let result = client.set_device_intensity(&session, "DSN123", 6).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("must be between 1 and 5"));
    }

    #[tokio::test]
    async fn test_get_current_epoch() {
        let epoch = get_current_epoch();
        // Should be a reasonable unix timestamp (after 2020)
        assert!(epoch > 1577836800); // Jan 1, 2020
    }

    #[tokio::test]
    async fn test_ensure_session_valid_with_valid_token() {
        // Create a session that won't expire for a long time
        let future_time = get_current_epoch() + 3600; // 1 hour from now
        let session = Arc::new(Mutex::new(create_test_session(future_time)));

        let result = ensure_session_valid(&session).await;

        // Should succeed without refreshing
        assert!(result.is_ok());

        // Token should remain unchanged
        let session_guard = session.lock().await;
        assert_eq!(session_guard.access_token, "test_access_token");
        assert_eq!(session_guard.refresh_token, "test_refresh_token");
    }

    #[tokio::test]
    async fn test_session_creation() {
        let session = create_test_session(12345);
        assert_eq!(session.access_token, "test_access_token");
        assert_eq!(session.refresh_token, "test_refresh_token");
        assert_eq!(session.expires_at, 12345);
    }

    #[tokio::test]
    async fn test_get_auth_headers() {
        let session = Arc::new(Mutex::new(create_test_session(12345)));
        let headers = get_auth_headers(&session).await;

        let auth_header = headers.get(AUTHORIZATION).unwrap();
        assert_eq!(auth_header, "auth_token test_access_token");
    }
}
