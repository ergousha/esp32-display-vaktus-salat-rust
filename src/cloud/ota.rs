//! Writes an OTA image into the inactive slot, and what the running image
//! knows about itself. Ported from esp32-opcua-gateway-rust (src/ota.rs).

use std::ffi::CStr;
use std::ptr;

use anyhow::{bail, Context, Result};
use embedded_svc::http::Headers;
use esp_idf_svc::http::client::{Configuration as HttpConfiguration, EspHttpConnection};
use esp_idf_svc::ota::EspOta;
use esp_idf_svc::sys::{
    esp, esp_ota_abort, esp_ota_begin, esp_ota_end, esp_ota_get_last_invalid_partition,
    esp_ota_get_next_update_partition, esp_ota_get_running_partition, esp_ota_get_state_partition,
    esp_ota_handle_t, esp_ota_img_states_t, esp_ota_img_states_t_ESP_OTA_IMG_NEW,
    esp_ota_img_states_t_ESP_OTA_IMG_PENDING_VERIFY, esp_ota_set_boot_partition, esp_ota_write,
    esp_partition_t, ESP_OK, OTA_WITH_SEQUENTIAL_WRITES,
};
use iot_jobs::BootFacts;
use log::{error, info};

/// Download attempts per update before it is given up (and the job reported
/// FAILED). A failed read resumes where it stopped (HTTP Range), so these cost
/// only what was lost. The usual cause is transient: with
/// CONFIG_MBEDTLS_DYNAMIC_BUFFER every incoming TLS record needs a fresh
/// ~16.7 KB block, and with ~57 KB free and the MQTT session receiving at the
/// same moment, one occasionally is not there (measured on the bench unit).
const MAX_DOWNLOAD_TRIES: u32 = 5;

/// Performs an Over-The-Air (OTA) update from the given HTTP(S) URL.
///
/// `before_activate` is called with the label of the slot the image went to,
/// once the image is written and verified and before that slot becomes the
/// boot slot. If it fails, the update is abandoned and the running image stays
/// the one that boots. Returns the slot label.
pub fn perform_ota(url: &str, before_activate: impl FnOnce(&str) -> Result<()>) -> Result<String> {
    info!(
        "Starting OTA firmware download (free heap {} B, largest block {} B).",
        unsafe { esp_idf_svc::sys::esp_get_free_heap_size() },
        largest_free_block()
    );

    // Raw esp_ota_* rather than EspOta: EspOta::initiate_update passes
    // OTA_SIZE_UNKNOWN, which erases the whole 1984 KiB slot up front. Flash
    // erases stall both cores, so the clock and touch would freeze for
    // seconds. OTA_WITH_SEQUENTIAL_WRITES erases each sector just before it
    // is written instead.
    // SAFETY: the partition pointer comes from the static partition table.
    let partition = unsafe { esp_ota_get_next_update_partition(ptr::null()) };
    let slot = label(partition).context("no OTA partition to update")?;
    let mut handle: esp_ota_handle_t = 0;
    esp!(unsafe { esp_ota_begin(partition, OTA_WITH_SEQUENTIAL_WRITES as usize, &mut handle,) })
        .context("starting the OTA write")?;
    // Frees the handle on any early return; disarmed once esp_ota_end ran.
    let mut guard = AbortOnDrop(Some(handle));

    info!("Downloading and writing firmware to OTA partition {slot}...");
    let mut download = Download {
        url,
        handle,
        written: 0,
        total: None,
    };
    let mut tries = 1;
    loop {
        match download.resume() {
            Ok(()) => break,
            Err(e) if tries < MAX_DOWNLOAD_TRIES => {
                log::warn!(
                    "OTA download interrupted at {} bytes ({e:#}); resuming, try {} of {MAX_DOWNLOAD_TRIES} \
                     (largest free block {} B)",
                    download.written,
                    tries + 1,
                    largest_free_block()
                );
                std::thread::sleep(std::time::Duration::from_secs(2 * u64::from(tries)));
                tries += 1;
            }
            Err(e) => {
                return Err(e).context(format!("OTA download failed {MAX_DOWNLOAD_TRIES} times"))
            }
        }
    }
    info!("Download complete. Total {} bytes.", download.written);

    // Verify the image (esp_ota_end), then set the boot partition, with the
    // caller's step in between.
    guard.0 = None;
    esp!(unsafe { esp_ota_end(handle) }).context("verifying the OTA image")?;
    before_activate(&slot).context("preparing to activate the new image")?;
    esp!(unsafe { esp_ota_set_boot_partition(partition) }).context("activating the new image")?;
    info!("OTA update successful. Ready for reboot.");

    Ok(slot)
}

/// An image download in progress, resumable from `written`.
struct Download<'a> {
    url: &'a str,
    handle: esp_ota_handle_t,
    /// Bytes written to the slot so far.
    written: usize,
    /// Image size, from the first response's Content-Length.
    total: Option<usize>,
}

impl Download<'_> {
    /// Fetches the rest of the image (from `written` on) and writes it.
    /// Returns once the whole image is written; an error leaves `written` at
    /// what was written, for the next call to resume from.
    fn resume(&mut self) -> Result<()> {
        let http_config = HttpConfiguration {
            crt_bundle_attach: Some(esp_idf_svc::sys::esp_crt_bundle_attach),
            // The request line carries the presigned URL (~1.5 KB).
            buffer_size_tx: Some(4096),
            buffer_size: Some(4096),
            ..Default::default()
        };
        let mut connection = EspHttpConnection::new(&http_config)?;
        let range = format!("bytes={}-", self.written);
        let mut headers = vec![("Accept", "application/octet-stream")];
        if self.written > 0 {
            headers.push(("Range", range.as_str()));
        }
        connection.initiate_request(esp_idf_svc::http::Method::Get, self.url, &headers)?;
        connection.initiate_response()?;

        let status = connection.status();
        let expected = if self.written == 0 { 200 } else { 206 };
        if status != expected {
            error!("OTA download: HTTP status {status}, expected {expected}");
            bail!("HTTP status {status} (expected {expected})");
        }
        if self.written == 0 {
            self.total = connection.content_len().map(|n| n as usize);
        } else {
            // "bytes <first>-<last>/<size>": it must continue where we stopped.
            let range = connection.header("Content-Range").unwrap_or_default();
            let first = range
                .strip_prefix("bytes ")
                .and_then(|r| r.split('-').next())
                .and_then(|n| n.parse::<usize>().ok());
            if first != Some(self.written) {
                bail!(
                    "resumed at the wrong offset (Content-Range {range:?}, wanted {})",
                    self.written
                );
            }
        }

        let mut buf = [0u8; 4096];
        loop {
            let n = connection.read(&mut buf)?;
            if n == 0 {
                break; // EOF
            }
            esp!(unsafe { esp_ota_write(self.handle, buf.as_ptr().cast(), n) })
                .context("writing the OTA image")?;
            let before = self.written;
            self.written += n;
            if self.written / (4096 * 40) != before / (4096 * 40) {
                info!("Downloaded {} bytes", self.written);
            }
        }
        if let Some(total) = self.total {
            if self.written < total {
                bail!("connection closed at {} of {total} bytes", self.written);
            }
        }
        Ok(())
    }
}

/// Largest block the 8-bit heap can hand out, for the download logs.
fn largest_free_block() -> usize {
    unsafe { esp_idf_svc::sys::heap_caps_get_largest_free_block(esp_idf_svc::sys::MALLOC_CAP_8BIT) }
}

/// Aborts an unfinished OTA write, freeing its handle.
struct AbortOnDrop(Option<esp_ota_handle_t>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        if let Some(handle) = self.0.take() {
            // SAFETY: a handle from esp_ota_begin that esp_ota_end has not
            // consumed.
            unsafe { esp_ota_abort(handle) };
        }
    }
}

/// Marks the current running firmware as valid so it won't rollback on next boot.
pub fn mark_valid() -> Result<()> {
    let mut ota = EspOta::new()?;
    ota.mark_running_slot_valid()?;
    info!("Firmware marked as valid (no rollback).");
    Ok(())
}

/// What this boot can tell about its own image, for settling a pending OTA
/// job. `valid` is the result of [`mark_valid`].
///
/// Never fails: a slot that cannot be read is reported as unknown, and
/// `iot_jobs::settle` then decides on the version alone.
pub fn boot_facts(valid: &Result<()>) -> BootFacts {
    // Sound: both return a pointer into the static partition table, or null.
    let running_slot = label(unsafe { esp_ota_get_running_partition() }).unwrap_or_default();
    let invalid_slot = label(unsafe { esp_ota_get_last_invalid_partition() });
    BootFacts {
        running_version: env!("CARGO_PKG_VERSION").to_string(),
        running_slot,
        invalid_slot,
        valid_error: valid.as_ref().err().map(|e| format!("{e:#}")),
    }
}

/// True while the running image is a fresh OTA image that has not marked
/// itself valid, i.e. one the bootloader rolls back if the device resets now.
pub fn running_unverified() -> bool {
    let mut state: esp_ota_img_states_t = Default::default();
    // Sound: the running partition is never null in a booted app, and
    // `state` outlives the call. An image without an otadata record reports
    // ESP_ERR_NOT_FOUND, and is not unverified either.
    let err = unsafe { esp_ota_get_state_partition(esp_ota_get_running_partition(), &mut state) };
    err == ESP_OK
        && (state == esp_ota_img_states_t_ESP_OTA_IMG_PENDING_VERIFY
            || state == esp_ota_img_states_t_ESP_OTA_IMG_NEW)
}

/// Label of a partition from the partition table, e.g. `ota_1`.
///
/// Read directly rather than through `EspOta::get_*_slot`, which also parses
/// the image's app descriptor into a 24-byte version string and fails outright
/// when the image in the slot has a longer one. ESP-IDF allows 32, and the
/// version it records is `git describe` of the build tree, e.g.
/// `v0.0.1-5-gea84f5a-dirty`, which is already 23.
fn label(partition: *const esp_partition_t) -> Option<String> {
    // Sound: a non-null pointer from the OTA API points into the partition
    // table, which lives for the whole program, and ESP-IDF keeps `label`
    // NUL-terminated (17 bytes for at most 16 characters).
    let partition = unsafe { partition.as_ref() }?;
    let label = unsafe { CStr::from_ptr(partition.label.as_ptr()) };
    let label = label.to_string_lossy().into_owned();
    (!label.is_empty()).then_some(label)
}
