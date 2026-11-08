//! Recording spike:
//! `cargo run --release -p framenab-capture --example record -- <out-dir> [seconds] [fps] [--cursor]
//!  [--region x,y,w,h] [--token-file <path>]`
//!
//! Records the screen into a project folder (`project.json` + QOI frames) and prints frame
//! statistics and the peak memory use. On Wayland the screen-sharing dialog appears the first
//! time; with `--token-file` the choice is remembered for the next run.

use std::time::{Duration, Instant};

use framenab_capture::{CaptureTarget, Crop, StreamOptions, record};
use framenab_core::recorder::ProjectRecorder;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let positional: Vec<&String> = {
        let mut skip = false;
        args.iter()
            .filter(|a| {
                if skip {
                    skip = false;
                    return false;
                }
                if a.as_str() == "--region" || a.as_str() == "--token-file" {
                    skip = true;
                    return false;
                }
                !a.starts_with("--")
            })
            .collect()
    };
    let out = positional.first().map_or("recording.framenab", |s| s.as_str()).to_owned();
    let seconds: u64 = positional.get(1).and_then(|s| s.parse().ok()).unwrap_or(5);
    let fps: u32 = positional.get(2).and_then(|s| s.parse().ok()).unwrap_or(15);
    let region = flag("--region").map(|r| {
        let v: Vec<i32> = r.split(',').map(|n| n.trim().parse().expect("--region x,y,w,h")).collect();
        (v[0], v[1], v[2] as u32, v[3] as u32)
    });
    let token_file = flag("--token-file");

    let backend = framenab_capture::backend();
    println!("platform: {}, {seconds} s at {fps} fps → {out}", backend.platform());
    let target = match region {
        Some((x, y, width, height)) => CaptureTarget::Region { x, y, width, height },
        None => CaptureTarget::AllMonitors,
    };
    let options = StreamOptions {
        cursor: args.iter().any(|a| a == "--cursor"),
        restore_token: token_file.as_ref().and_then(|f| std::fs::read_to_string(f).ok()),
    };

    let opened = Instant::now();
    let stream = backend.open_stream(&target, &options).expect("open stream");
    println!("stream open after {:?}", opened.elapsed());
    let origin = stream.origin();
    let crop = region.map(|(x, y, width, height)| Crop { x: x - origin.0, y: y - origin.1, width, height });

    let sink = ProjectRecorder::create(&out).expect("create project");
    let recording = record(stream, crop, fps, sink);
    if let (Some(file), Some(token)) = (&token_file, recording.restore_token()) {
        std::fs::write(file, token).expect("save restore token");
    }
    std::thread::sleep(Duration::from_secs(seconds));
    if let Some(err) = recording.error() {
        eprintln!("stream error: {err}");
    }
    let (sink, end, sampler, input) = recording.stop();
    let (project, stats) = sink.finish(end).expect("finish project");
    framenab_core::FrameStore::create(&out).and_then(|s| s.save_input(&input)).expect("save input");

    let expected = (end as f64 / 1000.0 * f64::from(fps)).round() as u64;
    let bytes: u64 = std::fs::read_dir(format!("{out}/frames"))
        .map(|d| d.filter_map(|e| e.ok()?.metadata().ok()).map(|m| m.len()).sum())
        .unwrap_or(0);
    println!("recorded {end} ms, {}×{} px", project.width, project.height);
    println!(
        "frames: {} written, {} dropped by the writer, {} mismatched, {} late ticks (max. {expected} at {fps} fps)",
        stats.frames, stats.dropped, stats.mismatched, sampler.late_ticks
    );
    println!("disk: {:.1} MB", bytes as f64 / 1e6);
    println!(
        "input: {} cursor samples, {} clicks, {} keys (first cursor sample: {:?})",
        input.cursor.len(),
        input.clicks.len(),
        input.keys.len(),
        input.cursor.first()
    );
    if let Ok(status) = std::fs::read_to_string("/proc/self/status")
        && let Some(line) = status.lines().find(|l| l.starts_with("VmHWM"))
    {
        println!("peak memory: {}", line.trim_start_matches("VmHWM:").trim());
    }
}
