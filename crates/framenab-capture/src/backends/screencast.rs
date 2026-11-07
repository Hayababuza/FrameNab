//! Wayland recording: xdg-desktop-portal ScreenCast + PipeWire.
//!
//! The portal asks the user which monitor to share (once: we ask it to remember the choice and
//! hand back a restore token). It then gives us a PipeWire remote and a node id; a dedicated
//! thread runs a PipeWire main loop and publishes every frame into a [`LatestFrame`].

use std::io::Cursor;
use std::os::fd::OwnedFd;
use std::sync::{Arc, Once};
use std::thread::{self, JoinHandle};

use ashpd::desktop::screencast::{CursorMode, Screencast, SelectSourcesOptions, SourceType};
use ashpd::desktop::{PersistMode, Session};
use pipewire as pw;
use pw::spa::param::format::{FormatProperties, MediaSubtype, MediaType};
use pw::spa::param::video::{VideoFormat, VideoInfoRaw};
use pw::spa::param::{ParamType, format_utils};
use pw::spa::pod::serialize::PodSerializer;
use pw::spa::pod::{self, Pod};
use pw::spa::utils::{Direction, Fraction, Rectangle, SpaTypes};
use pw::stream::{StreamFlags, StreamRc};

use crate::recording::{FrameStream, LatestFrame, SourceFrame, StreamOptions};
use crate::{CaptureError, Result};

use super::portal::{map_err, runtime};

pub struct PortalStream {
    latest: Arc<LatestFrame>,
    origin: (i32, i32),
    restore_token: Option<String>,
    quit: pw::channel::Sender<()>,
    thread: Option<JoinHandle<()>>,
    session: Option<Session<Screencast>>,
}

impl PortalStream {
    /// Opens a screen cast of a monitor, or with `window` of a single window (the user picks it
    /// in the portal dialog; window choices are not remembered).
    pub fn open(options: &StreamOptions, window: bool) -> Result<Self> {
        let cursor = if options.cursor { CursorMode::EMBEDDED } else { CursorMode::HIDDEN };
        let token = if window { None } else { options.restore_token.clone() };
        let (sources, persist) = if window {
            (SourceType::WINDOW, PersistMode::DoNot)
        } else {
            (SourceType::MONITOR, PersistMode::ExplicitlyRevoked)
        };
        let (session, node_id, position, fd, restore_token) = runtime()?.block_on(async move {
            let proxy = Screencast::new().await.map_err(map_err)?;
            let session = proxy.create_session(Default::default()).await.map_err(map_err)?;
            proxy
                .select_sources(
                    &session,
                    SelectSourcesOptions::default()
                        .set_cursor_mode(cursor)
                        .set_sources(sources)
                        .set_multiple(false)
                        .set_persist_mode(persist)
                        .set_restore_token(token.as_deref()),
                )
                .await
                .map_err(map_err)?;
            let streams =
                proxy.start(&session, None, Default::default()).await.map_err(map_err)?.response().map_err(map_err)?;
            let stream =
                streams.streams().first().ok_or_else(|| CaptureError::Backend("portal returned no stream".into()))?;
            let node_id = stream.pipe_wire_node_id();
            let position = stream.position().unwrap_or((0, 0));
            let restore_token = streams.restore_token().map(str::to_owned);
            let fd = proxy.open_pipe_wire_remote(&session, Default::default()).await.map_err(map_err)?;
            Ok::<_, CaptureError>((session, node_id, position, fd, restore_token))
        })?;
        tracing::info!(node_id, ?position, has_token = restore_token.is_some(), "screen cast started");

        let latest = Arc::new(LatestFrame::default());
        let (quit, quit_rx) = pw::channel::channel::<()>();
        let thread = {
            let latest = Arc::clone(&latest);
            thread::Builder::new()
                .name("framenab-pipewire".into())
                .spawn(move || {
                    if let Err(err) = run_pipewire(fd, node_id, &latest, quit_rx) {
                        tracing::error!(%err, "pipewire stream failed");
                        latest.set_error(err);
                    }
                })
                .map_err(|e| CaptureError::Backend(e.to_string()))?
        };

        Ok(Self { latest, origin: position, restore_token, quit, thread: Some(thread), session: Some(session) })
    }
}

impl FrameStream for PortalStream {
    fn latest(&self) -> Arc<LatestFrame> {
        Arc::clone(&self.latest)
    }

    fn origin(&self) -> (i32, i32) {
        self.origin
    }

    fn restore_token(&self) -> Option<String> {
        self.restore_token.clone()
    }

    fn stop(mut self: Box<Self>) {
        let _ = self.quit.send(());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        if let Some(session) = self.session.take()
            && let Ok(rt) = runtime()
            && let Err(err) = rt.block_on(session.close())
        {
            tracing::debug!(%err, "closing the screen cast session failed");
        }
    }
}

/// Runs the PipeWire main loop until a message arrives on `quit`.
fn run_pipewire(
    fd: OwnedFd,
    node_id: u32,
    latest: &Arc<LatestFrame>,
    quit: pw::channel::Receiver<()>,
) -> std::result::Result<(), String> {
    static INIT: Once = Once::new();
    INIT.call_once(pw::init);

    let err = |e: pw::Error| e.to_string();
    let main_loop = pw::main_loop::MainLoopRc::new(None).map_err(err)?;
    let context = pw::context::ContextRc::new(&main_loop, None).map_err(err)?;
    let core = context.connect_fd_rc(fd, None).map_err(err)?;

    let stream = StreamRc::new(
        core,
        "FrameNab",
        pw::properties::properties! {
            *pw::keys::MEDIA_TYPE => "Video",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE => "Screen",
        },
    )
    .map_err(err)?;

    let sink = Arc::clone(latest);
    let _listener = stream
        .add_local_listener_with_user_data(VideoInfoRaw::default())
        .param_changed(|_, format, id, param| {
            let Some(param) = param else { return };
            if id != ParamType::Format.as_raw() {
                return;
            }
            if let Ok((MediaType::Video, MediaSubtype::Raw)) = format_utils::parse_format(param) {
                if let Err(e) = format.parse(param) {
                    tracing::error!(?e, "could not parse the video format");
                } else {
                    tracing::debug!(format = ?format.format(), size = ?format.size(), "stream format");
                }
            }
        })
        .process(move |stream, format| {
            let Some(mut buffer) = stream.dequeue_buffer() else { return };
            let datas = buffer.datas_mut();
            let Some(data) = datas.first_mut() else { return };
            let chunk = data.chunk();
            let (offset, stride) = (chunk.offset() as usize, chunk.stride());
            let size = format.size();
            let Some(bytes) = data.data() else {
                sink.set_error("the compositor sent a buffer type FrameNab cannot read (DMA-BUF)");
                return;
            };
            match convert(&bytes[offset.min(bytes.len())..], size.width, size.height, stride, format.format()) {
                Some(rgba) => sink.publish(SourceFrame { width: size.width, height: size.height, rgba }),
                None => sink.set_error(format!("unsupported pixel format {:?}", format.format())),
            }
        })
        .register()
        .map_err(err)?;

    let format = pod::object!(
        SpaTypes::ObjectParamFormat,
        ParamType::EnumFormat,
        pod::property!(FormatProperties::MediaType, Id, MediaType::Video),
        pod::property!(FormatProperties::MediaSubtype, Id, MediaSubtype::Raw),
        pod::property!(
            FormatProperties::VideoFormat,
            Choice,
            Enum,
            Id,
            VideoFormat::BGRx,
            VideoFormat::BGRx,
            VideoFormat::BGRA,
            VideoFormat::RGBx,
            VideoFormat::RGBA,
            VideoFormat::RGB
        ),
        pod::property!(
            FormatProperties::VideoSize,
            Choice,
            Range,
            Rectangle,
            Rectangle { width: 1920, height: 1080 },
            Rectangle { width: 1, height: 1 },
            Rectangle { width: 16384, height: 16384 }
        ),
        pod::property!(
            FormatProperties::VideoFramerate,
            Choice,
            Range,
            Fraction,
            Fraction { num: 60, denom: 1 },
            Fraction { num: 0, denom: 1 },
            Fraction { num: 1000, denom: 1 }
        ),
    );
    let bytes = PodSerializer::serialize(Cursor::new(Vec::new()), &pod::Value::Object(format))
        .map_err(|e| format!("{e:?}"))?
        .0
        .into_inner();
    let mut params = [Pod::from_bytes(&bytes).ok_or("invalid format pod")?];
    stream
        .connect(Direction::Input, Some(node_id), StreamFlags::AUTOCONNECT | StreamFlags::MAP_BUFFERS, &mut params)
        .map_err(err)?;

    let main_loop_weak = main_loop.downgrade();
    let _quit = quit.attach(main_loop.loop_(), move |()| {
        if let Some(main_loop) = main_loop_weak.upgrade() {
            main_loop.quit();
        }
    });

    main_loop.run();
    Ok(())
}

/// Converts one frame to tightly packed RGBA, honoring the row stride.
pub(crate) fn convert(data: &[u8], width: u32, height: u32, stride: i32, format: VideoFormat) -> Option<Vec<u8>> {
    let bpp = match format {
        VideoFormat::RGB => 3,
        VideoFormat::RGBA | VideoFormat::RGBx | VideoFormat::BGRA | VideoFormat::BGRx => 4,
        _ => return None,
    };
    let (w, h) = (width as usize, height as usize);
    let stride = if stride > 0 { stride as usize } else { w * bpp };
    let mut out = Vec::with_capacity(w * h * 4);
    for row in 0..h {
        let line = data.get(row * stride..row * stride + w * bpp)?;
        match format {
            VideoFormat::RGB => {
                line.as_chunks::<3>().0.iter().for_each(|&[r, g, b]| out.extend_from_slice(&[r, g, b, 255]))
            }
            VideoFormat::RGBA => out.extend_from_slice(line),
            VideoFormat::RGBx => {
                line.as_chunks::<4>().0.iter().for_each(|&[r, g, b, _]| out.extend_from_slice(&[r, g, b, 255]))
            }
            VideoFormat::BGRA => {
                line.as_chunks::<4>().0.iter().for_each(|&[b, g, r, a]| out.extend_from_slice(&[r, g, b, a]))
            }
            _ => line.as_chunks::<4>().0.iter().for_each(|&[b, g, r, _]| out.extend_from_slice(&[r, g, b, 255])),
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_bgrx_with_row_padding() {
        // 2×2 BGRx with 4 bytes of padding per row (stride 12).
        let data = [
            1, 2, 3, 0, 4, 5, 6, 0, 99, 99, 99, 99, //
            7, 8, 9, 0, 10, 11, 12, 0, 99, 99, 99, 99,
        ];
        let rgba = convert(&data, 2, 2, 12, VideoFormat::BGRx).unwrap();
        assert_eq!(rgba, [3, 2, 1, 255, 6, 5, 4, 255, 9, 8, 7, 255, 12, 11, 10, 255]);
    }

    #[test]
    fn rejects_short_buffers_and_unknown_formats() {
        assert!(convert(&[0; 7], 2, 1, 8, VideoFormat::RGBA).is_none());
        assert!(convert(&[0; 8], 2, 1, 8, VideoFormat::I420).is_none());
    }
}
