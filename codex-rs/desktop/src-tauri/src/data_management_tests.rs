use pretty_assertions::assert_eq;

use super::APP_IDENTIFIER;
use super::remove_bridge_data_root;

#[test]
fn data_removal_is_restricted_to_the_application_identifier() {
    let temp = tempfile::tempdir().expect("temporary parent");
    let unrelated = temp.path().join("unrelated");
    std::fs::create_dir(&unrelated).expect("unrelated directory");

    let error = remove_bridge_data_root(&unrelated).expect_err("unrelated path must survive");

    assert!(
        error
            .to_string()
            .contains("unexpected application-data path")
    );
    assert!(unrelated.exists());
}

#[test]
fn data_removal_deletes_only_the_expected_absolute_root() {
    let temp = tempfile::tempdir().expect("temporary parent");
    let root = temp.path().join(APP_IDENTIFIER);
    std::fs::create_dir(&root).expect("application data directory");
    std::fs::write(root.join("state.json"), b"state").expect("application data fixture");
    let rollout_dir = root.join("codex-home").join("sessions");
    std::fs::create_dir_all(&rollout_dir).expect("embedded rollout directory");
    std::fs::write(rollout_dir.join("rollout.jsonl"), b"private history")
        .expect("embedded rollout fixture");

    remove_bridge_data_root(&root).expect("remove application data");

    assert_eq!(root.exists(), false);
}
