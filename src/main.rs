use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::collections::HashMap;
use std::io::IsTerminal;
use std::path::PathBuf;
use stoolie::report::{os_string, Snapshot};
use stoolie::{eventlog, receipt, registry, store, timeline, util, VERSION};

#[derive(Parser)]
#[command(
    name = "stoolie",
    version,
    about = "The receipts Windows throws away: what used your microphone, camera and location, and when",
    long_about = None,
    after_help = "Start with `stoolie`, then `stoolie receipt` for the HTML file."
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// Look back this many days
    #[arg(long, global = true, default_value_t = 13)]
    since: u32,

    /// Disable ANSI colour
    #[arg(long, global = true)]
    no_color: bool,

    /// Print this process's outbound connection count, then exit
    #[arg(long)]
    network: bool,
}

#[derive(Subcommand)]
enum Command {
    /// Write a self-contained HTML receipt
    Receipt {
        /// Where to write it (default: ./receipt.html)
        #[arg(long)]
        out: Option<PathBuf>,
        /// Open it in the default browser when it is written
        #[arg(long)]
        open: bool,
    },
    /// Machine-readable snapshot
    Json {
        /// One line instead of pretty-printed
        #[arg(long)]
        compact: bool,
    },
    /// Watch capability use in real time
    Watch {
        /// Seconds between registry reads
        #[arg(long, default_value_t = 1)]
        interval: u64,
    },
    /// Explain which data sources are available on this machine
    Doctor,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    if cli.network {
        return network_audit();
    }

    match cli.command {
        Some(Command::Receipt { out, open }) => write_receipt(cli.since, out, open),
        Some(Command::Json { compact }) => {
            let snapshot = Snapshot::build(cli.since)?;
            let text = if compact {
                snapshot.to_json_compact()
            } else {
                snapshot.to_json()
            };
            println!("{text}");
            Ok(())
        }
        Some(Command::Watch { interval }) => watch(interval.max(1)),
        Some(Command::Doctor) => doctor(),
        None => {
            let snapshot = Snapshot::build(cli.since)?;
            let colour = !cli.no_color && std::io::stdout().is_terminal();
            print!("{}", snapshot.summary(colour));
            Ok(())
        }
    }
}

fn write_receipt(days: u32, out: Option<PathBuf>, open: bool) -> Result<()> {
    let snapshot = Snapshot::build(days)?;
    let path = out.unwrap_or_else(|| PathBuf::from("receipt.html"));
    let html = receipt::render(&snapshot);
    std::fs::write(&path, html).with_context(|| format!("writing {}", path.display()))?;
    let absolute = std::path::absolute(&path).unwrap_or_else(|_| path.clone());
    println!("{} written", absolute.display());
    println!(
        "{} sessions archived · {} setting changes · 0 bytes sent",
        snapshot.archive_sessions,
        snapshot.events.len()
    );
    if open {
        #[cfg(windows)]
        {
            let _ = std::process::Command::new("cmd")
                .args(["/C", "start", ""])
                .arg(&absolute)
                .spawn();
        }
    }
    Ok(())
}

/// Poll the registry and print transitions. One line per event, so the output
/// can be piped, recorded or screenshotted without a TUI.
fn watch(interval: u64) -> Result<()> {
    println!(
        "watching capability use every {interval}s · Ctrl+C to stop · nothing leaves this machine"
    );
    let mut previous: HashMap<(String, String), (i64, Option<i64>)> = registry::read_sessions()
        .into_iter()
        .map(|session| {
            (
                (session.capability.clone(), session.app.clone()),
                (session.start, session.stop),
            )
        })
        .collect();

    loop {
        std::thread::sleep(std::time::Duration::from_secs(interval));
        let sessions = registry::read_sessions();
        let mut current: HashMap<(String, String), (i64, Option<i64>)> = HashMap::new();
        for session in &sessions {
            let key = (session.capability.clone(), session.app.clone());
            current.insert(key.clone(), (session.start, session.stop));
            match previous.get(&key) {
                None if session.live() => println!(
                    "⏺ {} opened — {} — {}",
                    session.capability,
                    session.app,
                    util::time(session.start)
                ),
                Some((start, Some(_))) if session.live() && *start != session.start => println!(
                    "⏺ {} opened — {} — {}",
                    session.capability,
                    session.app,
                    util::time(session.start)
                ),
                Some((_, None)) if session.stop.is_some() => {
                    let held = session
                        .duration()
                        .map(util::duration)
                        .unwrap_or_else(|| "?".to_string());
                    println!(
                        "⏹ {} closed — {} — {}   ({})",
                        session.capability,
                        session.app,
                        util::time(session.stop.unwrap_or(session.start)),
                        held
                    );
                }
                _ => {}
            }
        }
        for (key, (start, stop)) in &previous {
            if stop.is_none() && !current.contains_key(key) {
                println!(
                    "⏹ {} closed — {} — {} (no longer in the registry)",
                    key.0,
                    key.1,
                    util::time(*start)
                );
            }
        }
        previous = current;
    }
}

fn doctor() -> Result<()> {
    println!("stoolie {VERSION} · {}", os_string());
    let sessions = registry::read_sessions();
    println!(
        "consent store     {} capabilities · {} sessions with timestamps",
        registry::capability_count(),
        sessions.len()
    );
    let states = registry::read_states();
    for capability in stoolie::SENSOR_CAPABILITIES {
        let state = states
            .iter()
            .find(|state| state.name == capability)
            .map(|state| state.value.as_str())
            .unwrap_or("unknown");
        let count = sessions
            .iter()
            .filter(|session| session.capability == capability)
            .count();
        println!("  {capability:<15} setting {state:<7} · {count} app(s) with a timestamp");
    }
    let events = eventlog::read_events(400);
    if events.is_empty() {
        println!("privacy audit     unavailable (channel not readable, or nothing recorded)");
    } else {
        println!("privacy audit     {} events readable", events.len());
    }
    let timeline = timeline::load();
    if timeline.available() {
        println!(
            "timeline          {} activities · {} apps",
            timeline.activities.len(),
            timeline.apps()
        );
    } else {
        println!("timeline          unavailable on this machine");
    }
    match store::Store::open_default() {
        Ok(store) => println!(
            "archive           {} sessions · {} events · {}",
            store.session_count().unwrap_or(0),
            store.event_count().unwrap_or(0),
            store.path().display()
        ),
        Err(error) => println!("archive           unavailable: {error}"),
    }
    println!("network code      none — the binary contains no sockets");
    Ok(())
}

/// Count sockets owned by this process. There is no network code in stoolie, so
/// the honest answer is always zero — the point is that it is measured, not
/// asserted.
fn network_audit() -> Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let output = std::process::Command::new("netstat")
            .arg("-ano")
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .context("running netstat")?;
        let pid = std::process::id().to_string();
        let text = String::from_utf8_lossy(&output.stdout);
        let count = text
            .lines()
            .filter(|line| line.trim_end().ends_with(&pid))
            .filter(|line| line.contains("ESTABLISHED") || line.contains("SYN_SENT"))
            .count();
        println!("stoolie owns {count} outbound connections.");
        if count == 0 {
            println!("Nothing to believe: measured with netstat, not promised in a README.");
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        println!("stoolie owns 0 outbound connections (netstat audit is Windows-only).");
        Ok(())
    }
}
