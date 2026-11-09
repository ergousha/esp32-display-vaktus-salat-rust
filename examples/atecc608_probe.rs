//! ATECC608 bench probe for the 4.0" ESP32-32E board (I2C header: SDA=GPIO32, SCL=GPIO25).
//!
//! Read-only toward the chip: it wakes it, reads its revision and the full
//! configuration zone, reports lock state and slot configuration, runs the
//! chip's self-test, and — only if the data zone is already locked — reads the
//! public key of every private P-256 slot and signs a fixed digest with each,
//! so the signature can be verified on the host. It never writes, locks or
//! generates keys.
//!
//!   cargo build --release --example atecc608_probe
//!   espflash flash --monitor target/xtensa-esp32-espidf/release/examples/atecc608_probe

use std::time::Duration;

use esp_idf_svc::hal::{
    delay::{Ets, FreeRtos},
    gpio::{PinDriver, Pull},
    i2c::{I2cConfig, I2cDriver},
    peripherals::Peripherals,
    units::FromValueType,
};

/// FreeRTOS ticks (10 ms each at the default 100 Hz) per I2C transaction.
const I2C_TIMEOUT: u32 = 10;
const WAKE_TOKEN: [u8; 4] = [0x04, 0x11, 0x33, 0x43];
/// 0x60 = factory-blank ATECC608A/B and TrustCUSTOM default; 0x35 = Trust&Go
/// (ATECC608B-TNGTLS); 0x36 = TrustFLEX (ATECC608B-TFLXTLS).
const CANDIDATE_ADDRS: [u8; 3] = [0x60, 0x35, 0x36];
/// Fixed "digest" the probe asks each private key to sign; the host verifies it.
const TEST_DIGEST: [u8; 32] = *b"vaktus-atecc608-probe-digest-v1!";

const OP_READ: u8 = 0x02;
const OP_NONCE: u8 = 0x16;
const OP_RANDOM: u8 = 0x1B;
const OP_INFO: u8 = 0x30;
const OP_GENKEY: u8 = 0x40;
const OP_SIGN: u8 = 0x41;
const OP_SELFTEST: u8 = 0x77;

fn crc16(data: &[u8]) -> [u8; 2] {
    let mut crc: u16 = 0;
    for &byte in data {
        let mut mask = 0x01u8;
        while mask != 0 {
            let data_bit = (byte & mask != 0) as u16;
            let crc_bit = crc >> 15;
            crc <<= 1;
            if data_bit != crc_bit {
                crc ^= 0x8005;
            }
            mask = mask.wrapping_shl(1);
        }
    }
    crc.to_le_bytes()
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02X}")).collect()
}

struct Atecc<'d> {
    i2c: I2cDriver<'d>,
    addr: u8,
}

impl Atecc<'_> {
    /// Wake: hold SDA low >= 60 us (an address-0x00 write at 100 kHz does that
    /// for ~80 us and is NACKed), then wait tWHI (1.5 ms) and read the token.
    fn wake(&mut self) -> anyhow::Result<[u8; 4]> {
        let _ = self.i2c.write(0x00, &[0x00], I2C_TIMEOUT);
        Ets::delay_us(2_000);
        let mut tok = [0u8; 4];
        self.i2c
            .read(self.addr, &mut tok, I2C_TIMEOUT)
            .map_err(|e| anyhow::anyhow!("no wake response at 0x{:02X}: {e}", self.addr))?;
        Ok(tok)
    }

    fn sleep(&mut self) {
        let _ = self.i2c.write(self.addr, &[0x01], I2C_TIMEOUT);
    }

    /// Send one command and return its response payload (without count/CRC).
    /// A 1-byte payload is a status byte; non-zero means the command failed.
    fn cmd(
        &mut self,
        op: u8,
        p1: u8,
        p2: u16,
        data: &[u8],
        typ_ms: u32,
    ) -> anyhow::Result<Vec<u8>> {
        let mut pkt = vec![0x03, (7 + data.len()) as u8, op, p1];
        pkt.extend_from_slice(&p2.to_le_bytes());
        pkt.extend_from_slice(data);
        let crc = crc16(&pkt[1..]);
        pkt.extend_from_slice(&crc);
        self.i2c.write(self.addr, &pkt, I2C_TIMEOUT)?;

        FreeRtos::delay_ms(typ_ms.max(1));
        // The chip NACKs its address while it is still executing: poll.
        let mut count = [0u8; 1];
        let mut tries = 0;
        while self.i2c.read(self.addr, &mut count, I2C_TIMEOUT).is_err() {
            tries += 1;
            if tries > 150 {
                anyhow::bail!("op 0x{op:02X}: no response");
            }
            FreeRtos::delay_ms(2);
        }
        let n = count[0] as usize;
        if !(4..=155).contains(&n) {
            anyhow::bail!("op 0x{op:02X}: bad response length {n}");
        }
        let mut rest = vec![0u8; n - 1];
        self.i2c.read(self.addr, &mut rest, I2C_TIMEOUT)?;
        let mut full = vec![count[0]];
        full.extend_from_slice(&rest);
        if crc16(&full[..n - 2]) != full[n - 2..] {
            anyhow::bail!("op 0x{op:02X}: response CRC mismatch: {}", hex(&full));
        }
        Ok(full[1..n - 2].to_vec())
    }
}

fn status_name(s: u8) -> &'static str {
    match s {
        0x00 => "success",
        0x01 => "checkmac/verify miscompare",
        0x03 => "parse error",
        0x05 => "ECC fault",
        0x07 => "self-test error",
        0x08 => "health-test error",
        0x0F => "execution error (often: zone not locked / slot config forbids it)",
        0x11 => "after wake",
        0xEE => "watchdog about to expire",
        0xFF => "communication / CRC error",
        _ => "unknown",
    }
}

fn main() -> anyhow::Result<()> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();
    std::thread::sleep(Duration::from_millis(500));
    println!("\n=== ATECC608 PROBE START ===");

    let mut p = Peripherals::take()?;

    // 1. External pull-ups? Weak internal pull-DOWN; a line that still reads
    //    high is held up by a stronger external resistor (typ. 2.2k-10k).
    {
        // SAFETY: the drivers are dropped at the end of this block, before the
        // pins are handed to the I2C driver.
        let sda = PinDriver::input(unsafe { p.pins.gpio32.reborrow() }, Pull::Down)?;
        let scl = PinDriver::input(unsafe { p.pins.gpio25.reborrow() }, Pull::Down)?;
        Ets::delay_us(200);
        println!(
            "PULLUPS sda_gpio32={} scl_gpio25={} (1 = external pull-up present)",
            sda.is_high() as u8,
            scl.is_high() as u8
        );
    }

    let cfg = I2cConfig::new()
        .baudrate(100.kHz().into())
        .sda_enable_pullup(true)
        .scl_enable_pullup(true);
    let i2c = I2cDriver::new(p.i2c0, p.pins.gpio32, p.pins.gpio25, &cfg)?;
    let mut dev = Atecc { i2c, addr: 0x60 };

    // 2-3. Find the ATECC by its wake token. (No generic bus scan: an ATECC
    //      only ACKs while awake, and its ~1.3 s watchdog sends it back to
    //      sleep long before a 112-address scan reaches it.)
    let mut addr = None;
    for a in CANDIDATE_ADDRS {
        dev.addr = a;
        if let Ok(tok) = dev.wake() {
            println!("WAKE addr=0x{a:02X} token={}", hex(&tok));
            dev.sleep();
            if tok == WAKE_TOKEN {
                addr = Some(a);
                break;
            }
        }
    }
    let Some(addr) = addr else {
        println!("RESULT FAIL: no ATECC608 wake token on any address");
        println!("=== ATECC608 PROBE END ===");
        loop {
            FreeRtos::delay_ms(1000);
        }
    };
    dev.addr = addr;

    // 4. Revision.
    dev.wake()?;
    let rev = dev.cmd(OP_INFO, 0x00, 0, &[], 1)?;
    let model = match rev.get(2..4) {
        Some([0x60, 0x02]) => "ATECC608A",
        Some([0x60, 0x03]) => "ATECC608B",
        Some([0x50, _]) => "ATECC508A",
        _ => "unknown",
    };
    println!("INFO revision={} model={model}", hex(&rev));

    // 5. Configuration zone: 4 blocks of 32 bytes.
    let mut config = Vec::with_capacity(128);
    for block in 0u16..4 {
        let r = dev.cmd(OP_READ, 0x80, block << 3, &[], 1)?;
        if r.len() != 32 {
            anyhow::bail!(
                "config block {block}: status 0x{:02X} {}",
                r[0],
                status_name(r[0])
            );
        }
        config.extend_from_slice(&r);
    }
    dev.sleep();
    for (i, chunk) in config.chunks(16).enumerate() {
        println!("CONFIG {:03} {}", i * 16, hex(chunk));
    }
    let serial: Vec<u8> = config[0..4].iter().chain(&config[8..13]).copied().collect();
    let data_locked = config[86] != 0x55;
    let config_locked = config[87] != 0x55;
    println!("SERIAL {}", hex(&serial));
    println!(
        "ZONES i2c_addr_cfg=0x{:02X} chip_mode=0x{:02X} lock_data_otp=0x{:02X}({}) lock_config=0x{:02X}({}) slot_locked=0x{:02X}{:02X} chip_options=0x{:02X}{:02X}",
        config[16],
        config[19],
        config[86],
        if data_locked { "LOCKED" } else { "unlocked" },
        config[87],
        if config_locked { "LOCKED" } else { "unlocked" },
        config[89],
        config[88],
        config[91],
        config[90],
    );
    let mut private_p256 = Vec::new();
    for slot in 0..16usize {
        let sc = u16::from_le_bytes([config[20 + 2 * slot], config[21 + 2 * slot]]);
        let kc = u16::from_le_bytes([config[96 + 2 * slot], config[97 + 2 * slot]]);
        let key_type = (kc >> 2) & 0x7;
        let private = kc & 1 == 1;
        let kind = match (key_type, private) {
            (4, true) => "P256 PRIVATE",
            (4, false) => "P256 public",
            (6, _) => "AES",
            (7, _) => "SHA/data",
            _ => "other",
        };
        let ext_sign = private && key_type == 4 && sc & 0x1 != 0;
        let lock_bit_set =
            slot < 16 && (u16::from_le_bytes([config[88], config[89]]) >> slot) & 1 == 0;
        println!(
            "SLOT {slot:2} slot_config=0x{sc:04X} key_config=0x{kc:04X} {kind}{}{}",
            if ext_sign { " ext-sign" } else { "" },
            if lock_bit_set { " slot-locked" } else { "" },
        );
        if private && key_type == 4 {
            private_p256.push(slot as u16);
        }
    }

    // 6. Self-test (RNG, ECDSA sign/verify, ECDH, AES, SHA).
    dev.wake()?;
    match dev.cmd(OP_SELFTEST, 0x3F, 0, &[], 100) {
        Ok(r) => println!(
            "SELFTEST result=0x{:02X} ({})",
            r[0],
            if r[0] == 0 {
                "all pass"
            } else {
                "failure bitmap"
            }
        ),
        Err(e) => println!("SELFTEST error: {e}"),
    }

    // 7. RNG. With the config zone unlocked the chip returns a fixed
    //    FFFF0000 pattern instead of random bytes.
    let rnd = dev.cmd(OP_RANDOM, 0x00, 0, &[], 23)?;
    println!("RANDOM {}", hex(&rnd));
    dev.sleep();

    // 8. Keys: only meaningful once the data zone is locked.
    if !data_locked {
        println!("KEYS skipped: data zone unlocked, no usable key material yet");
    } else {
        println!("DIGEST {}", hex(&TEST_DIGEST));
        for slot in private_p256 {
            dev.wake()?;
            let pubkey = dev.cmd(OP_GENKEY, 0x00, slot, &[], 60)?;
            if pubkey.len() != 64 {
                println!(
                    "PUBKEY slot={slot} status=0x{:02X} {}",
                    pubkey[0],
                    status_name(pubkey[0])
                );
                dev.sleep();
                continue;
            }
            println!("PUBKEY slot={slot} {}", hex(&pubkey));
            let n = dev.cmd(OP_NONCE, 0x03, 0, &TEST_DIGEST, 7)?;
            if n[0] != 0 {
                println!(
                    "NONCE slot={slot} status=0x{:02X} {}",
                    n[0],
                    status_name(n[0])
                );
                dev.sleep();
                continue;
            }
            let sig = dev.cmd(OP_SIGN, 0x80, slot, &[], 60)?;
            if sig.len() == 64 {
                println!("SIG slot={slot} {}", hex(&sig));
            } else {
                println!(
                    "SIG slot={slot} status=0x{:02X} {}",
                    sig[0],
                    status_name(sig[0])
                );
            }
            dev.sleep();
        }
    }

    println!("RESULT OK");
    println!("=== ATECC608 PROBE END ===");
    loop {
        FreeRtos::delay_ms(1000);
    }
}
