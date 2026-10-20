use lamp_body::{
    Joint,
    calibration::{CalibrationRecord, UnitCalibration, UnitId},
    control::{BusPort, Clock, MotorControl, QualifiedEnvelope, Timing},
    runtime::{
        channel::Channels,
        wire::{Command, Control, Event, Motion, Observation},
        worker::Worker,
    },
    trajectory::{JointEnvelope, RawProfile},
    units::NormalizedPosition,
};
use lamp_turn_owner::{
    AdmissionState, AdmittedInput, BootId, CaptureState, Controller, MonoTime, OutputKind,
    Permission,
};
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    io,
    rc::Rc,
};
pub fn t(n: u64) -> MonoTime {
    MonoTime::from_micros(n)
}
pub fn b(n: u8) -> BootId {
    BootId::new([n; 16]).unwrap()
}
#[derive(Clone, Default)]
pub struct Manual(pub Rc<Cell<u64>>, pub Rc<Cell<bool>>);
impl Clock for Manual {
    fn now(&self) -> MonoTime {
        if self.1.get() {
            self.0.set(self.0.get() + 1);
        }
        t(self.0.get())
    }
}
impl Manual {
    pub fn advance(&self, n: u64) {
        self.0.set(self.0.get() + n)
    }
}
pub fn calibration() -> UnitCalibration {
    UnitCalibration::new(
        UnitId::new("inert").unwrap(),
        Joint::ALL.map(|joint| CalibrationRecord {
            joint,
            id: joint.id() as u16,
            drive_mode: 0,
            homing_offset: 333,
            range_min: 1000,
            range_max: 3000,
        }),
    )
    .unwrap()
}
pub fn qualification() -> QualifiedEnvelope {
    let n = |v| NormalizedPosition::new(v).unwrap();
    let e = JointEnvelope::new(
        n(-80.0),
        n(80.0),
        1000.0,
        50_000.0,
        RawProfile::new(10, 0, 500).unwrap(),
        100,
        1000,
        30.0,
        0.2,
    )
    .unwrap();
    QualifiedEnvelope::new([7; 32], calibration(), [e; 5], t(0), t(10_000_000)).unwrap()
}
pub struct Wire {
    pub rx: VecDeque<u8>,
    pub partial: Vec<u8>,
    pub packets: Vec<Vec<u8>>,
    pub goals: Vec<[u16; 5]>,
    positions: [u16; 5],
    pub read_max: usize,
    pub write_max: usize,
    pub missing: bool,
    pub bad_checksum: bool,
    pub disconnected: bool,
    pub discard_error: bool,
    pub discards: usize,
    pub write_delay_us: u64,
    pub profile_speed: u16,
}
impl Default for Wire {
    fn default() -> Self {
        Self {
            rx: VecDeque::new(),
            partial: vec![],
            packets: vec![],
            goals: vec![],
            positions: [2000; 5],
            read_max: 128,
            write_max: 23,
            missing: false,
            bad_checksum: false,
            disconnected: false,
            discard_error: false,
            discards: 0,
            write_delay_us: 0,
            profile_speed: 500,
        }
    }
}
impl Wire {
    fn packet(&mut self, v: Vec<u8>) {
        assert_eq!(v.len(), v[3] as usize + 4);
        assert_eq!(
            *v.last().unwrap(),
            !v[2..v.len() - 1]
                .iter()
                .fold(0u8, |s, b| s.wrapping_add(*b))
        );
        match v[4] {
            0x82 => {
                assert_eq!(&v[7..v.len() - 1], &[1, 2, 3, 4, 5]);
                if !self.missing {
                    for id in 1..=5u8 {
                        let data = match (v[5], v[6]) {
                            (3, 2) => 777u16.to_le_bytes().to_vec(),
                            (9, 4) => [1000u16.to_le_bytes(), 3000u16.to_le_bytes()].concat(),
                            (31, 3) => vec![0x4d, 1, 0],
                            (40, 8) => vec![
                                1,
                                10,
                                0,
                                0,
                                0,
                                0,
                                self.profile_speed as u8,
                                (self.profile_speed >> 8) as u8,
                            ],
                            (56, 15) => {
                                let mut d = vec![0; 15];
                                d[..2].copy_from_slice(
                                    &self.positions[id as usize - 1].to_le_bytes(),
                                );
                                d
                            }
                            other => panic!("unexpected register read {other:?}"),
                        };
                        let mut reply = vec![255, 255, id, data.len() as u8 + 2, 0];
                        reply.extend(data);
                        reply.push(
                            !reply[2..].iter().fold(0u8, |s, b| s.wrapping_add(*b))
                                ^ u8::from(self.bad_checksum),
                        );
                        self.rx.extend(reply);
                    }
                }
            }
            0x83 => {
                assert_eq!(&v[5..7], &[42, 2]);
                let mut counts = self.positions;
                let mut previous = 0;
                for entry in v[7..v.len() - 1].chunks_exact(3) {
                    assert!(entry[0] > previous && entry[0] <= 5);
                    previous = entry[0];
                    counts[usize::from(entry[0] - 1)] = u16::from_le_bytes([entry[1], entry[2]]);
                }
                self.positions = counts;
                self.goals.push(counts);
            }
            other => panic!("forbidden instruction {other}"),
        }
        self.packets.push(v);
    }
}
pub struct Port {
    pub wire: Rc<RefCell<Wire>>,
    pub clock: Manual,
}
impl BusPort for Port {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let mut w = self.wire.borrow_mut();
        if w.disconnected {
            return Ok(0);
        }
        if w.rx.is_empty() {
            return Err(io::ErrorKind::WouldBlock.into());
        }
        let n = out.len().min(w.read_max).min(w.rx.len());
        for x in &mut out[..n] {
            *x = w.rx.pop_front().unwrap()
        }
        Ok(n)
    }
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        let mut w = self.wire.borrow_mut();
        self.clock.advance(w.write_delay_us);
        let n = data.len().min(w.write_max);
        w.partial.extend(&data[..n]);
        if w.partial.len() >= 4 && w.partial.len() == w.partial[3] as usize + 4 {
            let p = std::mem::take(&mut w.partial);
            w.packet(p)
        }
        Ok(n)
    }
    fn discard_output(&mut self) -> io::Result<()> {
        let mut w = self.wire.borrow_mut();
        w.discards += 1;
        w.partial.clear();
        if w.discard_error {
            Err(io::ErrorKind::Other.into())
        } else {
            Ok(())
        }
    }
}
pub struct Rig {
    pub clock: Manual,
    pub wire: Rc<RefCell<Wire>>,
    pub controller: Controller,
    pub parent: Channels,
    pub child: Channels,
    pub worker: Option<Worker<MotorControl<Port, Manual>, Manual>>,
    pub events: Vec<Event>,
    request: u64,
}
impl Rig {
    pub fn new() -> Self {
        let clock = Manual::default();
        let wire = Rc::new(RefCell::new(Wire::default()));
        let calibration = calibration();
        let mut core = MotorControl::new(
            Port {
                wire: wire.clone(),
                clock: clock.clone(),
            },
            clock.clone(),
            b(1),
            b(2),
            calibration.unit_id(),
            calibration.clone(),
            Timing::new(40_000, 5_000, 200_000, 500_000, 10_000).unwrap(),
        )
        .unwrap();
        core.qualify(qualification()).unwrap();
        let worker = Worker::new(core, clock.clone(), b(1), b(2), b(3), 5_000_000).unwrap();
        let (parent, child) = Channels::pair(b(1), b(2)).unwrap();
        let mut controller = Controller::new(b(1), t(0));
        controller
            .set_microphone_permission(t(0), Permission::Allowed)
            .unwrap();
        controller
            .set_camera_permission(t(0), Permission::Allowed)
            .unwrap();
        controller
            .set_capture(t(0), CaptureState::RetainingUntil(t(1_000_000)))
            .unwrap();
        controller
            .set_admission(t(0), AdmissionState::OpenUntil(t(1_000_000)))
            .unwrap();
        Self {
            clock,
            wire,
            controller,
            parent,
            child,
            worker: Some(worker),
            events: vec![],
            request: 0,
        }
    }
    pub fn authority(&mut self) {
        let s = self.controller.snapshot(self.clock.now()).unwrap();
        self.parent
            .control
            .send(&Control::Authority { snapshot: s }, self.clock.now())
            .unwrap();
    }
    pub fn step(&mut self) -> io::Result<bool> {
        self.clock.advance(2_000);
        let result = self.worker.as_mut().unwrap().poll(&mut self.child);
        self.drain();
        result
    }
    pub fn drain(&mut self) {
        while let Some(e) = self.parent.control.receive(self.clock.now()).unwrap() {
            self.events.push(e)
        }
    }
    pub fn finish(&mut self) -> io::Result<()> {
        let r = self.worker.take().unwrap().finish(&mut self.child);
        self.drain();
        r
    }
    pub fn inspect(&mut self) {
        self.authority();
        self.request += 1;
        self.parent
            .data
            .send(
                &Command::Inspect {
                    request: self.request,
                    requested_at_us: self.clock.now().as_micros(),
                },
                self.clock.now(),
            )
            .unwrap();
        for _ in 0..100 {
            self.step().unwrap();
            if self
                .events
                .iter()
                .any(|e| matches!(e, Event::Inspected { .. }))
            {
                return;
            }
        }
        panic!("inspection incomplete")
    }
    pub fn segment(&mut self) -> Command {
        let now = self.clock.now();
        let owner = self.controller.admit(now, AdmittedInput::NewTurn).unwrap();
        let camera = self.controller.camera_grant(now).unwrap();
        let plan = self
            .controller
            .plan_output(now, owner, OutputKind::Motion, Some(camera))
            .unwrap();
        let permit = self.controller.issue_output(now, plan, 240_000).unwrap();
        self.authority();
        self.request += 1;
        Command::Segment {
            request: self.request,
            requested_at_us: now.as_micros(),
            permit,
            placement: Observation {
                source_boot: b(3),
                frame_sequence: self.request,
                acquired_at_us: now.as_micros(),
                valid_until_us: now.as_micros() + 500_000,
                camera,
                qualification_record: [7; 32],
            },
            target: [10.0; 5],
            duration_us: 80_000,
        }
    }
    pub fn send(&mut self, c: &Command) {
        self.parent.data.send(c, self.clock.now()).unwrap()
    }
    pub fn first_goal(&mut self) -> Motion {
        for _ in 0..60 {
            self.step().unwrap();
            if let Some(m) = self.events.iter().find_map(|e| {
                if let Event::HostAccepted { motion, .. } = e {
                    Some(*motion)
                } else {
                    None
                }
            }) {
                return m;
            }
        }
        panic!("no accepted goal")
    }
}
