//! Build-time configuration from `platform.toml` (iot-device-sdk's
//! platform-build): the product constant, FW_VERSION, the partition check and
//! the bootloader copy for espflash. No AWS access and no secrets: the claim
//! and Wi-Fi are written to a unit at the bench (`ergo commission`).

fn main() {
    platform_build::configure();
}
