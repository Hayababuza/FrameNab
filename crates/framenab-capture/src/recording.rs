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

use framenab_core::InputEvents;
use framenab_core::input::{ClickEvent, CursorSample, KeyEvent};
use image::RgbaImage;

/// A full frame from the source, tightly packed RGBA.
#[derive(Debug)]
pub struct SourceFrame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// A mouse cursor image (straight-alpha RGBA) and its hotspot.
#[derive(Debug)]
pub struct CursorImage {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub hotspot: (i32, i32),
}

/// Where the mouse is on the stream, and how it looks, when the source reports it separately
/// from the frames (Wayland portal metadata). Positions are in stream pixels.
#[derive(Debug, Clone, Default)]
pub struct CursorState {
    pub position: Option<(f32, f32)>,
    pub image: Option<Arc<CursorImage>>,
    /// Draw the cursor image into the sampled frames.
    pub draw: bool,
}

/// The most recent frame of a stream, shared between the source and the sampler.
#[derive(Debug, Default)]
pub struct LatestFrame {
    slot: Mutex<Option<Arc<SourceFrame>>>,
    /// Incremented for every new frame and every cursor change.
    seq: AtomicU64,
    error: Mutex<Option<String>>,
    cursor: Mutex<CursorState>,
}

impl LatestFrame {
    pub fn publish(&self, frame: SourceFrame) {
        *self.slot.lock().expect("frame lock") = Some(Arc::new(frame));
        self.seq.fetch_add(1, Ordering::Release);
    }

    /// Updates the cursor; a move or a new image counts as a change (the next sampled frame
    /// shows it).
    pub fn publish_cursor(&self, position: Option<(f32, f32)>, image: Option<Arc<CursorImage>>) {
        let mut cursor = self.cursor.lock().expect("cursor lock");
        let changed = position != cursor.position || image.is_some();
        cursor.position = position;
        if image.is_some() {
            cursor.image = image;
        }
        // Only a drawn cursor changes the picture; otherwise the position is just recorded.
        if changed && cursor.draw {
            self.seq.fetch_add(1, Ordering::Release);
        }
    }

    /// Whether the sampler draws the cursor image into the frames.
    pub fn set_draw_cursor(&self, draw: bool) {
        self.cursor.lock().expect("cursor lock").draw = draw;
    }

    fn cursor(&self) -> CursorState {
        self.cursor.lock().expect("cursor lock").clone()
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
    /// Frames per second the recording samples; sources that poll (Windows) grab this often.
    /// 0 = the source decides.
    pub fps: u32,
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

/// Recording time: wall time since the start minus pauses. Pauses are measured when `pause`
/// and `resume` are called, not at the sampler's next tick, so a busy machine (late ticks) does
/// not count part of a pause as recorded time.
#[derive(Debug)]
struct Clock {
    start: Instant,
    paused_total: Duration,
    paused_since: Option<Instant>,
}

impl Clock {
    fn elapsed(&self, now: Instant) -> Duration {
        let paused = self.paused_total + self.paused_since.map_or(Duration::ZERO, |since| now - since);
        (now - self.start).saturating_sub(paused)
    }
}

struct Shared {
    stop: AtomicBool,
    clock: Mutex<Clock>,
    input: Mutex<InputEvents>,
}

impl Shared {
    fn clock(&self) -> std::sync::MutexGuard<'_, Clock> {
        self.clock.lock().expect("clock lock")
    }
}

/// A running recording. Call [`Recording::stop`] to end it and get the sink back.
pub struct Recording<S: FrameSink> {
    shared: Arc<Shared>,
    latest: Arc<LatestFrame>,
    crop: Option<Crop>,
    stream: Option<Box<dyn FrameStream>>,
    sampler: Option<JoinHandle<(S, SamplerStats)>>,
}

impl<S: FrameSink> Recording<S> {
    pub fn pause(&self) {
        self.shared.clock().paused_since.get_or_insert_with(Instant::now);
    }

    pub fn resume(&self) {
        let mut clock = self.shared.clock();
        if let Some(since) = clock.paused_since.take() {
            clock.paused_total += since.elapsed();
        }
    }

    pub fn is_paused(&self) -> bool {
        self.shared.clock().paused_since.is_some()
    }

    /// Recorded time so far in ms, without pauses (from the recording clock, so it is exact
    /// even when the sampler runs late).
    pub fn elapsed_ms(&self) -> u64 {
        self.shared.clock().elapsed(Instant::now()).as_millis() as u64
    }

    /// An error reported by the source (e.g. an unsupported pixel format), if any.
    pub fn error(&self) -> Option<String> {
        self.latest.error()
    }

    pub fn restore_token(&self) -> Option<String> {
        self.stream.as_ref().and_then(|s| s.restore_token())
    }

    /// Records clicks and key presses into this recording (from any thread).
    pub fn input_recorder(&self) -> InputRecorder {
        InputRecorder { shared: Arc::clone(&self.shared), latest: Arc::clone(&self.latest), crop: self.crop }
    }

    /// Stops the recording. Returns the sink, the end timestamp in ms (when `stop` was called),
    /// the statistics and the recorded input.
    pub fn stop(mut self) -> (S, u64, SamplerStats, InputEvents) {
        let end = self.shared.clock().elapsed(Instant::now()).as_millis() as u64;
        self.shared.stop.store(true, Ordering::SeqCst);
        let (sink, stats) = self.sampler.take().expect("sampler runs until stop").join().expect("sampler panicked");
        if let Some(stream) = self.stream.take() {
            stream.stop();
        }
        let input = std::mem::take(&mut *self.shared.input.lock().expect("input lock"));
        (sink, end, stats, input)
    }
}

/// Starts sampling `stream` at `fps` into `sink`. `crop` limits the frames to an area.
pub fn record<S: FrameSink>(stream: Box<dyn FrameStream>, crop: Option<Crop>, fps: u32, sink: S) -> Recording<S> {
    let fps = fps.clamp(1, 60);
    let latest = stream.latest();
    let shared = Arc::new(Shared {
        stop: AtomicBool::new(false),
        clock: Mutex::new(Clock { start: Instant::now(), paused_total: Duration::ZERO, paused_since: None }),
        input: Mutex::new(InputEvents::default()),
    });
    let sampler = {
        let shared = Arc::clone(&shared);
        let latest = Arc::clone(&latest);
        thread::Builder::new()
            .name("framenab-sampler".into())
            .spawn(move || sample(&shared, &latest, crop, fps, sink))
            .expect("spawn sampler thread")
    };
    Recording { shared, latest, crop, stream: Some(stream), sampler: Some(sampler) }
}

fn sample<S: FrameSink>(
    shared: &Shared,
    latest: &LatestFrame,
    crop: Option<Crop>,
    fps: u32,
    mut sink: S,
) -> (S, SamplerStats) {
    let interval = Duration::from_secs_f64(1.0 / f64::from(fps));
    let mut seen = 0u64;
    let mut next = Instant::now();
    let mut stats = SamplerStats::default();

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
            return (sink, stats);
        }
        let (paused, elapsed) = {
            let clock = shared.clock();
            (clock.paused_since.is_some(), clock.elapsed(now))
        };
        if paused {
            continue;
        }
        let timestamp = elapsed.as_millis() as u64;

        let cursor = latest.cursor();
        if let Some(position) = cursor.position {
            record_cursor(shared, crop, timestamp, position);
        }

        let Some((seq, frame)) = latest.take_if_newer(seen) else { continue };
        seen = seq;
        let Some(mut image) = to_image(&frame, crop) else { continue };
        if cursor.draw
            && let (Some((x, y)), Some(cursor_image)) = (cursor.position, cursor.image.as_deref())
        {
            let (cx, cy) = crop.map_or((0, 0), |c| (c.x, c.y));
            let left = x.round() as i32 - cursor_image.hotspot.0 - cx;
            let top = y.round() as i32 - cursor_image.hotspot.1 - cy;
            draw_cursor(&mut image, cursor_image, left, top);
        }
        if sink.push(image, timestamp) {
            stats.pushed += 1;
        } else {
            stats.dropped += 1;
        }
    }
}

/// Appends a cursor sample (in recording pixels) if the mouse moved.
fn record_cursor(shared: &Shared, crop: Option<Crop>, t: u64, (x, y): (f32, f32)) {
    let (cx, cy) = crop.map_or((0.0, 0.0), |c| (c.x as f32, c.y as f32));
    let sample = CursorSample { t, x: x - cx, y: y - cy };
    let mut input = shared.input.lock().expect("input lock");
    if input.cursor.last().is_none_or(|last| (last.x, last.y) != (sample.x, sample.y)) {
        input.cursor.push(sample);
    }
}

/// Alpha-blends the cursor image into `image` with its top-left corner at `(left, top)`.
fn draw_cursor(image: &mut RgbaImage, cursor: &CursorImage, left: i32, top: i32) {
    for cy in 0..cursor.height as i32 {
        for cx in 0..cursor.width as i32 {
            let (x, y) = (left + cx, top + cy);
            if x < 0 || y < 0 || x >= image.width() as i32 || y >= image.height() as i32 {
                continue;
            }
            let i = ((cy as u32 * cursor.width + cx as u32) * 4) as usize;
            let Some(src) = cursor.rgba.get(i..i + 4) else { continue };
            let a = u32::from(src[3]);
            if a == 0 {
                continue;
            }
            let dst = image.get_pixel_mut(x as u32, y as u32);
            for (d, &s) in dst.0.iter_mut().zip(src).take(3) {
                *d = ((u32::from(s) * a + u32::from(*d) * (255 - a)) / 255) as u8;
            }
        }
    }
}

/// Records clicks and key presses into a running recording, with the recording time and (for
/// clicks) the current mouse position. Ignored while paused.
#[derive(Clone)]
pub struct InputRecorder {
    shared: Arc<Shared>,
    latest: Arc<LatestFrame>,
    crop: Option<Crop>,
}

impl InputRecorder {
    fn now(&self) -> Option<u64> {
        let clock = self.shared.clock();
        clock.paused_since.is_none().then(|| clock.elapsed(Instant::now()).as_millis() as u64)
    }

    /// A mouse button was pressed (0 = left, 1 = right, 2 = middle). Needs a known cursor
    /// position; without one the click is dropped.
    pub fn click(&self, button: u8) {
        let Some(t) = self.now() else { return };
        let Some((x, y)) = self.latest.cursor().position else { return };
        let (cx, cy) = self.crop.map_or((0.0, 0.0), |c| (c.x as f32, c.y as f32));
        self.shared.input.lock().expect("input lock").clicks.push(ClickEvent { t, x: x - cx, y: y - cy, button });
    }

    /// A key (combination) was pressed, e.g. `Ctrl + C`.
    pub fn key(&self, label: impl Into<String>) {
        let Some(t) = self.now() else { return };
        self.shared.input.lock().expect("input lock").keys.push(KeyEvent { t, label: label.into() });
    }

    /// The stream reports the cursor position itself (no global position needed).
    pub fn has_cursor(&self) -> bool {
        self.latest.cursor().position.is_some()
    }

    /// Sets the cursor position (stream pixels) from a global mouse position source.
    pub fn set_cursor(&self, position: (f32, f32)) {
        self.latest.publish_cursor(Some(position), None);
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
        let (sink, end, stats, _) = rec.stop();
        // At most 20 fps. No lower bound beyond "something": busy CI runners (macOS) skip ticks.
        let max = end / 50 + 2;
        assert!((1..=max as usize).contains(&sink.0.len()), "got {} frames in {end} ms", sink.0.len());
        assert!(sink.0.iter().all(|&(w, h, _)| (w, h) == (2, 2)), "crop is clamped to the frame");
        // A late tick may be followed by an on-time one, so only the average spacing is ~50 ms.
        let times: Vec<u64> = sink.0.iter().map(|f| f.2).collect();
        assert!(times.windows(2).all(|p| p[1] > p[0]), "timestamps increase: {times:?}");
        if let [first, .., last] = times[..] {
            let average = (last - first) / (times.len() as u64 - 1);
            assert!(average >= 45, "frames are ≥ 50 ms apart on average: {times:?}");
        }
        assert!(end >= 450, "end {end}");
        assert_eq!(stats.pushed as usize, sink.0.len());
    }

    #[test]
    fn elapsed_time_is_reported_while_recording() {
        let started = Instant::now();
        let stream = FakeStream::start(5);
        let rec = record(Box::new(stream), None, 15, Collect::default());
        thread::sleep(Duration::from_millis(400));
        let elapsed = rec.elapsed_ms();
        let wall = started.elapsed().as_millis() as u64;
        let _ = rec.stop();
        assert!(elapsed >= 380 && elapsed <= wall, "elapsed {elapsed} ms, wall time {wall} ms");
    }

    #[test]
    fn pause_is_not_counted() {
        // Compare with measured wall times, so a slow machine (late sleeps) does not matter.
        let started = Instant::now();
        let stream = FakeStream::start(5);
        let rec = record(Box::new(stream), None, 30, Collect::default());
        thread::sleep(Duration::from_millis(200));
        let pause_start = Instant::now();
        rec.pause();
        thread::sleep(Duration::from_millis(400));
        rec.resume();
        let paused = pause_start.elapsed();
        thread::sleep(Duration::from_millis(200));
        let total = started.elapsed();
        let (sink, end, _, _) = rec.stop();
        let expected = (total - paused).as_millis() as u64;
        assert!(end.abs_diff(expected) < 60, "recorded {end} ms, expected about {expected} ms");
        let gaps: Vec<u64> = sink.0.windows(2).map(|p| p[1].2 - p[0].2).collect();
        let paused_ms = paused.as_millis() as u64;
        assert!(gaps.iter().all(|&g| g < paused_ms), "no {paused_ms} ms jump from the pause: {gaps:?}");
    }

    #[test]
    fn records_cursor_clicks_and_keys_relative_to_the_crop() {
        let stream = FakeStream::start(5);
        let latest = stream.latest();
        let crop = Crop { x: 10, y: 20, width: 2, height: 2 };
        let rec = record(Box::new(stream), Some(crop), 30, Collect::default());
        let input = rec.input_recorder();
        latest.publish_cursor(Some((15.0, 25.0)), None);
        thread::sleep(Duration::from_millis(250));
        input.click(0);
        input.key("Ctrl + C");
        rec.pause();
        input.key("ignored while paused");
        rec.resume();
        let (_, _, _, events) = rec.stop();
        assert_eq!(events.cursor.first().map(|s| (s.x, s.y)), Some((5.0, 5.0)));
        assert_eq!(events.clicks.len(), 1);
        assert_eq!((events.clicks[0].x, events.clicks[0].y), (5.0, 5.0));
        assert_eq!(events.keys.iter().map(|k| k.label.as_str()).collect::<Vec<_>>(), ["Ctrl + C"]);
    }

    #[test]
    fn draws_the_cursor_into_frames_when_asked() {
        let latest = Arc::new(LatestFrame::default());
        latest.publish(SourceFrame { width: 4, height: 4, rgba: vec![0; 64] });
        latest.set_draw_cursor(true);
        let white = Arc::new(CursorImage { rgba: vec![255; 4], width: 1, height: 1, hotspot: (0, 0) });
        latest.publish_cursor(Some((2.0, 1.0)), Some(white));
        struct Static(Arc<LatestFrame>);
        impl FrameStream for Static {
            fn latest(&self) -> Arc<LatestFrame> {
                Arc::clone(&self.0)
            }
            fn stop(self: Box<Self>) {}
        }
        #[derive(Default)]
        struct Keep(Vec<RgbaImage>);
        impl FrameSink for Keep {
            fn push(&mut self, image: RgbaImage, _: u64) -> bool {
                self.0.push(image);
                true
            }
        }
        let rec = record(Box::new(Static(latest)), None, 30, Keep::default());
        thread::sleep(Duration::from_millis(300));
        let (sink, _, _, _) = rec.stop();
        let frame = sink.0.first().expect("a frame within 300 ms");
        assert_eq!(frame.get_pixel(2, 1).0, [255, 255, 255, 0]);
        assert_eq!(frame.get_pixel(0, 0).0, [0, 0, 0, 0]);
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
        let (sink, end, _, _) = rec.stop();
        assert_eq!(sink.0.len(), 1, "one frame that lasts the whole recording");
        assert!(end >= 250);
    }
}
