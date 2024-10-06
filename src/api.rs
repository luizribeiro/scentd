use reqwest::header::{HeaderMap, AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::env;
use std::error::Error;

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

fn get_auth_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    let token = env::var("AERA_TOKEN").expect("AERA_TOKEN not set");
    headers.insert(
        AUTHORIZATION,
        format!("auth_token {}", token).parse().unwrap(),
    );
    headers
}

pub async fn fetch_devices() -> Result<Vec<DeviceInfo>, Box<dyn Error>> {
    let client = reqwest::Client::new();
    let response = client
        .get("https://ads-field.aylanetworks.com/apiv1/devices.json")
        .headers(get_auth_headers())
        .send()
        .await?;

    let devices: Vec<Device> = response.json().await?;
    Ok(devices.into_iter().map(|d| d.device).collect())
}

pub async fn fetch_device_properties(dsn: &str) -> Result<Vec<PropertyInfo>, Box<dyn Error>> {
    let client = reqwest::Client::new();
    let url = format!(
        "https://ads-field.aylanetworks.com/apiv1/dsns/{}/properties.json",
        dsn
    );
    let response = client.get(&url).headers(get_auth_headers()).send().await?;

    let properties: Vec<Property> = response.json().await?;
    Ok(properties.into_iter().map(|p| p.property).collect())
}

pub async fn set_device_power_state(dsn: &str, state: bool) -> Result<(), Box<dyn Error>> {
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

    let mut headers = get_auth_headers();
    headers.insert(CONTENT_TYPE, "application/json".parse().unwrap());

    client
        .post(&url)
        .headers(headers)
        .json(&payload)
        .send()
        .await?;

    Ok(())
}

pub async fn set_device_intensity(dsn: &str, intensity: u8) -> Result<(), Box<dyn Error>> {
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

    let mut headers = get_auth_headers();
    headers.insert(CONTENT_TYPE, "application/json".parse().unwrap());

    client
        .post(&url)
        .headers(headers)
        .json(&payload)
        .send()
        .await?;

    Ok(())
}
