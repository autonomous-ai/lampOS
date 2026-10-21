//! Presentation follows the body's confirmed fold; it never claims a fold itself.
use crate::Output;
use lamp_body::unplug::{Phase, REQUEST_TOPIC, Request, STATE_TOPIC, State};
use lamp_bus::{Bus, Kind, Message, Published, Publisher};
use lamp_msgs::ExpressionFace;
use std::{io, time::Duration};

const ACK_TOPIC: &str = "expression/unplug_state";

pub(crate) struct Unplug {
    bus: Bus,
    acknowledgement: Publisher,
    resting: f32,
    latched: bool,
}

impl Unplug {
    pub(crate) fn new(bus: &Bus, resting: f32) -> io::Result<Self> {
        let mut acknowledgement = bus.publisher(ACK_TOPIC, Kind::Latest, "expression")?;
        acknowledgement.expires_after(Some(Duration::from_millis(500)));
        Ok(Self {
            bus: bus.clone(),
            acknowledgement,
            resting,
            latched: false,
        })
    }

    pub(crate) fn apply(
        &mut self,
        output: &mut Output,
        at_us: u64,
        wall_ms: u64,
    ) -> Option<String> {
        let message = self.bus.latest(REQUEST_TOPIC).ok().flatten();
        let request = message
            .as_ref()
            .and_then(|message| message.parse::<Request>().ok())
            .filter(Request::valid);
        self.latched |= request.is_some();
        if self.latched {
            output.body = None;
            output.state.body = false;
        }
        // In shadow nothing is presented, so nothing is acknowledged.
        if !output.state.enabled {
            return None;
        }
        let request = request?;
        let state = self.bus.latest(STATE_TOPIC).ok().flatten()?;
        if !ready(
            &request.request,
            message.as_ref()?.header.at_us,
            &state,
            at_us,
            wall_ms,
        ) {
            return None;
        }
        // Privacy authority is independent of voice and is never forged for sleep.
        let privacy = self.bus.latest(lamp_msgs::MicrophonePrivacy::TOPIC).ok()?;
        if crate::runtime::privacy_muted_in(privacy) {
            return None;
        }
        // With no light of its own to send (a voice turn cancelled, or no
        // fresh voice timing), Expression still asks nothing of the body and
        // the ring draws asleep itself: lamp-52e6 on 2026-10-20 never
        // acknowledged a Ready body while it waited for one (#1699).
        if let Some(light) = output.light.as_mut() {
            light.face = ExpressionFace::Resting;
            light.brightness = if light.setting.on { self.resting } else { 0.0 };
        }
        output.state.behaviours = vec!["asleep".into(), "ready to unplug".into()];
        Some(request.request)
    }

    /// Called only after the light publication succeeded, or when there was
    /// no light to publish; never in shadow.
    pub(crate) fn acknowledge(&mut self, at_us: u64, request: &str) -> io::Result<Published> {
        self.acknowledgement.publish_json(
            at_us,
            &serde_json::json!({
                "request": request, "asleep": true
            }),
        )
    }
}

fn ready(request: &str, requested_us: u64, message: &Message, at_us: u64, wall_ms: u64) -> bool {
    !message.expired
        && message.header.source == "body"
        && message
            .header
            .expires_ms
            .is_some_and(|ms| ms > 0 && ms <= 500)
        && requested_us <= message.header.at_us
        && message.header.at_us <= at_us
        && at_us.saturating_sub(message.header.at_us) <= 500_000
        && message.header.at_unix_ms <= wall_ms.saturating_add(50)
        && wall_ms.saturating_sub(message.header.at_unix_ms) <= 500
        && !message.header.expired_at(wall_ms)
        && message
            .parse::<State>()
            .ok()
            .is_some_and(|state| state.request == request && state.phase == Phase::Ready)
}
