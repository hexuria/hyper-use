//! Golden replay tests (issue #49, work item 2): the arena re-scores
//! recorded decisions deterministically — a checked-in divergence diary
//! pins agree / would-abstain / regress, and a fresh `--diary` run replays
//! to full agreement.

fn fixture(name: &str) -> String {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name)
        .to_str()
        .unwrap()
        .to_owned()
}

#[test]
fn checked_in_divergence_diary_reports_all_verdicts() {
    let dir = std::env::temp_dir().join(format!("aui-replay-fixture-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(
        fixture("diary-divergence.jsonl"),
        dir.join("divergence.jsonl"),
    )
    .unwrap();

    let out = aui_cli::execute(&[
        "replay".to_owned(),
        "--diary".to_owned(),
        dir.to_str().unwrap().to_owned(),
    ])
    .unwrap();

    assert!(out.contains("agree 1, would-abstain 1, regress 2"), "{out}");
    // Each divergence names the recorded verdict and the replayed one.
    assert!(
        out.contains("regress \"Click Go\": recorded CLICK:cancel"),
        "{out}"
    );
    assert!(
        out.contains("regress \"Click Go\": recorded abstain"),
        "{out}"
    );
    assert!(out.contains("would-abstain \"Click Go\""), "{out}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_fresh_instinct_diary_replays_to_full_agreement() {
    let fixture = fixture("agent-click-go.cdp.json");
    if !std::path::Path::new(&fixture).exists() {
        eprintln!("skip missing fixture {fixture}");
        return;
    }
    let dir = std::env::temp_dir().join(format!("aui-replay-e2e-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    aui_cli::execute(&[
        "run".to_owned(),
        "--goal".to_owned(),
        "Click Go".to_owned(),
        "--policy".to_owned(),
        "instinct".to_owned(),
        "--fixture".to_owned(),
        fixture,
        "--diary".to_owned(),
        dir.to_str().unwrap().to_owned(),
    ])
    .unwrap();

    let out = aui_cli::execute(&[
        "replay".to_owned(),
        "--diary".to_owned(),
        dir.to_str().unwrap().to_owned(),
    ])
    .unwrap();
    assert!(
        out.contains("2 decisions — agree 2, would-abstain 0, regress 0"),
        "{out}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
