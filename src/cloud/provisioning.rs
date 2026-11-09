//! AWS IoT Fleet Provisioning by claim, with a CSR signed in the ATECC608B.
//!
//! 1. Connect with the shared claim certificate (it may only provision).
//! 2. `CreateCertificateFromCsr`: AWS IoT issues a certificate for the key in
//!    the CSR, the chip's slot-0 key. No private key crosses the wire.
//! 3. `RegisterThing` on the Vaktus template. The pre-provisioning hook
//!    approves only if this MAC is registered for product `vaktus` with
//!    exactly that public key (`seed-device --product vaktus --public-key ..`).

use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Result};
use namaz_vakti_logic::{cloud as wire, csr};

use super::atecc;
use super::identity::Identity;
use super::mqtt::{self, Auth, Mqtt, MqttEvent};

const ANSWER_TIMEOUT: Duration = Duration::from_secs(20);

/// Why provisioning did not complete.
#[derive(Debug)]
pub enum Failure {
    /// AWS IoT refused: typically the device is not registered yet, or with
    /// another key.
    Rejected(String),
    /// Anything else (network, timeout, chip): worth retrying.
    Error(anyhow::Error),
}

impl From<anyhow::Error> for Failure {
    fn from(e: anyhow::Error) -> Self {
        Failure::Error(e)
    }
}

/// Where and as what to provision. A struct, not separate `&str` arguments:
/// see [`mqtt::Settings`] for the Xtensa argument-register limit.
pub struct Claim<'a> {
    pub uri: &'a str,
    pub template: &'a str,
    pub root_ca: &'a str,
    pub cert: &'a str,
    pub key: &'a str,
}

pub fn provision(
    claim: &Claim<'_>,
    mac: &[u8; 6],
    chip: &atecc::Identity,
) -> Result<Identity, Failure> {
    let thing = wire::thing_name(mac);

    // The CSR first: if the chip cannot sign, there is no point connecting.
    let info = csr::request_info(&thing, &chip.public_key);
    let signature = atecc::sign(&csr::digest(&info))?;
    let csr_pem = csr::to_pem(&csr::assemble(&info, &signature));

    let client_id = wire::claim_client_id(mac);
    let (client, events) = Mqtt::connect(&mqtt::Settings {
        uri: claim.uri,
        client_id: &client_id,
        root_ca: claim.root_ca,
        auth: Auth::Pem {
            cert: claim.cert,
            key: claim.key,
        },
        buffer_size: 4096,
    })?;
    mqtt::wait_connected(&events, Duration::from_secs(30))?;
    log::info!(
        "provisioning: connected with the claim certificate as {}",
        wire::claim_client_id(mac)
    );

    let csr_accepted = format!("{}/accepted", wire::CREATE_FROM_CSR_TOPIC);
    let csr_rejected = format!("{}/rejected", wire::CREATE_FROM_CSR_TOPIC);
    let register = wire::register_topic(claim.template);
    let reg_accepted = format!("{register}/accepted");
    let reg_rejected = format!("{register}/rejected");
    for t in [&csr_accepted, &csr_rejected, &reg_accepted, &reg_rejected] {
        client.subscribe(t)?;
    }
    // Let the SUBACKs land before the requests, or the answer can be missed.
    std::thread::sleep(Duration::from_millis(1500));

    client.publish(
        wire::CREATE_FROM_CSR_TOPIC,
        &wire::create_from_csr_request(&csr_pem),
    )?;
    let (topic, payload) = answer(&events, &[&csr_accepted, &csr_rejected])?;
    if topic == csr_rejected {
        return Err(Failure::Rejected(format!(
            "CreateCertificateFromCsr: {}",
            wire::describe_rejection(&payload)
        )));
    }
    let issued = wire::parse_issued(&payload).map_err(|e| anyhow!(e))?;
    log::info!(
        "provisioning: certificate {} issued for the ATECC608B key",
        issued.certificate_id
    );

    client.publish(
        &register,
        &wire::register_request(&issued.ownership_token, mac),
    )?;
    let (topic, payload) = answer(&events, &[&reg_accepted, &reg_rejected])?;
    if topic == reg_rejected {
        return Err(Failure::Rejected(format!(
            "RegisterThing: {}",
            wire::describe_rejection(&payload)
        )));
    }
    let thing_name = wire::parse_registered(&payload).map_err(|e| anyhow!(e))?;
    log::info!("provisioning: APPROVED, thing {thing_name}");
    drop(client);

    Ok(Identity {
        thing_name,
        certificate_pem: issued.certificate_pem,
    })
}

fn answer(events: &Receiver<MqttEvent>, topics: &[&str]) -> Result<(String, Vec<u8>)> {
    let deadline = Instant::now() + ANSWER_TIMEOUT;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match events.recv_timeout(left) {
            Ok(MqttEvent::Message { topic, data }) if topics.contains(&topic.as_str()) => {
                return Ok((topic, data))
            }
            Ok(MqttEvent::Disconnected) => bail!("claim connection dropped"),
            Ok(_) => continue,
            Err(_) => bail!(
                "no answer on {} within {}s",
                topics[0],
                ANSWER_TIMEOUT.as_secs()
            ),
        }
    }
}
