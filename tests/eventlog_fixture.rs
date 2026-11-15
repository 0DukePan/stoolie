//! Parse the captured privacy-auditing log exactly as the binary does.
//!
//! The fixture in `tests/fixtures/privacy-auditing-sample.xml` is a synthetic
//! document shaped like `wevtutil /f:xml` output: seven records covering a
//! consent change, a migration, a "no app" record, an escaped app name and a
//! capability the product does not report on.

use stoolie::eventlog::{self, parse};
use stoolie::SENSOR_CAPABILITIES;

fn fixture() -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("privacy-auditing-sample.xml");
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("reading {path:?}: {error}"))
}

#[test]
fn parses_every_record_in_the_fixture() {
    let events = parse(&fixture());
    assert_eq!(events.len(), 7, "every <Event> must yield one record");
    assert!(events.iter().all(|event| event.ts > 0));
    assert!(
        events.iter().all(|event| !event.capability.is_empty()),
        "a record with no SettingName is labelled unknown, never blank"
    );
}

#[test]
fn decodes_the_fields_a_receipt_shows() {
    let events = parse(&fixture());
    let photos = events
        .iter()
        .find(|event| {
            event
                .app
                .as_deref()
                .is_some_and(|app| app.starts_with("Microsoft.Windows.Photos"))
        })
        .expect("the Photos record must be present");
    assert_eq!(photos.capability, "microphone");
    assert_eq!(photos.old.as_deref(), Some("Prompt"));
    assert_eq!(photos.new.as_deref(), Some("Allow"));
    assert_eq!(photos.caller.as_deref(), Some("SystemSettings.exe"));
    assert_eq!(photos.event_id, 1004);
}

#[test]
fn records_without_an_app_are_not_turned_into_an_empty_name() {
    let events = parse(&fixture());
    let global = events
        .iter()
        .find(|event| event.capability == "location" && event.event_id == 1006)
        .expect("the machine-wide record must be present");
    assert_eq!(global.app, None);
}

#[test]
fn escapes_are_resolved() {
    let events = parse(&fixture());
    let weird = events
        .iter()
        .find(|event| event.capability.contains('&'))
        .expect("the escaped capability must survive");
    assert_eq!(weird.capability, "microphone & camera");
    assert_eq!(
        weird.app.as_deref(),
        Some("Weird_<App>_8wekyb3d8bbwe"),
        "an app name containing < and > must not be mangled"
    );
}

#[test]
fn only_sensor_capabilities_reach_the_ledger() {
    let events = parse(&fixture());
    let sensors = events
        .iter()
        .filter(|event| SENSOR_CAPABILITIES.contains(&event.capability.as_str()))
        .count();
    // Three microphone records, one webcam, one location. The `blocksRecording`
    // record and the escaped `microphone & camera` string are not capabilities
    // this product reports on, and a prefix match would have kept them.
    assert_eq!(sensors, 5);
}

#[test]
fn a_real_machine_reads_without_panicking() {
    // On CI this channel is empty; on a developer machine it has months of
    // history. Both are valid, and neither may abort the run.
    let reading = eventlog::read(&eventlog::ReadOptions {
        limit: 50,
        ..eventlog::ReadOptions::default()
    });
    assert!(reading.events.len() <= 50);
    if let Some(warning) = reading.warning {
        assert!(!warning.is_empty());
    }
}
