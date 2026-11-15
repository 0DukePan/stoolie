//! The receipt: one self-contained HTML file, no scripts, no fetched fonts.
//!
//! It is designed to be screenshotted and printed: paper background, tabular
//! numerals, one accent colour, and a footer that names every source so a
//! reader can check the claim instead of trusting it.

use crate::report::Snapshot;
use crate::util::{day, duration, escape_html, month, plural_days, stamp, time};

const PAPER: &str = "#faf8f3";
const INK: &str = "#15120c";
const MUTED: &str = "#6b6459";
const RULE: &str = "#d8d1c2";
const RED: &str = "#b3261e";

pub fn render(snapshot: &Snapshot) -> String {
    let microphone: Vec<&crate::model::Session> = snapshot.sessions_for("microphone");
    let camera: Vec<&crate::model::Session> = snapshot.sessions_for("webcam");
    let total_microphone_ms: i64 = microphone.iter().filter_map(|s| s.duration()).sum();
    let latest = microphone.first().copied();

    let mut rows = String::new();
    for session in &snapshot.sessions {
        let night = crate::util::local(session.start).format("%H").to_string();
        let night = night.parse::<u32>().map(|hour| hour < 6).unwrap_or(false);
        let chip = if night {
            r#"<span class="chip">NIGHT</span> "#
        } else {
            ""
        };
        let window = match session.stop {
            Some(stop) => format!("{} → {}", time(session.start), time(stop)),
            None => format!("{} → in use", time(session.start)),
        };
        let held = session
            .duration()
            .map(duration)
            .unwrap_or_else(|| "open".to_string());
        let context = if session.context.len() > 1 {
            format!("{} apps had records that second", session.context.len())
        } else {
            "—".to_string()
        };
        rows.push_str(&format!(
            "<tr><td class=\"mono\">{}</td><td class=\"mono\">{}</td><td>{}<span class=\"app\">{}</span></td>\
             <td class=\"mono num\">{}</td><td class=\"muted small\">{}</td></tr>",
            escape_html(&day(session.start)),
            escape_html(&window),
            chip,
            escape_html(&session.app),
            escape_html(&held),
            escape_html(&context),
        ));
    }

    let mut ledger = String::new();
    for event in snapshot.events.iter().rev().take(5) {
        ledger.push_str(&format!(
            "<tr><td class=\"mono\">{}</td><td class=\"mono\">{}</td><td>{}</td>\
             <td class=\"mono small\">{} → {}</td><td class=\"muted small\">{}</td></tr>",
            escape_html(&stamp(event.ts)),
            escape_html(&event.capability),
            escape_html(
                &event
                    .app
                    .as_deref()
                    .map(crate::util::packaged_short_name)
                    .unwrap_or_else(|| "all apps".to_string())
            ),
            escape_html(event.old.as_deref().unwrap_or("unset")),
            escape_html(event.new.as_deref().unwrap_or("unset")),
            escape_html(event.caller.as_deref().unwrap_or("system")),
        ));
    }
    if ledger.is_empty() {
        ledger.push_str(
            "<tr><td colspan=\"5\" class=\"muted small\">no sensor-setting changes in the log</td></tr>",
        );
    }

    let camera_state = allowed_word(snapshot.state("webcam"));
    let camera_line = if camera.is_empty() {
        format!(
            "<span class=\"flag\">Camera access is {}</span> — and nothing used it in {}.",
            escape_html(camera_state),
            escape_html(&plural_days(snapshot.days))
        )
    } else {
        format!(
            "{} camera sessions in {}.",
            camera.len(),
            escape_html(&plural_days(snapshot.days))
        )
    };

    let mut apps: Vec<&str> = microphone
        .iter()
        .map(|session| session.app.as_str())
        .collect();
    apps.sort_unstable();
    apps.dedup();
    let hero = if let Some(session) = latest {
        let who = if apps.len() == 1 {
            "One app held".to_string()
        } else {
            format!("{} apps held", apps.len())
        };
        format!(
            "{} your microphone {} in {} — <b>at {}</b>, for {}. \
             Windows kept that number for exactly one use: the next time the same app opened the mic, \
             it was gone. This receipt is the copy that survives.",
            who,
            if microphone.len() == 1 {
                "once".to_string()
            } else {
                format!("{} times", microphone.len())
            },
            escape_html(&plural_days(snapshot.days)),
            escape_html(&time(session.start)),
            escape_html(&duration(session.duration().unwrap_or(0))),
        )
    } else {
        format!(
            "No microphone session was recorded in the last {}. \
             That is a fact about this record, not a promise about your machine.",
            escape_html(&plural_days(snapshot.days))
        )
    };

    let strip = strip(snapshot);
    let microphone_status = status(snapshot, "microphone");
    let camera_status = status(snapshot, "webcam");
    let ledger_month = snapshot
        .events
        .first()
        .map(|event| month(event.ts))
        .unwrap_or_else(|| "the log".to_string());

    format!(
        r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8">
<meta name="generator" content="stoolie {version}">
<title>stoolie — sensor receipt</title>
<style>
  :root {{ color-scheme: light; }}
  * {{ box-sizing: border-box; }}
  body {{ margin:0; background:{paper}; color:{ink};
        font-family:"Cascadia Mono","Consolas",ui-monospace,monospace; }}
  .sheet {{ width:1240px; margin:0 auto; padding:44px 56px 40px; }}
  .mono {{ font-variant-numeric: tabular-nums; }}
  .muted {{ color:{muted}; }}
  .small {{ font-size:12px; }}
  .num {{ text-align:right; }}
  header {{ display:flex; justify-content:space-between; align-items:flex-start;
            border-bottom:2px solid {ink}; padding-bottom:14px; }}
  .brand {{ font-size:15px; letter-spacing:.34em; font-weight:700; }}
  .kicker {{ font-size:11px; letter-spacing:.22em; color:{muted}; margin-top:6px; }}
  .meta {{ text-align:right; font-size:11px; line-height:1.7; color:{muted}; }}
  .hero {{ display:flex; gap:28px; align-items:flex-end; margin:26px 0 6px; }}
  .big {{ font-size:86px; line-height:.9; letter-spacing:-.04em; font-weight:700; white-space:nowrap; }}
  .big small {{ font-size:26px; font-weight:400; letter-spacing:0; margin-left:6px; }}
  .hero-copy {{ font-size:14px; line-height:1.5; max-width:600px; padding-bottom:8px; }}
  .hero-copy b {{ color:{red}; }}
  .grid {{ display:grid; grid-template-columns:1fr 1fr 1fr; gap:0;
           border-top:1px solid {rule}; border-bottom:1px solid {rule}; margin:22px 0 4px; }}
  .cell {{ padding:14px 18px 14px 0; }}
  .cell + .cell {{ border-left:1px solid {rule}; padding-left:18px; }}
  .cell .k {{ font-size:10px; letter-spacing:.16em; color:{muted}; }}
  .cell .v {{ font-size:22px; margin-top:6px; }}
  h2 {{ font-size:10px; letter-spacing:.22em; color:{muted}; font-weight:600; margin:26px 0 8px; }}
  table {{ width:100%; border-collapse:collapse; }}
  td {{ padding:8px 12px 8px 0; border-bottom:1px solid {rule}; font-size:13px; vertical-align:top; }}
  tr:last-child td {{ border-bottom:none; }}
  .app {{ font-weight:700; }}
  .strip {{ display:flex; align-items:flex-end; gap:9px; height:120px; padding-top:10px; }}
  .strip-cell {{ display:flex; flex-direction:column; align-items:center;
                 justify-content:flex-end; width:52px; height:100%; }}
  .strip-bar {{ width:100%; background:#cfc6b3; border-radius:2px 2px 0 0; }}
  .strip-cell.hot .strip-bar {{ background:{red}; }}
  .strip-day {{ font-size:10px; color:{muted}; margin-top:6px; }}
  .strip-cell.hot .strip-day {{ color:{red}; font-weight:700; }}
  .now {{ display:flex; gap:34px; font-size:13px; margin-top:6px; }}
  .dot {{ display:inline-block; width:8px; height:8px; border-radius:50%;
          margin-right:7px; vertical-align:1px; background:#b9b1a1; }}
  .dot.live {{ background:{red}; }}
  .dot.idle {{ background:#b9b1a1; }}
  .foot {{ margin-top:26px; border-top:2px solid {ink}; padding-top:12px;
           display:flex; justify-content:space-between; font-size:11px; color:{muted}; }}
  .flag {{ color:{red}; font-weight:700; }}
  .chip {{ background:{red}; color:{paper}; font-size:9px; letter-spacing:.14em;
           padding:2px 6px 1px; border-radius:2px; margin-right:8px; vertical-align:2px; }}
</style></head>
<body><div class="sheet">

<header>
  <div>
    <div class="brand">STOOLIE</div>
    <div class="kicker">SENSOR RECEIPT · v{version}</div>
  </div>
  <div class="meta">
    {os} · {days} on record<br>
    generated {generated} · this machine · nothing sent<br>
    read locally · no administrator rights
  </div>
</header>

<div class="hero">
  <div class="big">{total}<small>s of microphone</small></div>
  <div class="hero-copy">{hero}</div>
</div>

<div class="grid">
  <div class="cell"><div class="k">APPS ON RECORD</div><div class="v">{apps}</div></div>
  <div class="cell"><div class="k">ACTIVITY RECORDS</div><div class="v">{activities}</div></div>
  <div class="cell"><div class="k">SETTING CHANGES</div><div class="v">{changes}</div></div>
</div>

<h2>WHEN THE MACHINE WAS IN USE · BAR HEIGHT = ACTIVITY RECORDS</h2>
<div class="strip">{strip}</div>

<h2>MICROPHONE</h2>
<table>
  <tr><td class="muted small">DAY</td><td class="muted small">WINDOW</td>
      <td class="muted small">APP</td><td class="muted small num">HELD FOR</td>
      <td class="muted small">CONTEXT</td></tr>
  {rows}
</table>
<div class="small muted" style="margin-top:8px">
  Access metadata, not content: this says the device was open, never that anything was recorded.
</div>

<h2>CAMERA</h2>
<div class="small">{camera_line}</div>

<h2>WHO CHANGED YOUR PRIVACY SETTINGS · {changes} CHANGES SINCE {ledger_month}</h2>
<table>{ledger}</table>

<h2>RIGHT NOW</h2>
<div class="now">
  <div><span class="dot {mic_dot}"></span>microphone: {mic_status} · setting {mic_setting}</div>
  <div><span class="dot {cam_dot}"></span>camera: {cam_status} · setting {cam_setting}</div>
</div>

<div class="foot">
  <div>read from ConsentStore · Privacy-Auditing/Operational · ActivitiesCache.db</div>
  <div>stoolie receipt · 0 bytes sent · {sessions} sessions archived</div>
</div>

</div></body></html>"#,
        version = escape_html(&snapshot.version),
        paper = PAPER,
        ink = INK,
        muted = MUTED,
        rule = RULE,
        red = RED,
        os = escape_html(&snapshot.os),
        days = escape_html(&plural_days(snapshot.days)),
        generated = escape_html(&stamp(snapshot.generated)),
        total = escape_html(&format!("{:.1}", total_microphone_ms as f64 / 1_000.0)),
        hero = hero,
        apps = snapshot.timeline.apps,
        activities = crate::report::thousands(snapshot.timeline.activities),
        changes = snapshot.events.len(),
        strip = strip,
        rows = rows,
        camera_line = camera_line,
        ledger = ledger,
        ledger_month = escape_html(&ledger_month),
        mic_dot = if snapshot.live("microphone") {
            "live"
        } else {
            "idle"
        },
        cam_dot = if snapshot.live("webcam") {
            "live"
        } else {
            "idle"
        },
        mic_status = microphone_status,
        cam_status = camera_status,
        mic_setting = escape_html(snapshot.state("microphone").unwrap_or("unknown")),
        cam_setting = escape_html(snapshot.state("webcam").unwrap_or("unknown")),
        sessions = crate::report::thousands(snapshot.archive_sessions.max(0) as usize),
    )
}

fn allowed_word(state: Option<&str>) -> &'static str {
    match state.map(|value| value.to_ascii_lowercase()).as_deref() {
        Some("allow") => "allowed for every app",
        Some("deny") => "blocked",
        Some("prompt") => "ask every time",
        _ => "unknown",
    }
}

fn status(snapshot: &Snapshot, capability: &str) -> String {
    if snapshot.live(capability) {
        "in use".to_string()
    } else {
        "idle".to_string()
    }
}

/// Thirteen bars, one per local day, with the day that had a session marked.
fn strip(snapshot: &Snapshot) -> String {
    let per_day = &snapshot.timeline.per_day;
    let mut cells = String::new();
    let peak = per_day.iter().map(|(_, count)| *count).max().unwrap_or(1);
    for (date, count) in per_day.iter().rev().take(14).rev() {
        let height = ((*count as f64 / peak as f64) * 96.0).max(6.0) as u32;
        let hot = snapshot.sessions.iter().any(|session| {
            crate::util::local(session.start)
                .format("%Y-%m-%d")
                .to_string()
                == *date
        });
        let label = date.rsplit('-').next().unwrap_or(date);
        cells.push_str(&format!(
            "<div class=\"strip-cell{}\"><div class=\"strip-bar\" style=\"height:{}px\"></div>\
             <div class=\"strip-day\">{}</div></div>",
            if hot { " hot" } else { "" },
            height,
            escape_html(label)
        ));
    }
    if cells.is_empty() {
        cells.push_str("<div class=\"muted small\">no activity timeline on this machine</div>");
    }
    cells
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ConsentEvent, Session, Source};
    use crate::report::TimelineSummary;

    fn snapshot() -> Snapshot {
        Snapshot {
            version: "0.1.0".into(),
            generated: 1_763_000_000_000,
            days: 13,
            os: "Windows 10 Pro 22H2 (build 19045)".into(),
            sessions: vec![Session {
                capability: "microphone".into(),
                app: "<script>alert(1)</script>.exe".into(),
                app_path: None,
                start: 1_763_000_000_000,
                stop: Some(1_763_000_014_100),
                source: Source::Registry,
                context: vec!["Brave".into(), "Discord".into()],
            }],
            states: Vec::new(),
            events: vec![ConsentEvent {
                ts: 1_763_000_000_000,
                event_id: 1004,
                capability: "microphone".into(),
                app: Some("Microsoft.Windows.Photos_8wekyb3d8bbwe".into()),
                old: Some("Prompt".into()),
                new: Some("Allow".into()),
                caller: Some("SystemSettings.exe".into()),
            }],
            timeline: TimelineSummary {
                available: true,
                activities: 1_246,
                apps: 61,
                first: Some(1_762_000_000),
                last: Some(1_763_000_000),
                per_day: vec![("2026-11-12".into(), 235), ("2026-11-13".into(), 75)],
            },
            archive_sessions: 1,
            archive_events: 1,
            archive_path: "C:/x/stoolie.db".into(),
            capabilities_seen: 29,
            event_log_available: true,
            warnings: Vec::new(),
        }
    }

    #[test]
    fn receipt_escapes_hostile_app_names() {
        let html = render(&snapshot());
        assert!(html.contains("&lt;script&gt;alert(1)&lt;/script&gt;.exe"));
        assert!(!html.contains("<script>alert(1)</script>"));
    }

    #[test]
    fn receipt_contains_the_headline_and_sources() {
        let html = render(&snapshot());
        assert!(html.contains("14.1"));
        assert!(html.contains("ConsentStore"));
        assert!(html.contains("0 bytes sent"));
        assert!(html.contains("Microsoft.Windows.Photos"));
    }
}
