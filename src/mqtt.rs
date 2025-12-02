use crate::api::{
    fetch_device_properties, set_device_intensity, set_device_power_state,
    set_pump_life_time_qr_scanned, DeviceInfo, PropertyInfo, Session,
};
use log::{debug, error, info, warn};
use rumqttc::{AsyncClient, Event, EventLoop, MqttOptions, Packet, QoS};
use serde_json::json;
use std::error::Error;
use std::sync::atomic::{AtomicU64, Ordering};

// Type alias for Send+Sync errors to work with tokio::spawn
type BoxError = Box<dyn Error + Send + Sync>;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{interval, Duration, Instant};

/// Tracks the last successful MQTT activity for watchdog monitoring
pub struct ActivityTracker {
    /// Timestamp of last activity as seconds since UNIX epoch
    last_activity_secs: AtomicU64,
    /// When the tracker was created (for calculating uptime)
    started_at: Instant,
}

impl ActivityTracker {
    pub fn new() -> Self {
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Self {
            last_activity_secs: AtomicU64::new(now_secs),
            started_at: Instant::now(),
        }
    }

    /// Record that MQTT activity just occurred
    pub fn record_activity(&self) {
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.last_activity_secs.store(now_secs, Ordering::Relaxed);
    }

    /// Returns seconds since last activity
    pub fn seconds_since_activity(&self) -> u64 {
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let last = self.last_activity_secs.load(Ordering::Relaxed);
        now_secs.saturating_sub(last)
    }

    /// Returns how long the tracker has been running
    pub fn uptime(&self) -> Duration {
        self.started_at.elapsed()
    }
}

impl Default for ActivityTracker {
    fn default() -> Self {
        Self::new()
    }
}

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

// Default cartridge capacity in pump lifetime units
// Based on typical Aera cartridge life (~800-1000 hours of runtime)
// This value can be adjusted based on actual cartridge specifications
const DEFAULT_CARTRIDGE_CAPACITY: f64 = 300000.0;

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
        .map(|v| v == 1)
        .unwrap_or(false);

    let intensity_state = properties
        .iter()
        .find(|p| p.name == "set_intensity_manual")
        .and_then(|p| p.value.as_u64())
        .unwrap_or(0);

    // Extract fragrance level data
    let pump_life_time = properties
        .iter()
        .find(|p| p.name == "pump_life_time")
        .and_then(|p| p.value.as_u64())
        .unwrap_or(0) as f64;

    let pump_life_time_qr_scanned = properties
        .iter()
        .find(|p| p.name == "pump_life_time_qr_scanned")
        .and_then(|p| p.value.as_u64())
        .unwrap_or(0) as f64;

    // Calculate fragrance level percentage
    // If QR code was never scanned (value is 0), we can't calculate accurate level
    let fragrance_level = if pump_life_time_qr_scanned > 0.0 {
        let usage = pump_life_time - pump_life_time_qr_scanned;
        let percentage_used = (usage / DEFAULT_CARTRIDGE_CAPACITY) * 100.0;
        let percentage_remaining = 100.0 - percentage_used;
        // Clamp between 0 and 100
        Some(percentage_remaining.max(0.0).min(100.0))
    } else {
        // QR code never scanned, can't determine level accurately
        None
    };

    // Extract fragrance identifier
    let fragrance_id = properties
        .iter()
        .find(|p| p.name == "set_fragrance_identifier")
        .and_then(|p| p.value.as_str())
        .unwrap_or("Unknown")
        .to_string();

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

    // Publish fragrance level as a separate topic (retained so Home Assistant gets it immediately on restart)
    let fragrance_level_topic = format!("{}/fragrance_level/state", base_topic);
    let fragrance_level_payload = match fragrance_level {
        Some(level) => format!("{:.1}", level),
        None => "unknown".to_string(),
    };
    mqtt_client
        .publish(
            fragrance_level_topic.clone(),
            QoS::AtLeastOnce,
            true,
            fragrance_level_payload.clone(),
        )
        .await?;
    debug!(
        "Published fragrance level for {} to topic {}: {}",
        device.product_name, fragrance_level_topic, fragrance_level_payload
    );

    // Publish fragrance identifier (retained so Home Assistant gets it immediately on restart)
    let fragrance_id_topic = format!("{}/fragrance_id/state", base_topic);
    mqtt_client
        .publish(
            fragrance_id_topic.clone(),
            QoS::AtLeastOnce,
            true,
            fragrance_id.clone(),
        )
        .await?;
    debug!(
        "Published fragrance ID for {} to topic {}: {}",
        device.product_name, fragrance_id_topic, fragrance_id
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

    // Publish fragrance level sensor configuration (retained so it persists across HA restarts)
    let fragrance_level_config_topic =
        format!("homeassistant/sensor/{}_fragrance_level/config", device.dsn);
    let fragrance_level_config_payload = json!({
        "name": "Fragrance Level",
        "state_topic": fragrance_level_topic,
        "unique_id": format!("{}_fragrance_level", device.dsn),
        "device": {
            "identifiers": [device.dsn],
            "name": device.product_name,
            "model": device.model,
            "manufacturer": device.oem_model,
            "sw_version": device.sw_version
        },
        "unit_of_measurement": "%",
        "device_class": "battery",
        "state_class": "measurement",
    });
    mqtt_client
        .publish(
            fragrance_level_config_topic.clone(),
            QoS::AtLeastOnce,
            true,
            fragrance_level_config_payload.to_string(),
        )
        .await?;
    debug!(
        "Published fragrance level config for {} to topic {}",
        device.product_name, fragrance_level_config_topic
    );

    // Publish fragrance identifier sensor configuration (retained so it persists across HA restarts)
    let fragrance_id_config_topic =
        format!("homeassistant/sensor/{}_fragrance_id/config", device.dsn);
    let fragrance_id_config_payload = json!({
        "name": "Fragrance",
        "state_topic": fragrance_id_topic,
        "unique_id": format!("{}_fragrance_id", device.dsn),
        "device": {
            "identifiers": [device.dsn],
            "name": device.product_name,
            "model": device.model,
            "manufacturer": device.oem_model,
            "sw_version": device.sw_version
        },
        "icon": "mdi:flask",
    });
    mqtt_client
        .publish(
            fragrance_id_config_topic.clone(),
            QoS::AtLeastOnce,
            true,
            fragrance_id_config_payload.to_string(),
        )
        .await?;
    debug!(
        "Published fragrance ID config for {} to topic {}",
        device.product_name, fragrance_id_config_topic
    );

    // Publish QR scan button configuration (retained so it persists across HA restarts)
    let qr_scan_button_config_topic = format!("homeassistant/button/{}_qr_scan/config", device.dsn);
    let qr_scan_command_topic = format!("{}/qr_scan/set", base_topic);
    let qr_scan_button_config_payload = json!({
        "name": "Reset Fragrance Level",
        "command_topic": qr_scan_command_topic,
        "unique_id": format!("{}_qr_scan", device.dsn),
        "device": {
            "identifiers": [device.dsn],
            "name": device.product_name,
            "model": device.model,
            "manufacturer": device.oem_model,
            "sw_version": device.sw_version
        },
        "icon": "mdi:qrcode-scan",
        "payload_press": "PRESS",
    });
    mqtt_client
        .publish(
            qr_scan_button_config_topic.clone(),
            QoS::AtLeastOnce,
            true,
            qr_scan_button_config_payload.to_string(),
        )
        .await?;
    debug!(
        "Published QR scan button config for {} to topic {}",
        device.product_name, qr_scan_button_config_topic
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

    let qr_scan_command_topic = format!("homeassistant/switch/{}/qr_scan/set", device_dsn);
    debug!(
        "Subscribing to QR scan command topic: {}",
        qr_scan_command_topic
    );
    mqtt_client
        .subscribe(qr_scan_command_topic.clone(), QoS::AtLeastOnce)
        .await?;
    debug!(
        "Subscribed to QR scan command topic: {}",
        qr_scan_command_topic
    );

    Ok(())
}

/// Watchdog timeout in seconds (15 minutes)
/// If no successful MQTT activity occurs within this time, the daemon exits
const WATCHDOG_TIMEOUT_SECS: u64 = 900;

/// How often the watchdog checks for activity (every 60 seconds)
const WATCHDOG_CHECK_INTERVAL_SECS: u64 = 60;

/// Monitors MQTT activity and exits if the connection appears dead
pub async fn watchdog(tracker: Arc<ActivityTracker>) -> Result<(), BoxError> {
    let mut interval = interval(Duration::from_secs(WATCHDOG_CHECK_INTERVAL_SECS));
    info!(
        "Starting MQTT watchdog (timeout: {}s, check interval: {}s)",
        WATCHDOG_TIMEOUT_SECS, WATCHDOG_CHECK_INTERVAL_SECS
    );

    loop {
        interval.tick().await;
        let inactive_secs = tracker.seconds_since_activity();
        let uptime = tracker.uptime();

        debug!(
            "Watchdog check: {}s since last activity, uptime: {:?}",
            inactive_secs, uptime
        );

        if inactive_secs > WATCHDOG_TIMEOUT_SECS {
            error!(
                "MQTT watchdog triggered: no activity for {}s (threshold: {}s). Connection may be dead.",
                inactive_secs, WATCHDOG_TIMEOUT_SECS
            );
            return Err(
                format!("Watchdog timeout: no MQTT activity for {}s", inactive_secs).into(),
            );
        }
    }
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
        info!("Polling device states ({} devices)", devices.len());

        let mut success_count = 0;
        let mut fail_count = 0;

        for device in &devices {
            match fetch_device_properties(&session, &device.dsn).await {
                Ok(properties) => {
                    debug!("Polled properties for {}: {:?}", device.dsn, properties);
                    if let Err(e) = publish_device_state(&mqtt_client, device, &properties).await {
                        warn!("Failed to publish polled state for {}: {}", device.dsn, e);
                        fail_count += 1;
                    } else {
                        debug!("Published polled state for {}", device.dsn);
                        // Note: Activity is tracked by the event handler when PubAck is received,
                        // not here. publish_device_state only queues the message locally.
                        success_count += 1;
                    }
                }
                Err(e) => {
                    warn!("Failed to poll properties for {}: {}", device.dsn, e);
                    fail_count += 1;
                }
            }
        }

        info!(
            "Polling complete: {} succeeded, {} failed",
            success_count, fail_count
        );
    }
}

/// Handle a power command in a spawned task (doesn't block the event loop)
async fn handle_power_command(
    session: Arc<Mutex<Session>>,
    mqtt_client: AsyncClient,
    device: DeviceInfo,
    state: bool,
) {
    let dsn = device.dsn.clone();

    if let Err(e) = set_device_power_state(&session, &dsn, state).await {
        warn!("Failed to set power state for {}: {}", dsn, e);
        return;
    }

    match fetch_device_properties(&session, &dsn).await {
        Ok(properties) => {
            if let Err(e) = publish_device_state(&mqtt_client, &device, &properties).await {
                warn!("Failed to publish device state for {}: {}", dsn, e);
            }
        }
        Err(e) => {
            warn!("Failed to fetch device properties for {}: {}", dsn, e);
        }
    }
}

/// Handle an intensity command in a spawned task (doesn't block the event loop)
async fn handle_intensity_command(
    session: Arc<Mutex<Session>>,
    mqtt_client: AsyncClient,
    device: DeviceInfo,
    intensity: u8,
) {
    let dsn = device.dsn.clone();

    if let Err(e) = set_device_intensity(&session, &dsn, intensity).await {
        warn!("Failed to set intensity for {}: {}", dsn, e);
        return;
    }

    match fetch_device_properties(&session, &dsn).await {
        Ok(properties) => {
            if let Err(e) = publish_device_state(&mqtt_client, &device, &properties).await {
                warn!("Failed to publish device state for {}: {}", dsn, e);
            }
        }
        Err(e) => {
            warn!("Failed to fetch device properties for {}: {}", dsn, e);
        }
    }
}

/// Handle a QR scan command in a spawned task (doesn't block the event loop)
async fn handle_qr_scan_command(
    session: Arc<Mutex<Session>>,
    mqtt_client: AsyncClient,
    device: DeviceInfo,
) {
    let dsn = device.dsn.clone();
    let product_name = device.product_name.clone();

    match fetch_device_properties(&session, &dsn).await {
        Ok(properties) => {
            let pump_life_time = properties
                .iter()
                .find(|p| p.name == "pump_life_time")
                .and_then(|p| p.value.as_u64())
                .unwrap_or(0);

            info!(
                "Setting pump_life_time_qr_scanned to {} for {}",
                pump_life_time, product_name
            );

            if let Err(e) = set_pump_life_time_qr_scanned(&session, &dsn, pump_life_time).await {
                warn!("Failed to set pump_life_time_qr_scanned for {}: {}", dsn, e);
                return;
            }

            info!("Successfully reset fragrance level for {}", product_name);

            match fetch_device_properties(&session, &dsn).await {
                Ok(updated_properties) => {
                    if let Err(e) =
                        publish_device_state(&mqtt_client, &device, &updated_properties).await
                    {
                        warn!("Failed to publish updated state for {}: {}", dsn, e);
                    } else {
                        info!("Published updated fragrance level for {}", dsn);
                    }
                }
                Err(e) => {
                    warn!("Failed to fetch updated properties for {}: {}", dsn, e);
                }
            }
        }
        Err(e) => {
            warn!("Failed to fetch properties for QR scan on {}: {}", dsn, e);
        }
    }
}

pub async fn handle_mqtt_events(
    session: Arc<Mutex<Session>>,
    mqtt_client: &AsyncClient,
    mut eventloop: EventLoop,
    devices: Vec<DeviceInfo>,
    activity_tracker: Arc<ActivityTracker>,
) -> Result<(), BoxError> {
    info!("MQTT event handler started, listening for events");
    loop {
        match eventloop.poll().await {
            Ok(notification) => {
                // Log connection-related events at info level for debugging
                match &notification {
                    Event::Incoming(Packet::ConnAck(ack)) => {
                        info!("MQTT connected: {:?}", ack);
                        activity_tracker.record_activity();
                    }
                    Event::Incoming(Packet::PingResp) => {
                        debug!("MQTT ping response received");
                        activity_tracker.record_activity();
                    }
                    Event::Incoming(Packet::SubAck(ack)) => {
                        debug!("MQTT subscription acknowledged: {:?}", ack);
                        activity_tracker.record_activity();
                    }
                    Event::Incoming(Packet::PubAck(ack)) => {
                        debug!("MQTT publish acknowledged: {:?}", ack);
                        activity_tracker.record_activity();
                    }
                    Event::Outgoing(outgoing) => {
                        debug!("MQTT outgoing: {:?}", outgoing);
                    }
                    _ => {
                        debug!("MQTT event: {:?}", notification);
                    }
                }

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
                        let qr_scan_command_topic = format!("{}/qr_scan/set", base_topic);

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
                                // Spawn command handler to not block the event loop
                                tokio::spawn(handle_power_command(
                                    session.clone(),
                                    mqtt_client.clone(),
                                    device.clone(),
                                    state,
                                ));
                            }
                        } else if topic == intensity_command_topic {
                            info!(
                                "Received intensity command for {}: {}",
                                device.product_name, payload
                            );
                            if let Ok(intensity) = payload.parse::<u8>() {
                                // Spawn command handler to not block the event loop
                                tokio::spawn(handle_intensity_command(
                                    session.clone(),
                                    mqtt_client.clone(),
                                    device.clone(),
                                    intensity,
                                ));
                            } else {
                                warn!("Invalid intensity value: {}", payload);
                            }
                        } else if topic == qr_scan_command_topic {
                            info!(
                                "Received QR scan command for {}: {}",
                                device.product_name, payload
                            );
                            // Spawn command handler to not block the event loop
                            tokio::spawn(handle_qr_scan_command(
                                session.clone(),
                                mqtt_client.clone(),
                                device.clone(),
                            ));
                        }
                    }
                }
            }
            Err(e) => {
                // Log the error and return it to cause the daemon to exit
                // systemd will restart it automatically
                error!(
                    "MQTT event loop error: {}. Connection may be dead, exiting for restart.",
                    e
                );
                return Err(format!("MQTT event loop error: {}", e).into());
            }
        }
    }
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
            device_type: None,
            lan_ip: None,
            connection_status: None,
            mac: None,
            connected_at: None,
            extra_fields: std::collections::HashMap::new(),
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
        assert_eq!(
            expected_intensity_state,
            "homeassistant/switch/test_dsn_123/intensity/state"
        );
        assert_eq!(
            expected_intensity_command,
            "homeassistant/switch/test_dsn_123/intensity/set"
        );
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
        assert_eq!(
            intensity_config_topic,
            "homeassistant/number/test_dsn_123/config"
        );
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

    #[test]
    fn test_fragrance_level_calculation_full_cartridge() {
        // Test nearly full cartridge (99% remaining)
        let pump_life_time = 585089.0;
        let pump_life_time_qr_scanned = 582062.0;
        let usage = pump_life_time - pump_life_time_qr_scanned; // 3027
        let percentage_used = (usage / DEFAULT_CARTRIDGE_CAPACITY) * 100.0;
        let percentage_remaining = (100.0 - percentage_used).max(0.0).min(100.0);

        // Should be approximately 99%
        assert!(percentage_remaining > 98.9 && percentage_remaining < 99.1);
    }

    #[test]
    fn test_fragrance_level_calculation_half_used() {
        // Test half-used cartridge
        let pump_life_time = 150000.0;
        let pump_life_time_qr_scanned = 0.0;
        let usage = pump_life_time - pump_life_time_qr_scanned;
        let percentage_used = (usage / DEFAULT_CARTRIDGE_CAPACITY) * 100.0;
        let percentage_remaining = (100.0 - percentage_used).max(0.0).min(100.0);

        // Should be 50%
        assert_eq!(percentage_remaining, 50.0);
    }

    #[test]
    fn test_fragrance_level_calculation_empty() {
        // Test empty cartridge (used full capacity)
        let pump_life_time = 300000.0;
        let pump_life_time_qr_scanned = 0.0;
        let usage = pump_life_time - pump_life_time_qr_scanned;
        let percentage_used = (usage / DEFAULT_CARTRIDGE_CAPACITY) * 100.0;
        let percentage_remaining = (100.0 - percentage_used).max(0.0).min(100.0);

        // Should be 0%
        assert_eq!(percentage_remaining, 0.0);
    }

    #[test]
    fn test_fragrance_level_calculation_over_capacity() {
        // Test over capacity (should clamp to 0%)
        let pump_life_time = 500000.0;
        let pump_life_time_qr_scanned = 0.0;
        let usage = pump_life_time - pump_life_time_qr_scanned;
        let percentage_used = (usage / DEFAULT_CARTRIDGE_CAPACITY) * 100.0;
        let percentage_remaining = (100.0 - percentage_used).max(0.0).min(100.0);

        // Should be clamped to 0%
        assert_eq!(percentage_remaining, 0.0);
    }

    #[test]
    fn test_fragrance_level_qr_not_scanned() {
        // Test when QR code was never scanned (value is 0)
        let pump_life_time_qr_scanned = 0.0;

        // Should return None to indicate unknown
        let fragrance_level: Option<f64> = if pump_life_time_qr_scanned > 0.0 {
            // Would calculate normally
            Some(0.0)
        } else {
            None
        };

        assert_eq!(fragrance_level, None);
    }

    #[test]
    fn test_fragrance_level_extraction_from_properties() {
        // Test extracting pump lifetime values from properties
        let properties = vec![
            PropertyInfo {
                name: "pump_life_time".to_string(),
                base_type: "integer".to_string(),
                read_only: true,
                value: serde_json::Value::from(585089),
            },
            PropertyInfo {
                name: "pump_life_time_qr_scanned".to_string(),
                base_type: "integer".to_string(),
                read_only: false,
                value: serde_json::Value::from(582062),
            },
        ];

        let pump_life_time = properties
            .iter()
            .find(|p| p.name == "pump_life_time")
            .and_then(|p| p.value.as_u64())
            .unwrap_or(0) as f64;

        let pump_life_time_qr_scanned = properties
            .iter()
            .find(|p| p.name == "pump_life_time_qr_scanned")
            .and_then(|p| p.value.as_u64())
            .unwrap_or(0) as f64;

        assert_eq!(pump_life_time, 585089.0);
        assert_eq!(pump_life_time_qr_scanned, 582062.0);
    }

    #[test]
    fn test_fragrance_level_topic_format() {
        let device = create_test_device();
        let base_topic = format!("homeassistant/switch/{}", device.dsn);
        let fragrance_level_topic = format!("{}/fragrance_level/state", base_topic);
        let fragrance_level_config_topic =
            format!("homeassistant/sensor/{}_fragrance_level/config", device.dsn);

        assert_eq!(
            fragrance_level_topic,
            "homeassistant/switch/test_dsn_123/fragrance_level/state"
        );
        assert_eq!(
            fragrance_level_config_topic,
            "homeassistant/sensor/test_dsn_123_fragrance_level/config"
        );
    }

    #[test]
    fn test_fragrance_id_extraction_from_properties() {
        // Test extracting fragrance identifier from properties
        let properties = vec![PropertyInfo {
            name: "set_fragrance_identifier".to_string(),
            base_type: "string".to_string(),
            read_only: false,
            value: serde_json::Value::from("MDN"),
        }];

        let fragrance_id = properties
            .iter()
            .find(|p| p.name == "set_fragrance_identifier")
            .and_then(|p| p.value.as_str())
            .unwrap_or("Unknown")
            .to_string();

        assert_eq!(fragrance_id, "MDN");
    }

    #[test]
    fn test_fragrance_id_extraction_missing() {
        // Test extracting fragrance identifier when property is missing
        let properties: Vec<PropertyInfo> = vec![];

        let fragrance_id = properties
            .iter()
            .find(|p| p.name == "set_fragrance_identifier")
            .and_then(|p| p.value.as_str())
            .unwrap_or("Unknown")
            .to_string();

        assert_eq!(fragrance_id, "Unknown");
    }

    #[test]
    fn test_fragrance_id_topic_format() {
        let device = create_test_device();
        let base_topic = format!("homeassistant/switch/{}", device.dsn);
        let fragrance_id_topic = format!("{}/fragrance_id/state", base_topic);
        let fragrance_id_config_topic =
            format!("homeassistant/sensor/{}_fragrance_id/config", device.dsn);

        assert_eq!(
            fragrance_id_topic,
            "homeassistant/switch/test_dsn_123/fragrance_id/state"
        );
        assert_eq!(
            fragrance_id_config_topic,
            "homeassistant/sensor/test_dsn_123_fragrance_id/config"
        );
    }

    #[test]
    fn test_qr_scan_button_topic_format() {
        let device = create_test_device();
        let base_topic = format!("homeassistant/switch/{}", device.dsn);
        let qr_scan_command_topic = format!("{}/qr_scan/set", base_topic);
        let qr_scan_button_config_topic =
            format!("homeassistant/button/{}_qr_scan/config", device.dsn);

        assert_eq!(
            qr_scan_command_topic,
            "homeassistant/switch/test_dsn_123/qr_scan/set"
        );
        assert_eq!(
            qr_scan_button_config_topic,
            "homeassistant/button/test_dsn_123_qr_scan/config"
        );
    }

    #[test]
    fn test_activity_tracker_new() {
        let tracker = ActivityTracker::new();
        // Just created, so seconds since activity should be 0 or very small
        assert!(tracker.seconds_since_activity() < 2);
    }

    #[test]
    fn test_activity_tracker_record_activity() {
        let tracker = ActivityTracker::new();
        std::thread::sleep(std::time::Duration::from_millis(100));
        tracker.record_activity();
        // Just recorded activity, should be very recent
        assert!(tracker.seconds_since_activity() < 2);
    }

    #[test]
    fn test_activity_tracker_uptime() {
        let tracker = ActivityTracker::new();
        std::thread::sleep(std::time::Duration::from_millis(50));
        let uptime = tracker.uptime();
        assert!(uptime.as_millis() >= 50);
    }

    #[test]
    fn test_watchdog_timeout_constant() {
        // Watchdog should be 15 minutes (900 seconds)
        assert_eq!(WATCHDOG_TIMEOUT_SECS, 900);
        // Check interval should be 60 seconds
        assert_eq!(WATCHDOG_CHECK_INTERVAL_SECS, 60);
    }
}
