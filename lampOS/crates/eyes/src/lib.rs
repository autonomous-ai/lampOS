//! The lamp's eyes: where a face is in what its camera sees.
//!
//! [`picture`] turns a camera frame into what the detector takes, [`faces`]
//! runs the detector and makes its outputs into boxes, and [`sight`] picks
//! the one face the lamp should look at and says where it is. `lamp-eyes`
//! (`src/main.rs`) owns the camera and sends that to the body. No frame
//! leaves the process unless it is started with `--sight-file`: then
//! [`kept`] holds the newest one in a private file for the voice service to
//! show the model, and no other. With `LAMP_BUS=on`, [`bus_out`] publishes
//! the faces in view on the lamp's bus: boxes and whether each is turned to
//! the lamp, never a picture. [`who`] says whose face it is, for the people
//! who asked the lamp to remember them, and nobody else. With
//! `--leave-still-faces on`, [`still`] leaves out a face at a place where no
//! person has been seen, only small faint faces for half an hour. With
//! `--full-look`, [`full_look`] makes one larger still for a request the
//! lamp said it would look for, after its head has settled. [`permit`] says
//! whether the camera may be open. [`steady`] says when one face has sat
//! still long enough for the eyes to look half as often.
pub mod bus_out;
pub mod faces;
pub mod facing;
pub mod full_look;
pub mod kept;
pub mod permit;
pub mod picture;
pub mod sight;
pub mod speaking;
pub mod steady;
pub mod still;
pub mod who;
