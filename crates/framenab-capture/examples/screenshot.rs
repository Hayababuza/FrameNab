//! Capture spike: `cargo run -p framenab-capture --example screenshot [out.png] [count]`
//! Takes `count` screenshots of all monitors in one process (default 1) with the backend for
//! the current platform and saves the last one. Several shots in one process catch backends
//! that only work once (e.g. a dead D-Bus connection on the second portal call).

fn main() {
    let mut args = std::env::args().skip(1);
    let out = args.next().unwrap_or_else(|| "screenshot.png".into());
    let count: u32 = args.next().and_then(|c| c.parse().ok()).unwrap_or(1);
    let backend = framenab_capture::backend();
    println!("platform: {}", backend.platform());

    let mut last = None;
    for i in 1..=count {
        let start = std::time::Instant::now();
        match backend.screenshot(&framenab_capture::CaptureTarget::AllMonitors) {
            Ok(image) => {
                println!("#{i}: captured {}x{} in {:?}", image.width(), image.height(), start.elapsed());
                last = Some(image);
            }
            Err(err) => {
                eprintln!("#{i}: capture failed: {err}");
                std::process::exit(1);
            }
        }
    }
    if let Some(image) = last {
        image.save(&out).expect("save failed");
        println!("saved to {out}");
    }
}
