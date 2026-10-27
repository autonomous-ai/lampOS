//! The setup light (#806): what the ring shows while a lamp is being set up.
//! Then nothing else of the lamp runs: no voice, so no turn and no permit to
//! hold. Like the asleep face it needs none: [`SetupRing`] can show the
//! setup colours over the whole ring, no brighter than [`PEAK_CHANNEL`], and
//! black, and nothing else. `lamp-setup` chooses which and when.

use crate::{ChannelCeiling, EncodedFrame, FrameSink, PIXEL_COUNT, Rgb, encode};
use std::io;

/// What the setup light says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetupLook {
    /// The hotspot is up and the lamp waits for its person: soft white.
    Waiting,
    /// The person pressed Setup and the lamp is joining the home's Wi-Fi:
    /// soft blue, as V1 showed blue while joining.
    Joining,
    /// The last join failed and the hotspot is back for another try: amber.
    Failed,
    /// The lamp has shut down and may be unplugged (#3697): soft green,
    /// steady, left on as the power goes.
    Unplug,
}

/// The brightest any channel gets: the product voice's own ring ceiling
/// (`--ring-channel-ceiling 24`), so setup is no brighter than the lamp after it.
pub const PEAK_CHANNEL: u8 = 24;

impl SetupLook {
    /// Its colour at full level. No channel is over [`PEAK_CHANNEL`].
    pub const fn colour(self) -> Rgb {
        match self {
            Self::Waiting => Rgb::new(24, 20, 14),
            Self::Joining => Rgb::new(4, 12, 24),
            Self::Failed => Rgb::new(24, 8, 0),
            Self::Unplug => Rgb::new(2, 16, 6),
        }
    }
}

/// The whole ring in one colour: `share` (0 to 1) of the way from `from`'s
/// colour to `to`'s, at `level` (0 to 1) of it. Every channel stays at or
/// under the brighter of the two colours'.
pub fn setup_pixels(from: SetupLook, to: SetupLook, share: f32, level: f32) -> [Rgb; PIXEL_COUNT] {
    let share = share.clamp(0.0, 1.0);
    let level = level.clamp(0.0, 1.0);
    let (a, b) = (from.colour(), to.colour());
    let mix = |a: u8, b: u8| {
        let mixed = f32::from(a) + (f32::from(b) - f32::from(a)) * share;
        (mixed * level).round() as u8
    };
    [Rgb::new(
        mix(a.red, b.red),
        mix(a.green, b.green),
        mix(a.blue, b.blue),
    ); PIXEL_COUNT]
}

/// A ring writer that shows only the setup light or black. Opening it sends
/// black; [`SetupRing::shutdown`] sends black again. It holds no authority
/// and grants none: `lamp-setup` uses it only while the lamp is not set up,
/// and closes it before the lamp starts and the voice opens the device.
pub struct SetupRing<S: FrameSink> {
    sink: S,
}

impl<S: FrameSink> SetupRing<S> {
    pub fn new(mut sink: S) -> io::Result<Self> {
        sink.write_frame(&EncodedFrame::black())?;
        Ok(Self { sink })
    }

    /// Show [`setup_pixels`] for these.
    pub fn show(
        &mut self,
        from: SetupLook,
        to: SetupLook,
        share: f32,
        level: f32,
    ) -> io::Result<()> {
        self.sink.write_frame(&encode(
            &setup_pixels(from, to, share, level),
            ChannelCeiling(PEAK_CHANNEL),
        ))
    }

    /// Black, then the writer is gone.
    pub fn shutdown(mut self) -> io::Result<()> {
        self.sink.write_frame(&EncodedFrame::black())
    }

    /// The writer goes and its last frame stays on the ring: the
    /// safe-to-unplug light after the shutdown (#3697).
    pub fn leave(self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOOKS: [SetupLook; 4] = [
        SetupLook::Waiting,
        SetupLook::Joining,
        SetupLook::Failed,
        SetupLook::Unplug,
    ];

    #[test]
    fn the_whole_ring_is_one_colour_no_brighter_than_the_peak() {
        for from in LOOKS {
            for to in LOOKS {
                for share in [0.0, 0.3, 1.0] {
                    for level in [0.0, 0.35, 1.0] {
                        let pixels = setup_pixels(from, to, share, level);
                        assert!(pixels.iter().all(|pixel| *pixel == pixels[0]));
                        let Rgb { red, green, blue } = pixels[0];
                        assert!(red.max(green).max(blue) <= PEAK_CHANNEL);
                    }
                }
            }
        }
        assert_eq!(
            setup_pixels(SetupLook::Waiting, SetupLook::Waiting, 0.0, 0.0)[0],
            Rgb::BLACK
        );
    }

    #[test]
    fn a_change_goes_from_one_colour_to_the_other() {
        let start = setup_pixels(SetupLook::Waiting, SetupLook::Joining, 0.0, 1.0)[0];
        let end = setup_pixels(SetupLook::Waiting, SetupLook::Joining, 1.0, 1.0)[0];
        assert_eq!(start, SetupLook::Waiting.colour());
        assert_eq!(end, SetupLook::Joining.colour());
        let half = setup_pixels(SetupLook::Waiting, SetupLook::Joining, 0.5, 1.0)[0];
        assert_eq!(half, Rgb::new(14, 16, 19));
    }

    #[test]
    fn each_look_reads_as_its_own_colour() {
        let Rgb { red, green, blue } = SetupLook::Waiting.colour();
        assert!(
            red >= green && green >= blue && blue > 0,
            "white, a little warm"
        );
        let Rgb { red, green, blue } = SetupLook::Joining.colour();
        assert!(blue > green && green > red, "blue");
        let Rgb { red, green, blue } = SetupLook::Failed.colour();
        assert!(red > 2 * green && blue == 0, "amber");
        let Rgb { red, green, blue } = SetupLook::Unplug.colour();
        assert!(green > 2 * blue && blue > red, "green");
    }

    struct Frames(Vec<EncodedFrame>);

    impl FrameSink for &mut Frames {
        fn write_frame(&mut self, frame: &EncodedFrame) -> io::Result<()> {
            self.0.push(frame.clone());
            Ok(())
        }
    }

    #[test]
    fn it_opens_and_closes_with_black() {
        let mut frames = Frames(Vec::new());
        let mut ring = SetupRing::new(&mut frames).unwrap();
        ring.show(SetupLook::Failed, SetupLook::Failed, 0.0, 1.0)
            .unwrap();
        ring.shutdown().unwrap();
        assert_eq!(frames.0.len(), 3);
        assert_eq!(frames.0[0], EncodedFrame::black());
        assert_eq!(
            frames.0[1],
            encode(
                &[SetupLook::Failed.colour(); PIXEL_COUNT],
                ChannelCeiling(24)
            )
        );
        assert_eq!(frames.0[2], EncodedFrame::black());
    }
}
