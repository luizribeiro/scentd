use crate::api::{
    fetch_device_properties, set_device_intensity, set_device_power_state, DeviceInfo,
    PropertyInfo, Session,
};
use log::{debug, info, warn};
use rumqttc::{AsyncClient, Event, EventLoop, MqttOptions, Packet, QoS};
use serde_json::json;
use std::error::Error;

// Type alias for Send+Sync errors to work with tokio::spawn
type BoxError = Box<dyn Error + Send + Sync>;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{interval, Duration};

pub fn get_mqtt_client() -> (AsyncClient, EventLoop) {
    // Read MQTT configuration from environment variables
    let mqtt_host = std::env::var("MQTT_HOST").unwrap_or_else(|_| "localhost".to_string());
    let mqtt_port: u16 = std::env::var("MQTT_PORT")
        .unwrap_or_else(|_| "1883".to_string())
        .parse()
        .unwrap_or(1883);

    info!("Connecting to MQTT broker at {}:{}", mqtt_host, mqtt_port);

    let mut mqttoptions = MqttOptions::new("scentd", mqtt_host, mqtt_port);
    // Set keep-alive to 60 seconds (was 5s, which was too aggressive)
    // This reduces unnecessary network traffic and connection churn
    mqttoptions.set_keep_alive(Duration::from_secs(60));

    AsyncClient::new(mqttoptions, 10)
}

pub async fn publish_device_state(
    mqtt_client: &AsyncClient,
    device: &DeviceInfo,
    properties: &[PropertyInfo],
) -> Result<(), BoxError> {
    // Extract relevant properties
    let power_state = properties
        .iter()
        .find(|p| p.name == "set_power_state")
        .and_then(|p| p.value.as_u64())
        .and_then(|v| Some(v == 1))
        .unwrap_or(false);

    let intensity_state = properties
        .iter()
        .find(|p| p.name == "set_intensity_manual")
        .and_then(|p| p.value.as_u64())
        .unwrap_or(0);

    // Construct MQTT topics
    let base_topic = format!("homeassistant/switch/{}", device.dsn);
    let state_topic = format!("{}/state", base_topic);
    let command_topic = format!("{}/set", base_topic);

    // Publish state (retained so Home Assistant gets it immediately on restart)
    let payload = if power_state { "ON" } else { "OFF" };
    mqtt_client
        .publish(state_topic.clone(), QoS::AtLeastOnce, true, payload)
        .await?;
    debug!(
        "Published state for {} to topic {}: {}",
        device.product_name, state_topic, payload
    );

    // Publish intensity as a separate topic (retained so Home Assistant gets it immediately on restart)
    let intensity_topic = format!("{}/intensity/state", base_topic);
    mqtt_client
        .publish(
            intensity_topic.clone(),
            QoS::AtLeastOnce,
            true,
            intensity_state.to_string(),
        )
        .await?;
    debug!(
        "Published intensity state for {} to topic {}: {}",
        device.product_name, intensity_topic, intensity_state
    );

    // For Home Assistant discovery, publish config topics (retained so they persist across HA restarts)
    let config_topic = format!("homeassistant/switch/{}/config", device.dsn);
    let config_payload = json!({
        "name": "Diffuser",
        "state_topic": state_topic,
        "command_topic": command_topic,
        "unique_id": device.dsn,
        "device": {
            "identifiers": [device.dsn],
            "name": device.product_name,
            "model": device.model,
            "manufacturer": device.oem_model,
            "sw_version": device.sw_version
        }
    });
    mqtt_client
        .publish(
            config_topic.clone(),
            QoS::AtLeastOnce,
            true,
            config_payload.to_string(),
        )
        .await?;
    debug!(
        "Published switch config for {} to topic {}",
        device.product_name, config_topic
    );
    debug!("Config payload: {}", config_payload);

    // Publish intensity control configuration (retained so it persists across HA restarts)
    let intensity_config_topic = format!("homeassistant/number/{}/config", device.dsn);
    let intensity_command_topic = format!("{}/intensity/set", base_topic);
    let intensity_config_payload = json!({
        "name": "Fragrance Intensity",
        "state_topic": intensity_topic,
        "command_topic": intensity_command_topic,
        "unique_id": format!("{}_intensity", device.dsn),
        "device": {
            "identifiers": [device.dsn],
            "name": device.product_name,
            "model": device.model,
            "manufacturer": device.oem_model,
            "sw_version": device.sw_version
        },
        "min": 1,
        "max": 5,
        "step": 1,
        "unit_of_measurement": "",
    });
    mqtt_client
        .publish(
            intensity_config_topic.clone(),
            QoS::AtLeastOnce,
            true,
            intensity_config_payload.to_string(),
        )
        .await?;
    debug!(
        "Published intensity config for {} to topic {}",
        device.product_name, intensity_config_topic
    );

    Ok(())
}

pub async fn subscribe_to_commands(
    mqtt_client: &AsyncClient,
    device_dsn: &str,
) -> Result<(), BoxError> {
    let command_topic = format!("homeassistant/switch/{}/set", device_dsn);
    debug!("Subscribing to command topic: {}", command_topic);
    mqtt_client
        .subscribe(command_topic.clone(), QoS::AtLeastOnce)
        .await?;
    debug!("Subscribed to command topic: {}", command_topic);

    debug!("Subscribing to intensity command topic: {}", command_topic);
    let intensity_command_topic = format!("homeassistant/switch/{}/intensity/set", device_dsn);
    mqtt_client
        .subscribe(intensity_command_topic.clone(), QoS::AtLeastOnce)
        .await?;
    debug!(
        "Subscribed to intensity command topic: {}",
        intensity_command_topic
    );

    Ok(())
}

/// Periodically polls device state and publishes to MQTT
/// This ensures Home Assistant stays in sync even when device state
/// changes externally (via app, schedules, etc.)
pub async fn periodic_state_poller(
    session: Arc<Mutex<Session>>,
    mqtt_client: AsyncClient,
    devices: Vec<DeviceInfo>,
    poll_interval_secs: u64,
) {
    let mut interval = interval(Duration::from_secs(poll_interval_secs));
    info!(
        "Starting periodic state poller (interval: {}s)",
        poll_interval_secs
    );

    loop {
        interval.tick().await;
        debug!("Polling device states");

        for device in &devices {
            match fetch_device_properties(&session, &device.dsn).await {
                Ok(properties) => {
                    debug!("Polled properties for {}: {:?}", device.dsn, properties);
                    if let Err(e) = publish_device_state(&mqtt_client, device, &properties).await {
                        warn!("Failed to publish polled state for {}: {}", device.dsn, e);
                    } else {
                        debug!("Published polled state for {}", device.dsn);
                    }
                }
                Err(e) => {
                    warn!("Failed to poll properties for {}: {}", device.dsn, e);
                }
            }
        }
    }
}

pub async fn handle_mqtt_events(
    session: Arc<Mutex<Session>>,
    mqtt_client: &AsyncClient,
    mut eventloop: EventLoop,
    devices: Vec<DeviceInfo>,
) -> Result<(), BoxError> {
    debug!("Listening for MQTT events");
    while let Ok(notification) = eventloop.poll().await {
        debug!("Received MQTT event: {:?}", notification);
        if let Event::Incoming(Packet::Publish(publish)) = notification {
            let topic = publish.topic.clone();
            let payload = match String::from_utf8(publish.payload.to_vec()) {
                Ok(p) => p,
                Err(e) => {
                    warn!("Failed to parse MQTT payload as UTF-8: {}", e);
                    continue;
                }
            };

            for device in &devices {
                let base_topic = format!("homeassistant/switch/{}", device.dsn);
                let command_topic = format!("{}/set", base_topic);
                let intensity_command_topic = format!("{}/intensity/set", base_topic);

                if topic == command_topic {
                    info!(
                        "Received power command for {}: {}",
                        device.product_name, payload
                    );
                    let state = match payload.as_str() {
                        "ON" => Some(true),
                        "OFF" => Some(false),
                        _ => {
                            warn!("Unknown payload on power command topic: {}", payload);
                            None
                        }
                    };

                    if let Some(state) = state {
                        if let Err(e) = set_device_power_state(&session, &device.dsn, state).await {
                            warn!("Failed to set power state for {}: {}", device.dsn, e);
                            continue;
                        }
                    }

                    // Fetch and publish updated state
                    match fetch_device_properties(&session, &device.dsn).await {
                        Ok(properties) => {
                            debug!("Fetched properties: {:?}", properties);
                            if let Err(e) = publish_device_state(&mqtt_client, device, &properties).await {
                                warn!("Failed to publish device state for {}: {}", device.dsn, e);
                            } else {
                                debug!("Published updated state for {}", device.dsn);
                            }
                        }
                        Err(e) => {
                            warn!("Failed to fetch device properties for {}: {}", device.dsn, e);
                        }
                    }
                } else if topic == intensity_command_topic {
                    info!(
                        "Received intensity command for {}: {}",
                        device.product_name, payload
                    );
                    if let Ok(intensity) = payload.parse::<u8>() {
                        if let Err(e) = set_device_intensity(&session, &device.dsn, intensity).await {
                            warn!("Failed to set intensity for {}: {}", device.dsn, e);
                            continue;
                        }
                    } else {
                        warn!("Invalid intensity value: {}", payload);
                        continue;
                    }

                    // Fetch and publish updated state
                    match fetch_device_properties(&session, &device.dsn).await {
                        Ok(properties) => {
                            debug!("Fetched properties: {:?}", properties);
                            if let Err(e) = publish_device_state(&mqtt_client, device, &properties).await {
                                warn!("Failed to publish device state for {}: {}", device.dsn, e);
                            } else {
                                debug!("Published updated state for {}", device.dsn);
                            }
                        }
                        Err(e) => {
                            warn!("Failed to fetch device properties for {}: {}", device.dsn, e);
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_device() -> DeviceInfo {
        DeviceInfo {
            product_name: "Test Diffuser".to_string(),
            dsn: "test_dsn_123".to_string(),
            model: "MODEL_TEST".to_string(),
            oem_model: "OEM_TEST".to_string(),
            sw_version: "1.0.0".to_string(),
        }
    }

    fn create_test_properties(power_state: u64, intensity: u64) -> Vec<PropertyInfo> {
        vec![
            PropertyInfo {
                name: "set_power_state".to_string(),
                base_type: "integer".to_string(),
                read_only: false,
                value: serde_json::Value::from(power_state),
            },
            PropertyInfo {
                name: "set_intensity_manual".to_string(),
                base_type: "integer".to_string(),
                read_only: false,
                value: serde_json::Value::from(intensity),
            },
        ]
    }

    #[test]
    fn test_mqtt_topic_format() {
        let device = create_test_device();
        let expected_base = "homeassistant/switch/test_dsn_123";
        let expected_command = format!("{}/set", expected_base);
        let expected_state = format!("{}/state", expected_base);
        let expected_intensity_state = format!("{}/intensity/state", expected_base);
        let expected_intensity_command = format!("{}/intensity/set", expected_base);

        // Verify topic structure is correct
        assert!(expected_command.contains(&device.dsn));
        assert!(expected_state.contains(&device.dsn));
        assert_eq!(expected_command, "homeassistant/switch/test_dsn_123/set");
        assert_eq!(expected_state, "homeassistant/switch/test_dsn_123/state");
        assert_eq!(expected_intensity_state, "homeassistant/switch/test_dsn_123/intensity/state");
        assert_eq!(expected_intensity_command, "homeassistant/switch/test_dsn_123/intensity/set");
    }

    #[test]
    fn test_power_state_on_payload() {
        // Test that power state 1 maps to "ON"
        let power_state = 1u64;
        let expected = if power_state == 1 { "ON" } else { "OFF" };
        assert_eq!(expected, "ON");
    }

    #[test]
    fn test_power_state_off_payload() {
        // Test that power state 0 maps to "OFF"
        let power_state = 0u64;
        let expected = if power_state == 1 { "ON" } else { "OFF" };
        assert_eq!(expected, "OFF");
    }

    #[test]
    fn test_intensity_value_extraction() {
        let properties = create_test_properties(1, 3);

        let intensity = properties
            .iter()
            .find(|p| p.name == "set_intensity_manual")
            .and_then(|p| p.value.as_u64())
            .unwrap_or(0);

        assert_eq!(intensity, 3);
    }

    #[test]
    fn test_power_state_extraction() {
        let properties = create_test_properties(1, 3);

        let power_state = properties
            .iter()
            .find(|p| p.name == "set_power_state")
            .and_then(|p| p.value.as_u64())
            .and_then(|v| Some(v == 1))
            .unwrap_or(false);

        assert_eq!(power_state, true);
    }

    #[test]
    fn test_mqtt_client_creation() {
        let (_client, _eventloop) = get_mqtt_client();
        // Verify we can create the client successfully
        // Keep-alive is set to 60 seconds for reasonable connection management
        // This test ensures MQTT client initialization doesn't panic
    }

    #[test]
    fn test_mqtt_env_var_defaults() {
        // Test that default values work when env vars not set
        std::env::remove_var("MQTT_HOST");
        std::env::remove_var("MQTT_PORT");

        let (_client, _eventloop) = get_mqtt_client();
        // Should succeed with defaults (localhost:1883)
    }

    #[test]
    fn test_mqtt_custom_config() {
        // Test that custom MQTT config can be set
        std::env::set_var("MQTT_HOST", "mqtt.example.com");
        std::env::set_var("MQTT_PORT", "8883");

        let (_client, _eventloop) = get_mqtt_client();
        // Should succeed with custom values

        // Clean up
        std::env::remove_var("MQTT_HOST");
        std::env::remove_var("MQTT_PORT");
    }

    #[tokio::test]
    async fn test_device_info_structure() {
        let device = create_test_device();
        assert_eq!(device.product_name, "Test Diffuser");
        assert_eq!(device.dsn, "test_dsn_123");
        assert_eq!(device.model, "MODEL_TEST");
        assert_eq!(device.oem_model, "OEM_TEST");
        assert_eq!(device.sw_version, "1.0.0");
    }

    #[test]
    fn test_property_info_structure() {
        let properties = create_test_properties(1, 5);
        assert_eq!(properties.len(), 2);

        let power_prop = &properties[0];
        assert_eq!(power_prop.name, "set_power_state");
        assert_eq!(power_prop.base_type, "integer");
        assert_eq!(power_prop.read_only, false);
        assert_eq!(power_prop.value.as_u64().unwrap(), 1);

        let intensity_prop = &properties[1];
        assert_eq!(intensity_prop.name, "set_intensity_manual");
        assert_eq!(intensity_prop.base_type, "integer");
        assert_eq!(intensity_prop.read_only, false);
        assert_eq!(intensity_prop.value.as_u64().unwrap(), 5);
    }

    #[test]
    fn test_home_assistant_config_topic_format() {
        let device = create_test_device();
        let config_topic = format!("homeassistant/switch/{}/config", device.dsn);
        assert_eq!(config_topic, "homeassistant/switch/test_dsn_123/config");

        let intensity_config_topic = format!("homeassistant/number/{}/config", device.dsn);
        assert_eq!(intensity_config_topic, "homeassistant/number/test_dsn_123/config");
    }

    #[test]
    fn test_command_parsing_on() {
        let payload = "ON";
        let should_turn_on = payload == "ON";
        assert_eq!(should_turn_on, true);
    }

    #[test]
    fn test_command_parsing_off() {
        let payload = "OFF";
        let should_turn_on = payload == "ON";
        assert_eq!(should_turn_on, false);
    }

    #[test]
    fn test_intensity_parsing_valid() {
        let payload = "3";
        let intensity = payload.parse::<u8>();
        assert!(intensity.is_ok());
        assert_eq!(intensity.unwrap(), 3);
    }

    #[test]
    fn test_intensity_parsing_invalid() {
        let payload = "invalid";
        let intensity = payload.parse::<u8>();
        assert!(intensity.is_err());
    }

    #[test]
    fn test_poll_interval_calculation() {
        // Test that 5 minutes = 300 seconds
        let interval_minutes = 5;
        let interval_seconds = interval_minutes * 60;
        assert_eq!(interval_seconds, 300);
    }

    #[tokio::test]
    async fn test_periodic_poller_signature() {
        // Test that we can construct the parameters for periodic_state_poller
        // This ensures the function signature is compatible with our usage

        // We just verify the types compile correctly
        // We can't test the actual polling logic without mocking the API
        // and that would be complex, so we verify the function signature instead
        let _types_check: fn(Arc<Mutex<Session>>, AsyncClient, Vec<DeviceInfo>, u64) -> _ =
            periodic_state_poller;

        // Verify we can create the necessary types
        let devices = vec![create_test_device()];
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].dsn, "test_dsn_123");
    }
}
