use crate::api::{set_device_intensity, set_device_power_state, DeviceInfo, PropertyInfo};
use log::{info, warn};
use rumqttc::{AsyncClient, Event, EventLoop, Incoming, MqttOptions, Packet, QoS};
use serde_json::json;
use std::error::Error;
use tokio::time::{self, Duration};

pub fn get_mqtt_client() -> (AsyncClient, EventLoop) {
    let mut mqttoptions = MqttOptions::new("fragrance_bridge", "localhost", 1883);
    mqttoptions.set_keep_alive(Duration::from_secs(5));

    AsyncClient::new(mqttoptions, 10)
}

pub async fn publish_device_state(
    mqtt_client: &AsyncClient,
    device: &DeviceInfo,
    properties: &[PropertyInfo],
) -> Result<(), Box<dyn Error>> {
    // Extract relevant properties
    let power_state = properties
        .iter()
        .find(|p| p.name == "power_state")
        .and_then(|p| p.value.as_bool())
        .unwrap_or(false);

    let intensity_state = properties
        .iter()
        .find(|p| p.name == "intensity_state")
        .and_then(|p| p.value.as_u64())
        .unwrap_or(0);

    // Construct MQTT topics
    let base_topic = format!("homeassistant/switch/{}", device.dsn);
    let state_topic = format!("{}/state", base_topic);
    let command_topic = format!("{}/set", base_topic);

    // Publish state
    let payload = if power_state { "ON" } else { "OFF" };
    mqtt_client
        .publish(state_topic.clone(), QoS::AtLeastOnce, false, payload)
        .await?;

    // Publish intensity as a separate topic
    let intensity_topic = format!("{}/intensity/state", base_topic);
    mqtt_client
        .publish(
            intensity_topic.clone(),
            QoS::AtLeastOnce,
            false,
            intensity_state.to_string(),
        )
        .await?;

    // For Home Assistant discovery, publish config topics
    let config_topic = format!("homeassistant/switch/{}/config", device.dsn);
    let config_payload = json!({
        "name": device.product_name,
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
            config_topic,
            QoS::AtLeastOnce,
            false,
            config_payload.to_string(),
        )
        .await?;

    // Publish intensity control configuration
    let intensity_config_topic = format!("homeassistant/number/{}/config", device.dsn);
    let intensity_command_topic = format!("{}/intensity/set", base_topic);
    let intensity_config_payload = json!({
        "name": format!("{} Intensity", device.product_name),
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
            intensity_config_topic,
            QoS::AtLeastOnce,
            false,
            intensity_config_payload.to_string(),
        )
        .await?;

    Ok(())
}

pub async fn subscribe_to_commands(
    mqtt_client: &AsyncClient,
    device_dsn: &str,
) -> Result<(), Box<dyn Error>> {
    let command_topic = format!("homeassistant/switch/{}/set", device_dsn);
    mqtt_client
        .subscribe(command_topic, QoS::AtLeastOnce)
        .await?;

    let intensity_command_topic = format!("homeassistant/switch/{}/intensity/set", device_dsn);
    mqtt_client
        .subscribe(intensity_command_topic, QoS::AtLeastOnce)
        .await?;

    Ok(())
}

pub async fn handle_mqtt_events(
    mut eventloop: EventLoop,
    devices: Vec<DeviceInfo>,
) -> Result<(), Box<dyn Error>> {
    while let Ok(notification) = eventloop.poll().await {
        if let Event::Incoming(Packet::Publish(publish)) = notification {
            let topic = publish.topic.clone();
            let payload = String::from_utf8(publish.payload.to_vec())?;

            for device in &devices {
                let base_topic = format!("homeassistant/switch/{}", device.dsn);
                let command_topic = format!("{}/set", base_topic);
                let intensity_command_topic = format!("{}/intensity/set", base_topic);

                if topic == command_topic {
                    info!(
                        "Received power command for {}: {}",
                        device.product_name, payload
                    );
                    match payload.as_str() {
                        "ON" => {
                            set_device_power_state(&device.dsn, true).await?;
                        }
                        "OFF" => {
                            set_device_power_state(&device.dsn, false).await?;
                        }
                        _ => warn!("Unknown payload on power command topic: {}", payload),
                    }
                } else if topic == intensity_command_topic {
                    info!(
                        "Received intensity command for {}: {}",
                        device.product_name, payload
                    );
                    if let Ok(intensity) = payload.parse::<u8>() {
                        set_device_intensity(&device.dsn, intensity).await?;
                    } else {
                        warn!("Invalid intensity value: {}", payload);
                    }
                }
            }
        }
    }
    Ok(())
}
