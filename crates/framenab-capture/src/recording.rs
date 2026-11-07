//! Screen recording, split into two parts:
//!
//! * a [`FrameStream`] per backend that only keeps the **latest** frame of the screen (PipeWire,
//!   Windows/macOS/X11 via xcap). Many compositors send frames only when something changes.
//! * [`record`]: one sampler for all platforms that takes the latest frame at the chosen FPS,
//!   crops it to the region and passes it to a [`FrameSink`] with a timestamp. If nothing
//!   changed, nothing is pushed, so the previous frame simply lasts longer. Paused time does
//!   not count.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use image::RgbaImage;

/// A full frame from the source, tightly packed RGBA.
#[derive(Debug)]
pub struct SourceFrame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// The most recent frame of a stream, shared between the source and the sampler.
#[derive(Debug, Default)]
pub struct LatestFrame {
    slot: Mutex<Option<Arc<SourceFrame>>>,
    /// Incremented for every new frame.
    seq: AtomicU64,
    error: Mutex<Option<String>>,
}

impl LatestFrame {
    pub fn publish(&self, frame: SourceFrame) {
        *self.slot.lock().expect("frame lock") = Some(Arc::new(frame));
        self.seq.fetch_add(1, Ordering::Release);
    }

    pub fn set_error(&self, error: impl Into<String>) {
        let mut slot = self.error.lock().expect("error lock");
        if slot.is_none() {
            *slot = Some(error.into());
        }
    }

    fn take_if_newer(&self, seen: u64) -> Option<(u64, Arc<SourceFrame>)> {
        let seq = self.seq.load(Ordering::Acquire);
        if seq == seen {
            return None;
        }
        self.slot.lock().expect("frame lock").clone().map(|f| (seq, f))
    }

    pub fn error(&self) -> Option<String> {
        self.error.lock().expect("error lock").clone()
    }
}

/// Options for opening a stream.
#[derive(Debug, Clone, Default)]
pub struct StreamOptions {
    /// Draw the mouse cursor into the frames.
    pub cursor: bool,
    /// Wayland: token from a previous session, so the screen-sharing dialog is skipped.
    pub restore_token: Option<String>,
}

/// A running capture source (one monitor).
pub trait FrameStream: Send {
    fn latest(&self) -> Arc<LatestFrame>;
    /// Where the stream's top-left corner is in the virtual desktop (physical pixels), used to
    /// translate a [`crate::CaptureTarget::Region`] into the stream's pixels.
    fn origin(&self) -> (i32, i32) {
        (0, 0)
    }
    /// Wayland: token to restore this screen selection next time without asking.
    fn restore_token(&self) -> Option<String> {
        None
    }
    fn stop(self: Box<Self>);
}

/// Receives the sampled frames. `push` returns `false` if the frame had to be dropped.
pub trait FrameSink: Send + 'static {
    fn push(&mut self, image: RgbaImage, timestamp_ms: u64) -> bool;
}

impl FrameSink for framenab_core::recorder::ProjectRecorder {
    fn push(&mut self, image: RgbaImage, timestamp_ms: u64) -> bool {
        Self::push(self, image, timestamp_ms)
    }
}

/// Area to record, in the stream's pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Crop {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SamplerStats {
    /// Frames passed to the sink.
    pub pushed: u64,
    /// Frames the sink dropped.
    pub dropped: u64,
    /// Ticks where the sampler itself was late by more than one interval.
    pub late_ticks: u64,
}

struct Shared {
    stop: AtomicBool,
    paused: AtomicBool,
    /// Recording time in ms (pauses excluded), for the UI.
    elapsed_ms: AtomicU64,
}

/// A running recording. Call [`Recording::stop`] to end it and get the sink back.
pub struct Recording<S: FrameSink> {
    shared: Arc<Shared>,
    latest: Arc<LatestFrame>,
    stream: Option<Box<dyn FrameStream>>,
    sampler: Option<JoinHandle<(S, u64, SamplerStats)>>,
}

impl<S: FrameSink> Recording<S> {
    pub fn pause(&self) {
        self.shared.paused.store(true, Ordering::SeqCst);
    }

    pub fn resume(&self) {
        self.shared.paused.store(false, Ordering::SeqCst);
    }

    pub fn is_paused(&self) -> bool {
        self.shared.paused.load(Ordering::SeqCst)
    }

    /// Recorded time so far in ms, without pauses.
    pub fn elapsed_ms(&self) -> u64 {
        self.shared.elapsed_ms.load(Ordering::Relaxed)
    }

    /// An error reported by the source (e.g. an unsupported pixel format), if any.
    pub fn error(&self) -> Option<String> {
        self.latest.error()
    }

    pub fn restore_token(&self) -> Option<String> {
        self.stream.as_ref().and_then(|s| s.restore_token())
    }

    /// Stops the recording. Returns the sink, the end timestamp in ms and the statistics.
    pub fn stop(mut self) -> (S, u64, SamplerStats) {
        self.shared.stop.store(true, Ordering::SeqCst);
        let result = self.sampler.take().expect("sampler runs until stop").join().expect("sampler panicked");
        if let Some(stream) = self.stream.take() {
            stream.stop();
        }
        result
    }
}

/// Starts sampling `stream` at `fps` into `sink`. `crop` limits the frames to an area.
pub fn record<S: FrameSink>(stream: Box<dyn FrameStream>, crop: Option<Crop>, fps: u32, sink: S) -> Recording<S> {
    let fps = fps.clamp(1, 60);
    let latest = stream.latest();
    let shared = Arc::new(Shared {
        stop: AtomicBool::new(false),
        paused: AtomicBool::new(false),
        elapsed_ms: AtomicU64::new(0),
    });
    let sampler = {
        let shared = Arc::clone(&shared);
        let latest = Arc::clone(&latest);
        thread::Builder::new()
            .name("framenab-sampler".into())
            .spawn(move || sample(&shared, &latest, crop, fps, sink))
            .expect("spawn sampler thread")
    };
    Recording { shared, latest, stream: Some(stream), sampler: Some(sampler) }
}

fn sample<S: FrameSink>(
    shared: &Shared,
    latest: &LatestFrame,
    crop: Option<Crop>,
    fps: u32,
    mut sink: S,
) -> (S, u64, SamplerStats) {
    let interval = Duration::from_secs_f64(1.0 / f64::from(fps));
    let start = Instant::now();
    let mut paused_total = Duration::ZERO;
    let mut paused_since: Option<Instant> = None;
    let mut seen = 0u64;
    let mut next = start;
    let mut stats = SamplerStats::default();

    let recorded = |now: Instant, paused_total: Duration| (now - start).saturating_sub(paused_total);

    loop {
        let now = Instant::now();
        if next > now {
            thread::sleep(next - now);
        }
        let now = Instant::now();
        // Catch up without a burst if we fell behind (e.g. the machine was busy).
        next = if now > next + interval {
            stats.late_ticks += 1;
            now + interval
        } else {
            next + interval
        };

        if shared.stop.load(Ordering::SeqCst) {
            if let Some(since) = paused_since {
                paused_total += now - since;
            }
            let end = recorded(now, paused_total).as_millis() as u64;
            return (sink, end, stats);
        }

        if shared.paused.load(Ordering::SeqCst) {
            paused_since.get_or_insert(now);
            continue;
        }
        if let Some(since) = paused_since.take() {
            paused_total += now - since;
        }
        let timestamp = recorded(now, paused_total).as_millis() as u64;
        shared.elapsed_ms.store(timestamp, Ordering::Relaxed);

        let Some((seq, frame)) = latest.take_if_newer(seen) else { continue };
        seen = seq;
        let Some(image) = to_image(&frame, crop) else { continue };
        if sink.push(image, timestamp) {
            stats.pushed += 1;
        } else {
            stats.dropped += 1;
        }
    }
}

/// Copies the (cropped) frame into an image.
fn to_image(frame: &SourceFrame, crop: Option<Crop>) -> Option<RgbaImage> {
    let crop = crop.unwrap_or(Crop { x: 0, y: 0, width: frame.width, height: frame.height });
    let x0 = crop.x.clamp(0, frame.width as i32) as u32;
    let y0 = crop.y.clamp(0, frame.height as i32) as u32;
    let w = crop.width.min(frame.width - x0);
    let h = crop.height.min(frame.height - y0);
    if w == 0 || h == 0 {
        return None;
    }
    let stride = frame.width as usize * 4;
    let mut out = Vec::with_capacity(w as usize * h as usize * 4);
    for row in y0..y0 + h {
        let start = row as usize * stride + x0 as usize * 4;
        out.extend_from_slice(frame.rgba.get(start..start + w as usize * 4)?);
    }
    RgbaImage::from_raw(w, h, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stream that publishes a new solid-color frame every `every` milliseconds.
    struct FakeStream {
        latest: Arc<LatestFrame>,
        stop: Arc<AtomicBool>,
        thread: Option<JoinHandle<()>>,
    }

    impl FakeStream {
        fn start(every: u64) -> Self {
            let latest = Arc::new(LatestFrame::default());
            let stop = Arc::new(AtomicBool::new(false));
            let thread = {
                let (latest, stop) = (Arc::clone(&latest), Arc::clone(&stop));
                thread::spawn(move || {
                    let mut n = 0u8;
                    while !stop.load(Ordering::SeqCst) {
                        n = n.wrapping_add(1);
                        let rgba = (0..4 * 3).flat_map(|i| [n, i as u8, 0, 255]).collect();
                        latest.publish(SourceFrame { width: 4, height: 3, rgba });
                        thread::sleep(Duration::from_millis(every));
                    }
                })
            };
            Self { latest, stop, thread: Some(thread) }
        }
    }

    impl FrameStream for FakeStream {
        fn latest(&self) -> Arc<LatestFrame> {
            Arc::clone(&self.latest)
        }
        fn stop(mut self: Box<Self>) {
            self.stop.store(true, Ordering::SeqCst);
            if let Some(t) = self.thread.take() {
                let _ = t.join();
            }
        }
    }

    #[derive(Default)]
    struct Collect(Vec<(u32, u32, u64)>);

    impl FrameSink for Collect {
        fn push(&mut self, image: RgbaImage, ts: u64) -> bool {
            self.0.push((image.width(), image.height(), ts));
            true
        }
    }

    #[test]
    fn samples_at_most_at_the_target_fps_and_crops() {
        let stream = FakeStream::start(2); // source much faster than the sampler
        let crop = Crop { x: 1, y: 1, width: 2, height: 5 };
        let rec = record(Box::new(stream), Some(crop), 20, Collect::default());
        thread::sleep(Duration::from_millis(500));
        let (sink, end, stats) = rec.stop();
        // ~10 frames in 500 ms at 20 fps (generous bounds for slow CI machines)
        assert!((5..=12).contains(&sink.0.len()), "got {} frames", sink.0.len());
        assert!(sink.0.iter().all(|&(w, h, _)| (w, h) == (2, 2)), "crop is clamped to the frame");
        assert!(sink.0.windows(2).all(|p| p[1].2 >= p[0].2 + 40), "frames are ≥ 50 ms apart");
        assert!(end >= 450, "end {end}");
        assert_eq!(stats.pushed as usize, sink.0.len());
    }

    #[test]
    fn elapsed_time_is_reported_while_recording() {
        let stream = FakeStream::start(5);
        let rec = record(Box::new(stream), None, 15, Collect::default());
        thread::sleep(Duration::from_millis(400));
        let elapsed = rec.elapsed_ms();
        let _ = rec.stop();
        assert!(elapsed >= 250, "elapsed should grow while recording, got {elapsed}");
    }

    #[test]
    fn pause_is_not_counted() {
        let stream = FakeStream::start(5);
        let rec = record(Box::new(stream), None, 30, Collect::default());
        thread::sleep(Duration::from_millis(200));
        rec.pause();
        thread::sleep(Duration::from_millis(400));
        rec.resume();
        thread::sleep(Duration::from_millis(200));
        let (sink, end, _) = rec.stop();
        assert!((300..550).contains(&end), "recorded time should exclude the pause, got {end}");
        let gaps: Vec<u64> = sink.0.windows(2).map(|p| p[1].2 - p[0].2).collect();
        assert!(gaps.iter().all(|&g| g < 150), "no 400 ms gap from the pause: {gaps:?}");
    }

    #[test]
    fn unchanged_screen_pushes_nothing_new() {
        let latest = Arc::new(LatestFrame::default());
        latest.publish(SourceFrame { width: 1, height: 1, rgba: vec![1, 2, 3, 255] });
        struct Static(Arc<LatestFrame>);
        impl FrameStream for Static {
            fn latest(&self) -> Arc<LatestFrame> {
                Arc::clone(&self.0)
            }
            fn stop(self: Box<Self>) {}
        }
        let rec = record(Box::new(Static(latest)), None, 30, Collect::default());
        thread::sleep(Duration::from_millis(300));
        let (sink, end, _) = rec.stop();
        assert_eq!(sink.0.len(), 1, "one frame that lasts the whole recording");
        assert!(end >= 250);
    }
}
