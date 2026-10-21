//! `memory/forgot`: memory tells the lamp's other parts that it has
//! forgotten everything it kept under a person, asked by the app or by
//! voice, so each lets its own go too (the first builder, 10:5x on #2467).
//! The person's id only, never a name. Published after the asker has its
//! answer; a bus that can't be written to is said in the journal and tried
//! again at the next forget.
use crate::protocol::Forgot;
use lamp_bus::{Bus, Publisher};
use lamp_msgs::MemoryForgot;
use std::{io, path::Path};

/// Memory's source on the bus.
const SOURCE: &str = "memory";

pub struct ForgotOut {
    bus: Bus,
    publisher: Option<Publisher>,
}

impl ForgotOut {
    pub fn on(root: &Path) -> io::Result<Self> {
        Ok(Self {
            bus: Bus::new(root)?,
            publisher: None,
        })
    }

    /// What the message says of `forgot`.
    pub fn message(forgot: &Forgot) -> MemoryForgot {
        MemoryForgot {
            person: match forgot {
                Forgot::One(person) => Some(person.clone()),
                Forgot::Every => None,
            },
        }
    }

    /// Tells `forgot`, unless its person is no id of memory's own (a slug
    /// a caller chose, which could be a name): that is never told
    /// ([`MemoryForgot::check`]).
    pub fn tell(&mut self, forgot: &Forgot) -> io::Result<()> {
        let message = Self::message(forgot);
        if message.check().is_err() {
            return Ok(());
        }
        if self.publisher.is_none() {
            self.publisher = Some(self.bus.publisher_declared(
                MemoryForgot::TOPIC,
                &MemoryForgot::declaration(),
                SOURCE,
            )?);
        }
        let result = self
            .publisher
            .as_mut()
            .expect("just made")
            .publish_json(lamp_ipc::monotonic_us(), &message)
            .map(|_| ());
        if result.is_err() {
            // Made again at the next forget, as the health reports' is.
            self.publisher = None;
        }
        result
    }
}
