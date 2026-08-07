//! Repro harness for the quality-switch crash.
//!
//! Opens a recording at Original (`stream.mpg`, Transport::Direct), plays it,
//! drops the player, then reopens the same recording as an HLS transcode
//! (`hls/master.m3u8`, Transport::Hls) at the position playback had reached —
//! which is exactly what `quality_menu` -> `open_recording` -> `spawn_open`
//! does. Loops so the fault has many chances to show.
//!
//! Not shipped: `examples/` is only built with `cargo build --example`.

#[path = "../src/player/mod.rs"]
mod player;

/// The player asks the application what to identify itself as. This harness is
/// not the application, so it answers for itself — and says so plainly, because
/// a request from a test should never be mistaken in a server's logs for one
/// from the real client.
mod settings {
    pub fn user_agent() -> String {
        "RustDVR-quality-switch-harness".to_string()
    }
}

use std::time::{Duration, Instant};

use player::{Player, Transport};

fn watch(label: &str, uri: &str, resume_at: f64, secs: f64) -> f64 {
    eprintln!("\n=== {label} :: {uri}");
    let opened = Instant::now();
    let p = match Player::open(uri, Transport::of(uri), || {}) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("=== {label}: open FAILED: {e:#}");
            return 0.0;
        }
    };
    eprintln!(
        "=== {label}: opened in {:.2}s, seekable {}, range {:?}",
        opened.elapsed().as_secs_f64(),
        p.is_seekable(),
        p.seek_range()
    );

    // Same shape as Msg::PlayerOpened.
    if resume_at > 5.0 {
        let origin = p.seek_range().map(|(s, _)| s).unwrap_or(0.0);
        eprintln!("=== {label}: resume {resume_at:.1}s from origin {origin:.1}s");
        p.seek_to(origin + resume_at);
    }

    let until = Instant::now() + Duration::from_secs_f64(secs);
    while Instant::now() < until {
        std::thread::sleep(Duration::from_millis(50));
        // The UI thread reads the published frame every repaint. Do the same so
        // the frame mutex and buffer recycling see the same traffic.
        let slot = p.frame();
        let _ = (slot.width, slot.height, slot.pixels.len(), slot.generation);
        drop(slot);
        if let Some(e) = p.error() {
            eprintln!("=== {label}: player error: {e}");
            break;
        }
    }

    let origin = p.seek_range().map(|(s, _)| s).unwrap_or(0.0);
    let elapsed = p.position().map(|pos| (pos - origin).max(0.0)).unwrap_or(0.0);
    eprintln!(
        "=== {label}: decoded {} dropped {} elapsed {:.1}s",
        p.decoded(),
        p.dropped(),
        elapsed
    );
    let dropping = Instant::now();
    drop(p);
    eprintln!("=== {label}: dropped in {:.2}s", dropping.elapsed().as_secs_f64());
    elapsed
}

fn main() {
    let server = std::env::var("RD_SERVER").unwrap_or_else(|_| "http://127.0.0.1:8089".into());
    let id = std::env::var("RD_FILE").unwrap_or_else(|_| "1".into());
    let cycles: usize = std::env::var("RD_CYCLES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20);
    let hold: f64 = std::env::var("RD_HOLD")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8.0);
    let heights: Vec<u32> = std::env::var("RD_HEIGHTS")
        .unwrap_or_else(|_| "540,720,360,1080".into())
        .split(',')
        .filter_map(|v| v.trim().parse().ok())
        .collect();

    let original = format!("{server}/dvr/files/{id}/stream.mpg");

    let mut position = 0.0f64;
    for cycle in 0..cycles {
        let h = heights[cycle % heights.len()];
        let kbps = match h {
            1080 => 8000,
            720 => 4000,
            540 => 2500,
            _ => 1200,
        };
        let transcode = format!(
            "{server}/dvr/files/{id}/hls/master.m3u8?vcodec=h264&acodec=copy&resolution={h}&bitrate={kbps}"
        );

        eprintln!("\n########## cycle {cycle} (Original -> {h}p) ##########");
        if std::env::var("RD_HLS_ONLY").is_err() {
            position = watch(&format!("c{cycle} original"), &original, position, hold);
        }
        position = watch(&format!("c{cycle} {h}p"), &transcode, position, hold);
    }
    eprintln!("\n########## survived {cycles} switch cycles ##########");
}
