//! Candidate-only interoperability through actual Unix HTTP operator endpoints.
#[path = "release_pair/fixture.rs"]
mod fixture;

#[tokio::test]
async fn current_cli_candidate_smoke() {
    fixture::current().await;
}

#[tokio::test]
async fn http_complete_reply_is_framed() {
    fixture::http_control(false).await;
}

#[tokio::test]
async fn http_dribbling_reply_cannot_renew_deadline() {
    fixture::http_control(true).await;
}

#[tokio::test]
#[ignore = "requires pinned actual releases in an isolated native Linux ARM64 container"]
async fn release_6408_surround_candidates_cross_6498_and_return() {
    fixture::released(
        6408,
        6498,
        "release_6408_surround_candidates_cross_6498_and_return",
    )
    .await;
}

#[tokio::test]
#[ignore = "requires pinned actual releases in an isolated native Linux ARM64 container"]
async fn release_6498_surround_candidates_cross_6408_and_return() {
    fixture::released(
        6498,
        6408,
        "release_6498_surround_candidates_cross_6408_and_return",
    )
    .await;
}
