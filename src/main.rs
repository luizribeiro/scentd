use std::error::Error;

// Type alias for Send+Sync errors to work with tokio::spawn
type BoxError = Box<dyn Error + Send + Sync>;

use log::{debug, error, info};
use scentd::api::{fetch_device_properties, fetch_devices, login};
use scentd::mqtt::{
    handle_mqtt_events, periodic_state_poller, publish_device_state, subscribe_to_commands,
    watchdog, ActivityTracker,
};
use std::env;
use std::sync::Arc;
use tokio::signal;
use tokio::sync::Mutex;
use tokio::task;

#[tokio::main]
async fn main() -> Result<(), BoxError> {
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
    let (mqtt_client, mut eventloop) = scentd::mqtt::get_mqtt_client();

    // Create activity tracker for watchdog
    let activity_tracker = Arc::new(ActivityTracker::new());

    // Verify MQTT connection by polling once
    // This ensures we fail fast if the broker is unreachable
    match eventloop.poll().await {
        Ok(_) => {
            info!("Successfully connected to MQTT broker");
            activity_tracker.record_activity();
        }
        Err(e) => {
            error!("Failed to connect to MQTT broker: {}", e);
            return Err(format!("MQTT connection failed: {}", e).into());
        }
    }

    // Spawn initialization task
    let devices_clone = devices.clone();
    let mqtt_client_clone = mqtt_client.clone();
    let session_clone = session.clone();
    task::spawn(async move {
        info!("Starting device initialization");
        // Subscribe to command topics and publish initial states
        for device in &devices_clone {
            info!(
                "Initializing device: {} ({})",
                device.product_name, device.dsn
            );

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

    // Handle MQTT events in a task so we can wait for shutdown signals
    info!("Starting MQTT event handler");
    let session_clone = session.clone();
    let devices_clone = devices.clone();
    let mqtt_client_clone = mqtt_client.clone();
    let activity_tracker_clone = activity_tracker.clone();
    let mqtt_handle = task::spawn(async move {
        if let Err(e) = handle_mqtt_events(
            session_clone,
            &mqtt_client_clone,
            eventloop,
            devices_clone,
            activity_tracker_clone,
        )
        .await
        {
            error!("MQTT event handler exited with error: {}", e);
            return Err(e);
        }
        Ok(())
    });

    // Spawn periodic state poller to keep Home Assistant in sync
    // Polls every 5 minutes to detect external changes (app, schedules, etc.)
    // Wait 1 minute before starting to avoid startup congestion
    let mqtt_client_poller = mqtt_client.clone();
    let session_poller = session.clone();
    let devices_poller = devices.clone();
    task::spawn(async move {
        // Wait 1 minute before starting periodic polling
        tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
        periodic_state_poller(session_poller, mqtt_client_poller, devices_poller, 300).await;
    });

    // Spawn watchdog to detect dead MQTT connections
    let activity_tracker_watchdog = activity_tracker.clone();
    let watchdog_handle = task::spawn(async move { watchdog(activity_tracker_watchdog).await });

    // Wait for shutdown signal, MQTT handler exit, or watchdog trigger
    tokio::select! {
        result = mqtt_handle => {
            // MQTT handler exited (likely due to connection error)
            error!("MQTT handler exited unexpectedly");
            if let Err(e) = result {
                error!("MQTT handler task error: {}", e);
                return Err("MQTT handler failed".into());
            }
            Err("MQTT handler exited unexpectedly".into())
        }
        result = watchdog_handle => {
            // Watchdog triggered (no MQTT activity for too long)
            error!("Watchdog triggered - MQTT connection appears dead");
            if let Err(e) = result {
                error!("Watchdog task error: {}", e);
            }
            Err("Watchdog timeout - restarting".into())
        }
        result = signal::ctrl_c() => {
            match result {
                Ok(()) => {
                    info!("Received shutdown signal (SIGINT/SIGTERM)");
                    info!("Shutting down gracefully...");
                    info!("Shutdown complete");
                    Ok(())
                }
                Err(err) => {
                    error!("Failed to listen for shutdown signal: {}", err);
                    Err(err.into())
                }
            }
        }
    }
}
