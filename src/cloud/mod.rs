//! The link to the AWS IoT platform (iot-platform-infra, product `vaktus`):
//! identity in the ATECC608B, fleet provisioning with a CSR, MQTT with the
//! secure element, OTA updates through AWS IoT Jobs, and a shadow report.
//!
//! It runs on its own thread, next to the UI, and the dashboard never waits
//! for it: prayer times keep working with no cloud at all.
//!
//! Rollback: a freshly installed image is kept only once it has reached AWS
//! IoT ([`ota::mark_valid`]). [`start_rollback_guard`] restarts an image that
//! has not within [`VERIFY_DEADLINE`], whatever the UI is doing (stuck in WiFi
//! setup, say), and the bootloader then rolls it back.

mod atecc;
mod identity;
mod job_store;
mod jobs;
mod mqtt;
mod ota;
mod provisioning;

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use iot_jobs::client::Next;
use namaz_vakti_logic::cloud as wire;
use namaz_vakti_logic::language::Language;

use identity::IdentityStore;
use job_store::JobStore;
use jobs::{JobsClient, Link, Ota, RESTART_GRACE};
use mqtt::{Auth, Mqtt, MqttEvent};

const ROOT_CA: &str = include_str!("../../certs/AmazonRootCA1.pem");
const CLAIM_CERT: &str = include_str!("../../certs/claim.crt.pem");
const CLAIM_KEY: &str = include_str!("../../certs/claim.private.key");

/// How long a fresh OTA image has to reach AWS IoT before it is rolled back.
const VERIFY_DEADLINE: Duration = Duration::from_secs(10 * 60);
/// Provisioning retry backoff while the device is not registered yet.
const PROVISION_RETRY_MIN: Duration = Duration::from_secs(60);
const PROVISION_RETRY_MAX: Duration = Duration::from_secs(15 * 60);
/// Backoff before the cloud link starts over after an error (a chip or NVS
/// hiccup, a connection that dropped while it was being set up).
const RUN_RETRY_MIN: Duration = Duration::from_secs(30);
const RUN_RETRY_MAX: Duration = Duration::from_secs(10 * 60);
/// A run that lasted this long was healthy: the next error backs off from the
/// start again.
const RUN_HEALTHY: Duration = Duration::from_secs(10 * 60);
/// How often a failed Jobs subscription is tried again while connected.
const JOBS_RETRY: Duration = Duration::from_secs(30);
/// How often the shadow report is refreshed.
const REPORT_EVERY: Duration = Duration::from_secs(6 * 3600);
/// The MQTT input buffer: a `$next/get/accepted` with a presigned URL is
/// ~2.5 KB; larger messages are reassembled from chunks anyway.
const MQTT_BUFFER: i32 = 4096;
const CLOUD_STACK: usize = 16 * 1024;

static MARKED_VALID: AtomicBool = AtomicBool::new(false);
static STATUS: Mutex<String> = Mutex::new(String::new());

/// One line for the About page.
pub fn status_line() -> String {
    let s = STATUS.lock().map(|s| s.clone()).unwrap_or_default();
    if s.is_empty() {
        "-".into()
    } else {
        s
    }
}

fn set_status(s: impl Into<String>) {
    let s = s.into();
    log::info!("cloud: {s}");
    if let Ok(mut status) = STATUS.lock() {
        *status = s;
    }
}

/// Call first thing at boot: restarts an unverified OTA image that has not
/// reached AWS IoT in time, so the bootloader rolls it back.
pub fn start_rollback_guard() {
    if !ota::running_unverified() {
        return;
    }
    log::warn!(
        "running a new OTA image that is not verified yet; it is rolled back unless it reaches AWS IoT within {}s",
        VERIFY_DEADLINE.as_secs()
    );
    let spawned = std::thread::Builder::new()
        .name("rollback-guard".into())
        .stack_size(3 * 1024)
        .spawn(|| {
            std::thread::sleep(VERIFY_DEADLINE);
            if !MARKED_VALID.load(Ordering::SeqCst) {
                log::error!("the new image never reached AWS IoT; restarting so the bootloader rolls it back");
                unsafe { esp_idf_svc::sys::esp_restart() };
            }
        });
    if let Err(e) = spawned {
        log::error!("could not start the rollback guard: {e}");
    }
}

/// Starts the cloud thread. Call once WiFi is up and the clock is set.
pub fn spawn(nvs: EspDefaultNvsPartition, mac: [u8; 6]) {
    if crate::CONFIG.iot_endpoint.is_empty()
        || crate::CONFIG.provisioning_template.is_empty()
        || CLAIM_CERT.contains("PLACEHOLDER")
    {
        set_status("not configured in this build");
        log::warn!(
            "cloud: no IoT endpoint, template or claim identity in this build (see build.rs)"
        );
        return;
    }
    // Big enough for the TLS of the OTA download, which runs on this thread.
    let spawned = std::thread::Builder::new()
        .name("cloud".into())
        .stack_size(CLOUD_STACK)
        .spawn(move || {
            // Never give up: a cloud link that stopped would mean no OTA until
            // a power cycle, and on a fresh image a rollback by the guard.
            let mut backoff = RUN_RETRY_MIN;
            loop {
                let started = Instant::now();
                if let Err(e) = run(nvs.clone(), mac) {
                    log::error!("cloud: {e:#}");
                    set_status(format!("error, retrying: {e:#}"));
                }
                if started.elapsed() >= RUN_HEALTHY {
                    backoff = RUN_RETRY_MIN;
                }
                std::thread::sleep(backoff);
                backoff = (backoff * 2).min(RUN_RETRY_MAX);
            }
        });
    if let Err(e) = spawned {
        log::error!("could not start the cloud thread: {e}");
    }
}

// Few, small arguments on purpose: see mqtt::Settings on the Xtensa argument
// limit. The endpoint and template come from CONFIG.
fn run(nvs: EspDefaultNvsPartition, mac: [u8; 6]) -> Result<()> {
    let uri = format!("mqtts://{}:8883", crate::CONFIG.iot_endpoint);
    let claim = provisioning::Claim {
        uri: &uri,
        template: crate::CONFIG.provisioning_template,
        root_ca: ROOT_CA,
        cert: CLAIM_CERT,
        key: CLAIM_KEY,
    };

    // TLS checks certificate dates: wait for SNTP, which the UI started.
    while !clock_is_set() {
        set_status("waiting for the clock");
        std::thread::sleep(Duration::from_secs(5));
    }

    set_status("secure element...");
    let chip = atecc::ensure_identity().context("secure element")?;
    // What the operator registers (iot-platform-infra README, Device Registry).
    log::info!(
        "REGISTER product={} mac={} atecc_serial={} public_key={}",
        wire::PRODUCT,
        wire::mac_string(&mac),
        chip.serial,
        chip.public_key_hex()
    );

    let store = IdentityStore::new(nvs.clone())?;
    let stored = store.load().filter(|id| {
        let ours = id.public_key == chip.public_key_hex();
        if !ours {
            log::warn!(
                "cloud: the stored identity {} is not for this chip's key; provisioning again",
                id.thing_name
            );
        }
        ours
    });
    let identity = match stored {
        Some(id) => id,
        None => {
            let mut backoff = PROVISION_RETRY_MIN;
            loop {
                set_status("provisioning...");
                match provisioning::provision(&claim, &mac, &chip) {
                    Ok(id) => {
                        store.save(&id)?;
                        break id;
                    }
                    Err(provisioning::Failure::NotRegistered(why)) => {
                        log::warn!("provisioning refused by the registry: {why}");
                        set_status(format!(
                            "awaiting registration ({})",
                            wire::mac_string(&mac)
                        ));
                    }
                    Err(provisioning::Failure::Rejected(why)) => {
                        log::warn!("provisioning rejected: {why}");
                        set_status(format!("provisioning refused: {why}"));
                    }
                    Err(provisioning::Failure::Error(e)) => {
                        log::warn!("provisioning failed: {e:#}");
                        set_status("provisioning failed; retrying");
                    }
                }
                std::thread::sleep(backoff);
                backoff = (backoff * 2).min(PROVISION_RETRY_MAX);
            }
        }
    };

    set_status(format!("connecting as {}", identity.thing_name));
    let (client, events) = Mqtt::connect(&mqtt::Settings {
        uri: &uri,
        // The device policy only lets a thing connect with its own name.
        client_id: &identity.thing_name,
        root_ca: ROOT_CA,
        auth: Auth::SecureElement {
            cert: &identity.certificate_pem,
        },
        buffer_size: MQTT_BUFFER,
    })?;
    // esp-mqtt keeps reconnecting by itself; an unverified image is bounded by
    // the rollback guard, so there is no deadline here.
    loop {
        match mqtt::wait_connected(&events, Duration::from_secs(60)) {
            Ok(()) => break,
            Err(_) => log::warn!("cloud: still connecting to AWS IoT..."),
        }
    }
    set_status(format!("online: {}", identity.thing_name));

    // This image reached AWS IoT with its identity: keep it.
    let valid = ota::mark_valid();
    match &valid {
        Ok(()) => MARKED_VALID.store(true, Ordering::SeqCst),
        Err(e) => log::warn!("could not mark the firmware valid: {e:#}"),
    }
    let boot = ota::boot_facts(&valid);
    let running_slot = boot.running_slot.clone();

    let mut jobs = JobsClient::new(&identity.thing_name, JobStore::new(nvs.clone())?, Ota, boot)
        .with_product(wire::PRODUCT);
    let mut backlog: VecDeque<MqttEvent> = VecDeque::new();
    macro_rules! link {
        () => {
            &mut Link {
                client: &client,
                events: &events,
                backlog: &mut backlog,
            }
        };
    }
    // Not fatal: the connection may already have dropped again. Retried every
    // JOBS_RETRY until it goes through, and on every reconnect.
    let mut jobs_started = start_jobs(&mut jobs, link!());
    let mut last_jobs_start = Instant::now();
    let reporter = Reporter {
        client: &client,
        thing: &identity.thing_name,
        nvs: &nvs,
        chip: &chip,
        slot: &running_slot,
    };
    reporter.send();
    let mut last_report = Instant::now();
    let mut last_tick = Instant::now();

    loop {
        let event = match backlog.pop_front() {
            Some(e) => Ok(e),
            None => events.recv_timeout(Duration::from_secs(1)),
        };
        match event {
            Ok(MqttEvent::Connected) => {
                log::info!("cloud: MQTT reconnected; restoring the Jobs subscriptions");
                set_status(format!("online: {}", identity.thing_name));
                jobs_started = start_jobs(&mut jobs, link!());
                last_jobs_start = Instant::now();
            }
            Ok(MqttEvent::Disconnected) => set_status("offline; reconnecting"),
            Ok(MqttEvent::Message { topic, data }) => {
                if jobs.owns(&topic) && jobs.handle(&topic, &data, link!()) == Next::Restart {
                    set_status("update installed; restarting");
                    std::thread::sleep(RESTART_GRACE);
                    unsafe { esp_idf_svc::sys::esp_restart() };
                }
            }
            Err(_) => {}
        }
        if last_tick.elapsed() >= Duration::from_secs(1) {
            if !jobs_started && last_jobs_start.elapsed() >= JOBS_RETRY {
                jobs_started = start_jobs(&mut jobs, link!());
                last_jobs_start = Instant::now();
            }
            jobs.tick(link!(), Instant::now());
            last_tick = Instant::now();
        }
        if last_report.elapsed() >= REPORT_EVERY {
            reporter.send();
            last_report = Instant::now();
        }
    }
}

/// Subscribes the Jobs topics and asks for queued work; logs a failure.
fn start_jobs(jobs: &mut JobsClient, link: &mut Link<'_>) -> bool {
    match jobs.start(link) {
        Ok(()) => true,
        Err(e) => {
            log::warn!("cloud: Jobs subscriptions not set up yet: {e}");
            false
        }
    }
}

/// Publishes the `vaktus` shadow report.
struct Reporter<'a> {
    client: &'a Mqtt,
    thing: &'a str,
    nvs: &'a EspDefaultNvsPartition,
    chip: &'a atecc::Identity,
    slot: &'a str,
}

impl Reporter<'_> {
    /// Best effort.
    fn send(&self) {
        let (client, thing, nvs, chip, slot) =
            (self.client, self.thing, self.nvs, self.chip, self.slot);
        let location = crate::location::open(nvs.clone())
            .map(|n| crate::location::load(&n))
            .ok();
        let language = crate::settings::open(nvs.clone())
            .map(|n| crate::settings::load(&n).language)
            .unwrap_or_default();
        let report = wire::Report {
            product: wire::PRODUCT,
            fw: env!("CARGO_PKG_VERSION").into(),
            slot: slot.into(),
            secure_element: chip.serial.clone(),
            location: location
                .as_ref()
                .map(|l| l.district_name.clone())
                .unwrap_or_default(),
            location_id: location.map(|l| l.district_id).unwrap_or_default(),
            language: match language {
                Language::Turkish => "tr",
                Language::English => "en",
                Language::Nederlands => "nl",
            },
            free_heap: unsafe { esp_idf_svc::sys::esp_get_free_heap_size() },
        };
        if let Err(e) = client.publish(
            &wire::shadow_update_topic(thing),
            &wire::shadow_report(&report),
        ) {
            log::warn!("cloud: shadow report not sent: {e:#}");
        }
    }
}

/// True once SNTP has set the clock.
fn clock_is_set() -> bool {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() > 1_750_000_000)
        .unwrap_or(false)
}
