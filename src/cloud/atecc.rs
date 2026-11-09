//! The ATECC608B on the I2C header (SDA GPIO32, SCL GPIO25, 0xC0), through
//! Microchip CryptoAuthLib (`espressif/esp-cryptoauthlib`, TrustCustom).
//!
//! Slot 0 holds the device's P-256 private key. It is generated inside the
//! chip on first boot and can never be read, written or regenerated: the
//! slot is locked right after. esp-tls signs the TLS handshake with it
//! (`use_secure_element`); this module only sets the chip up, reads the
//! public key, and signs the CSR digest during provisioning.
//!
//! Every call runs on a private CryptoAuthLib device context, not the global
//! one: esp-tls calls `atcab_release()` at the end of *every* TLS session
//! (prayer-time HTTPS included), which frees the global context and would
//! pull it out from under a call in progress here.

use std::sync::Mutex;

use anyhow::{bail, Result};
use esp_idf_svc::sys::atca;

const KEY_SLOT: u16 = 0;
const LOCK_ZONE_CONFIG: u8 = 0x00;
const LOCK_ZONE_DATA: u8 = 0x01;

/// One user of the chip at a time.
static CHIP: Mutex<()> = Mutex::new(());

/// What the chip says about itself.
#[derive(Debug, Clone)]
pub struct Identity {
    /// 9-byte serial number, hex.
    pub serial: String,
    /// Slot-0 public key, X||Y.
    pub public_key: [u8; 64],
}

impl Identity {
    /// The public key as the registry takes it (`seed-device --public-key`).
    pub fn public_key_hex(&self) -> String {
        hex(&self.public_key)
    }
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

/// Runs `f` on a fresh private device context.
fn with_chip<T>(f: impl FnOnce(atca::ATCADevice) -> Result<T>) -> Result<T> {
    let _guard = CHIP.lock().unwrap_or_else(|e| e.into_inner());
    let mut device: atca::ATCADevice = core::ptr::null_mut();
    // SAFETY: the default config is CryptoAuthLib's static, built from
    // sdkconfig; the device pointer is ours until atcab_release_ext.
    let st = unsafe {
        atca::atcab_init_ext(
            &mut device,
            core::ptr::addr_of_mut!(atca::cfg_ateccx08a_i2c_default),
        )
    };
    if st != 0 || device.is_null() {
        bail!("ATECC608B not reachable (atcab_init_ext 0x{st:02X}); is it on the I2C header?");
    }
    let result = f(device);
    unsafe { atca::atcab_release_ext(&mut device) };
    result
}

fn check(what: &str, st: atca::ATCA_STATUS) -> Result<()> {
    if st != 0 {
        bail!("ATECC608B {what} failed: 0x{st:02X}");
    }
    Ok(())
}

/// Makes sure the chip is set up and returns its identity. On a blank chip
/// this is the one-way step: lock the configuration zone as it came from the
/// factory (slot 0 is already a P-256 private key, external signing, written
/// only by GenKey), lock the data zone, generate the key in slot 0, and lock
/// slot 0 so the key can never be replaced. Each step checks the lock state
/// first, so a power cut between steps resumes where it stopped.
pub fn ensure_identity() -> Result<Identity> {
    with_chip(|dev| unsafe {
        let mut serial = [0u8; 9];
        check(
            "read serial",
            atca::calib_read_serial_number(dev, serial.as_mut_ptr()),
        )?;

        let mut config = [0u8; 128];
        check(
            "read config",
            atca::calib_read_config_zone(dev, config.as_mut_ptr()),
        )?;
        let slot0_config = u16::from_le_bytes([config[20], config[21]]);
        let slot0_key_config = u16::from_le_bytes([config[96], config[97]]);
        let key_type = (slot0_key_config >> 2) & 0x7;
        let private = slot0_key_config & 1 == 1;
        let ext_sign = slot0_config & 0x1 != 0;
        let genkey_writes = slot0_config & 0x2000 != 0;
        if !(private && key_type == 4 && ext_sign && genkey_writes) {
            bail!(
                "ATECC608B slot 0 is not a P-256 private key for external signing \
                 (SlotConfig 0x{slot0_config:04X}, KeyConfig 0x{slot0_key_config:04X})"
            );
        }

        let mut locked = false;
        check(
            "lock state",
            atca::calib_is_locked(dev, LOCK_ZONE_CONFIG, &mut locked),
        )?;
        if !locked {
            log::warn!("ATECC608B: locking the configuration zone (one-way)");
            check("lock config zone", atca::calib_lock_config_zone(dev))?;
        }
        check(
            "lock state",
            atca::calib_is_locked(dev, LOCK_ZONE_DATA, &mut locked),
        )?;
        if !locked {
            log::warn!("ATECC608B: locking the data zone (one-way)");
            check("lock data zone", atca::calib_lock_data_zone(dev))?;
        }

        // SlotLocked (bytes 88-89): bit n clear = slot n locked. Re-read: the
        // config zone could only have been read before the lock above.
        check(
            "read config",
            atca::calib_read_config_zone(dev, config.as_mut_ptr()),
        )?;
        let slot0_locked = u16::from_le_bytes([config[88], config[89]]) & 1 == 0;

        let mut public_key = [0u8; 64];
        if slot0_locked {
            check(
                "read public key",
                atca::calib_get_pubkey(dev, KEY_SLOT, public_key.as_mut_ptr()),
            )?;
        } else {
            log::warn!(
                "ATECC608B: generating the device key in slot 0 and locking the slot (one-way)"
            );
            check(
                "generate key",
                atca::calib_genkey(dev, KEY_SLOT, public_key.as_mut_ptr()),
            )?;
            check("lock slot 0", atca::calib_lock_data_slot(dev, KEY_SLOT))?;
        }

        Ok(Identity {
            serial: hex(&serial),
            public_key,
        })
    })
}

/// Signs a SHA-256 digest with the slot-0 key; returns r||s.
pub fn sign(digest: &[u8; 32]) -> Result<[u8; 64]> {
    with_chip(|dev| {
        let mut signature = [0u8; 64];
        // SAFETY: both buffers are the sizes the call expects.
        check("sign", unsafe {
            atca::calib_sign_ext(dev, KEY_SLOT, digest.as_ptr(), signature.as_mut_ptr())
        })?;
        Ok(signature)
    })
}
