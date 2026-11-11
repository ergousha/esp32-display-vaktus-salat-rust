//! The product's own cloud wire format: the `vaktus` shadow report.
//!
//! Everything the platform defines (enrolment, OTA, the `platform` shadow) is
//! in iot-device-sdk; this is only what Vaktus adds on top. Pure, so it is
//! tested on the host.

use serde::Serialize;

/// This firmware's product on the platform: its releases live under
/// `s3://<bucket>/vaktus/`, its registry records and provisioning template
/// carry this id, and an OTA job naming another product is refused.
pub const PRODUCT: &str = "vaktus";

/// Named shadow the device reports its state in.
pub const SHADOW_NAME: &str = "vaktus";

/// `$aws/things/{thing}/shadow/name/vaktus/update`.
pub fn shadow_update_topic(thing: &str) -> String {
    format!("$aws/things/{thing}/shadow/name/{SHADOW_NAME}/update")
}

/// What the device reports in its shadow.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Report {
    pub product: &'static str,
    /// Running firmware version.
    pub fw: String,
    /// Running OTA slot, e.g. `ota_0`.
    pub slot: String,
    /// ATECC608B serial number, hex.
    pub secure_element: String,
    /// Selected location: district name and id.
    pub location: String,
    pub location_id: String,
    /// UI language, e.g. `tr`.
    pub language: &'static str,
    pub free_heap: u32,
}

/// Shadow update payload: `{"state":{"reported":{..}}}`.
pub fn shadow_report(report: &Report) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({ "state": { "reported": report } }))
        .expect("serializable")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn the_report_is_a_reported_shadow_state() {
        let report = Report {
            product: PRODUCT,
            fw: "0.8.0".into(),
            slot: "ota_0".into(),
            secure_element: "0123C471D3B627CFEE".into(),
            location: "HAARLEM".into(),
            location_id: "13877".into(),
            language: "tr",
            free_heap: 150_000,
        };
        let v: Value = serde_json::from_slice(&shadow_report(&report)).unwrap();
        assert_eq!(v["state"]["reported"]["product"], "vaktus");
        assert_eq!(v["state"]["reported"]["fw"], "0.8.0");
        assert_eq!(v["state"]["reported"]["location_id"], "13877");
        assert_eq!(
            shadow_update_topic("38182B8456F4"),
            "$aws/things/38182B8456F4/shadow/name/vaktus/update"
        );
    }
}
