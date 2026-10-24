//! A bounded loopback-only MQTT-v5 peer using the production dependency's codec.
//! It authenticates no real account and implements only the exercised packets.
use super::support::{Owned, record};
use bytes::BytesMut;
use rumqttc::v5::mqttbytes::{
    QoS,
    v5::{
        ConnAck, ConnectReturnCode, Packet, PingResp, PubAck, Publish, SubAck, SubscribeReasonCode,
    },
};
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

pub struct Broker {
    socket: TcpStream,
    buffer: BytesMut,
    root: PathBuf,
    stage: String,
    next_id: u16,
    publishes: usize,
    deadline: Instant,
}
impl Broker {
    pub fn listener() -> TcpListener {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        listener
    }
    pub fn accept(
        listener: TcpListener,
        root: &Path,
        stage: &str,
        child: &mut Owned,
        deadline: Instant,
    ) -> Self {
        let until = deadline.min(Instant::now() + Duration::from_secs(10));
        let socket = loop {
            child.alive();
            match listener.accept() {
                Ok((s, peer)) => {
                    assert!(peer.ip().is_loopback());
                    break s;
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) => panic!("local accept: {e}"),
            }
            assert!(Instant::now() < until, "local broker connect deadline");
            thread::sleep(Duration::from_millis(5));
        };
        socket
            .set_write_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut b = Self {
            socket,
            buffer: BytesMut::new(),
            root: root.into(),
            stage: stage.into(),
            next_id: 100,
            publishes: 0,
            deadline,
        };
        match b.next() {
            Packet::Connect(connect, will, login) => {
                assert_eq!(connect.client_id, "os-server-compat-dummy");
                assert!(will.is_none(), "no unrelated Cloud switch enabled");
                let login = login.unwrap();
                assert_eq!(login.username, "fixture");
                assert_eq!(login.password, "fixture-only");
                record(
                    root,
                    json!({"event":"mqtt_connect","stage":stage,"client_id":connect.client_id,"dummy_login_verified":true}),
                );
            }
            _ => panic!("expected actual MQTT-v5 CONNECT"),
        }
        b.send(Packet::ConnAck(ConnAck {
            session_present: false,
            code: ConnectReturnCode::Success,
            properties: None,
        }));
        let mut subscribed = false;
        let mut info = false;
        for _ in 0..16 {
            match b.next() {
                Packet::Subscribe(s) => {
                    assert!(!subscribed);
                    assert_eq!(s.filters.len(), 1);
                    assert_eq!(s.filters[0].path, "compat/fa");
                    b.send(Packet::SubAck(SubAck {
                        pkid: s.pkid,
                        return_codes: vec![SubscribeReasonCode::Success(QoS::AtLeastOnce)],
                        properties: None,
                    }));
                    subscribed = true;
                }
                Packet::Publish(p) => {
                    let v = b.published(p);
                    assert_eq!(v["type"], "info");
                    assert!(!info);
                    info = true;
                }
                Packet::PingReq(_) => b.send(Packet::PingResp(PingResp)),
                _ => panic!("unexpected handshake packet"),
            }
            if subscribed && info {
                return b;
            }
        }
        panic!("bounded MQTT handshake inventory");
    }
    fn next(&mut self) -> Packet {
        loop {
            assert!(Instant::now() < self.deadline, "case deadline");
            if let Ok(packet) = Packet::read(&mut self.buffer, None) {
                return packet;
            }
            assert!(self.buffer.len() < 128 * 1024, "bounded MQTT packet");
            self.socket
                .set_read_timeout(Some(
                    Duration::from_secs(2)
                        .min(self.deadline.saturating_duration_since(Instant::now())),
                ))
                .unwrap();
            let mut bytes = [0; 8192];
            let n = self
                .socket
                .read(&mut bytes)
                .expect("bounded local MQTT read");
            assert!(n > 0, "actual Cloud connection closed");
            self.buffer.extend_from_slice(&bytes[..n]);
        }
    }
    fn send(&mut self, packet: Packet) {
        assert!(Instant::now() < self.deadline);
        let mut out = BytesMut::new();
        packet.write(&mut out, None).unwrap();
        self.socket.write_all(&out).unwrap();
    }
    fn published(&mut self, p: Publish) -> Value {
        assert_eq!(p.topic.as_ref(), b"compat/fd");
        assert_eq!(p.qos, QoS::AtLeastOnce);
        assert!(p.payload.len() <= 64 * 1024 && self.publishes < 16);
        self.publishes += 1;
        let value: Value = serde_json::from_slice(&p.payload).unwrap();
        assert_eq!(value["device"], "lamp");
        assert_eq!(value["generation"], "lamp-v2");
        assert_eq!(value["id"], "compat-dummy");
        record(
            &self.root,
            json!({"event":"mqtt_publish","stage":self.stage,"packet_id":p.pkid,"topic":"compat/fd","payload":value}),
        );
        self.send(Packet::PubAck(PubAck::new(p.pkid, None)));
        value
    }
    pub fn list(&mut self, label: &str) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        let command = json!({"cmd":"data","kind":"skills.list","data":{}}).to_string();
        let mut p = Publish::new("compat/fa", QoS::AtLeastOnce, command, None);
        p.pkid = id;
        record(
            &self.root,
            json!({"event":"mqtt_request","stage":self.stage,"label":label,"packet_id":id,"payload":{"cmd":"data","kind":"skills.list","data":{}}}),
        );
        self.send(Packet::Publish(p));
        let mut acked = false;
        let mut reply = None;
        for _ in 0..16 {
            match self.next() {
                Packet::PubAck(a) => {
                    assert_eq!(a.pkid, id);
                    assert!(!acked);
                    acked = true;
                }
                Packet::Publish(p) => {
                    let v = self.published(p);
                    assert_eq!(v["type"], "data");
                    assert_eq!(v["kind"], "skills.list");
                    assert_eq!(v["status"], "success");
                    assert!(reply.replace(v).is_none(), "exactly one command result");
                }
                Packet::PingReq(_) => self.send(Packet::PingResp(PingResp)),
                _ => panic!("unexpected list packet"),
            }
            if acked && let Some(v) = &reply {
                return v.clone();
            }
        }
        panic!("bounded MQTT command inventory");
    }
}
