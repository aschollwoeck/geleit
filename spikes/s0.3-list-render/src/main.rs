//! S0.3 throwaway spike — render a virtualized message list of ~50,000 rows in Slint and
//! scroll through it, to measure FPS and memory.
//!
//! `ROWS` env var sets the row count (default 50000). Run under
//! `SLINT_DEBUG_PERFORMANCE=refresh_full_speed,console` for FPS and `/usr/bin/time -v` for RSS.
//! Auto-exits after ~6s. THROWAWAY — not held to project guidelines.

use std::rc::Rc;
use std::time::Duration;

use slint::{ComponentHandle, Timer, TimerMode, VecModel};

slint::slint! {
    import { ListView } from "std-widgets.slint";

    struct Row {
        sender: string,
        subject: string,
        snippet: string,
        date: string,
        unread: bool,
        attachment: bool,
    }

    export component Main inherits Window {
        in property <[Row]> rows;
        in property <length> scroll-y;
        preferred-width: 900px;
        preferred-height: 700px;
        title: "S0.3 list spike";

        list := ListView {
            viewport-y: -root.scroll-y;
            for row in root.rows: Rectangle {
                height: 64px;
                background: white;
                HorizontalLayout {
                    padding: 8px;
                    spacing: 8px;
                    Rectangle {
                        width: 12px;
                        Rectangle {
                            width: 10px;
                            height: 10px;
                            border-radius: 5px;
                            background: row.unread ? #2d6cdf : transparent;
                        }
                    }
                    VerticalLayout {
                        spacing: 2px;
                        Text {
                            text: row.sender;
                            font-weight: row.unread ? 700 : 400;
                        }
                        Text { text: row.subject; }
                        Text {
                            text: row.snippet;
                            color: #777777;
                        }
                    }
                    VerticalLayout {
                        alignment: start;
                        Text {
                            text: row.date;
                            color: #999999;
                        }
                        Text { text: row.attachment ? "[paperclip]" : ""; }
                    }
                }
            }
        }
    }
}

fn main() -> Result<(), slint::PlatformError> {
    let n: usize = std::env::var("ROWS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(50_000);
    eprintln!("[spike] building model with {n} rows");

    let rows: Vec<Row> = (0..n)
        .map(|i| Row {
            sender: format!("Sender {i}").into(),
            subject: format!("Subject line number {i} about something or other").into(),
            snippet: format!("This is a preview snippet of message {i} with a bit of text…").into(),
            date: format!("Jun {}", (i % 28) + 1).into(),
            unread: i % 3 == 0,
            attachment: i % 5 == 0,
        })
        .collect();

    let main = Main::new()?;
    main.set_rows(Rc::new(VecModel::from(rows)).into());

    // Continuous scroll that traverses the WHOLE list within the run (so deep rows are actually
    // rendered, not just the first screenful). Logs progression so the evidence proves traversal.
    let row_h = 64.0_f32;
    let total = (n as f32) * row_h;
    let span = (total - 700.0).max(0.0);
    let step = (span / 300.0).max(20.0); // ~300 ticks (~5s) to cover the full span
    let weak = main.as_weak();
    let scroll_timer = Timer::default();
    let mut y = 0.0_f32;
    let mut ticks = 0u32;
    let mut deepest_row = 0u64;
    scroll_timer.start(TimerMode::Repeated, Duration::from_millis(16), move || {
        if let Some(m) = weak.upgrade() {
            y += step;
            if y > span {
                y = 0.0;
            }
            let top_row = (y / row_h) as u64;
            deepest_row = deepest_row.max(top_row);
            m.set_scroll_y(y);
            ticks += 1;
            if ticks % 60 == 0 {
                eprintln!(
                    "[spike] scroll y={y:.0}px topRow={top_row} deepestRow={deepest_row} span={span:.0}"
                );
            }
        }
    });

    // Auto-exit so the run is scriptable.
    let quit_timer = Timer::default();
    quit_timer.start(TimerMode::SingleShot, Duration::from_secs(6), || {
        let _ = slint::quit_event_loop();
    });

    main.run()?;
    Ok(())
}
