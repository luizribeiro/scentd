# 🌸 scentd

**A Home Assistant integration for Aera smart fragrance diffusers**

`scentd` is a lightweight daemon that bridges [Aera for Home](https://www.aeraforhome.com/) diffusers with Home Assistant through MQTT, providing full control and monitoring of your smart fragrance devices.

## Features

### Device Control
- **Power On/Off** - Turn diffusers on and off remotely
- **Intensity Control** - Adjust fragrance intensity (1-5) in real-time
- **Automatic State Sync** - Periodic polling keeps Home Assistant in sync with external changes (app, schedules)

### Monitoring
- **Fragrance Level** - Track remaining fragrance as a percentage (0-100%)
- **Fragrance Identifier** - See which fragrance cartridge is currently installed (e.g., "MDN", "BSM", "CLS")
- **Real-time Updates** - All state changes reflected immediately in Home Assistant

### Smart Features
- **Reset Fragrance Level Button** - Simulate QR code scanning to reset fragrance level to 100% when you install a new cartridge (no physical QR scanning needed!)
- **Home Assistant Auto-Discovery** - Devices automatically appear in Home Assistant with proper icons and device classes
- **Retained MQTT Messages** - State persists across Home Assistant restarts

## Requirements

- **Rust** 1.70+ (for building from source)
- **MQTT Broker** (e.g., Mosquitto)
- **Home Assistant** with MQTT integration configured
- **Aera Account** with one or more Aera diffusers registered

## Installation

### Building from Source

```bash
# Clone the repository
git clone https://github.com/yourusername/scentd.git
cd scentd

# Build the project
cargo build --release

# The binary will be in target/release/scentd
```

## Configuration

`scentd` is configured entirely through environment variables:

### Required Variables

```bash
# Aera account credentials
export AERA_USERNAME="your-email@example.com"
export AERA_PASSWORD="your-password"
```

### Optional Variables

```bash
# MQTT broker configuration (defaults shown)
export MQTT_HOST="localhost"
export MQTT_PORT="1883"

# Logging level
export RUST_LOG="info"  # Options: error, warn, info, debug, trace
```

### Running the Daemon

```bash
# Run directly
AERA_USERNAME="your-email@example.com" AERA_PASSWORD="your-password" ./target/release/scentd

# Or set up as a systemd service (see systemd documentation)
# Or run in Docker (you'll need to build your own image)
```

## Home Assistant Entities

For each Aera diffuser, `scentd` automatically creates the following entities:

### Switch
- **Diffuser** - Main power switch (on/off)
  - Icon: `mdi:spray`
  - State topic: `homeassistant/switch/{DSN}/state`
  - Command topic: `homeassistant/switch/{DSN}/set`

### Number
- **Fragrance Intensity** - Intensity slider (1-5)
  - Icon: `mdi:gauge`
  - Range: 1-5, step: 1
  - State topic: `homeassistant/switch/{DSN}/intensity/state`
  - Command topic: `homeassistant/switch/{DSN}/intensity/set`

### Sensors
- **Fragrance Level** - Remaining fragrance percentage
  - Icon: Battery icon (changes based on level)
  - Device class: `battery`
  - Unit: `%`
  - State topic: `homeassistant/switch/{DSN}/fragrance_level/state`
  - Note: Only appears if QR code was scanned or "Reset Fragrance Level" button was pressed

- **Fragrance** - Current fragrance identifier
  - Icon: `mdi:flask`
  - State topic: `homeassistant/switch/{DSN}/fragrance_id/state`
  - Shows codes like "MDN", "BSM", "CLS", etc.

### Button
- **Reset Fragrance Level** - Simulates QR code scanning
  - Icon: `mdi:qrcode-scan`
  - Command topic: `homeassistant/switch/{DSN}/qr_scan/set`
  - Press this when installing a new fragrance cartridge to reset level to 100%

## Usage

### Basic Control

Once `scentd` is running, your Aera diffusers will automatically appear in Home Assistant. You can:

1. **Turn diffusers on/off** using the main switch
2. **Adjust intensity** using the intensity number slider (1-5)
3. **Monitor fragrance level** to know when to replace cartridges
4. **See which fragrance is installed** via the fragrance identifier sensor

### Changing Fragrance Cartridges

When you install a new fragrance cartridge:

1. Install the cartridge in your diffuser
2. In Home Assistant, press the **"Reset Fragrance Level"** button for that diffuser
3. The fragrance level sensor will immediately show 100%
4. Usage tracking starts from that point

No need to physically scan QR codes anymore!

### Automation Examples

**Low Fragrance Alert:**
```yaml
automation:
  - alias: "Alert when fragrance is low"
    trigger:
      - platform: numeric_state
        entity_id: sensor.diffuser_fragrance_level
        below: 10
    action:
      - service: notify.mobile_app
        data:
          message: "Diffuser fragrance is running low ({{ states('sensor.diffuser_fragrance_level') }}%)"
```

**Turn off at night:**
```yaml
automation:
  - alias: "Turn off diffuser at night"
    trigger:
      - platform: time
        at: "22:00:00"
    action:
      - service: switch.turn_off
        target:
          entity_id: switch.diffuser
```

**Adjust intensity based on time:**
```yaml
automation:
  - alias: "Set diffuser intensity by time of day"
    trigger:
      - platform: time
        at: "08:00:00"
    action:
      - service: number.set_value
        target:
          entity_id: number.diffuser_fragrance_intensity
        data:
          value: 5

  - alias: "Lower diffuser intensity in evening"
    trigger:
      - platform: time
        at: "18:00:00"
    action:
      - service: number.set_value
        target:
          entity_id: number.diffuser_fragrance_intensity
        data:
          value: 2
```

## MQTT Topics

### Discovery Topics
- Switch config: `homeassistant/switch/{DSN}/config`
- Intensity config: `homeassistant/number/{DSN}/config`
- Fragrance level config: `homeassistant/sensor/{DSN}_fragrance_level/config`
- Fragrance ID config: `homeassistant/sensor/{DSN}_fragrance_id/config`
- QR scan button config: `homeassistant/button/{DSN}_qr_scan/config`

### State Topics
- Power state: `homeassistant/switch/{DSN}/state` (Payload: `ON` or `OFF`)
- Intensity state: `homeassistant/switch/{DSN}/intensity/state` (Payload: `1`-`5`)
- Fragrance level: `homeassistant/switch/{DSN}/fragrance_level/state` (Payload: `0.0`-`100.0`)
- Fragrance ID: `homeassistant/switch/{DSN}/fragrance_id/state` (Payload: e.g., `MDN`)

### Command Topics
- Power: `homeassistant/switch/{DSN}/set` (Payload: `ON` or `OFF`)
- Intensity: `homeassistant/switch/{DSN}/intensity/set` (Payload: `1`-`5`)
- QR scan: `homeassistant/switch/{DSN}/qr_scan/set` (Payload: `PRESS`)

All state topics use **retained messages** so Home Assistant receives the current state immediately on restart.

## How Fragrance Level Tracking Works

The fragrance level is calculated based on two values from the Aera API:

- **`pump_life_time`** - Total cumulative pump runtime since device was manufactured
- **`pump_life_time_qr_scanned`** - The pump lifetime value when a fresh cartridge's QR code was scanned

**Formula:**
```
Usage = pump_life_time - pump_life_time_qr_scanned
Percentage Used = (Usage / 300,000) × 100
Percentage Remaining = 100 - Percentage Used
```

The default cartridge capacity is **300,000 pump lifetime units**. This value was determined empirically but may not be perfectly accurate for all cartridge types or usage patterns. You may need to adjust this value based on your own observations of when cartridges actually run out.

When you press the "Reset Fragrance Level" button, `scentd` sets `pump_life_time_qr_scanned` to the current `pump_life_time`, effectively marking the cartridge as fresh at 100%.

## Development

### Environment Setup

This project uses [devenv](https://devenv.sh/) for development environment management. Create a `.env` file in the project root with your credentials for testing:

```bash
# .env (ignored by git)
AERA_USERNAME=your-email@example.com
AERA_PASSWORD=your-password
MQTT_HOST=localhost
MQTT_PORT=1883
RUST_LOG=debug
```

### Building

```bash
cargo build
```

### Running Tests

```bash
cargo test
```

Currently **41 tests** covering:
- API authentication and token refresh
- Device property fetching
- MQTT message formatting
- Fragrance level calculations
- Command parsing and state management

### Debug Tools

`scentd` includes a debug tool to inspect API responses:

```bash
# Build and run the property inspector
cargo run --bin inspect_properties

# Shows all properties for all devices including:
# - pump_life_time
# - pump_life_time_qr_scanned
# - set_fragrance_identifier
# - power_state, intensity_state, etc.
```

### Project Structure

```
scentd/
├── src/
│   ├── main.rs           # Main daemon entry point
│   ├── lib.rs            # Library exports
│   ├── api.rs            # Aera API client
│   ├── mqtt.rs           # MQTT publishing and event handling
│   └── bin/
│       └── inspect_properties.rs  # Debug tool
├── Cargo.toml            # Rust dependencies
└── README.md             # This file
```

### How It Works

`scentd` bridges the Aera Cloud API with Home Assistant via MQTT. It authenticates with your Aera account, fetches device properties, publishes them to MQTT for Home Assistant discovery, and listens for commands to send back to the Aera API. Device state is polled every 5 minutes to stay in sync.

## Contributing

Contributions are welcome! Make sure tests pass (`cargo test`) before submitting a PR.

## License

This project is licensed under the MIT License - see the LICENSE file for details.

## Acknowledgments

- Built with Rust using [tokio](https://tokio.rs/) for async runtime
- MQTT client: [rumqttc](https://github.com/bytebeamio/rumqtt)
- HTTP client: [reqwest](https://github.com/seanmonstar/reqwest)
- Special thanks to the Aera for Home team for creating great smart home fragrance diffusers

## Disclaimer

This project is not affiliated with, endorsed by, or sponsored by Aera. It is an independent integration created for personal use and shared with the community.

---

Made with ❤️ for the Home Assistant community
