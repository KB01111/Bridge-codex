use std::fs::FileTimes;
use std::io::Write;
use std::sync::Arc;
use std::time::Duration;
use std::time::SystemTime;

use pretty_assertions::assert_eq;

use super::Diagnostics;
use super::MAX_LOG_AGE;
use super::MAX_LOG_FILES;
use super::RedactedLogWriter;
use super::managed_log_paths;
use super::sanitize_log_record;

#[test]
fn sensitive_records_are_replaced_instead_of_partially_masked() {
    assert_eq!(
        sanitize_log_record(b"authorization: Bearer secret-value"),
        b"[redacted sensitive diagnostic record]\n"
    );
}

#[test]
fn rotation_never_creates_more_than_five_managed_files() {
    let directory = tempfile::tempdir().expect("temp directory");
    let writer = RedactedLogWriter::new(directory.path().to_path_buf()).expect("log writer");
    for _ in 0..(MAX_LOG_FILES + 3) {
        let current = directory.path().join("bridge.log");
        let file = std::fs::File::create(&current).expect("seed current log");
        file.set_len(super::MAX_LOG_FILE_BYTES)
            .expect("grow current log");
        writer.append(b"rotate\n").expect("rotate log");
    }
    let files = managed_log_paths(directory.path())
        .filter(|(_, path)| path.exists())
        .count();
    assert_eq!(files, MAX_LOG_FILES);
}

#[test]
fn writer_reports_full_input_even_when_record_is_truncated() {
    let directory = tempfile::tempdir().expect("temp directory");
    let writer = RedactedLogWriter::new(directory.path().to_path_buf()).expect("log writer");
    let mut record = tracing_subscriber::fmt::MakeWriter::make_writer(&writer);
    let input = vec![b'x'; super::MAX_LOG_RECORD_BYTES + 12];
    assert_eq!(record.write(&input).expect("buffer record"), input.len());
}

#[test]
fn support_export_prunes_logs_older_than_seven_days() {
    let app_data = tempfile::tempdir().expect("application data");
    let log_root = app_data.path().join("logs");
    let writer = RedactedLogWriter::new(log_root.clone()).expect("log writer");
    let expired = log_root.join("bridge.log");
    let file = std::fs::File::create(&expired).expect("expired log fixture");
    file.set_times(
        FileTimes::new().set_modified(SystemTime::now() - MAX_LOG_AGE - Duration::from_secs(1)),
    )
    .expect("age log fixture");
    let diagnostics = Diagnostics {
        app_data_dir: Arc::new(app_data.path().to_path_buf()),
        writer,
    };

    diagnostics
        .export_support_bundle()
        .expect("support bundle export");

    assert!(!expired.exists());
}

#[test]
fn append_replaces_an_expired_current_log_instead_of_refreshing_its_age() {
    let directory = tempfile::tempdir().expect("temp directory");
    let writer = RedactedLogWriter::new(directory.path().to_path_buf()).expect("log writer");
    let current = directory.path().join("bridge.log");
    writer.append(b"expired record\n").expect("expired fixture");
    let expired_start = SystemTime::now() - MAX_LOG_AGE - Duration::from_secs(1);
    writer.gate.lock().expect("log state").segment_started_at = expired_start;
    super::persist_segment_started_at(directory.path(), expired_start)
        .expect("persist expired segment age");
    std::fs::File::options()
        .write(true)
        .open(&current)
        .expect("recently appended log")
        .set_times(FileTimes::new().set_modified(SystemTime::now()))
        .expect("simulate recent append");

    writer.append(b"current record\n").expect("append record");

    assert_eq!(
        std::fs::read(&current).expect("current log"),
        b"current record\n"
    );
    assert!(!directory.path().join("bridge.1.log").exists());
}
