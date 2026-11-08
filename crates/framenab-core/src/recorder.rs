//! Writes a recording into a project folder while it is running.
//!
//! Frames arrive with timestamps from the capture thread and are handed to a writer thread
//! through a small bounded queue, so encoding and disk I/O never block the capture. If the
//! writer falls behind, new frames are dropped (and counted) instead of piling up in memory.
//! A frame's delay is the time until the next frame; the last one lasts until [`finish`].
//!
//! [`finish`]: ProjectRecorder::finish

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::thread::{self, JoinHandle};

use image::RgbaImage;

use crate::{FrameStore, Project, Result};

/// How many frames may wait for the writer (at 1080p about 8 MB each).
const QUEUE: usize = 8;

enum Message {
    Frame(RgbaImage, u64),
    Finish(u64),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecordingStats {
    /// Frames written to disk.
    pub frames: u64,
    /// Frames dropped because the writer could not keep up.
    pub dropped: u64,
    /// Frames skipped because their size did not match the first frame.
    pub mismatched: u64,
}

pub struct ProjectRecorder {
    tx: Option<SyncSender<Message>>,
    writer: Option<JoinHandle<Result<(Project, RecordingStats)>>>,
    dropped: u64,
}

impl ProjectRecorder {
    /// Starts a writer for a new project in `root` (created if needed).
    pub fn create(root: impl Into<PathBuf>) -> Result<Self> {
        let store = FrameStore::create(root)?;
        let (tx, rx) = mpsc::sync_channel(QUEUE);
        let writer = thread::Builder::new()
            .name("framenab-writer".into())
            .spawn(move || write_frames(&store, &rx))
            .expect("spawn writer thread");
        Ok(Self { tx: Some(tx), writer: Some(writer), dropped: 0 })
    }

    /// Queues a frame captured at `timestamp_ms` (since the start, pauses excluded).
    /// Returns `false` if it was dropped because the writer is busy.
    pub fn push(&mut self, image: RgbaImage, timestamp_ms: u64) -> bool {
        let Some(tx) = &self.tx else { return false };
        match tx.try_send(Message::Frame(image, timestamp_ms)) {
            Ok(()) => true,
            Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => {
                self.dropped += 1;
                false
            }
        }
    }

    /// Ends the recording at `end_ms`, waits for the writer and saves `project.json`.
    pub fn finish(mut self, end_ms: u64) -> Result<(Project, RecordingStats)> {
        if let Some(tx) = self.tx.take() {
            // Blocking send: the finish message must not be dropped.
            let _ = tx.send(Message::Finish(end_ms));
        }
        let writer = self.writer.take().expect("writer runs until finish");
        let (project, mut stats) = writer.join().expect("writer thread panicked")?;
        stats.dropped = self.dropped;
        Ok((project, stats))
    }
}

fn write_frames(store: &FrameStore, rx: &Receiver<Message>) -> Result<(Project, RecordingStats)> {
    let mut project: Option<Project> = None;
    let mut stats = RecordingStats::default();
    let mut last_ts = 0u64;

    let close_previous = |project: &mut Project, ts: u64, last_ts: u64| {
        if let Some(prev) = project.frames.last_mut() {
            prev.delay_ms = u32::try_from(ts.saturating_sub(last_ts)).unwrap_or(u32::MAX).max(1);
        }
    };

    let end = loop {
        match rx.recv() {
            Ok(Message::Frame(image, ts)) => {
                let p = project.get_or_insert_with(|| Project::new(image.width(), image.height()));
                if (image.width(), image.height()) != (p.width, p.height) {
                    stats.mismatched += 1;
                    continue;
                }
                close_previous(p, ts, last_ts);
                store.push_frame(p, &image, 1)?;
                if let Some(frame) = p.frames.last_mut() {
                    frame.time_ms = Some(ts);
                }
                last_ts = ts;
                stats.frames += 1;
            }
            Ok(Message::Finish(end)) => break end,
            Err(_) => break last_ts,
        }
    };

    let mut project = project.unwrap_or_else(|| Project::new(1, 1));
    close_previous(&mut project, end, last_ts);
    store.save_project(&project)?;
    Ok((project, stats))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(v: u8) -> RgbaImage {
        RgbaImage::from_pixel(4, 2, image::Rgba([v, v, v, 255]))
    }

    #[test]
    fn delays_come_from_timestamps() {
        let dir = tempfile::tempdir().unwrap();
        let mut rec = ProjectRecorder::create(dir.path()).unwrap();
        for (i, ts) in [0u64, 66, 200, 250].into_iter().enumerate() {
            assert!(rec.push(frame(i as u8), ts));
            // Give the writer time so the small queue never overflows in this test.
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let (project, stats) = rec.finish(1000).unwrap();
        let delays: Vec<u32> = project.frames.iter().map(|f| f.delay_ms).collect();
        assert_eq!(delays, [66, 134, 50, 750]);
        assert_eq!(stats, RecordingStats { frames: 4, dropped: 0, mismatched: 0 });

        let store = FrameStore::create(dir.path()).unwrap();
        assert_eq!(store.load_project().unwrap(), project);
        assert_eq!(store.load_frame(&project.frames[2]).unwrap(), frame(2));
    }

    #[test]
    fn mismatched_sizes_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let mut rec = ProjectRecorder::create(dir.path()).unwrap();
        rec.push(frame(1), 0);
        rec.push(RgbaImage::new(9, 9), 10);
        let (project, stats) = rec.finish(100).unwrap();
        assert_eq!(project.frames.len(), 1);
        assert_eq!(project.frames[0].delay_ms, 100);
        assert_eq!(stats.mismatched, 1);
    }
}
