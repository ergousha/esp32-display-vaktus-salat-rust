//! The device's AWS IoT identity: its thing name and certificate, in NVS, with
//! the public key the certificate was issued for.
//!
//! Nothing here is secret: the private key never leaves the ATECC608B. The key
//! is kept so a stored identity is only used with the chip it belongs to (a
//! swapped module provisions again instead of failing every TLS handshake).
//! Losing the record only means provisioning again, which the registry allows
//! for the same chip.

use anyhow::{Context, Result};
use esp_idf_svc::nvs::{EspDefaultNvsPartition, EspNvs, NvsDefault};

const NAMESPACE: &str = "cloud";
const KEY_THING: &str = "thing";
const KEY_CERT: &str = "cert";
const KEY_PUBLIC_KEY: &str = "pubkey";
/// A certificate is ~1.2 KB of PEM.
const MAX_CERT: usize = 4000;

pub struct Identity {
    pub thing_name: String,
    pub certificate_pem: String,
    /// The chip's public key (X||Y hex) the certificate was issued for.
    pub public_key: String,
}

pub struct IdentityStore {
    nvs: EspNvs<NvsDefault>,
}

impl IdentityStore {
    pub fn new(partition: EspDefaultNvsPartition) -> Result<Self> {
        Ok(Self {
            nvs: EspNvs::new(partition, NAMESPACE, true).context("opening NVS namespace cloud")?,
        })
    }

    pub fn load(&self) -> Option<Identity> {
        let mut thing = [0u8; 129];
        let mut cert = vec![0u8; MAX_CERT];
        let mut key = [0u8; 160];
        let thing_name = self
            .nvs
            .get_str(KEY_THING, &mut thing)
            .ok()
            .flatten()?
            .to_string();
        let certificate_pem = self
            .nvs
            .get_str(KEY_CERT, &mut cert)
            .ok()
            .flatten()?
            .to_string();
        // Missing in records from before the key was stored: treated as a
        // mismatch, so such a device provisions once more.
        let public_key = self
            .nvs
            .get_str(KEY_PUBLIC_KEY, &mut key)
            .ok()
            .flatten()
            .unwrap_or_default()
            .to_string();
        (!thing_name.is_empty() && certificate_pem.contains("BEGIN CERTIFICATE")).then_some(
            Identity {
                thing_name,
                certificate_pem,
                public_key,
            },
        )
    }

    pub fn save(&self, identity: &Identity) -> Result<()> {
        self.nvs
            .set_str(KEY_CERT, &identity.certificate_pem)
            .context("saving the certificate")?;
        self.nvs
            .set_str(KEY_PUBLIC_KEY, &identity.public_key)
            .context("saving the public key")?;
        // The thing name last: `load` needs it.
        self.nvs
            .set_str(KEY_THING, &identity.thing_name)
            .context("saving the thing name")?;
        Ok(())
    }
}
