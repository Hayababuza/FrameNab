//! Capture spike: `cargo run -p framenab-capture --example screenshot [out.png]`
//! Takes a screenshot of all monitors with the backend for the current platform.

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| "screenshot.png".into());
    let backend = framenab_capture::backend();
    println!("platform: {}", backend.platform());
    let start = std::time::Instant::now();
    match backend.screenshot(&framenab_capture::CaptureTarget::AllMonitors) {
        Ok(image) => {
            println!("captured {}x{} in {:?}", image.width(), image.height(), start.elapsed());
            image.save(&out).expect("save failed");
            println!("saved to {out}");
        }
        Err(err) => {
            eprintln!("capture failed: {err}");
            std::process::exit(1);
        }
    }
}
