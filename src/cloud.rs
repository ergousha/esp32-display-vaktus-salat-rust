//! The link to the AWS IoT platform, on iot-device-sdk: identity in the
//! ATECC608B, enrolment with a CSR, OTA updates through AWS IoT Jobs, the
//! `platform` shadow (all the SDK's), and this product's `vaktus` shadow.
//!
//! It runs on its own thread, next to the UI, and the dashboard never waits
//! for it: prayer times keep working with no cloud at all.

use std::time::{Duration, Instant};

use device_sdk::{Event, Platform, Session};
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use namaz_vakti_logic::cloud as wire;
use namaz_vakti_logic::language::Language;

/// How often the `vaktus` shadow report is refreshed.
const REPORT_EVERY: Duration = Duration::from_secs(6 * 3600);
/// Backoff before the cloud link starts over after an error (a chip or NVS
/// hiccup); the SDK paces connections and enrolment itself.
const RUN_RETRY: Duration = Duration::from_secs(60);
const CLOUD_STACK: usize = 16 * 1024;

/// One line for the About page.
pub fn status_line() -> String {
    device_sdk::status()
}

/// Starts the cloud thread. Call once WiFi is up; it waits for the clock.
pub fn spawn(platform: Platform, nvs: EspDefaultNvsPartition) {
    // Big enough for the TLS of the OTA download, which runs on this thread.
    let spawned = std::thread::Builder::new()
        .name("cloud".into())
        .stack_size(CLOUD_STACK)
        .spawn(move || loop {
            // Never give up: a cloud link that stopped would mean no OTA
            // until a power cycle, and on a fresh image a rollback.
            if let Err(e) = run(&platform, &nvs) {
                log::error!("cloud: {e:#}");
            }
            std::thread::sleep(RUN_RETRY);
        });
    if let Err(e) = spawned {
        log::error!("could not start the cloud thread: {e}");
    }
}

fn run(platform: &Platform, nvs: &EspDefaultNvsPartition) -> anyhow::Result<()> {
    // TLS checks certificate dates: wait for SNTP, which the UI started.
    while !device_sdk::clock_is_set() {
        std::thread::sleep(Duration::from_secs(5));
    }
    let identity = platform.identity()?;
    let mut session = platform.connect(&identity, &Default::default())?;
    report(&session, nvs);
    let mut last_report = Instant::now();
    loop {
        match session.poll(Duration::from_secs(1)) {
            Some(Event::Connected) => {
                report(&session, nvs);
                last_report = Instant::now();
            }
            Some(Event::Restart) => device_sdk::restart_into_update(),
            // Vaktus subscribes to nothing of its own.
            Some(Event::Message { .. } | Event::Disconnected) | None => {}
        }
        if last_report.elapsed() >= REPORT_EVERY {
            report(&session, nvs);
            last_report = Instant::now();
        }
    }
}

/// Publishes the `vaktus` shadow report. Best effort.
fn report(session: &Session, nvs: &EspDefaultNvsPartition) {
    let location = crate::location::open(nvs.clone())
        .map(|n| crate::location::load(&n))
        .ok();
    let language = crate::settings::open(nvs.clone())
        .map(|n| crate::settings::load(&n).language)
        .unwrap_or_default();
    let report = wire::Report {
        product: crate::PLATFORM.id,
        fw: crate::PLATFORM.fw_version.into(),
        slot: device_sdk::running_slot(),
        secure_element: device_sdk::secure_element_serial().unwrap_or_default(),
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
    if let Err(e) = session.publish(
        &wire::shadow_update_topic(session.thing_name()),
        &wire::shadow_report(&report),
    ) {
        log::warn!("cloud: vaktus shadow report not sent: {e:#}");
    }
}
