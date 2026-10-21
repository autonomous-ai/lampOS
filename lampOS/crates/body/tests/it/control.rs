use lamp_body::{
    Joint,
    calibration::{CalibrationRecord, UnitCalibration, UnitId},
    command::PositionFrame,
    control::{BusPort, Clock, Error, MotorControl, Placement, QualifiedEnvelope, Tick, Timing},
    trajectory::{JointEnvelope, RawProfile, Segment, TrajectoryError},
    units::{EncoderCounts, NormalizedPosition},
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

fn t(us: u64) -> MonoTime {
    MonoTime::from_micros(us)
}
fn n(value: f64) -> NormalizedPosition {
    NormalizedPosition::new(value).unwrap()
}
fn boot() -> BootId {
    BootId::new([1; 16]).unwrap()
}
fn unit() -> UnitId {
    UnitId::new("synthetic-no-hardware").unwrap()
}
fn calibration() -> UnitCalibration {
    calibration_span(1000, 3000)
}
fn calibration_span(minimum: i32, maximum: i32) -> UnitCalibration {
    UnitCalibration::new(
        unit(),
        Joint::ALL.map(|joint| CalibrationRecord {
            joint,
            id: u16::from(joint.id()),
            drive_mode: 0,
            homing_offset: 333,
            range_min: minimum,
            range_max: maximum,
        }),
    )
    .unwrap()
}
fn envelope() -> JointEnvelope {
    JointEnvelope::new(
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
    .unwrap()
}
fn qualification() -> QualifiedEnvelope {
    QualifiedEnvelope::new([7; 32], calibration(), [envelope(); 5], t(0), t(1_000_000)).unwrap()
}

#[derive(Clone, Default)]
struct ManualClock(Rc<Cell<u64>>);
impl Clock for ManualClock {
    fn now(&self) -> MonoTime {
        t(self.0.get())
    }
}
impl ManualClock {
    fn advance(&self, us: u64) {
        self.0.set(self.0.get() + us);
    }
}

struct Wire {
    rx: VecDeque<u8>,
    partial: Vec<u8>,
    packets: Vec<Vec<u8>>,
    goals: Vec<[u16; 5]>,
    positions: [u16; 5],
    load: u16,
    velocity: u16,
    status: u8,
    moving: u8,
    read_max: usize,
    write_max: usize,
    missing_id: Option<u8>,
    checksum_fault: bool,
    profile_speed: u16,
    profile_override: Option<[[u8; 8]; 5]>,
    missing_ack: bool,
    ack_id: u8,
    ack_status: u8,
    ignore_profile_write: bool,
    torque: u8,
    model: u16,
    minimum: u16,
    maximum: u16,
    hold_position: bool,
    discards: usize,
    discard_error: bool,
    writes_blocked: bool,
    reads_blocked: bool,
    io_delay_us: u64,
    write_delay_us: u64,
}
impl Default for Wire {
    fn default() -> Self {
        Self {
            rx: VecDeque::new(),
            partial: Vec::new(),
            packets: Vec::new(),
            goals: Vec::new(),
            positions: [2000; 5],
            load: 0,
            velocity: 0,
            status: 0,
            moving: 0,
            read_max: 128,
            write_max: 23,
            missing_id: None,
            checksum_fault: false,
            profile_speed: 500,
            profile_override: None,
            missing_ack: false,
            ack_id: 5,
            ack_status: 0,
            ignore_profile_write: false,
            torque: 1,
            model: 777,
            minimum: 1000,
            maximum: 3000,
            hold_position: false,
            discards: 0,
            discard_error: false,
            writes_blocked: false,
            reads_blocked: false,
            io_delay_us: 0,
            write_delay_us: 0,
        }
    }
}
impl Wire {
    fn packet(&mut self, bytes: Vec<u8>) {
        assert_eq!(bytes.len(), usize::from(bytes[3]) + 4);
        assert_eq!(
            bytes[bytes.len() - 1],
            !bytes[2..bytes.len() - 1]
                .iter()
                .fold(0u8, |s, &b| s.wrapping_add(b))
        );
        self.packets.push(bytes.clone());
        match bytes[4] {
            3 => {
                assert_eq!(bytes[2], 5, "profile setup must be ID5-only");
                let profiles = self
                    .profile_override
                    .as_mut()
                    .expect("explicit fixture binding");
                assert!(
                    matches!((bytes[5], bytes.len()), (46, 9) | (41, 8)),
                    "only speed/acceleration"
                );
                if !self.ignore_profile_write {
                    match bytes[5] {
                        46 => profiles[4][6..8].copy_from_slice(&bytes[6..8]),
                        41 => profiles[4][1] = bytes[6],
                        _ => unreachable!(),
                    }
                }
                if !self.missing_ack {
                    let mut ack = vec![255, 255, self.ack_id, 2, self.ack_status];
                    ack.push(!ack[2..].iter().fold(0u8, |s, b| s.wrapping_add(*b)));
                    self.rx.extend(ack);
                }
            }
            0x82 => {
                assert_eq!(&bytes[7..bytes.len() - 1], &[1, 2, 3, 4, 5]);
                for id in 1..=5 {
                    if self.missing_id == Some(id) {
                        continue;
                    }
                    let data = match (bytes[5], bytes[6]) {
                        (3, 2) => self.model.to_le_bytes().to_vec(),
                        (9, 4) => [self.minimum.to_le_bytes(), self.maximum.to_le_bytes()].concat(),
                        (31, 3) => vec![0x4d, 1, 0],
                        (40, 8) => {
                            if let Some(values) = self.profile_override {
                                values[usize::from(id - 1)].to_vec()
                            } else {
                                let speed = self.profile_speed.to_le_bytes();
                                vec![self.torque, 10, 0, 0, 0, 0, speed[0], speed[1]]
                            }
                        }
                        (41, 7) => {
                            let speed = self.profile_speed.to_le_bytes();
                            vec![10, 0, 0, 0, 0, speed[0], speed[1]]
                        }
                        (56, 15) => {
                            let mut data = vec![0; 15];
                            data[..2].copy_from_slice(
                                &self.positions[usize::from(id - 1)].to_le_bytes(),
                            );
                            data[2..4].copy_from_slice(&self.velocity.to_le_bytes());
                            data[4..6].copy_from_slice(&self.load.to_le_bytes());
                            data[9] = self.status;
                            data[10] = self.moving;
                            data
                        }
                        other => panic!("unexpected read {other:?}"),
                    };
                    let mut reply = vec![0xff, 0xff, id, (data.len() + 2) as u8, 0];
                    reply.extend(data);
                    let sum = !reply[2..].iter().fold(0u8, |s, &b| s.wrapping_add(b));
                    reply.push(sum ^ u8::from(self.checksum_fault));
                    self.rx.extend(reply);
                }
            }
            0x83 => {
                assert_eq!(&bytes[5..7], &[42, 2]);
                let mut goals = self.positions;
                let mut previous = 0;
                for entry in bytes[7..bytes.len() - 1].chunks_exact(3) {
                    assert!(entry[0] > previous && entry[0] <= 5);
                    previous = entry[0];
                    goals[usize::from(entry[0] - 1)] = u16::from_le_bytes([entry[1], entry[2]]);
                }
                self.goals.push(goals);
                if !self.hold_position {
                    self.positions = goals;
                    if let Some(profiles) = &mut self.profile_override {
                        for i in 0..5 {
                            profiles[i][2..4].copy_from_slice(&goals[i].to_le_bytes());
                        }
                    }
                }
            }
            other => panic!("unexpected instruction {other}"),
        }
    }
}

struct Port {
    wire: Rc<RefCell<Wire>>,
    clock: ManualClock,
}
impl BusPort for Port {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let mut wire = self.wire.borrow_mut();
        self.clock.advance(wire.io_delay_us);
        if wire.reads_blocked || wire.rx.is_empty() {
            return Err(io::ErrorKind::WouldBlock.into());
        }
        let count = out.len().min(wire.read_max).min(wire.rx.len());
        for value in &mut out[..count] {
            *value = wire.rx.pop_front().unwrap();
        }
        Ok(count)
    }
    fn write(&mut self, input: &[u8]) -> io::Result<usize> {
        let mut wire = self.wire.borrow_mut();
        self.clock.advance(wire.io_delay_us + wire.write_delay_us);
        if wire.writes_blocked {
            return Err(io::ErrorKind::WouldBlock.into());
        }
        let count = input.len().min(wire.write_max);
        wire.partial.extend(&input[..count]);
        if wire.partial.len() >= 4 && wire.partial.len() == usize::from(wire.partial[3]) + 4 {
            let bytes = std::mem::take(&mut wire.partial);
            wire.packet(bytes);
        }
        Ok(count)
    }
    fn discard_output(&mut self) -> io::Result<()> {
        let mut wire = self.wire.borrow_mut();
        wire.discards += 1;
        self.clock.advance(wire.io_delay_us);
        if wire.discard_error {
            Err(io::ErrorKind::Other.into())
        } else {
            Ok(())
        }
    }
}

struct Rig {
    calibration: UnitCalibration,
    motor: MotorControl<Port, ManualClock>,
    clock: ManualClock,
    wire: Rc<RefCell<Wire>>,
    controller: Controller,
}
impl Rig {
    fn new() -> Self {
        Self::with_span(1000, 3000)
    }
    fn with_span(minimum: u16, maximum: u16) -> Self {
        Self::with_span_and_worker(minimum, maximum, 2)
    }
    fn with_span_and_worker(minimum: u16, maximum: u16, worker: u8) -> Self {
        let calibration = calibration_span(i32::from(minimum), i32::from(maximum));
        let clock = ManualClock::default();
        let wire = Rc::new(RefCell::new(Wire::default()));
        wire.borrow_mut().minimum = minimum;
        wire.borrow_mut().maximum = maximum;
        let motor = MotorControl::new(
            Port {
                wire: wire.clone(),
                clock: clock.clone(),
            },
            clock.clone(),
            boot(),
            BootId::new([worker; 16]).unwrap(),
            &unit(),
            calibration.clone(),
            Timing::new(40_000, 5_000, 200_000, 500_000, 10_000).unwrap(),
        )
        .unwrap();
        let mut controller = Controller::new(boot(), t(0));
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
            calibration,
            motor,
            clock,
            wire,
            controller,
        }
    }
    fn step(&mut self) -> Result<Tick, Error> {
        self.clock.advance(2_000);
        self.motor.tick()
    }
    fn inspect(&mut self) {
        self.motor.inspect().unwrap();
        for _ in 0..200 {
            if self.step().unwrap() == Tick::Inspected {
                return;
            }
        }
        panic!("inspection did not complete");
    }
    fn authority(&mut self) -> (lamp_turn_owner::OutputPermit, Placement) {
        let now = self.clock.now();
        let owner = self.controller.admit(now, AdmittedInput::NewTurn).unwrap();
        let camera = self.controller.camera_grant(now).unwrap();
        let plan = self
            .controller
            .plan_output(now, owner, OutputKind::Motion, Some(camera))
            .unwrap();
        let permit = self.controller.issue_output(now, plan, 240_000).unwrap();
        self.motor
            .install(self.controller.snapshot(now).unwrap())
            .unwrap();
        let placement = Placement {
            source_boot: boot(),
            frame_sequence: 1,
            acquired_at: now,
            valid_until: now.checked_add(500_000).unwrap(),
            camera,
            qualification_record: [7; 32],
        };
        (permit, placement)
    }
    fn ready(&mut self) {
        self.motor
            .qualify(
                QualifiedEnvelope::new(
                    [7; 32],
                    self.calibration.clone(),
                    [envelope(); 5],
                    t(0),
                    t(1_000_000),
                )
                .unwrap(),
            )
            .unwrap();
        self.inspect();
    }
    fn start(&mut self) -> lamp_body::control::MotionId {
        let (permit, placement) = self.authority();
        self.motor
            .start_segment(permit, placement, [n(10.0); 5], 80_000)
            .unwrap()
    }
    fn first_goal(&mut self) {
        for _ in 0..80 {
            if matches!(self.step().unwrap(), Tick::PositionAccepted { .. }) {
                return;
            }
        }
        panic!("no goal");
    }
    fn failure(&mut self) -> Error {
        for _ in 0..200 {
            if let Err(error) = self.step() {
                return error;
            }
        }
        panic!("no failure");
    }
}

#[test]
fn endpoint_is_quantized_once_not_repeatedly_biased_by_inverse_roundtrip() {
    let mut rig = Rig::with_span(0, 4095);
    rig.wire.borrow_mut().positions = [650; 5];
    rig.ready();
    let (permit, placement) = rig.authority();
    let target = n(656.5 / 4095.0 * 200.0 - 100.0);
    let expected = rig
        .calibration
        .bind(&unit())
        .unwrap()
        .joint(Joint::BaseYaw)
        .denormalize(target);
    assert_eq!(expected.get(), 656);
    rig.motor
        .start_segment(permit, placement, [target; 5], 80_000)
        .unwrap();
    let mut reached = false;
    for _ in 0..100 {
        if matches!(rig.step().unwrap(), Tick::Reached { .. }) {
            reached = true;
            break;
        }
    }
    assert!(reached);
    assert_eq!(rig.wire.borrow().goals.last(), Some(&[expected.get(); 5]));
}

#[test]
fn position_packet_covers_all_counts_and_all_five_little_endian_ids() {
    for count in 0..=4095 {
        let values =
            std::array::from_fn(|i| EncoderCounts::new((count + i as i32) % 4096).unwrap());
        let frame = PositionFrame::encode(values);
        let bytes = frame.bytes();
        assert_eq!(&bytes[..7], &[255, 255, 254, 19, 131, 42, 2]);
        assert_eq!(bytes.len(), 23);
        for joint in Joint::ALL {
            let offset = 7 + 3 * joint.index();
            assert_eq!(bytes[offset], joint.id());
            assert_eq!(
                u16::from_le_bytes([bytes[offset + 1], bytes[offset + 2]]),
                values[joint.index()].get()
            );
        }
        assert_eq!(
            bytes[22],
            !bytes[2..22].iter().fold(0u8, |s, &b| s.wrapping_add(b))
        );
    }
}

#[test]
fn profile_and_trajectory_reject_unbounded_settings_and_bad_units() {
    for (acc, speed) in [(0, 500), (1, 0), (1, 32768), (1, u16::MAX)] {
        assert!(RawProfile::new(acc, 0, speed).is_err());
    }
    let limits = [envelope(); 5];
    assert_eq!(
        Segment::new([n(0.0); 5], [n(81.0); 5], 100_000, &limits).unwrap_err(),
        TrajectoryError::Range(Joint::BaseYaw)
    );
    assert_eq!(
        Segment::new([n(-80.0); 5], [n(80.0); 5], 100_000, &limits).unwrap_err(),
        TrajectoryError::Velocity(Joint::BaseYaw)
    );
    assert_eq!(
        Segment::new([n(0.0); 5], [n(1.0); 5], 1_000, &limits).unwrap_err(),
        TrajectoryError::Velocity(Joint::BaseYaw)
    );
    let slow_acc = JointEnvelope::new(
        n(-80.0),
        n(80.0),
        1000.0,
        100.0,
        RawProfile::new(1, 0, 1).unwrap(),
        1,
        1,
        1.0,
        1.0,
    )
    .unwrap();
    assert_eq!(
        Segment::new([n(0.0); 5], [n(1.0); 5], 100_000, &[slow_acc; 5]).unwrap_err(),
        TrajectoryError::Acceleration(Joint::BaseYaw)
    );
    for v in [0.0, -1.0, f64::INFINITY, f64::NAN] {
        assert!(
            JointEnvelope::new(
                n(-10.0),
                n(10.0),
                v,
                1.0,
                RawProfile::new(1, 0, 1).unwrap(),
                1,
                1,
                1.0,
                1.0
            )
            .is_err()
        );
    }
    for us in [0, 999, 200_001, u64::MAX] {
        assert_eq!(
            Segment::new([n(0.0); 5], [n(1.0); 5], us, &limits).unwrap_err(),
            TrajectoryError::Duration
        );
    }
}

#[test]
fn cubic_segment_is_monotonic_bounded_and_rest_to_rest() {
    let segment = Segment::new([n(-10.0); 5], [n(10.0); 5], 100_000, &[envelope(); 5]).unwrap();
    let mut previous = -10.0;
    for us in 0..=100_000 {
        let value = segment.sample(us)[0].get();
        assert!(value >= previous && (-10.0..=10.0).contains(&value));
        previous = value;
    }
    assert_eq!(segment.sample(u64::MAX), [n(10.0); 5]);
    assert!((segment.sample(1)[0].get() + 10.0) < 0.000001);
    assert!((10.0 - segment.sample(99_999)[0].get()) < 0.000001);
}

#[test]
fn startup_is_idle_and_success_means_five_checked_readbacks_not_write_ack() {
    let mut rig = Rig::new();
    for _ in 0..3 {
        assert_eq!(rig.step().unwrap(), Tick::Idle);
    }
    assert!(rig.wire.borrow().packets.is_empty());
    rig.ready();
    let id = rig.start();
    let mut writes = 0;
    let mut reached = false;
    for _ in 0..100 {
        match rig.step().unwrap() {
            Tick::PositionAccepted { motion, .. } => {
                assert_eq!(motion, id);
                writes += 1;
            }
            Tick::Reached { motion, .. } => {
                assert_eq!(motion, id);
                reached = true;
                break;
            }
            _ => {}
        }
    }
    assert!(reached && writes > 1);
    assert_eq!(rig.wire.borrow().goals.last(), Some(&[2100; 5]));
    assert_eq!(
        rig.wire.borrow().packets[..5]
            .iter()
            .map(|p| (p[4], p[5]))
            .collect::<Vec<_>>(),
        vec![(0x82, 3), (0x82, 9), (0x82, 31), (0x82, 40), (0x82, 56)]
    );
    assert_eq!(
        rig.wire.borrow().packets.last().unwrap()[4..7],
        [0x82, 56, 15]
    );
    let count = rig.wire.borrow().packets.len();
    for _ in 0..20 {
        assert_eq!(rig.step().unwrap(), Tick::Idle);
    }
    assert_eq!(rig.wire.borrow().packets.len(), count);
}

#[test]
fn no_motion_without_qualification_inspection_and_original_placement_grant() {
    let mut rig = Rig::new();
    rig.inspect();
    let (permit, placement) = rig.authority();
    assert_eq!(
        rig.motor
            .start_segment(permit, placement, [n(1.0); 5], 80_000),
        Err(Error::Qualification)
    );
    rig.motor.qualify(qualification()).unwrap();
    let mut bad = placement;
    bad.frame_sequence = 0;
    assert_eq!(
        rig.motor.start_segment(permit, bad, [n(1.0); 5], 80_000),
        Err(Error::Placement)
    );
    bad = placement;
    bad.qualification_record = [8; 32];
    assert_eq!(
        rig.motor.start_segment(permit, bad, [n(1.0); 5], 80_000),
        Err(Error::Placement)
    );
    bad = placement;
    bad.valid_until = rig.clock.now();
    assert_eq!(
        rig.motor.start_segment(permit, bad, [n(1.0); 5], 80_000),
        Err(Error::Placement)
    );
    bad = placement;
    bad.acquired_at = rig.clock.now().checked_add(1).unwrap();
    assert_eq!(
        rig.motor.start_segment(permit, bad, [n(1.0); 5], 80_000),
        Err(Error::Placement)
    );
    assert!(rig.wire.borrow().goals.is_empty());
    let mut fresh = Rig::new();
    fresh.motor.qualify(qualification()).unwrap();
    let (p, place) = fresh.authority();
    assert_eq!(
        fresh.motor.start_segment(p, place, [n(1.0); 5], 80_000),
        Err(Error::NotInspected)
    );
}

#[test]
fn profile_mismatch_does_not_write_or_replace_qualified_caps() {
    let mut rig = Rig::new();
    rig.wire.borrow_mut().profile_speed = 0;
    rig.ready();
    let (permit, placement) = rig.authority();
    assert_eq!(
        rig.motor
            .start_segment(permit, placement, [n(1.0); 5], 80_000),
        Err(Error::Profile(Joint::BaseYaw))
    );
    assert!(rig.wire.borrow().goals.is_empty());
}

#[test]
fn admission_requires_rest_and_does_not_queue_a_second_segment() {
    let mut rig = Rig::new();
    rig.wire.borrow_mut().velocity = 1;
    rig.ready();
    let (p, place) = rig.authority();
    assert_eq!(
        rig.motor.start_segment(p, place, [n(1.0); 5], 80_000),
        Err(Error::NotAtRest(Joint::BaseYaw))
    );
    let mut rig = Rig::new();
    rig.ready();
    let (p, place) = rig.authority();
    rig.motor
        .start_segment(p, place, [n(1.0); 5], 80_000)
        .unwrap();
    assert_eq!(
        rig.motor.start_segment(p, place, [n(2.0); 5], 80_000),
        Err(Error::Busy)
    );
}

#[test]
fn valid_stale_snapshot_does_not_cancel_new_motion_but_privacy_does() {
    let mut rig = Rig::new();
    rig.ready();
    let id = rig.start();
    let old = rig.controller.snapshot(rig.clock.now()).unwrap();
    rig.clock.advance(1);
    let current = rig.controller.snapshot(rig.clock.now()).unwrap();
    rig.motor.install(current).unwrap();
    assert!(matches!(
        rig.motor.install(old),
        Err(Error::Authority(lamp_turn_owner::Error::StaleSnapshot))
    ));
    assert!(rig.motor.stop_report().is_none());
    rig.first_goal();
    let count = rig.wire.borrow().goals.len();
    rig.controller
        .set_camera_permission(rig.clock.now(), Permission::Denied)
        .unwrap();
    assert!(
        rig.motor
            .install(rig.controller.snapshot(rig.clock.now()).unwrap())
            .is_err()
    );
    assert!(rig.motor.stop_report().is_some());
    assert_eq!(rig.motor.cancel(id), Err(Error::Stopped));
    assert_eq!(rig.motor.tick(), Err(Error::Stopped));
    assert_eq!(rig.wire.borrow().goals.len(), count);
}

#[test]
fn cancellation_discards_partial_packet_once_and_never_sends_its_tail() {
    let mut rig = Rig::new();
    rig.ready();
    let id = rig.start();
    rig.wire.borrow_mut().write_max = 3;
    for _ in 0..20 {
        rig.step().unwrap();
        if !rig.wire.borrow().partial.is_empty() {
            break;
        }
    }
    assert_eq!(rig.wire.borrow().partial.len(), 3);
    let cancelled_at = rig.clock.now();
    let report = rig.motor.cancel(id).unwrap();
    assert_eq!(report.partial_request_bytes, 3);
    assert_eq!(report.reason, Error::Cancelled);
    assert_eq!(report.stopped_at, cancelled_at);
    assert_eq!(rig.wire.borrow().discards, 1);
    assert_eq!(rig.motor.shutdown(), report);
    assert_eq!(rig.motor.inspect(), Err(Error::Stopped));
    for _ in 0..20 {
        assert_eq!(rig.step(), Err(Error::Stopped));
    }
    assert_eq!(rig.wire.borrow().partial.len(), 3);
    assert!(rig.wire.borrow().goals.is_empty());
}

#[test]
fn stalled_partial_write_faults_once_without_retry_or_reusing_bus() {
    let mut rig = Rig::new();
    rig.ready();
    rig.start();
    rig.wire.borrow_mut().write_max = 3;
    for _ in 0..20 {
        rig.step().unwrap();
        if !rig.wire.borrow().partial.is_empty() {
            break;
        }
    }
    rig.wire.borrow_mut().writes_blocked = true;
    assert!(matches!(rig.failure(), Error::Deadline { .. }));
    assert_eq!(rig.motor.stop_report().unwrap().partial_request_bytes, 3);
    rig.wire.borrow_mut().writes_blocked = false;
    assert_eq!(rig.motor.tick(), Err(Error::Stopped));
    assert!(rig.wire.borrow().goals.is_empty());
}

#[test]
fn fragmented_read_write_is_bounded_and_corrupt_missing_replies_are_terminal() {
    let mut rig = Rig::new();
    rig.wire.borrow_mut().read_max = 16;
    rig.wire.borrow_mut().write_max = 4;
    rig.ready();
    assert!(rig.motor.stop_report().is_none());
    for fault in 0..3 {
        let mut rig = Rig::new();
        match fault {
            0 => rig.wire.borrow_mut().checksum_fault = true,
            1 => rig.wire.borrow_mut().missing_id = Some(3),
            _ => rig.wire.borrow_mut().model = 1,
        }
        rig.motor.inspect().unwrap();
        let error = rig.failure();
        match fault {
            0 => assert!(matches!(error, Error::Protocol(_))),
            1 => assert_eq!(
                error,
                Error::Deadline {
                    pending_joint_mask: 4
                }
            ),
            _ => assert_eq!(error, Error::Model(Joint::BaseYaw)),
        }
        let count = rig.wire.borrow().packets.len();
        assert_eq!(rig.motor.tick(), Err(Error::Stopped));
        assert_eq!(rig.wire.borrow().packets.len(), count);
        assert!(rig.wire.borrow().goals.is_empty());
    }
}

#[test]
fn prior_or_trailing_traffic_is_not_relabelled_fresh_by_parser_reset() {
    let mut rig = Rig::new();
    rig.wire.borrow_mut().rx.extend([0xff, 0xff, 1]);
    rig.motor.inspect().unwrap();
    assert_eq!(rig.failure(), Error::UnexpectedTraffic);
    assert!(rig.wire.borrow().packets.is_empty());
    let mut rig = Rig::new();
    rig.ready();
    rig.start();
    rig.wire.borrow_mut().rx.extend([1, 2, 3]);
    assert_eq!(rig.failure(), Error::UnexpectedTraffic);
    assert!(rig.wire.borrow().goals.is_empty());
}

#[test]
fn live_overload_and_bad_status_lock_out_without_torque_release() {
    for mode in 0..4 {
        let mut rig = Rig::new();
        rig.ready();
        rig.start();
        rig.first_goal();
        {
            let mut wire = rig.wire.borrow_mut();
            match mode {
                0 => wire.load = 101,
                1 => wire.status = 0x20,
                2 => wire.velocity = 1001,
                _ => wire.moving = 2,
            }
        }
        let error = rig.failure();
        match mode {
            0 => assert_eq!(error, Error::Load(Joint::BaseYaw)),
            2 => assert_eq!(error, Error::Velocity(Joint::BaseYaw)),
            _ => assert_eq!(error, Error::Readback(Joint::BaseYaw)),
        }
        assert!(
            rig.wire
                .borrow()
                .packets
                .iter()
                .all(|p| p[4] == 0x82 || (p[4] == 0x83 && p[5] == 42))
        );
        let count = rig.wire.borrow().goals.len();
        assert_eq!(rig.motor.tick(), Err(Error::Stopped));
        assert_eq!(rig.wire.borrow().goals.len(), count);
    }
}

#[test]
fn load_direction_is_not_load_magnitude_but_unknown_bits_fault() {
    let mut rig = Rig::new();
    rig.wire.borrow_mut().load = 0x0401;
    rig.ready();
    rig.start();
    rig.first_goal();
    rig.wire.borrow_mut().load = 0x0801;
    assert_eq!(rig.failure(), Error::Load(Joint::BaseYaw));
}

#[test]
fn successful_writes_without_endpoint_readback_never_report_reached() {
    let mut rig = Rig::new();
    rig.ready();
    rig.wire.borrow_mut().hold_position = true;
    rig.start();
    let mut reached = false;
    let error = loop {
        match rig.step() {
            Ok(Tick::Reached { .. }) => {
                reached = true;
                break Error::Stopped;
            }
            Err(error) => break error,
            _ => {}
        }
    };
    assert!(!reached);
    assert_eq!(error, Error::SegmentExpired);
    assert!(!rig.wire.borrow().goals.is_empty());
}

#[test]
fn authority_expiry_and_observed_io_overrun_stop_without_fabricated_recovery() {
    let mut rig = Rig::new();
    rig.ready();
    rig.start();
    rig.clock.advance(250_000);
    assert!(matches!(rig.motor.tick(), Err(Error::Authority(_))));
    assert!(rig.wire.borrow().goals.is_empty());
    let mut rig = Rig::new();
    rig.ready();
    rig.start();
    rig.wire.borrow_mut().io_delay_us = 5_001;
    assert_eq!(rig.failure(), Error::IoOverrun);
    assert_eq!(
        rig.motor.stop_report().unwrap().discard_error,
        Some(Error::IoOverrun)
    );
}

#[test]
fn full_goal_accepted_before_observed_overrun_remains_in_failure_evidence() {
    let mut rig = Rig::new();
    rig.ready();
    rig.start();
    rig.wire.borrow_mut().write_delay_us = 5_001;
    assert_eq!(rig.failure(), Error::IoOverrun);
    let report = rig.motor.stop_report().unwrap();
    assert_eq!(report.partial_request_bytes, 0);
    assert_eq!(report.accepted_request_bytes, 23);
    assert!(report.position_request);
    assert_eq!(rig.wire.borrow().goals.len(), 1);
    assert_eq!(rig.motor.tick(), Err(Error::Stopped));
    assert_eq!(rig.wire.borrow().goals.len(), 1);
}

#[test]
fn shutdown_is_terminal_even_if_host_output_flush_fails() {
    let mut rig = Rig::new();
    rig.ready();
    rig.start();
    rig.wire.borrow_mut().discard_error = true;
    let report = rig.motor.shutdown();
    assert_eq!(report.reason, Error::Shutdown);
    assert!(report.discard_error.is_some());
    assert_eq!(rig.motor.tick(), Err(Error::Stopped));
    assert_eq!(rig.wire.borrow().discards, 1);
    assert!(rig.wire.borrow().goals.is_empty());
}

#[test]
fn cancellation_at_every_local_tick_adds_no_wait_and_no_later_goal() {
    let mut elapsed_ns = Vec::new();
    for cancellation_tick in 0..45 {
        let mut rig = Rig::new();
        rig.ready();
        let id = rig.start();
        let mut complete = false;
        for _ in 0..cancellation_tick {
            if matches!(rig.step().unwrap(), Tick::Reached { .. }) {
                complete = true;
                break;
            }
        }
        if complete {
            continue;
        }
        let before = rig.clock.now();
        let goals = rig.wire.borrow().goals.len();
        // Delivery on the next 2 ms host-harness tick, before ordinary bus work.
        rig.clock.advance(2_000);
        let wall = std::time::Instant::now();
        let report = rig.motor.cancel(id).unwrap();
        elapsed_ns.push(wall.elapsed().as_nanos());
        assert!(report.stopped_at.as_micros() - before.as_micros() <= 20_000);
        assert_eq!(rig.motor.tick(), Err(Error::Stopped));
        assert_eq!(rig.wire.borrow().goals.len(), goals);
    }
    elapsed_ns.sort_unstable();
    eprintln!(
        "synthetic_cancel n={} virtual_dispatch_us=2000 host_call_p50_ns={} host_call_p99_ns={} host_call_max_ns={}",
        elapsed_ns.len(),
        elapsed_ns[elapsed_ns.len() / 2],
        elapsed_ns[(elapsed_ns.len() - 1) * 99 / 100],
        elapsed_ns.last().unwrap()
    );
}

#[test]
fn worker_restart_does_not_reuse_old_occurrence_identity() {
    let mut prior = Rig::new();
    prior.ready();
    let old = prior.start();
    prior.motor.shutdown();
    let mut new = Rig::with_span_and_worker(1000, 3000, 3);
    new.ready();
    let current = new.start();
    assert_eq!(old.owner(), current.owner());
    assert_eq!(old.sequence(), current.sequence());
    assert_ne!(old.worker_boot(), current.worker_boot());
    assert_eq!(new.motor.cancel(old), Err(Error::StaleMotion));
    new.first_goal();
    assert!(new.motor.stop_report().is_none());
}

#[test]
fn stalled_clock_does_not_allow_unbounded_transaction_io_retries() {
    let mut rig = Rig::new();
    rig.ready();
    rig.start();
    rig.wire.borrow_mut().write_max = 3;
    for _ in 0..20 {
        rig.step().unwrap();
        if !rig.wire.borrow().partial.is_empty() {
            break;
        }
    }
    rig.wire.borrow_mut().writes_blocked = true;
    let mut failure = None;
    for _ in 0..512 {
        if let Err(error) = rig.motor.tick() {
            failure = Some(error);
            break;
        }
    }
    assert_eq!(failure, Some(Error::OperationLimit));
    assert_eq!(rig.motor.stop_report().unwrap().partial_request_bytes, 3);
    assert!(rig.wire.borrow().goals.is_empty());
}

#[test]
fn cancellation_from_completed_occurrence_cannot_stop_new_owner() {
    let mut rig = Rig::new();
    rig.ready();
    let old = rig.start();
    for _ in 0..100 {
        if matches!(rig.step().unwrap(), Tick::Reached { .. }) {
            break;
        }
    }
    let new = rig.start();
    assert_ne!(new, old);
    assert_eq!(rig.motor.cancel(old), Err(Error::StaleMotion));
    assert!(rig.motor.stop_report().is_none());
    rig.first_goal();
    assert_eq!(rig.motor.cancel(new).unwrap().reason, Error::Cancelled);
}

#[test]
fn control_loss_and_raii_drop_discard_without_any_park_or_torque_write() {
    let mut rig = Rig::new();
    rig.ready();
    rig.start();
    rig.first_goal();
    let count = rig.wire.borrow().packets.len();
    assert_eq!(rig.motor.control_lost().reason, Error::ControlLost);
    assert_eq!(rig.motor.tick(), Err(Error::Stopped));
    assert_eq!(rig.wire.borrow().packets.len(), count);
    let wire = rig.wire.clone();
    drop(rig);
    assert_eq!(wire.borrow().discards, 1);
    let mut other = Rig::new();
    other.ready();
    other.start();
    let wire = other.wire.clone();
    drop(other);
    assert_eq!(wire.borrow().discards, 1);
    assert!(wire.borrow().goals.is_empty());
}

#[test]
fn camera_close_reopen_cannot_relabel_an_old_placement() {
    let mut rig = Rig::new();
    rig.ready();
    let (_, old) = rig.authority();
    rig.controller
        .set_camera_permission(rig.clock.now(), Permission::Denied)
        .unwrap();
    rig.controller
        .set_camera_permission(rig.clock.now(), Permission::Allowed)
        .unwrap();
    let (permit, new) = rig.authority();
    assert_ne!(old.camera, new.camera);
    assert_eq!(
        rig.motor.start_segment(permit, old, [n(1.0); 5], 80_000),
        Err(Error::Placement)
    );
    assert!(
        rig.motor
            .start_segment(permit, new, [n(1.0); 5], 80_000)
            .is_ok()
    );
}

#[test]
fn changed_unit_calibration_and_forged_wider_envelope_are_not_substituted() {
    let mut rig = Rig::new();
    let evidence = QualifiedEnvelope::new(
        [7; 32],
        calibration_span(0, 4095),
        [envelope(); 5],
        t(0),
        t(1_000_000),
    )
    .unwrap();
    assert_eq!(rig.motor.qualify(evidence), Err(Error::Qualification));
    assert!(
        QualifiedEnvelope::new([0; 32], calibration(), [envelope(); 5], t(0), t(1_000_000))
            .is_err()
    );
    rig.ready();
    let (permit, placement) = rig.authority();
    assert_eq!(
        rig.motor
            .start_segment(permit, placement, [n(90.0); 5], 80_000),
        Err(Error::Trajectory(TrajectoryError::Range(Joint::BaseYaw)))
    );
    assert!(rig.wire.borrow().goals.is_empty());
}

#[test]
fn out_of_span_position_and_excess_tracking_error_are_visible_faults() {
    for position in [4095, 2600] {
        let mut rig = Rig::new();
        rig.ready();
        rig.start();
        rig.first_goal();
        rig.wire.borrow_mut().positions = [position; 5];
        let error = rig.failure();
        if position == 4095 {
            assert!(matches!(error, Error::Calibration(_)));
        } else {
            assert_eq!(error, Error::Tracking(Joint::BaseYaw));
        }
        assert!(rig.motor.stop_report().is_some());
    }
}

#[test]
fn unqualified_inspection_reports_nonzero_device_status() {
    let mut rig = Rig::new();
    rig.wire.borrow_mut().status = 0x20;
    rig.motor.inspect().unwrap();
    assert_eq!(rig.failure(), Error::Readback(Joint::BaseYaw));
    assert!(rig.motor.stop_report().is_some());
}

#[test]
fn start_fault_cannot_be_healed_by_requalifying_cached_readback() {
    let mut rig = Rig::new();
    rig.wire.borrow_mut().load = 101;
    rig.inspect();
    rig.motor.qualify(qualification()).unwrap();
    let (permit, placement) = rig.authority();
    assert_eq!(
        rig.motor
            .start_segment(permit, placement, [n(1.0); 5], 80_000),
        Err(Error::Load(Joint::BaseYaw))
    );
    assert_eq!(
        rig.motor.stop_report().map(|report| report.reason),
        Some(Error::Load(Joint::BaseYaw))
    );
    assert_eq!(rig.motor.qualify(qualification()), Err(Error::Stopped));
    assert_eq!(rig.motor.inspect(), Err(Error::Stopped));
}

#[test]
fn clock_regression_latches_fault_and_stale_readback_cannot_start() {
    let mut rig = Rig::new();
    rig.ready();
    rig.start();
    rig.clock.0.set(0);
    assert_eq!(rig.motor.tick(), Err(Error::ClockRegression));
    assert_eq!(rig.motor.inspect(), Err(Error::Stopped));
    let mut rig = Rig::new();
    rig.ready();
    rig.clock.advance(200_001);
    let (permit, placement) = rig.authority();
    assert_eq!(
        rig.motor
            .start_segment(permit, placement, [n(1.0); 5], 80_000),
        Err(Error::ReadbackExpired)
    );
    assert!(rig.wire.borrow().goals.is_empty());
}

#[test]
fn disabled_or_unknown_torque_cannot_admit_a_goal_or_enable_the_device() {
    for torque in [0, 2, 255] {
        let mut rig = Rig::new();
        rig.wire.borrow_mut().torque = torque;
        rig.ready();
        let (permit, placement) = rig.authority();
        let result = rig
            .motor
            .start_segment(permit, placement, [n(10.0); 5], 80_000);
        assert!(
            result.is_err(),
            "torque {torque} must not authorize a position write"
        );
        assert!(rig.wire.borrow().goals.is_empty());
        assert!(rig.wire.borrow().packets.iter().all(|p| p[4] == 0x82));
        assert!(rig.motor.stop_report().is_some());
    }
}

#[test]
fn position_bytes_match_the_pinned_sdk_executed_with_an_inert_port() {
    #[derive(serde::Deserialize)]
    struct Fixture {
        cases: Vec<Case>,
    }
    #[derive(serde::Deserialize)]
    struct Case {
        counts: [u16; 5],
        wire_bytes: Vec<u8>,
    }
    let fixture: Fixture =
        serde_json::from_str(include_str!("../fixtures/sdk-position-frames.json")).unwrap();
    assert_eq!(fixture.cases.len(), 3);
    for case in fixture.cases {
        let frame = PositionFrame::encode(
            case.counts
                .map(|n| EncoderCounts::new(i32::from(n)).unwrap()),
        );
        assert_eq!(frame.bytes().as_slice(), case.wire_bytes);
    }
}

#[test]
fn first_host_command_is_under_twenty_ms_in_two_ms_local_tick_harness() {
    let mut rig = Rig::new();
    rig.ready();
    let admitted_at = rig.clock.now();
    let id = rig.start();
    for _ in 0..9 {
        if let Tick::PositionAccepted { motion, at, .. } = rig.step().unwrap() {
            assert_eq!(motion, id);
            let delay = at.as_micros() - admitted_at.as_micros();
            assert!(delay < 20_000, "host acceptance took {delay} us");
            eprintln!("synthetic_accept_to_command_us={delay}, tick_us=2000, interval_us=10000");
            return;
        }
    }
    panic!("no bounded first command");
}

#[test]
fn input_end_or_new_owner_revokes_partial_motion_before_the_next_byte() {
    for new_owner in [false, true] {
        let mut rig = Rig::new();
        rig.ready();
        let motion = rig.start();
        rig.wire.borrow_mut().write_max = 1;
        for _ in 0..9 {
            rig.step().unwrap();
            if !rig.wire.borrow().partial.is_empty() {
                break;
            }
        }
        assert_eq!(rig.wire.borrow().partial.len(), 1);
        let now = rig.clock.now();
        if new_owner {
            rig.controller
                .admit(now, AdmittedInput::Interruption)
                .unwrap();
        } else {
            rig.controller.input_ended(now, motion.owner()).unwrap();
        }
        let expected = if new_owner {
            lamp_turn_owner::Error::StaleOwner
        } else {
            lamp_turn_owner::Error::StalePresentation
        };
        assert_eq!(
            rig.motor.install(rig.controller.snapshot(now).unwrap()),
            Err(Error::Authority(expected))
        );
        assert_eq!(rig.motor.stop_report().unwrap().partial_request_bytes, 1);
        assert_eq!(rig.motor.tick(), Err(Error::Stopped));
        assert!(rig.wire.borrow().goals.is_empty());
        // A flush cannot recall bytes already transmitted. Cancellation must
        // not append the rest of the old packet or reopen this poisoned stream.
        assert_eq!(rig.wire.borrow().partial.len(), 1);
        assert_eq!(rig.wire.borrow().discards, 1);
    }
}

#[test]
fn shared_normalization_never_widens_separate_eeprom_command_bounds() {
    use lamp_body::calibration::RegisterBindingRecord;
    let mut rig = Rig::new();
    rig.calibration = UnitCalibration::with_register_bindings(
        unit(),
        Joint::ALL.map(|joint| CalibrationRecord {
            joint,
            id: joint.id() as u16,
            drive_mode: 0,
            homing_offset: 0,
            range_min: 1000,
            range_max: 3000,
        }),
        Joint::ALL.map(|joint| RegisterBindingRecord {
            joint,
            minimum: 1800,
            maximum: 2200,
            homing_offset: 333,
        }),
    )
    .unwrap();
    rig.wire.borrow_mut().minimum = 1800;
    rig.wire.borrow_mut().maximum = 2200;
    rig.motor = MotorControl::new(
        Port {
            wire: rig.wire.clone(),
            clock: rig.clock.clone(),
        },
        rig.clock.clone(),
        boot(),
        BootId::new([2; 16]).unwrap(),
        &unit(),
        rig.calibration.clone(),
        Timing::new(40_000, 5_000, 200_000, 500_000, 10_000).unwrap(),
    )
    .unwrap();
    rig.ready();
    let (permit, placement) = rig.authority();
    // +30 shared units maps to2300, beyond the separately bound hardware2200.
    assert_eq!(
        rig.motor
            .start_segment(permit, placement, [n(30.0); 5], 80_000),
        Err(Error::Position(Joint::BaseYaw))
    );
    assert!(rig.wire.borrow().goals.is_empty());
    assert!(
        rig.motor.stop_report().is_some(),
        "discovered register/range fault remains terminal"
    );
}

#[test]
fn selected_core_omits_body_ids_and_retains_exact_unselected_read_counts() {
    use lamp_body::command::JointSelection;
    let mut rig = Rig::with_span(0, 4095);
    // 650 is an existing regression value whose floating round trip loses a count.
    rig.wire.borrow_mut().positions = [650, 651, 652, 653, 654];
    rig.ready();
    let (permit, placement) = rig.authority();
    let target = n(674.5 / 4095.0 * 200.0 - 100.0);
    let selected = JointSelection::from_mask(0x10).unwrap();
    let motion = rig
        .motor
        .start_selected_segment(permit, placement, [target; 5], 80_000, selected)
        .unwrap();
    let mut reached = false;
    let mut accepted = 0;
    for _ in 0..90 {
        match rig.step().unwrap() {
            Tick::PositionAccepted {
                selection,
                motion: id,
                counts,
                ..
            } => {
                assert_eq!(selection, selected);
                assert_eq!(id, motion);
                assert_eq!(
                    counts[..4].iter().map(|c| c.get()).collect::<Vec<_>>(),
                    [650, 651, 652, 653]
                );
                accepted += 1;
            }
            Tick::Reached { motion: id, .. } => {
                assert_eq!(id, motion);
                reached = true;
                break;
            }
            _ => {}
        }
    }
    assert!(reached && accepted > 0);
    let wire = rig.wire.borrow();
    assert_eq!(wire.positions, [650, 651, 652, 653, 674]);
    for packet in wire.packets.iter().filter(|p| p[4] == 0x83) {
        assert_eq!(packet.len(), 11);
        assert_eq!(&packet[5..8], &[42, 2, 5]);
    }
}

#[test]
fn selected_core_preserves_cancel_boundary_after_partial_request() {
    use lamp_body::command::JointSelection;
    let mut rig = Rig::new();
    rig.ready();
    let (permit, placement) = rig.authority();
    let motion = rig
        .motor
        .start_selected_segment(
            permit,
            placement,
            [n(10.0); 5],
            80_000,
            JointSelection::from_mask(0x18).unwrap(),
        )
        .unwrap();
    rig.wire.borrow_mut().write_max = 2;
    for _ in 0..30 {
        rig.step().unwrap();
        if !rig.wire.borrow().partial.is_empty() {
            break;
        }
    }
    assert_eq!(rig.wire.borrow().partial.len(), 2);
    let report = rig.motor.cancel(motion).unwrap();
    assert_eq!(report.partial_request_bytes, 2);
    assert!(report.position_request);
    assert!(rig.step().is_err());
    assert!(rig.wire.borrow().goals.is_empty());
    assert_eq!(rig.wire.borrow().discards, 1);
    assert_eq!(rig.motor.shutdown(), report);
}

#[test]
fn exported_readback_keeps_original_time_and_never_survives_expiry_or_stop() {
    let mut r = Rig::new();
    assert_eq!(r.motor.readback(), Err(Error::NotInspected));
    r.ready();
    let first = r.motor.readback().unwrap();
    r.clock.advance(10_000);
    assert_eq!(r.motor.readback().unwrap(), first);
    r.clock.advance(200_000);
    assert_eq!(r.motor.readback(), Err(Error::ReadbackExpired));
    r.motor.shutdown();
    assert_eq!(r.motor.readback(), Err(Error::Stopped));
}

fn selected_profile_rig(prepared: bool) -> Rig {
    use lamp_body::command::JointSelection;
    let mut r = Rig::new();
    let observed = RawProfile::observed_binding(0, 0, 0).unwrap();
    let inert = JointEnvelope::new(
        n(-80.0),
        n(80.0),
        1000.0,
        50_000.0,
        observed,
        100,
        1000,
        30.0,
        0.2,
    )
    .unwrap();
    let mut limits = [inert; 5];
    limits[4] = envelope();
    r.motor
        .qualify(
            QualifiedEnvelope::selected(
                [7; 32],
                r.calibration.clone(),
                limits,
                t(0),
                t(1_000_000),
                JointSelection::from_mask(16).unwrap(),
                Some([observed; 5]),
            )
            .unwrap(),
        )
        .unwrap();
    let mut profiles = [[1, 0, 0xd0, 7, 0, 0, 0, 0]; 5];
    if prepared {
        profiles[4][1] = 10;
        profiles[4][6..8].copy_from_slice(&500u16.to_le_bytes());
    }
    r.wire.borrow_mut().profile_override = Some(profiles);
    r.inspect();
    r
}
fn complete_profile(r: &mut Rig) -> lamp_body::control::ProfileProgress {
    for _ in 0..80 {
        if let Tick::ProfilePrepared { progress, .. } = r.step().unwrap() {
            return progress;
        }
    }
    panic!("profile preparation not completed");
}
#[test]
fn profile_setup_is_exact_speed_ack_readback_then_acceleration_and_no_position() {
    let mut r = selected_profile_rig(false);
    let (permit, placement) = r.authority();
    r.motor
        .prepare_head_profile(permit, placement, 500, 10)
        .unwrap();
    let p = complete_profile(&mut r);
    let wire = r.wire.borrow();
    let writes: Vec<_> = wire.packets.iter().filter(|p| p[4] == 3).cloned().collect();
    assert_eq!(
        writes,
        vec![
            vec![255, 255, 5, 5, 3, 46, 0xf4, 1, 0xcf],
            vec![255, 255, 5, 4, 3, 41, 10, 0xc0]
        ]
    );
    assert!(wire.goals.is_empty());
    assert_eq!(p.speed_request_bytes, 9);
    assert_eq!(p.acceleration_request_bytes, 8);
    assert!(p.speed_ack_at_us.unwrap() < p.speed_readback_at_us.unwrap());
    assert!(p.speed_readback_at_us.unwrap() < p.acceleration_ack_at_us.unwrap());
    assert!(p.acceleration_ack_at_us.unwrap() < p.final_readback_at_us.unwrap());
    assert_eq!(p.before[4], [1, 0, 0xd0, 7, 0, 0, 0, 0]);
    assert_eq!(p.after.unwrap()[4], [1, 10, 0xd0, 7, 0, 0, 0xf4, 1]);
    for i in 0..4 {
        assert_eq!(p.before[i], p.after.unwrap()[i]);
    }
    drop(wire);
    assert_eq!(r.motor.readback().unwrap().counts(), [2000; 5]);
}
#[test]
fn inactive_zero_profiles_never_allow_all_or_body_position_packets() {
    use lamp_body::command::JointSelection;
    let mut r = selected_profile_rig(true);
    let (permit, placement) = r.authority();
    assert_eq!(
        r.motor
            .start_segment(permit, placement, [n(1.0); 5], 50_000),
        Err(Error::Selection)
    );
    assert_eq!(
        r.motor.start_selected_segment(
            permit,
            placement,
            [n(1.0); 5],
            50_000,
            JointSelection::from_mask(17).unwrap()
        ),
        Err(Error::Selection)
    );
    assert!(r.wire.borrow().goals.is_empty());
    r.motor
        .start_selected_segment(
            permit,
            placement,
            [n(1.0); 5],
            50_000,
            JointSelection::from_mask(16).unwrap(),
        )
        .unwrap();
    r.first_goal();
    for p in r.wire.borrow().packets.iter().filter(|p| p[4] == 0x83) {
        assert_eq!(p.len(), 11);
        assert_eq!(p[7], 5);
    }
}
#[test]
fn zero_selected_profile_still_rejects_motion_without_preparation() {
    let mut r = selected_profile_rig(false);
    let (p, o) = r.authority();
    assert_eq!(
        r.motor.start_selected_segment(
            p,
            o,
            [n(1.0); 5],
            50_000,
            lamp_body::command::JointSelection::from_mask(16).unwrap()
        ),
        Err(Error::Profile(Joint::WristPitch))
    );
    assert!(r.motor.stop_report().is_some());
    assert!(r.wire.borrow().goals.is_empty());
}
#[test]
fn profile_setup_cancel_between_writes_retains_verified_partial_change_without_reset() {
    let mut r = selected_profile_rig(false);
    let (p, o) = r.authority();
    let id = r.motor.prepare_head_profile(p, o, 500, 10).unwrap();
    for _ in 0..80 {
        r.step().unwrap();
        if r.wire
            .borrow()
            .packets
            .iter()
            .filter(|v| v[5] == 56)
            .count()
            == 2
        {
            break;
        }
    }
    // Consume and verify that response, but do not queue the acceleration write.
    r.step().unwrap();
    let report = r.motor.cancel(id).unwrap();
    assert!(
        report
            .profile_preparation
            .unwrap()
            .speed_readback_at_us
            .is_some()
    );
    assert!(!report.position_request);
    assert_eq!(report.profile_preparation.unwrap().speed_request_bytes, 9);
    assert_eq!(
        r.wire.borrow().profile_override.unwrap()[4][6..8],
        500u16.to_le_bytes()
    );
    assert_eq!(r.wire.borrow().profile_override.unwrap()[4][1], 0);
    assert_eq!(r.motor.tick(), Err(Error::Stopped));
    assert_eq!(
        r.wire.borrow().packets.iter().filter(|v| v[4] == 3).count(),
        1
    );
}
#[test]
fn profile_ack_readback_faults_never_advance_to_acceleration() {
    for case in 0..4 {
        let mut r = selected_profile_rig(false);
        let (p, o) = r.authority();
        {
            let mut w = r.wire.borrow_mut();
            match case {
                0 => w.missing_ack = true,
                1 => w.ack_id = 4,
                2 => w.ack_status = 0x20,
                3 => w.ignore_profile_write = true,
                _ => unreachable!(),
            }
        }
        r.motor.prepare_head_profile(p, o, 500, 10).unwrap();
        let failure = r.failure();
        assert!(
            matches!(
                failure,
                Error::Deadline { .. } | Error::Protocol(_) | Error::Profile(_)
            ),
            "{failure:?}"
        );
        let evidence = r.motor.stop_report().unwrap().profile_preparation.unwrap();
        assert_eq!(evidence.speed_request_bytes, 9);
        assert_eq!(evidence.acceleration_request_bytes, 0);
        assert!(evidence.final_readback_at_us.is_none());
        assert_eq!(
            r.wire.borrow().packets.iter().filter(|v| v[4] == 3).count(),
            1
        );
    }
}
#[test]
fn profile_partial_write_privacy_and_expiry_are_terminal_with_original_byte_evidence() {
    for privacy in [false, true] {
        let mut r = selected_profile_rig(false);
        let (p, o) = r.authority();
        r.wire.borrow_mut().write_max = 3;
        r.motor.prepare_head_profile(p, o, 500, 10).unwrap();
        r.step().unwrap();
        r.step().unwrap();
        assert_eq!(r.wire.borrow().partial.len(), 3);
        if privacy {
            r.controller
                .set_camera_permission(r.clock.now(), Permission::Denied)
                .unwrap();
            assert!(
                r.motor
                    .install(r.controller.snapshot(r.clock.now()).unwrap())
                    .is_err()
            );
        } else {
            r.clock.advance(240_000);
            assert!(r.motor.tick().is_err());
        }
        let stop = r.motor.stop_report().unwrap();
        assert_eq!(stop.partial_request_bytes, 3);
        assert!(!stop.position_request);
        assert_eq!(stop.profile_preparation.unwrap().speed_request_bytes, 3);
        assert_eq!(r.wire.borrow().profile_override.unwrap()[4][6..8], [0, 0]);
        assert_eq!(r.motor.tick(), Err(Error::Stopped));
    }
}
#[test]
fn profile_preparation_rejects_wrong_values_and_nonstationary_goal_without_writes() {
    let mut r = selected_profile_rig(false);
    let (p, o) = r.authority();
    for (speed, accel) in [(0, 10), (500, 0), (501, 10), (500, 11)] {
        assert_eq!(
            r.motor.prepare_head_profile(p, o, speed, accel),
            Err(Error::ProfilePreparation)
        );
    }
    assert!(r.wire.borrow().packets.iter().all(|v| v[4] == 0x82));
    let mut r = selected_profile_rig(false);
    r.wire.borrow_mut().profile_override.as_mut().unwrap()[0][2] = 0;
    r.inspect();
    let (p, o) = r.authority();
    assert_eq!(
        r.motor.prepare_head_profile(p, o, 500, 10),
        Err(Error::NotAtRest(Joint::BaseYaw))
    );
    assert!(r.wire.borrow().packets.iter().all(|v| v[4] == 0x82));
}

#[test]
fn selected_profile_worker_uses_real_channels_original_evidence_and_readback_receipt() {
    use lamp_body::runtime::{
        channel::Channels,
        wire::{Command, Control, Event, Observation},
        worker::Worker,
    };
    let mut r = selected_profile_rig(false);
    let (permit, placement) = r.authority();
    let worker_boot = BootId::new([2; 16]).unwrap();
    let (mut parent, mut child) = Channels::pair(boot(), worker_boot).unwrap();
    parent
        .control
        .send(
            &Control::Authority {
                snapshot: r.controller.snapshot(r.clock.now()).unwrap(),
            },
            r.clock.now(),
        )
        .unwrap();
    let observation = Observation {
        source_boot: placement.source_boot,
        frame_sequence: placement.frame_sequence,
        acquired_at_us: placement.acquired_at.as_micros(),
        valid_until_us: placement.valid_until.as_micros(),
        camera: placement.camera,
        qualification_record: placement.qualification_record,
    };
    parent
        .data
        .send(
            &Command::PrepareHeadProfile {
                request: 1,
                requested_at_us: r.clock.now().as_micros(),
                permit,
                placement: observation,
                speed_raw: 500,
                acceleration_raw: 10,
            },
            r.clock.now(),
        )
        .unwrap();
    let mut worker = Worker::new(
        r.motor,
        r.clock.clone(),
        boot(),
        worker_boot,
        boot(),
        1_000_000,
    )
    .unwrap();
    let admitted_at = r.clock.now();
    let mut events = Vec::new();
    let mut first_write = None;
    for _ in 0..80 {
        r.clock.advance(2_000);
        assert!(!worker.poll(&mut child).unwrap());
        if first_write.is_none() && r.wire.borrow().packets.iter().any(|p| p[4] == 3) {
            first_write = Some(r.clock.now().as_micros());
        }
        while let Some(event) = parent.control.receive::<Event>(r.clock.now()).unwrap() {
            events.push(event);
        }
        if events
            .iter()
            .any(|e| matches!(e, Event::ProfilePrepared { .. }))
        {
            break;
        }
    }
    assert!(first_write.unwrap() - admitted_at.as_micros() < 20_000);
    let started = events
        .iter()
        .find_map(|e| {
            if let Event::ProfileStarted { motion, .. } = e {
                Some(*motion)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(started.owner, permit.owner());
    assert_eq!(started.worker, worker_boot);
    let completed = events
        .iter()
        .find_map(|e| {
            if let Event::ProfilePrepared {
                motion,
                progress,
                readback,
                ..
            } = e
            {
                Some((motion, progress, readback))
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(*completed.0, started);
    assert!(completed.1.final_readback_at_us.is_some());
    assert_eq!(completed.2.counts(), [2000; 5]);
    assert!(!events.iter().any(|e| matches!(
        e,
        Event::HostAccepted { .. } | Event::ReadbackReached { .. }
    )));
    parent
        .control
        .send(&Control::Stop {}, r.clock.now())
        .unwrap();
    assert!(worker.poll(&mut child).unwrap());
    worker.finish(&mut child).unwrap();
    let mut stopped = false;
    while let Some(e) = parent.control.receive::<Event>(r.clock.now()).unwrap() {
        if let Event::Inhibited { report, .. } = e {
            assert!(
                report
                    .profile_preparation
                    .unwrap()
                    .final_readback_at_us
                    .is_some()
            );
            stopped = true;
        }
    }
    assert!(stopped);
    assert!(r.wire.borrow().goals.is_empty());
}
#[test]
fn selected_profile_worker_priority_privacy_and_stop_prevent_queued_register_write() {
    use lamp_body::runtime::{
        channel::Channels,
        wire::{Command, Control, Observation},
        worker::Worker,
    };
    for privacy in [false, true] {
        let mut r = selected_profile_rig(false);
        let (permit, placement) = r.authority();
        let worker_boot = BootId::new([2; 16]).unwrap();
        let (mut parent, mut child) = Channels::pair(boot(), worker_boot).unwrap();
        parent
            .control
            .send(
                &Control::Authority {
                    snapshot: r.controller.snapshot(r.clock.now()).unwrap(),
                },
                r.clock.now(),
            )
            .unwrap();
        parent
            .data
            .send(
                &Command::PrepareHeadProfile {
                    request: 1,
                    requested_at_us: r.clock.now().as_micros(),
                    permit,
                    placement: Observation {
                        source_boot: boot(),
                        frame_sequence: 1,
                        acquired_at_us: placement.acquired_at.as_micros(),
                        valid_until_us: placement.valid_until.as_micros(),
                        camera: placement.camera,
                        qualification_record: [7; 32],
                    },
                    speed_raw: 500,
                    acceleration_raw: 10,
                },
                r.clock.now(),
            )
            .unwrap();
        let mut worker = Worker::new(
            r.motor,
            r.clock.clone(),
            boot(),
            worker_boot,
            boot(),
            1_000_000,
        )
        .unwrap();
        assert!(!worker.poll(&mut child).unwrap()); // Queues frame, no write yet.
        r.clock.advance(1);
        let control = if privacy {
            r.controller
                .set_camera_permission(r.clock.now(), Permission::Denied)
                .unwrap();
            Control::Authority {
                snapshot: r.controller.snapshot(r.clock.now()).unwrap(),
            }
        } else {
            Control::Stop {}
        };
        parent.control.send(&control, r.clock.now()).unwrap();
        let result = worker.poll(&mut child);
        assert!(if privacy {
            result.is_err()
        } else {
            result.unwrap()
        });
        assert!(r.wire.borrow().packets.iter().all(|p| p[4] == 0x82));
        let result = worker.finish(&mut child);
        assert_eq!(result.is_err(), privacy);
        assert!(r.wire.borrow().goals.is_empty());
    }
}

#[test]
fn profile_preparation_keeps_exact_inactive_bindings_and_single_attempt() {
    let mut r = selected_profile_rig(false);
    r.wire.borrow_mut().profile_override.as_mut().unwrap()[1][1] = 1;
    r.inspect();
    let (p, o) = r.authority();
    assert_eq!(
        r.motor.prepare_head_profile(p, o, 500, 10),
        Err(Error::Profile(Joint::BasePitch))
    );
    assert!(r.wire.borrow().packets.iter().all(|v| v[4] == 0x82));
    let mut r = selected_profile_rig(false);
    let (p, o) = r.authority();
    r.motor.prepare_head_profile(p, o, 500, 10).unwrap();
    complete_profile(&mut r);
    assert_eq!(
        r.motor.prepare_head_profile(p, o, 500, 10),
        Err(Error::ProfilePreparation)
    );
    assert_eq!(
        r.wire.borrow().packets.iter().filter(|v| v[4] == 3).count(),
        2
    );
}
