//! Against vectors made with Python's `cryptography`, as V1's
//! `hal/drivers/sensing/crypto.py` uses it (`tests/fixtures/v1-vector.json`,
//! a throwaway test key no server holds), and round trips through the same
//! key.
use super::*;
use rsa::{BigUint, RsaPrivateKey};
use serde_json::Value;

const VECTOR: &str = include_str!("../../tests/fixtures/v1-vector.json");
const PUBLIC_SPKI: &str = include_str!("../../tests/fixtures/test-server-public.pem");
const PUBLIC_PKCS1: &str = include_str!("../../tests/fixtures/test-server-public-pkcs1.pem");

fn vector() -> Value {
    serde_json::from_str(VECTOR).unwrap()
}

fn field(name: &str) -> String {
    vector()[name].as_str().unwrap().to_owned()
}

fn number(name: &str) -> BigUint {
    BigUint::parse_bytes(field(name).as_bytes(), 16).unwrap()
}

fn private() -> RsaPrivateKey {
    RsaPrivateKey::from_components(
        number("n"),
        number("e"),
        number("d"),
        vec![number("p"), number("q")],
    )
    .unwrap()
}

fn session_key() -> [u8; KEY_BYTES] {
    STANDARD
        .decode(field("session_key_b64"))
        .unwrap()
        .try_into()
        .unwrap()
}

/// A session with the vector's own key, as V1 made it.
fn v1_session() -> Session {
    let server = public_key_from_pem(PUBLIC_SPKI).unwrap();
    Session::with_key(&session_key(), &server, &mut rand_core::OsRng).unwrap()
}

#[test]
fn both_pem_forms_of_the_servers_key_are_read_and_match() {
    let spki = public_key_from_pem(PUBLIC_SPKI).unwrap();
    let pkcs1 = public_key_from_pem(PUBLIC_PKCS1).unwrap();
    assert_eq!(spki, pkcs1);
    assert_eq!(spki, private().to_public_key());
    for bad in [
        "",
        "not a key",
        "-----BEGIN PUBLIC KEY-----\nAAAA\n-----END PUBLIC KEY-----",
    ] {
        assert_eq!(public_key_from_pem(bad), Err(Error::PublicKey), "{bad}");
    }
}

#[test]
fn v1s_encrypted_session_key_opens_with_oaep_sha256() {
    // V1: OAEP(mgf=MGF1(SHA256()), algorithm=SHA256(), label=None).
    let sealed = STANDARD.decode(field("encrypted_key_b64")).unwrap();
    let opened = private()
        .decrypt(Oaep::new::<sha2::Sha256>(), &sealed)
        .unwrap();
    assert_eq!(opened, session_key());
}

#[test]
fn the_same_key_and_nonce_give_v1s_bytes_exactly() {
    let session = v1_session();
    let nonce: [u8; NONCE_BYTES] = STANDARD
        .decode(field("nonce_b64"))
        .unwrap()
        .try_into()
        .unwrap();
    let cipher = session
        .encrypt_with_nonce(&nonce, field("plain").as_bytes())
        .unwrap();
    assert_eq!(cipher.nonce, field("nonce_b64"));
    assert_eq!(cipher.cipher_data, field("cipher_data_b64"));
    // pydantic's model_dump_json of WSCipherMessage: its fields in order,
    // no spaces.
    assert_eq!(
        Session::ws_envelope(cipher).unwrap(),
        format!(
            r#"{{"nonce":"{}","cipher_data":"{}","type":"encrypted"}}"#,
            field("nonce_b64"),
            field("cipher_data_b64")
        )
    );
}

#[test]
fn v1s_websocket_message_opens() {
    let raw = format!(
        r#"{{"nonce":"{}","cipher_data":"{}","type":"encrypted"}}"#,
        field("nonce_b64"),
        field("cipher_data_b64")
    );
    assert_eq!(
        v1_session().unwrap_ws_message(&raw).unwrap(),
        field("plain")
    );
    // V1 takes `type` as optional.
    let untyped = format!(
        r#"{{"nonce":"{}","cipher_data":"{}"}}"#,
        field("nonce_b64"),
        field("cipher_data_b64")
    );
    assert_eq!(
        v1_session().unwrap_ws_message(&untyped).unwrap(),
        field("plain")
    );
}

#[test]
fn a_round_trip_through_the_test_key() {
    let server = public_key_from_pem(PUBLIC_SPKI).unwrap();
    let session = Session::with_system_random(&server).unwrap();
    // The server opens the session key with its private key.
    let sealed = STANDARD.decode(session.encrypted_key_b64()).unwrap();
    let key = private()
        .decrypt(Oaep::new::<sha2::Sha256>(), &sealed)
        .unwrap();
    assert_eq!(key.len(), KEY_BYTES);
    let server_side = Session::with_key(
        &key.clone().try_into().unwrap(),
        &server,
        &mut rand_core::OsRng,
    )
    .unwrap();

    // A WebSocket message each way.
    let message = r#"{"type": "frame", "task": "action", "frame_b64": "AAAA"}"#;
    let wrapped = session
        .wrap_ws_message(message, &mut rand_core::OsRng)
        .unwrap();
    assert!(!wrapped.contains("frame_b64"), "no plaintext on the wire");
    assert_eq!(server_side.unwrap_ws_message(&wrapped).unwrap(), message);
    let reply = server_side
        .wrap_ws_message(r#"{"detected_classes": []}"#, &mut rand_core::OsRng)
        .unwrap();
    assert_eq!(
        session.unwrap_ws_message(&reply).unwrap(),
        r#"{"detected_classes": []}"#
    );

    // An HTTP request carries the encrypted key, as V1's.
    let body = session
        .wrap_http_request(b"{\"image_b64\":\"AAAA\"}", &mut rand_core::OsRng)
        .unwrap();
    let request: Value = serde_json::from_slice(&body).unwrap();
    let keys: Vec<&str> = request
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(keys.len(), 3);
    assert_eq!(request["encrypted_key"], session.encrypted_key_b64());
    let opened = server_side
        .decrypt(&Cipher {
            nonce: request["nonce"].as_str().unwrap().into(),
            cipher_data: request["cipher_data"].as_str().unwrap().into(),
        })
        .unwrap();
    assert_eq!(opened, b"{\"image_b64\":\"AAAA\"}");
    let response = serde_json::to_vec(
        &server_side
            .encrypt(b"{\"detections\":[]}", &mut rand_core::OsRng)
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        session.unwrap_http_response(&response).unwrap(),
        b"{\"detections\":[]}"
    );

    // The key exchange, in V1's shape.
    let exchange: Value = serde_json::from_str(&session.key_exchange()).unwrap();
    assert_eq!(exchange["type"], "key_exchange");
    assert_eq!(exchange["encrypted_key"], session.encrypted_key_b64());
    assert!(
        session
            .key_exchange()
            .starts_with(r#"{"type":"key_exchange","#)
    );
}

#[test]
fn each_message_gets_its_own_nonce_and_two_sessions_their_own_keys() {
    let server = public_key_from_pem(PUBLIC_SPKI).unwrap();
    let one = Session::with_system_random(&server).unwrap();
    let two = Session::with_system_random(&server).unwrap();
    let a = one.encrypt(b"same", &mut rand_core::OsRng).unwrap();
    let b = one.encrypt(b"same", &mut rand_core::OsRng).unwrap();
    assert_ne!(a.nonce, b.nonce);
    assert_ne!(a.cipher_data, b.cipher_data);
    assert_eq!(STANDARD.decode(&a.nonce).unwrap().len(), NONCE_BYTES);
    // GCM's tag follows the ciphertext.
    assert_eq!(STANDARD.decode(&a.cipher_data).unwrap().len(), 4 + 16);
    assert_ne!(one.encrypted_key_b64(), two.encrypted_key_b64());
    assert_eq!(two.decrypt(&a), Err(Error::Decrypt));
}

#[test]
fn anything_tampered_or_misshapen_is_refused_never_passed_through() {
    let session = v1_session();
    let good = |nonce: &str, data: &str| {
        format!(r#"{{"nonce":"{nonce}","cipher_data":"{data}","type":"encrypted"}}"#)
    };
    let nonce = field("nonce_b64");
    let data = field("cipher_data_b64");
    let mut flipped = STANDARD.decode(&data).unwrap();
    flipped[0] ^= 1;
    let flipped = STANDARD.encode(flipped);
    for (raw, why) in [
        (good(&nonce, &flipped), Error::Decrypt),
        (good("AAAA", &data), Error::Envelope),
        (good("not base64!", &data), Error::Envelope),
        (good(&nonce, "not base64!"), Error::Envelope),
        (
            format!(r#"{{"nonce":"{nonce}","cipher_data":"{data}","type":"plain"}}"#),
            Error::Envelope,
        ),
        (field("plain"), Error::Envelope),
        ("{}".to_owned(), Error::Envelope),
    ] {
        assert_eq!(session.unwrap_ws_message(&raw), Err(why), "{raw}");
    }
    assert_eq!(
        session.unwrap_http_response(field("plain").as_bytes()),
        Err(Error::Envelope)
    );
}

#[test]
fn no_randomness_no_session_and_no_message() {
    struct Broken;
    impl RngCore for Broken {
        fn next_u32(&mut self) -> u32 {
            0
        }
        fn next_u64(&mut self) -> u64 {
            0
        }
        fn fill_bytes(&mut self, _: &mut [u8]) {}
        fn try_fill_bytes(&mut self, _: &mut [u8]) -> Result<(), rand_core::Error> {
            Err(rand_core::Error::new("no randomness"))
        }
    }
    impl CryptoRng for Broken {}
    let server = public_key_from_pem(PUBLIC_SPKI).unwrap();
    assert_eq!(
        Session::new(&server, &mut Broken).unwrap_err(),
        Error::Random
    );
    assert_eq!(
        v1_session().wrap_ws_message("x", &mut Broken).unwrap_err(),
        Error::Random
    );
}

#[test]
fn the_key_exchange_answer_is_read_as_v1_reads_it() {
    assert!(key_exchange_ok(r#"{"status":"key_exchange_ok"}"#));
    assert!(key_exchange_ok(r#"{"status": "key_exchange_ok", "x": 1}"#));
    for reply in [
        r#"{"status":"ok"}"#,
        r#"{"status":"key_exchange_failed"}"#,
        "{}",
        "key_exchange_ok",
        "",
    ] {
        assert!(!key_exchange_ok(reply), "{reply}");
    }
}
