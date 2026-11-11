//! Persists the user's WiFi credentials to NVS flash so they survive reboots,
//! seeded on first boot from the Wi-Fi written at the bench, if any.
//!
//! Follows the open/load/save convention of [`crate::cache`] and
//! [`crate::touch_calibration`]. Stored as a `serde_json` blob (like
//! [`crate::cache`], and unlike the fixed-byte touch-calibration record): the
//! record holds only strings, and the Xtensa `serde_json` codegen bug called
//! out in the touch-calibration module is float-specific, so strings round-trip
//! safely (`cache.rs` relies on the same guarantee).
//!
//! The [`WifiCredentials`] type and its validation are pure, host-testable code
//! in the `namaz-vakti-logic` crate; this module is the hardware-facing NVS half.

use esp_idf_svc::nvs::{EspDefaultNvsPartition, EspNvs, NvsDefault};

pub use namaz_vakti_logic::wifi_credentials::WifiCredentials;

const NAMESPACE: &str = "wifi";
const KEY: &str = "creds";

/// Opens (creating if needed) the `wifi` NVS namespace. Mirrors
/// [`crate::cache::open`].
pub fn open(nvs: EspDefaultNvsPartition) -> anyhow::Result<EspNvs<NvsDefault>> {
    Ok(EspNvs::new(nvs, NAMESPACE, true)?)
}

/// Loads the saved credentials, or `None` if none are stored, the blob is
/// unreadable/corrupt, or the stored values fail validation (a stale blob from
/// an older/mismatched schema is treated as absent so boot falls into setup
/// rather than trying to connect with garbage).
pub fn load(nvs: &EspNvs<NvsDefault>) -> Option<WifiCredentials> {
    let len = match nvs.blob_len(KEY) {
        Ok(Some(len)) => len,
        _ => return None,
    };
    let mut buf = vec![0u8; len];
    let creds: WifiCredentials = match nvs.get_blob(KEY, &mut buf) {
        Ok(Some(bytes)) => serde_json::from_slice(bytes).ok()?,
        _ => return None,
    };
    creds.is_valid().then_some(creds)
}

/// Persists the given credentials. Logs and swallows write errors (consistent
/// with the other NVS modules — a failed persist must not crash the device).
pub fn save(nvs: &EspNvs<NvsDefault>, creds: &WifiCredentials) {
    match serde_json::to_vec(creds) {
        Ok(bytes) => {
            if let Err(e) = nvs.set_blob(KEY, &bytes) {
                log::warn!("Failed to persist WiFi credentials to NVS: {e:?}");
            }
        }
        Err(e) => log::warn!("Failed to serialize WiFi credentials: {e:?}"),
    }
}
