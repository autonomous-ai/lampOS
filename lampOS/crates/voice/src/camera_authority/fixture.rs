//! Actual SCM_RIGHTS fixture, never a production bypass. Callers still publish
//! V3, seal exact audio and use production permission/Engine/Services paths.
use super::*;
use lamp_ipc::camera_authority::HoldWitness;
use std::os::unix::{fs::PermissionsExt, net::UnixListener};

pub(crate) struct Fixture {
    pub(crate) known: PathBuf,
    pub(crate) witness: Arc<HoldWitness>,
    pub(crate) client: Arc<Client>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    pub(super) socket: PathBuf,
}
impl Fixture {
    /// known's parent must already exist and be private, as on the real unit.
    /// publisher/hold also go in the actual V3 publication (no hidden rewrite).
    pub(crate) fn start(known: PathBuf, publisher: [u8; 16], hold: u64) -> Self {
        let token = Token::new(publisher, hold).unwrap();
        let witness = Arc::new(HoldWitness::new(token).unwrap());
        let mut name = known.file_name().unwrap().to_os_string();
        name.push(".authority.d");
        let directory = known.parent().unwrap().join(name);
        fs::create_dir(&directory).unwrap();
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
        let socket = directory.join("hold.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        fs::set_permissions(&socket, fs::Permissions::from_mode(0o600)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let ended = stop.clone();
        let served = witness.clone();
        let worker = thread::spawn(move || {
            while !ended.load(Ordering::Acquire) {
                if !lamp_ipc::readable(&listener, POLL).unwrap_or(false) {
                    continue;
                }
                let Ok((stream, _)) = listener.accept() else {
                    continue;
                };
                stream.set_nonblocking(true).unwrap();
                let until = Instant::now() + GRANT_WITHIN;
                let mut request = [0; 49];
                let mut used = 0;
                while used < request.len()
                    && Instant::now() < until
                    && !ended.load(Ordering::Acquire)
                {
                    match (&stream).read(&mut request[used..]) {
                        Ok(0) => break,
                        Ok(n) => used += n,
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                            let _ = lamp_ipc::readable(&stream, POLL);
                        }
                        Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                        Err(_) => break,
                    }
                }
                if used != 48
                    || &request[..8] != b"LMPASK03"
                    || request[8..24] != publisher
                    || request[24..32] != hold.to_be_bytes()
                    || request[40..48] != [0; 8]
                {
                    continue;
                }
                let Some(nonce) =
                    NonZeroU64::new(u64::from_be_bytes(request[32..40].try_into().unwrap()))
                else {
                    continue;
                };
                let Ok(mut grant) = served.grant(nonce, until) else {
                    continue;
                };
                while Instant::now() < until && !ended.load(Ordering::Acquire) {
                    match grant.try_send(&stream) {
                        Ok(true) | Err(_) => break,
                        Ok(false) => {
                            thread::park_timeout(Duration::from_millis(1));
                        }
                    }
                }
            }
        });
        let client = Client::start(known.clone()).unwrap();
        Self {
            known,
            witness,
            client,
            stop,
            worker: Some(worker),
            socket,
        }
    }
    pub(crate) fn cache(&self) -> Arc<crate::memory_scope::FaceCache> {
        let cache = Arc::new(crate::memory_scope::FaceCache::default());
        assert!(cache.set_camera_client(self.client.clone()));
        cache
    }
    /// Bounded host fixture wait; does not change any production request bound.
    pub(crate) fn ready(&self, lease: &ScopeLease) -> crate::second_provider::PermissionState {
        let until = Instant::now() + GRANT_WITHIN;
        loop {
            let Some(source) = read(&self.known) else {
                return crate::second_provider::PermissionState::Lost;
            };
            let state = permission(&self.known, lease, &source);
            if state != crate::second_provider::PermissionState::Pending || Instant::now() >= until
            {
                return state;
            }
            thread::sleep(Duration::from_millis(1));
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.witness.revoke();
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        let _ = fs::remove_file(&self.socket);
        if let Some(parent) = self.socket.parent() {
            let _ = fs::remove_dir(parent);
        }
    }
}
