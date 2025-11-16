// Command-line utility for managing Aera diffusers
// Run with: cargo run --bin scent -- <command>

use clap::{Parser, Subcommand};
use scentd::api::{
    fetch_device_properties, fetch_devices, login, set_fragrance_identifier,
    set_pump_life_time_qr_scanned,
};
use std::env;
use std::error::Error;
use std::sync::Arc;
use tokio::sync::Mutex;

type BoxError = Box<dyn Error + Send + Sync>;

#[derive(Parser)]
#[command(name = "scent")]
#[command(about = "Command-line utility for managing Aera diffusers", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// List all diffusers
    List,

    /// Show detailed information about a specific diffuser
    Info {
        /// Device Serial Number (DSN) of the diffuser
        dsn: String,
        /// Show raw API data for debugging
        #[arg(long)]
        debug: bool,
    },

    /// Reset the fragrance level for a diffuser (simulates scanning QR code)
    ResetLevel {
        /// Device Serial Number (DSN) of the diffuser
        dsn: String,
    },

    /// Set the fragrance type for a diffuser
    SetFragrance {
        /// Device Serial Number (DSN) of the diffuser
        dsn: String,
        /// Fragrance identifier (e.g., "MDN", "CIT", etc.)
        fragrance_id: String,
    },
}

#[tokio::main]
async fn main() -> Result<(), BoxError> {
    // Initialize logger with INFO as default level unless RUST_LOG is set
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let cli = Cli::parse();

    // Login to Aera Networks
    let username = env::var("AERA_USERNAME").expect("AERA_USERNAME not set");
    let password = env::var("AERA_PASSWORD").expect("AERA_PASSWORD not set");

    let session = Arc::new(Mutex::new(login(&username, &password).await?));

    match cli.command {
        Commands::List => list_diffusers(&session).await?,
        Commands::Info { dsn, debug } => show_info(&session, &dsn, debug).await?,
        Commands::ResetLevel { dsn } => reset_level(&session, &dsn).await?,
        Commands::SetFragrance { dsn, fragrance_id } => {
            set_fragrance(&session, &dsn, &fragrance_id).await?
        }
    }

    Ok(())
}

async fn list_diffusers(session: &Arc<Mutex<scentd::api::Session>>) -> Result<(), BoxError> {
    let devices = fetch_devices(&session).await?;

    if devices.is_empty() {
        println!("No diffusers found.");
        return Ok(());
    }

    println!("Found {} diffuser(s):\n", devices.len());

    for device in devices {
        println!("  DSN: {}", device.dsn);
        println!("    Model: {} ({})", device.oem_model, device.model);
        if let Some(ref mac) = device.mac {
            println!("    MAC: {}", mac);
        }
        if let Some(ref ip) = device.lan_ip {
            println!("    IP: {}", ip);
        }
        if let Some(ref status) = device.connection_status {
            println!("    Status: {}", status);
        }
        println!();
    }

    Ok(())
}

async fn show_info(
    session: &Arc<Mutex<scentd::api::Session>>,
    dsn: &str,
    debug: bool,
) -> Result<(), BoxError> {
    // First, get device info
    let devices = fetch_devices(&session).await?;
    let device = devices
        .iter()
        .find(|d| d.dsn == dsn)
        .ok_or_else(|| format!("Device with DSN '{}' not found", dsn))?;

    println!("Device Information:");
    println!("  DSN: {}", device.dsn);
    println!("  Model: {} ({})", device.oem_model, device.model);
    if let Some(ref mac) = device.mac {
        println!("  MAC: {}", mac);
    }
    if let Some(ref ip) = device.lan_ip {
        println!("  IP: {}", ip);
    }
    if let Some(ref status) = device.connection_status {
        println!("  Status: {}", status);
    }
    println!("  SW Version: {}", device.sw_version);
    println!();

    // Get properties
    let properties = fetch_device_properties(&session, dsn).await?;

    // Extract key properties
    let power_state = properties
        .iter()
        .find(|p| p.name == "set_power_state")
        .and_then(|p| p.value.as_u64())
        .map(|v| if v == 1 { "ON" } else { "OFF" })
        .unwrap_or("Unknown");

    let intensity = properties
        .iter()
        .find(|p| p.name == "set_intensity_manual")
        .and_then(|p| p.value.as_u64())
        .unwrap_or(0);

    let fragrance_id = properties
        .iter()
        .find(|p| p.name == "set_fragrance_identifier")
        .and_then(|p| p.value.as_str())
        .unwrap_or("Unknown");

    let pump_life_time = properties
        .iter()
        .find(|p| p.name == "pump_life_time")
        .and_then(|p| p.value.as_u64())
        .unwrap_or(0);

    let pump_life_time_qr_scanned = properties
        .iter()
        .find(|p| p.name == "pump_life_time_qr_scanned")
        .and_then(|p| p.value.as_u64())
        .unwrap_or(0);

    // Calculate fragrance level
    const DEFAULT_CARTRIDGE_CAPACITY: f64 = 300000.0;
    let fragrance_level = if pump_life_time_qr_scanned > 0 {
        let usage = (pump_life_time - pump_life_time_qr_scanned) as f64;
        let percentage_used = (usage / DEFAULT_CARTRIDGE_CAPACITY) * 100.0;
        let percentage_remaining = (100.0 - percentage_used).max(0.0).min(100.0);
        format!("{:.1}%", percentage_remaining)
    } else {
        "Unknown (QR code not scanned)".to_string()
    };

    println!("Status:");
    println!("  Power: {}", power_state);
    println!("  Intensity: {}/5", intensity);
    println!("  Fragrance: {}", fragrance_id);
    println!("  Fragrance Level: {}", fragrance_level);
    println!("  Pump Life Time: {}", pump_life_time);
    println!("  QR Scanned At: {}", pump_life_time_qr_scanned);

    // Show debug info if requested
    if debug {
        println!();
        println!("========================================");
        println!("Debug: Raw API Data");
        println!("========================================");
        println!("Device fields:");
        println!("  product_name: {}", device.product_name);
        println!("  model: {}", device.model);
        println!("  oem_model: {}", device.oem_model);
        println!("  sw_version: {}", device.sw_version);
        if let Some(ref dt) = device.device_type {
            println!("  device_type: {}", dt);
        }
        if let Some(ref mac) = device.mac {
            println!("  mac: {}", mac);
        }
        if let Some(ref ip) = device.lan_ip {
            println!("  lan_ip: {}", ip);
        }
        if let Some(ref status) = device.connection_status {
            println!("  connection_status: {}", status);
        }
        if let Some(ref connected) = device.connected_at {
            println!("  connected_at: {}", connected);
        }

        println!("\nExtra fields from API:");
        for (key, value) in &device.extra_fields {
            println!("  {}: {}", key, value);
        }

        println!("\nAll properties:");
        for prop in &properties {
            println!("  {}: {} (type: {}, read_only: {})",
                prop.name, prop.value, prop.base_type, prop.read_only);
        }
    }

    Ok(())
}

async fn reset_level(
    session: &Arc<Mutex<scentd::api::Session>>,
    dsn: &str,
) -> Result<(), BoxError> {
    println!("Resetting fragrance level for device {}...", dsn);

    // Fetch current pump_life_time
    let properties = fetch_device_properties(&session, dsn).await?;
    let pump_life_time = properties
        .iter()
        .find(|p| p.name == "pump_life_time")
        .and_then(|p| p.value.as_u64())
        .ok_or_else(|| "Could not find pump_life_time property")?;

    // Set pump_life_time_qr_scanned to current pump_life_time
    set_pump_life_time_qr_scanned(&session, dsn, pump_life_time).await?;

    println!("✓ Fragrance level reset to 100% (set QR scan value to {})", pump_life_time);

    Ok(())
}

async fn set_fragrance(
    session: &Arc<Mutex<scentd::api::Session>>,
    dsn: &str,
    fragrance_id: &str,
) -> Result<(), BoxError> {
    // First check if the device supports setting fragrance identifier
    let properties = fetch_device_properties(&session, dsn).await?;
    let has_set_fragrance = properties
        .iter()
        .any(|p| p.name == "set_fragrance_identifier" && !p.read_only);

    if !has_set_fragrance {
        // Get device info to provide a helpful message
        let devices = fetch_devices(&session).await?;
        let device = devices.iter().find(|d| d.dsn == dsn);

        if let Some(device) = device {
            return Err(format!(
                "Cannot set fragrance on {} ({}) - this device automatically detects cartridges.\n\
                \n\
                The {} model uses automatic cartridge detection and doesn't support manual \n\
                fragrance setting via the API. This command only works on aeraMini devices.\n\
                \n\
                Simply insert a cartridge and the device will detect it automatically.",
                dsn, device.oem_model, device.oem_model
            ).into());
        } else {
            return Err(format!(
                "Cannot set fragrance on {} - device not found or doesn't support this feature.\n\
                \n\
                This command only works on aeraMini devices that have a writable \n\
                'set_fragrance_identifier' property.",
                dsn
            ).into());
        }
    }

    println!("Setting fragrance to '{}' for device {}...", fragrance_id, dsn);

    set_fragrance_identifier(&session, dsn, fragrance_id).await?;

    println!("✓ Fragrance identifier set to '{}'", fragrance_id);

    Ok(())
}
