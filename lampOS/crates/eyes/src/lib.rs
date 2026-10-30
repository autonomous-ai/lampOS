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
//! still long enough for the eyes to look half as often. With
//! `--look-on-change on`, [`change`] says when a frame of an empty room is
//! like the last one the detector ran on, and needs no detector. With
//! `LAMP_PRESENCE=on`, [`away`] says whether anyone is at the lamp, from the
//! faces and the voice's lines, on `room/presence` (#1677). With
//! `LAMP_TURN_TO_ANSWERED=on`, [`answered`] picks the face seen asking as an
//! answer begins, for the body to turn to while it lasts (#1810).
pub mod activity;
pub mod answered;
pub mod away;
pub mod blendshapes;
pub mod bus_out;
pub mod camera_helper;
pub mod change;
pub mod face_repair;
pub mod faces;
pub mod facing;
pub mod full_look;
pub mod glance_log;
pub mod kept;
pub mod latest;
pub mod mesh;
pub mod permit;
pub mod photo;
pub mod picture;
pub mod remember_speaker;
pub mod reopen;
pub mod repair_writer;
pub mod seems;
pub mod seems_log;
pub mod sight;
pub mod speaking;
pub mod steady;
pub mod still;
pub mod tap;
pub mod who;
pub mod yawn;
