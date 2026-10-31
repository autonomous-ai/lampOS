//! Product-route speech transport with immutable PCM and private raw-frame evidence.
mod stream;
use crate::{
    Result, hash, invalid,
    live_http::{contains_secret, save, wire_safe_to_save},
    model::{Call, Provenance, Provider, Reply},
};
use futures_util::{SinkExt, StreamExt};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs::{self, File},
    net::IpAddr,
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{Notify, oneshot};
use tokio_tungstenite::{
    Connector, connect_async_tls_with_config,
    tungstenite::{
        Message,
        client::IntoClientRequest,
        http::HeaderValue,
        protocol::{WebSocketConfig, frame::coding::CloseCode},
    },
};

const MAX_FRAME: usize = 1 << 20;
const MAX_WIRE: usize = 4 << 20;
const MODEL: &str = "nova-3-general";
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub endpoint: String,
    pub expected_model_arch: String,
    pub expected_model_revision: Option<String>,
}
impl Config {
    fn url(&self, fixture: bool) -> Result<Url> {
        let mut url =
            Url::parse(&self.endpoint).map_err(|_| invalid("invalid public speech endpoint"))?;
        let loopback = url
            .host_str()
            .and_then(|s| s.trim_matches(['[', ']']).parse::<IpAddr>().ok())
            .is_some_and(|ip| ip.is_loopback());
        if !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || url.host_str().is_none()
            || if fixture {
                url.scheme() != "ws" || !loopback
            } else {
                url.scheme() != "wss"
            }
        {
            return Err(invalid(
                "speech endpoint must be credential-free WSS, or an explicit loopback fixture",
            ));
        }
        for value in
            std::iter::once(&self.expected_model_arch).chain(self.expected_model_revision.iter())
        {
            if value.is_empty()
                || value.len() > 128
                || !value
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
            {
                return Err(invalid("invalid pinned speech identity"));
            }
        }
        url.query_pairs_mut().extend_pairs([
            ("model", MODEL),
            ("encoding", "linear16"),
            ("sample_rate", "16000"),
            ("channels", "1"),
            ("language", "en"),
            ("smart_format", "true"),
            ("interim_results", "true"),
            ("endpointing", "300"),
            ("vad_events", "true"),
            ("keyterm", "Lamp"),
        ]);
        Ok(url)
    }
}
pub struct Speech {
    config: Config,
    url: Url,
    key: String,
    auth: HeaderValue,
    tls: Arc<rustls::ClientConfig>,
    runtime: tokio::runtime::Runtime,
    artifacts: PathBuf,
    next: u64,
    provenance: Option<Provenance>,
}
impl Speech {
    pub fn from_environment(config: Config, key_variable: &str, artifacts: &Path) -> Result<Self> {
        config.url(false)?;
        if key_variable.is_empty()
            || key_variable.len() > 128
            || !key_variable
                .bytes()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == b'_')
        {
            return Err(invalid("invalid speech credential environment name"));
        }
        let key = std::env::var(key_variable)
            .map_err(|_| invalid("required speech credential is not set"))?;
        Self::create(config, key, artifacts, false)
    }
    pub fn loopback_fixture(config: Config, artifacts: &Path) -> Result<Self> {
        Self::create(
            config,
            "bench-offline-speech-credential".into(),
            artifacts,
            true,
        )
    }
    fn create(config: Config, key: String, artifacts: &Path, fixture: bool) -> Result<Self> {
        let url = config.url(fixture)?;
        if key.len() < 8
            || key.len() > 4096
            || !key.bytes().all(|c| c.is_ascii_graphic())
            || contains_secret(&serde_json::to_value(&config)?, &key)
        {
            return Err(invalid("invalid speech credential or public configuration"));
        }
        let mut auth = HeaderValue::from_str(&format!("Token {key}"))
            .map_err(|_| invalid("invalid speech credential"))?;
        auth.set_sensitive(true);
        let roots = rustls::RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        };
        let tls = Arc::new(
            rustls::ClientConfig::builder_with_provider(Arc::new(
                rustls::crypto::ring::default_provider(),
            ))
            .with_safe_default_protocol_versions()
            .map_err(|_| invalid("speech TLS setup failed"))?
            .with_root_certificates(roots)
            .with_no_client_auth(),
        );
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        fs::DirBuilder::new().mode(0o700).create(artifacts)?;
        File::open(
            artifacts
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new(".")),
        )?
        .sync_all()?;
        Ok(Self {
            config,
            url,
            key,
            auth,
            tls,
            runtime,
            artifacts: artifacts.into(),
            next: 0,
            provenance: None,
        })
    }
}
fn frame(raw: &mut Vec<u8>, value: Value) -> Result<()> {
    let bytes = serde_json::to_vec(&value)?;
    if raw.len().saturating_add(bytes.len() + 1) > MAX_WIRE {
        return Err(invalid("speech frame evidence exceeds four MiB"));
    }
    raw.extend(bytes);
    raw.push(b'\n');
    Ok(())
}
impl Provider for Speech {
    fn invoke(&mut self, call: &Call, deadline: Instant) -> Result<Reply> {
        self.provenance = None;
        call.validate()?;
        let Call::Transcribe { audio } = call else {
            return Err(invalid("speech transport requires verified PCM"));
        };
        if Instant::now() >= deadline || self.next >= 100_000 {
            return Err(invalid("speech expired or call limit reached"));
        }
        let meta = fs::symlink_metadata(&self.artifacts)?;
        if !meta.is_dir() || meta.permissions().mode() & 0o077 != 0 {
            return Err(invalid("speech artifact directory is not private"));
        }
        let request_body = serde_json::to_vec(
            &json!({"url":self.url.as_str(),"audio":audio,"frame_ms":20,"pacing":"real_time","controls":["Finalize","CloseStream"],"expected_model_arch":self.config.expected_model_arch,"expected_model_revision":self.config.expected_model_revision}),
        )?;
        self.next += 1;
        let artifact = format!("{:06}", self.next);
        let dir = self.artifacts.join(&artifact);
        fs::DirBuilder::new().mode(0o700).create(&dir)?;
        save(&dir.join("request.json"), &request_body)?;
        File::open(&dir)?.sync_all()?;
        File::open(&self.artifacts)?.sync_all()?;
        let mut request = self
            .url
            .as_str()
            .into_client_request()
            .map_err(|_| invalid("speech handshake setup failed"))?;
        request
            .headers_mut()
            .insert("authorization", self.auth.clone());
        let mut parser = stream::Stream::new(
            &self.config.expected_model_arch,
            self.config.expected_model_revision.as_deref(),
        );
        let mut raw = Vec::new();
        let mut sent = 0u64;
        let mut normal_close = false;
        let mut status = None;
        let mut credential_echo = false;
        if Instant::now() >= deadline {
            return Err(invalid("speech expired while saving request"));
        }
        let result=self.runtime.block_on(async {
            tokio::time::timeout_at(tokio::time::Instant::from_std(deadline),async {
                let socket_config=WebSocketConfig::default().max_message_size(Some(MAX_FRAME)).max_frame_size(Some(MAX_FRAME));
                let (socket,response)=tokio::time::timeout(Duration::from_secs(10),connect_async_tls_with_config(request,Some(socket_config),true,Some(Connector::Rustls(self.tls.clone())))).await
                    .map_err(|_|invalid("speech connection deadline expired"))?.map_err(|_|invalid("speech handshake failed"))?;
                status=Some(response.status().as_u16());
                let (mut write,mut read)=socket.split();
                let finalized=Arc::new(Notify::new());let heard_finalized=finalized.clone();
                let (closing,mut closed_request)=oneshot::channel::<tokio::time::Instant>();
                let upload=async {
                    let mut ticks=tokio::time::interval(Duration::from_millis(20));ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                    for bytes in audio.pcm().chunks(640) {
                        ticks.tick().await;
                        write.send(Message::Binary(bytes.to_vec().into())).await.map_err(|_|invalid("speech audio upload failed"))?;
                        sent+=bytes.len() as u64/2;
                    }
                    write.send(Message::text(r#"{"type":"Finalize"}"#)).await.map_err(|_|invalid("speech finalize failed"))?;
                    let _=tokio::time::timeout(Duration::from_secs(1),finalized.notified()).await;
                    write.send(Message::text(r#"{"type":"CloseStream"}"#)).await.map_err(|_|invalid("speech close request failed"))?;
                    let _=closing.send(tokio::time::Instant::now());
                    Ok::<(),crate::Error>(())
                };
                let receive=async {
                    let mut finish_by=None;let mut frames=0usize;
                    loop {
                        let by=finish_by.unwrap_or_else(||tokio::time::Instant::now()+Duration::from_secs(30));
                        let next=tokio::select! {
                            biased;
                            closed=&mut closed_request, if finish_by.is_none()=>{
                                finish_by=Some(closed.map_err(|_|invalid("speech writer ended before close request"))?+Duration::from_secs(15));continue;
                            }
                            next=tokio::time::timeout_at(by,read.next())=>next.map_err(|_|invalid("speech response stalled or final close expired"))?,
                        };
                        let message=next.ok_or_else(||invalid("speech stream ended without a close frame"))?.map_err(|_|invalid("speech response transport failed"))?;
                        frames+=1;if frames>16_384 {return Err(invalid("speech frame count exceeds bound"));}
                        match message {
                            Message::Text(text)=>{
                                frame(&mut raw,json!({"kind":"text","text":text.as_str()}))?;
                                if !wire_safe_to_save(text.as_bytes(),&self.key) {credential_echo=true;return Err(invalid("speech response cannot be archived safely"));}
                                let value:Value=serde_json::from_str(&text).map_err(|_|invalid("invalid speech JSON frame"))?;
                                if parser.feed(&value)? {heard_finalized.notify_one();}
                            }
                            Message::Close(close)=>{
                                let code=close.as_ref().map(|f|u16::from(f.code));frame(&mut raw,json!({"kind":"close","code":code}))?;
                                if close.as_ref().is_some_and(|f|f.code!=CloseCode::Normal) {return Err(invalid("speech service closed abnormally"));}
                                normal_close=true;return Ok::<(),crate::Error>(());
                            }
                            Message::Ping(bytes)|Message::Pong(bytes)=>{frame(&mut raw,json!({"kind":"keepalive","bytes":bytes.len(),"sha256":hash(&bytes)}))?;}
                            _=>return Err(invalid("unexpected binary speech response")),
                        }
                    }
                };
                tokio::try_join!(upload,receive)?;
                if sent!=audio.samples() {return Err(invalid("speech stream did not upload all verified samples"));}
                if parser.duration.as_ref().is_some_and(|u|u.audio_micros.saturating_add(50_000)<audio.reserve_micros()) {return Err(invalid("speech service processed only part of the audio"));}
                parser.reply(audio)
            }).await.map_err(|_|invalid("speech absolute deadline expired"))?
        });
        let mut evidence = parser.evidence(audio, sent);
        if credential_echo {
            evidence.model_revision = None;
            evidence.model_uuid = None;
        }
        let mut provenance = Provenance {
            transport: "deepgram_websocket".into(),
            requested_model: MODEL.into(),
            reported_model: (!credential_echo)
                .then(|| parser.model_name.clone())
                .flatten(),
            provider_request_id: (!credential_echo)
                .then(|| parser.request_id.clone())
                .flatten(),
            system_fingerprint: None,
            finish_reason: Some(
                if normal_close {
                    "normal_close"
                } else {
                    "incomplete"
                }
                .into(),
            ),
            http_status: status,
            request_sha256: hash(&request_body),
            response_sha256: hash(&raw),
            artifact,
            response_complete: parser.metadata_seen || normal_close,
            audio: Some(evidence),
            evidence_complete: false,
            usage: None,
        };
        self.provenance = Some(provenance.clone());
        if credential_echo {
            return Err(invalid(
                "speech response echoed a credential or could not be scanned",
            ));
        }
        save(&dir.join("response.jsonl"), &raw)?;
        provenance.evidence_complete = true;
        save(
            &dir.join("provenance.json"),
            &serde_json::to_vec_pretty(&provenance)?,
        )?;
        File::open(&dir)?.sync_all()?;
        self.provenance = Some(provenance);
        result
    }
    fn take_provenance(&mut self) -> Option<Provenance> {
        self.provenance.take()
    }
}

/// One Provider and Client own chat and speech; neither service gets a separate
/// campaign or a hidden retry path. An absent transport fails before dispatch.
pub struct Services {
    pub chat: Option<crate::live_http::Http>,
    pub speech: Option<Speech>,
    last: Option<Provenance>,
}
impl Services {
    pub fn new(chat: Option<crate::live_http::Http>, speech: Option<Speech>) -> Self {
        Self {
            chat,
            speech,
            last: None,
        }
    }
}
impl Provider for Services {
    fn invoke(&mut self, call: &Call, deadline: Instant) -> Result<Reply> {
        self.last = None;
        match call {
            Call::Transcribe { .. } => {
                let service = self
                    .speech
                    .as_mut()
                    .ok_or_else(|| invalid("speech service not configured"))?;
                let result = service.invoke(call, deadline);
                self.last = service.take_provenance();
                result
            }
            Call::Chat { .. } => {
                let service = self
                    .chat
                    .as_mut()
                    .ok_or_else(|| invalid("chat service not configured"))?;
                let result = service.invoke(call, deadline);
                self.last = service.take_provenance();
                result
            }
            Call::Embedding { .. } => Err(invalid("live embedding service not configured")),
        }
    }
    fn take_provenance(&mut self) -> Option<Provenance> {
        self.last.take()
    }
}
