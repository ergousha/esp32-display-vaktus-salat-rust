//! AWS IoT wire formats of the cloud link (iot-platform-infra, product
//! `vaktus`): fleet provisioning with a CSR, and the device shadow report.
//!
//! Pure functions so they are tested on the host; the firmware's `cloud`
//! module moves the bytes.

use serde::{Deserialize, Serialize};

/// This firmware's product on the platform: its releases live under
/// `s3://<bucket>/vaktus/`, its registry records and provisioning template
/// carry this id, and an OTA job naming another product is refused.
pub const PRODUCT: &str = "vaktus";

/// Named shadow the device reports its state in.
pub const SHADOW_NAME: &str = "vaktus";

/// `CreateCertificateFromCsr` request topic. `/accepted` and `/rejected`
/// answer it.
pub const CREATE_FROM_CSR_TOPIC: &str = "$aws/certificates/create-from-csr/json";

/// The thing name the provisioning template gives the device: its MAC as 12
/// uppercase hex digits (the `SerialNumber` parameter).
pub fn thing_name(mac: &[u8; 6]) -> String {
    mac.iter().map(|b| format!("{b:02X}")).collect()
}

/// The MAC as the registry stores it: `AA:BB:CC:DD:EE:FF`.
pub fn mac_string(mac: &[u8; 6]) -> String {
    mac.iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}

/// Client id of the claim connection. Distinct from the thing name, which the
/// device policy reserves for the device's own connection.
pub fn claim_client_id(mac: &[u8; 6]) -> String {
    format!("claim-{}", thing_name(mac))
}

/// `RegisterThing` request topic for `template`.
pub fn register_topic(template: &str) -> String {
    format!("$aws/provisioning-templates/{template}/provision/json")
}

/// Payload of a `CreateCertificateFromCsr` request.
pub fn create_from_csr_request(csr_pem: &str) -> Vec<u8> {
    #[derive(Serialize)]
    struct Req<'a> {
        #[serde(rename = "certificateSigningRequest")]
        csr: &'a str,
    }
    serde_json::to_vec(&Req { csr: csr_pem }).expect("serializable")
}

/// What `CreateCertificateFromCsr` answered.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct IssuedCertificate {
    /// The device certificate, for the key in the CSR.
    #[serde(rename = "certificatePem")]
    pub certificate_pem: String,
    #[serde(rename = "certificateId")]
    pub certificate_id: String,
    /// Proves ownership in `RegisterThing`; valid for one hour.
    #[serde(rename = "certificateOwnershipToken")]
    pub ownership_token: String,
}

/// Parses `.../create-from-csr/json/accepted`.
pub fn parse_issued(payload: &[u8]) -> Result<IssuedCertificate, String> {
    let issued: IssuedCertificate = serde_json::from_slice(payload)
        .map_err(|e| format!("unreadable certificate response: {e}"))?;
    if !issued.certificate_pem.contains("BEGIN CERTIFICATE") || issued.ownership_token.is_empty() {
        return Err("certificate response without a certificate or ownership token".into());
    }
    Ok(issued)
}

/// Payload of a `RegisterThing` request. No `Secret`: this product proves
/// itself with the key its certificate was issued for, which the hook compares
/// with the one registered for its MAC.
pub fn register_request(ownership_token: &str, mac: &[u8; 6]) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "certificateOwnershipToken": ownership_token,
        "parameters": {
            "SerialNumber": thing_name(mac),
            "MacAddress": mac_string(mac),
        }
    }))
    .expect("serializable")
}

/// Parses `.../provision/json/accepted` into the thing name.
pub fn parse_registered(payload: &[u8]) -> Result<String, String> {
    #[derive(Deserialize)]
    struct Resp {
        #[serde(rename = "thingName")]
        thing_name: String,
    }
    let resp: Resp = serde_json::from_slice(payload)
        .map_err(|e| format!("unreadable registration response: {e}"))?;
    if resp.thing_name.is_empty() {
        return Err("registration response without a thing name".into());
    }
    Ok(resp.thing_name)
}

/// A `/rejected` answer of either provisioning API, as one line.
pub fn describe_rejection(payload: &[u8]) -> String {
    #[derive(Deserialize, Default)]
    #[serde(default)]
    struct Rejected {
        #[serde(rename = "statusCode")]
        status_code: u32,
        #[serde(rename = "errorCode")]
        error_code: String,
        #[serde(rename = "errorMessage")]
        error_message: String,
    }
    match serde_json::from_slice::<Rejected>(payload) {
        Ok(r) => format!("{} {}: {}", r.status_code, r.error_code, r.error_message),
        Err(_) => String::from_utf8_lossy(payload).into_owned(),
    }
}

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

    const MAC: [u8; 6] = [0x38, 0x18, 0x2B, 0x84, 0x56, 0xF4];

    #[test]
    fn names_match_the_registry_and_the_template() {
        assert_eq!(thing_name(&MAC), "38182B8456F4");
        assert_eq!(mac_string(&MAC), "38:18:2B:84:56:F4");
        assert_eq!(claim_client_id(&MAC), "claim-38182B8456F4");
        assert_eq!(
            register_topic("esp32-ztp-vaktus-fleet-template"),
            "$aws/provisioning-templates/esp32-ztp-vaktus-fleet-template/provision/json"
        );
    }

    #[test]
    fn the_csr_request_carries_the_pem() {
        let v: Value =
            serde_json::from_slice(&create_from_csr_request("-----BEGIN X-----\n")).unwrap();
        assert_eq!(v["certificateSigningRequest"], "-----BEGIN X-----\n");
    }

    #[test]
    fn an_issued_certificate_is_read() {
        let ok = br#"{"certificatePem":"-----BEGIN CERTIFICATE-----\nAA\n-----END CERTIFICATE-----\n","certificateId":"abc","certificateOwnershipToken":"tok"}"#;
        let issued = parse_issued(ok).unwrap();
        assert_eq!(issued.certificate_id, "abc");
        assert_eq!(issued.ownership_token, "tok");
        assert!(parse_issued(
            br#"{"certificatePem":"x","certificateId":"a","certificateOwnershipToken":"t"}"#
        )
        .is_err());
        assert!(parse_issued(b"not json").is_err());
    }

    #[test]
    fn the_registration_sends_serial_and_mac_but_no_secret() {
        let v: Value = serde_json::from_slice(&register_request("tok", &MAC)).unwrap();
        assert_eq!(v["certificateOwnershipToken"], "tok");
        assert_eq!(v["parameters"]["SerialNumber"], "38182B8456F4");
        assert_eq!(v["parameters"]["MacAddress"], "38:18:2B:84:56:F4");
        assert!(v["parameters"].get("Secret").is_none());
        assert_eq!(
            parse_registered(br#"{"thingName":"38182B8456F4","deviceConfiguration":{}}"#).unwrap(),
            "38182B8456F4"
        );
        assert!(parse_registered(br#"{"thingName":""}"#).is_err());
    }

    #[test]
    fn rejections_are_one_line() {
        assert_eq!(
            describe_rejection(br#"{"statusCode":403,"errorCode":"AccessDenied","errorMessage":"Pre-provisioning hook rejected"}"#),
            "403 AccessDenied: Pre-provisioning hook rejected"
        );
        assert_eq!(describe_rejection(b"oops"), "oops");
    }

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
