mod api;
mod mqtt;

use std::error::Error;

use api::{fetch_device_properties, fetch_devices, login};
use log::debug;
use mqtt::{handle_mqtt_events, publish_device_state, subscribe_to_commands};
use std::env;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::task;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Initialize logging
    env_logger::init();

    // Perform login to obtain tokens
    let username = env::var("AERA_USERNAME").expect("AERA_USERNAME not set");
    let password = env::var("AERA_PASSWORD").expect("AERA_PASSWORD not set");
    let session = Arc::new(Mutex::new(login(&username, &password).await?));

    // Fetch devices from the API
    let devices = fetch_devices(&session).await?;
    debug!("Fetched devices: {:?}", devices);

    // Initialize MQTT client
    let (mqtt_client, eventloop) = mqtt::get_mqtt_client();

    let devices_clone = devices.clone();
    let mqtt_client_clone = mqtt_client.clone();
    let session_clone = session.clone();
    task::spawn(async move {
        // Subscribe to command topics and publish initial states
        for device in &devices_clone {
            // Fetch device properties
            let properties = fetch_device_properties(&session_clone, &device.dsn)
                .await
                .unwrap();
            debug!("Fetched properties for {}: {:?}", device.dsn, properties);

            // Publish initial state to MQTT
            publish_device_state(&mqtt_client_clone, device, &properties)
                .await
                .unwrap();
            debug!("Published initial state for {}", device.dsn);

            // Subscribe to command topics for this device
            subscribe_to_commands(&mqtt_client_clone, &device.dsn)
                .await
                .unwrap();
            debug!("Subscribed to command topics for {}", device.dsn);
        }
        debug!("Initialization complete");
    });

    // Handle MQTT events in a separate task
    if let Err(e) = handle_mqtt_events(session, &mqtt_client, eventloop, devices).await {
        eprintln!("Error handling MQTT events: {:?}", e);
    }

    // Keep the main task alive
    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
    }
}
