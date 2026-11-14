//! stoolie — the receipts Windows throws away.
//!
//! Windows records, per application, the start and stop time of the most recent
//! microphone, camera and location use — and overwrites it on the next use.
//! `stoolie` reads that record, keeps every future session in its own
//! append-only archive, and writes a receipt you can keep or post.
//!
//! Everything here is read-only, local and unprivileged. The crate contains no
//! network code, and the release binary makes no outbound connections.

#![forbid(unsafe_code)]

pub mod eventlog;
pub mod model;
pub mod receipt;
pub mod registry;
pub mod report;
pub mod store;
pub mod timeline;
pub mod util;

/// Capabilities shown as "sensors" in the summary and the receipt.
pub const SENSOR_CAPABILITIES: [&str; 3] = ["microphone", "webcam", "location"];

/// Version reported by `stoolie --version` and printed on every receipt.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
