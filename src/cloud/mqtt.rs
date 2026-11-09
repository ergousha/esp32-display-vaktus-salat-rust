//! MQTT over mutual TLS, on the raw ESP-IDF `esp-mqtt` API.
//!
//! esp-idf-svc's `MqttClientConfiguration` (0.52) cannot set
//! `credentials.authentication.use_secure_element`, which the device
//! connection needs: its private key is in the ATECC608B, and esp-tls signs
//! the TLS handshake there (slot 0). So this wraps the few C calls itself.
//! Events are copied in the esp-mqtt task and handed over on a channel.

use std::ffi::{c_void, CString};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Mutex;
use std::time::Duration;

use anyhow::{bail, Result};
use esp_idf_svc::sys::{
    esp_event_base_t, esp_mqtt_client_config_t, esp_mqtt_client_destroy, esp_mqtt_client_handle_t,
    esp_mqtt_client_init, esp_mqtt_client_publish, esp_mqtt_client_register_event,
    esp_mqtt_client_start, esp_mqtt_client_subscribe_single, esp_mqtt_client_unsubscribe,
    esp_mqtt_event_id_t_MQTT_EVENT_ANY, esp_mqtt_event_id_t_MQTT_EVENT_CONNECTED,
    esp_mqtt_event_id_t_MQTT_EVENT_DATA, esp_mqtt_event_id_t_MQTT_EVENT_DISCONNECTED,
    esp_mqtt_event_t, ESP_OK,
};

/// QoS 1 for everything: AWS IoT does not support QoS 2.
pub const QOS1: i32 = 1;

/// Largest message reassembled from chunks. A Jobs document with a presigned
/// URL is ~3 KB; the length comes from the broker, and an allocation failure
/// inside the esp-mqtt task would abort the firmware.
const MAX_MESSAGE: usize = 16 * 1024;

/// An event copied out of the esp-mqtt task.
#[derive(Debug)]
pub enum MqttEvent {
    Connected,
    Disconnected,
    Message { topic: String, data: Vec<u8> },
}

/// How the client proves its identity.
pub enum Auth<'a> {
    /// Certificate and private key in RAM (the shared claim identity).
    Pem { cert: &'a str, key: &'a str },
    /// Certificate in RAM, key in the ATECC608B (the device identity).
    SecureElement { cert: &'a str },
}

/// Everything a connection needs, passed by reference.
///
/// One struct rather than separate arguments: on Xtensa, a call with more
/// than six argument words (each `&str` is two) has its later arguments read
/// wrong by this toolchain. Five `&str`s here once arrived as a 1 GB length
/// (see esp32-opcua-gateway-rust `mqtt_util::Creds`).
pub struct Settings<'a> {
    /// `mqtts://host:8883`
    pub uri: &'a str,
    pub client_id: &'a str,
    pub root_ca: &'a str,
    pub auth: Auth<'a>,
    pub buffer_size: i32,
}

/// What the event callback needs; boxed so its address is stable.
struct Context {
    tx: Mutex<Sender<MqttEvent>>,
    /// A message larger than the input buffer arrives in chunks; only the
    /// first carries the topic.
    partial: Mutex<Option<(String, Vec<u8>, usize)>>,
}

/// A running client. Dropping it disconnects and frees everything.
pub struct Mqtt {
    handle: esp_mqtt_client_handle_t,
    // Referenced by the client until it is destroyed.
    _context: Box<Context>,
    _strings: Vec<CString>,
}

// The handle is used from one thread at a time; esp-mqtt locks internally.
unsafe impl Send for Mqtt {}

impl Mqtt {
    /// Starts connecting. The connection comes up asynchronously: wait for
    /// [`MqttEvent::Connected`].
    pub fn connect(settings: &Settings<'_>) -> Result<(Self, Receiver<MqttEvent>)> {
        let c = |s: &str| CString::new(s).map_err(|_| anyhow::anyhow!("NUL in MQTT setting"));
        let uri = c(settings.uri)?;
        let client_id = c(settings.client_id)?;
        let root_ca = c(settings.root_ca)?;
        let buffer_size = settings.buffer_size;
        let (cert, key, secure_element) = match settings.auth {
            Auth::Pem { cert, key } => (c(cert)?, Some(c(key)?), false),
            Auth::SecureElement { cert } => (c(cert)?, None, true),
        };

        // SAFETY: a zeroed config is esp-mqtt's "all defaults"; every pointer
        // set below points into a CString kept alive in `_strings`.
        let mut cfg: esp_mqtt_client_config_t = unsafe { core::mem::zeroed() };
        cfg.broker.address.uri = uri.as_ptr();
        // PEM, NUL-terminated: a zero length means "up to the NUL".
        cfg.broker.verification.certificate = root_ca.as_ptr();
        cfg.credentials.client_id = client_id.as_ptr();
        cfg.credentials.authentication.certificate = cert.as_ptr();
        if let Some(key) = &key {
            cfg.credentials.authentication.key = key.as_ptr();
        }
        cfg.credentials.authentication.use_secure_element = secure_element;
        cfg.session.keepalive = 60;
        cfg.network.reconnect_timeout_ms = 10_000;
        cfg.network.timeout_ms = 20_000;
        cfg.buffer.size = buffer_size;
        cfg.buffer.out_size = buffer_size;
        // The TLS handshake (and with the secure element, the ATECC calls)
        // runs on this task.
        cfg.task.stack_size = 8192;

        let (tx, rx) = channel();
        let context = Box::new(Context {
            tx: Mutex::new(tx),
            partial: Mutex::new(None),
        });

        // SAFETY: `cfg` and every string it points to outlive the call; the
        // client copies what it keeps or keeps pointing at `_strings`.
        let handle = unsafe { esp_mqtt_client_init(&cfg) };
        if handle.is_null() {
            bail!("esp_mqtt_client_init failed");
        }
        let mut strings = vec![uri, client_id, root_ca, cert];
        strings.extend(key);
        let client = Self {
            handle,
            _context: context,
            _strings: strings,
        };

        let arg = &*client._context as *const Context as *mut c_void;
        // SAFETY: `arg` stays valid until `esp_mqtt_client_destroy` in Drop,
        // after which no more events are delivered.
        let err = unsafe {
            esp_mqtt_client_register_event(
                handle,
                esp_mqtt_event_id_t_MQTT_EVENT_ANY,
                Some(on_event),
                arg,
            )
        };
        if err != ESP_OK {
            bail!("esp_mqtt_client_register_event: {err}");
        }
        let err = unsafe { esp_mqtt_client_start(handle) };
        if err != ESP_OK {
            bail!("esp_mqtt_client_start: {err}");
        }
        Ok((client, rx))
    }

    pub fn publish(&self, topic: &str, payload: &[u8]) -> Result<()> {
        let topic_c = CString::new(topic)?;
        // SAFETY: both buffers outlive the call; esp-mqtt copies them.
        let id = unsafe {
            esp_mqtt_client_publish(
                self.handle,
                topic_c.as_ptr(),
                payload.as_ptr().cast(),
                payload.len() as i32,
                QOS1,
                0,
            )
        };
        if id < 0 {
            bail!("publishing to {topic} failed ({id})");
        }
        Ok(())
    }

    pub fn subscribe(&self, topic: &str) -> Result<()> {
        let topic_c = CString::new(topic)?;
        let id = unsafe { esp_mqtt_client_subscribe_single(self.handle, topic_c.as_ptr(), QOS1) };
        if id < 0 {
            bail!("subscribing to {topic} failed ({id})");
        }
        Ok(())
    }

    pub fn unsubscribe(&self, topic: &str) -> Result<()> {
        let topic_c = CString::new(topic)?;
        let id = unsafe { esp_mqtt_client_unsubscribe(self.handle, topic_c.as_ptr()) };
        if id < 0 {
            bail!("unsubscribing from {topic} failed ({id})");
        }
        Ok(())
    }
}

impl Drop for Mqtt {
    fn drop(&mut self) {
        // SAFETY: the handle came from esp_mqtt_client_init and is destroyed
        // once; this also stops the task, so the context is no longer used.
        unsafe { esp_mqtt_client_destroy(self.handle) };
    }
}

/// Waits up to `timeout` for the connection to come up.
pub fn wait_connected(events: &Receiver<MqttEvent>, timeout: Duration) -> Result<()> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        match events.recv_timeout(left) {
            Ok(MqttEvent::Connected) => return Ok(()),
            Ok(_) => continue,
            Err(_) => bail!("no MQTT connection within {}s", timeout.as_secs()),
        }
    }
}

unsafe extern "C" fn on_event(
    arg: *mut c_void,
    _base: esp_event_base_t,
    event_id: i32,
    event_data: *mut c_void,
) {
    // SAFETY: `arg` is the boxed Context (see `connect`), `event_data` the
    // esp_mqtt_event_t esp-mqtt passes for the duration of the call.
    let ctx = unsafe { &*(arg as *const Context) };
    let send = |e| {
        if let Ok(tx) = ctx.tx.lock() {
            let _ = tx.send(e);
        }
    };
    match event_id {
        id if id == esp_mqtt_event_id_t_MQTT_EVENT_CONNECTED => send(MqttEvent::Connected),
        id if id == esp_mqtt_event_id_t_MQTT_EVENT_DISCONNECTED => send(MqttEvent::Disconnected),
        id if id == esp_mqtt_event_id_t_MQTT_EVENT_DATA => {
            let ev = unsafe { &*(event_data as *const esp_mqtt_event_t) };
            let bytes = |p: *const core::ffi::c_char, n: i32| -> &[u8] {
                if p.is_null() || n <= 0 {
                    &[]
                } else {
                    unsafe { core::slice::from_raw_parts(p.cast(), n as usize) }
                }
            };
            let data = bytes(ev.data, ev.data_len);
            let total = ev.total_data_len.max(0) as usize;
            if total > MAX_MESSAGE {
                if ev.current_data_offset == 0 {
                    log::warn!("dropping a {total} B MQTT message (limit {MAX_MESSAGE} B)");
                }
                return;
            }
            let Ok(mut partial) = ctx.partial.lock() else {
                return;
            };
            if ev.current_data_offset == 0 {
                let topic = String::from_utf8_lossy(bytes(ev.topic, ev.topic_len)).into_owned();
                if data.len() >= total {
                    send(MqttEvent::Message {
                        topic,
                        data: data.to_vec(),
                    });
                    return;
                }
                let mut buf = Vec::with_capacity(total);
                buf.extend_from_slice(data);
                *partial = Some((topic, buf, total));
                return;
            }
            if let Some((_, buf, total)) = partial.as_mut() {
                buf.extend_from_slice(data);
                if buf.len() >= *total {
                    let (topic, data, _) = partial.take().expect("checked above");
                    send(MqttEvent::Message { topic, data });
                }
            }
        }
        _ => {}
    }
}
