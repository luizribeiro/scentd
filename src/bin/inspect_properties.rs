// Debug tool to inspect all properties returned by the Aera API
// Run with: RUST_LOG=info cargo run --bin inspect_properties

use scentd::api::{fetch_device_properties, fetch_devices, login};
use std::env;
use std::error::Error;

type BoxError = Box<dyn Error + Send + Sync>;

#[tokio::main]
async fn main() -> Result<(), BoxError> {
    env_logger::init();

    let username = env::var("AERA_USERNAME").expect("AERA_USERNAME not set");
    let password = env::var("AERA_PASSWORD").expect("AERA_PASSWORD not set");

    println!("Logging in to Aera Networks...");
    let session = std::sync::Arc::new(tokio::sync::Mutex::new(login(&username, &password).await?));

    println!("Fetching devices...");
    let devices = fetch_devices(&session).await?;
    println!("Found {} device(s)\n", devices.len());

    for device in devices {
        println!("Device: {}", device.product_name);
        println!("  DSN: {}", device.dsn);
        println!("  Model: {}", device.model);
        println!("  OEM Model: {}", device.oem_model);
        println!("  SW Version: {}", device.sw_version);
        println!("\nProperties:");

        let properties = fetch_device_properties(&session, &device.dsn).await?;

        for prop in properties {
            println!("  - Name: {}", prop.name);
            println!("    Type: {}", prop.base_type);
            println!("    Read-only: {}", prop.read_only);
            println!("    Value: {}", prop.value);
            println!();
        }
        println!("{}", "=".repeat(60));
    }

    Ok(())
}
