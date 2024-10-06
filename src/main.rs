mod api;
mod mqtt;

use api::{fetch_device_properties, fetch_devices};
use mqtt::{handle_mqtt_events, publish_device_state, subscribe_to_commands};
use rumqttc::AsyncClient;
use std::error::Error;
use tokio::task;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Initialize logging
    env_logger::init();

    // Fetch devices from the API
    let devices = fetch_devices().await?;

    // Initialize MQTT client
    let (mqtt_client, eventloop) = mqtt::get_mqtt_client();

    // Subscribe to command topics and publish initial states
    for device in &devices {
        // Fetch device properties
        let properties = fetch_device_properties(&device.dsn).await?;

        // Publish initial state to MQTT
        publish_device_state(&mqtt_client, device, &properties).await?;

        // Subscribe to command topics for this device
        subscribe_to_commands(&mqtt_client, &device.dsn).await?;
    }

    // Handle MQTT events in a separate task
    let devices_clone = devices.clone();
    task::spawn(async move {
        if let Err(e) = handle_mqtt_events(eventloop, devices_clone).await {
            eprintln!("Error handling MQTT events: {:?}", e);
        }
    });

    // Keep the main task alive
    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
    }
}
