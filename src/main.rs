mod api;
mod mqtt;

use std::error::Error;

use api::{fetch_device_properties, fetch_devices, login};
use log::{debug, error, info};
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
    info!("Starting scentd");
    let username = env::var("AERA_USERNAME").expect("AERA_USERNAME not set");
    let password = env::var("AERA_PASSWORD").expect("AERA_PASSWORD not set");

    info!("Logging in to Aera Networks");
    let session = Arc::new(Mutex::new(login(&username, &password).await?));

    // Fetch devices from the API
    info!("Fetching devices from Aera Networks");
    let devices = fetch_devices(&session).await?;
    info!("Found {} device(s)", devices.len());
    debug!("Fetched devices: {:?}", devices);

    // Initialize MQTT client
    info!("Connecting to MQTT broker");
    let (mqtt_client, eventloop) = mqtt::get_mqtt_client();

    let devices_clone = devices.clone();
    let mqtt_client_clone = mqtt_client.clone();
    let session_clone = session.clone();
    task::spawn(async move {
        info!("Starting device initialization");
        // Subscribe to command topics and publish initial states
        for device in &devices_clone {
            info!("Initializing device: {} ({})", device.product_name, device.dsn);

            // Fetch device properties
            let properties = match fetch_device_properties(&session_clone, &device.dsn).await {
                Ok(props) => props,
                Err(e) => {
                    error!("Failed to fetch properties for {}: {}", device.dsn, e);
                    continue;
                }
            };
            debug!("Fetched properties for {}: {:?}", device.dsn, properties);

            // Publish initial state to MQTT
            if let Err(e) = publish_device_state(&mqtt_client_clone, device, &properties).await {
                error!("Failed to publish initial state for {}: {}", device.dsn, e);
                // Continue anyway - we can try again later
            } else {
                debug!("Published initial state for {}", device.dsn);
            }

            // Subscribe to command topics for this device
            if let Err(e) = subscribe_to_commands(&mqtt_client_clone, &device.dsn).await {
                error!("Failed to subscribe to commands for {}: {}", device.dsn, e);
                // Continue anyway - device might still work
            } else {
                debug!("Subscribed to command topics for {}", device.dsn);
            }
        }
        info!("Device initialization complete");
    });

    // Handle MQTT events in the main loop
    info!("Starting MQTT event handler");
    if let Err(e) = handle_mqtt_events(session, &mqtt_client, eventloop, devices).await {
        error!("MQTT event handler exited with error: {}", e);
        return Err(e);
    }

    // If we get here, the event loop exited cleanly (which shouldn't happen in normal operation)
    error!("MQTT event handler exited unexpectedly");
    Ok(())
}
