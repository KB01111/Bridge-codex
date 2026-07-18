use pretty_assertions::assert_eq;

use super::A2aPersistence;
use crate::a2a_protocol::A2aServerSettings;
use crate::a2a_protocol::A2aTask;
use crate::a2a_protocol::TaskState;
use crate::a2a_protocol::TaskStatus;
use crate::a2a_task_store::MAX_RETAINED_TASKS;

#[tokio::test]
async fn settings_are_persisted_without_credentials() {
    let temp = tempfile::tempdir().expect("temporary app data");
    let persistence = A2aPersistence::new(temp.path().to_path_buf());
    let settings = A2aServerSettings {
        enabled: true,
        port: 18_120,
    };

    persistence
        .save_settings(settings)
        .await
        .expect("save settings");
    let serialized =
        std::fs::read_to_string(temp.path().join("a2a/settings-v1.json")).expect("settings JSON");

    assert_eq!(
        persistence.load_settings().expect("load settings"),
        settings
    );
    assert!(!serialized.to_ascii_lowercase().contains("token"));
    assert!(!serialized.to_ascii_lowercase().contains("secret"));
}

#[tokio::test]
async fn task_and_context_mapping_survives_restart_and_stays_bounded() {
    let temp = tempfile::tempdir().expect("temporary app data");
    let persistence = A2aPersistence::new(temp.path().to_path_buf());
    let tasks = (0..MAX_RETAINED_TASKS + 2)
        .map(test_task)
        .collect::<Vec<_>>();

    persistence
        .save_tasks(tasks.clone())
        .await
        .expect("save tasks");
    let restored = persistence.load_tasks().expect("load tasks");

    assert_eq!(restored.list_oldest_first(), tasks[2..].to_vec());
}

#[test]
fn corrupt_primary_state_recovers_from_the_last_backup() {
    let temp = tempfile::tempdir().expect("temporary app data");
    let persistence = A2aPersistence::new(temp.path().to_path_buf());
    let directory = temp.path().join("a2a");
    std::fs::create_dir_all(&directory).expect("A2A directory");
    std::fs::write(directory.join("settings-v1.json"), b"not-json").expect("corrupt primary");
    std::fs::write(
        directory.join("settings-v1.json.bak"),
        br#"{"version":1,"settings":{"enabled":true,"port":18120}}"#,
    )
    .expect("settings backup");

    assert_eq!(
        persistence.load_settings().expect("recover settings"),
        A2aServerSettings {
            enabled: true,
            port: 18_120,
        }
    );
}

fn test_task(index: usize) -> A2aTask {
    A2aTask {
        id: format!("task-{index:03}"),
        context_id: format!("context-{index:03}"),
        status: TaskStatus {
            state: TaskState::Completed,
            timestamp: format!("timestamp-{index:03}"),
            message: None,
        },
        artifacts: Vec::new(),
    }
}
