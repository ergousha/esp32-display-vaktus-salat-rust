//! AWS IoT Jobs firmware updates: the shared `iot-jobs` state machine (from
//! esp32-opcua-gateway-rust, host-tested there) on this device's MQTT
//! ([`Link`]), NVS ([`JobStore`]) and OTA partitions ([`Ota`]).

use std::collections::VecDeque;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use iot_jobs::client::{self, Installer, Transport};

use super::job_store::JobStore;
use super::mqtt::{Mqtt, MqttEvent};

pub type JobsClient = client::JobsClient<JobStore, Ota>;

/// How long the status that precedes a restart into a new image gets to reach
/// the wire. Losing it is not fatal: the next boot settles the job from NVS.
pub const RESTART_GRACE: Duration = Duration::from_millis(1500);

/// The MQTT connection as the Jobs client sees it. Messages that arrive while
/// it waits for its own answer go to `backlog`, which the cloud loop drains
/// before reading the channel again.
pub struct Link<'a> {
    pub client: &'a Mqtt,
    pub events: &'a Receiver<MqttEvent>,
    pub backlog: &'a mut VecDeque<MqttEvent>,
}

impl Transport for Link<'_> {
    fn publish(&mut self, topic: &str, payload: &[u8]) -> Result<(), String> {
        self.client
            .publish(topic, payload)
            .map_err(|e| format!("{e:#}"))
    }

    fn subscribe(&mut self, topic: &str) -> Result<(), String> {
        self.client.subscribe(topic).map_err(|e| format!("{e:#}"))
    }

    fn unsubscribe(&mut self, topic: &str) -> Result<(), String> {
        self.client.unsubscribe(topic).map_err(|e| format!("{e:#}"))
    }

    fn wait_for(&mut self, topics: &[&str], timeout: Duration) -> Option<(String, Vec<u8>)> {
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.events.recv_timeout(left) {
                Ok(MqttEvent::Message { topic, data }) if topics.contains(&topic.as_str()) => {
                    return Some((topic, data));
                }
                Ok(other) => self.backlog.push_back(other),
                Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => return None,
            }
        }
    }
}

/// The OTA partitions.
pub struct Ota;

impl Installer for Ota {
    fn install(
        &mut self,
        url: &str,
        before_activate: &mut dyn FnMut(&str) -> Result<(), String>,
    ) -> Result<String, String> {
        super::ota::perform_ota(url, |slot| {
            before_activate(slot).map_err(anyhow::Error::msg)
        })
        .map_err(|e| format!("{e:#}"))
    }
}
