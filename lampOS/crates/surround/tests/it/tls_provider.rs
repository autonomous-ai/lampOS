#[test]
fn adding_surround_preserves_the_existing_voices_implicit_tls_provider() {
    // Voice's WebSocket client uses Rustls's implicit provider selection.
    // Cargo unifies dependency features across a workspace build. Pulling in
    // both ring and aws_lc_rs would panic on its first TLS connection even
    // though local ws:// tests and a workspace compile still succeed.
    let _builder = rustls::ClientConfig::builder();
}
