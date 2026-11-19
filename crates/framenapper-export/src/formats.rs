//! Encoders written for FrameNapper: APNG (via the `png` crate), animated lossless WebP (frames
//! encoded by `image-webp`, put into the animation container here) and PNG sequences as ZIP.

use std::fs::File;
use std::io::{BufWriter, Cursor, Write};
use std::path::Path;

use image::RgbaImage;

use crate::animation::ExportError;

fn io(path: &Path) -> impl Fn(std::io::Error) -> ExportError + '_ {
    move |source| ExportError::Io { path: path.to_owned(), source }
}

fn encode_err(e: impl std::fmt::Display) -> ExportError {
    ExportError::Encode(e.to_string())
}

/// Streams frames into an animated PNG.
pub struct ApngWriter {
    writer: png::Writer<BufWriter<File>>,
}

impl ApngWriter {
    pub fn create(path: &Path, size: (u32, u32), frames: u32, loop_count: u16) -> Result<Self, ExportError> {
        let file = BufWriter::new(File::create(path).map_err(io(path))?);
        let mut encoder = png::Encoder::new(file, size.0, size.1);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Fast);
        encoder.set_animated(frames.max(1), u32::from(loop_count)).map_err(encode_err)?;
        Ok(Self { writer: encoder.write_header().map_err(encode_err)? })
    }

    pub fn add(&mut self, image: &RgbaImage, delay_ms: u32) -> Result<(), ExportError> {
        let delay = u16::try_from(delay_ms.max(1)).unwrap_or(u16::MAX);
        self.writer.set_frame_delay(delay, 1000).map_err(encode_err)?;
        self.writer.write_image_data(image.as_raw()).map_err(encode_err)
    }

    pub fn finish(self) -> Result<(), ExportError> {
        self.writer.finish().map_err(encode_err)
    }
}

/// Collects losslessly encoded frames and writes an animated WebP.
pub struct WebpWriter {
    size: (u32, u32),
    loop_count: u16,
    /// `ANMF` chunks.
    frames: Vec<u8>,
}

fn chunk(out: &mut Vec<u8>, fourcc: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(fourcc);
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(data);
    if data.len() % 2 == 1 {
        out.push(0);
    }
}

fn u24(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes()[..3]);
}

impl WebpWriter {
    #[must_use]
    pub fn new(size: (u32, u32), loop_count: u16) -> Self {
        Self { size, loop_count, frames: Vec::new() }
    }

    pub fn add(&mut self, image: &RgbaImage, delay_ms: u32) -> Result<(), ExportError> {
        let mut encoded = Vec::new();
        image_webp::WebPEncoder::new(&mut encoded)
            .encode(image.as_raw(), image.width(), image.height(), image_webp::ColorType::Rgba8)
            .map_err(encode_err)?;
        let bitstream = find_chunk(&encoded, b"VP8L").ok_or_else(|| encode_err("no VP8L chunk"))?;
        let mut frame = Vec::with_capacity(bitstream.len() + 24);
        u24(&mut frame, 0); // x / 2
        u24(&mut frame, 0); // y / 2
        u24(&mut frame, image.width() - 1);
        u24(&mut frame, image.height() - 1);
        u24(&mut frame, delay_ms.clamp(1, 0xFF_FFFF));
        frame.push(0b10); // do not blend, do not dispose
        chunk(&mut frame, b"VP8L", bitstream);
        chunk(&mut self.frames, b"ANMF", &frame);
        Ok(())
    }

    pub fn write(self, path: &Path) -> Result<(), ExportError> {
        let mut body = Vec::with_capacity(self.frames.len() + 64);
        body.extend_from_slice(b"WEBP");
        let mut vp8x = vec![0b0001_0010, 0, 0, 0]; // alpha + animation
        u24(&mut vp8x, self.size.0 - 1);
        u24(&mut vp8x, self.size.1 - 1);
        chunk(&mut body, b"VP8X", &vp8x);
        let mut anim = vec![0, 0, 0, 0]; // background color
        anim.extend_from_slice(&self.loop_count.to_le_bytes());
        chunk(&mut body, b"ANIM", &anim);
        body.extend_from_slice(&self.frames);

        let mut file = BufWriter::new(File::create(path).map_err(io(path))?);
        file.write_all(b"RIFF").map_err(io(path))?;
        file.write_all(&(body.len() as u32).to_le_bytes()).map_err(io(path))?;
        file.write_all(&body).map_err(io(path))?;
        file.flush().map_err(io(path))
    }
}

/// The payload of the first chunk `fourcc` in a RIFF/WEBP file.
fn find_chunk<'a>(file: &'a [u8], fourcc: &[u8; 4]) -> Option<&'a [u8]> {
    let mut pos = 12; // RIFF, size, WEBP
    while pos + 8 <= file.len() {
        let size = u32::from_le_bytes(file[pos + 4..pos + 8].try_into().ok()?) as usize;
        let data = file.get(pos + 8..pos + 8 + size)?;
        if &file[pos..pos + 4] == fourcc {
            return Some(data);
        }
        pos += 8 + size + size % 2;
    }
    None
}

/// Writes every frame as a numbered PNG into a ZIP archive.
pub struct PngSequenceWriter {
    zip: zip::ZipWriter<BufWriter<File>>,
    digits: usize,
    count: usize,
}

impl PngSequenceWriter {
    pub fn create(path: &Path, frames: usize) -> Result<Self, ExportError> {
        let file = BufWriter::new(File::create(path).map_err(io(path))?);
        Ok(Self { zip: zip::ZipWriter::new(file), digits: frames.to_string().len().max(4), count: 0 })
    }

    pub fn add(&mut self, image: &RgbaImage) -> Result<(), ExportError> {
        self.count += 1;
        let mut png = Cursor::new(Vec::new());
        image.write_to(&mut png, image::ImageFormat::Png).map_err(encode_err)?;
        let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        let name = format!("frame_{:0width$}.png", self.count, width = self.digits);
        self.zip.start_file(name, options).map_err(encode_err)?;
        self.zip.write_all(png.get_ref()).map_err(encode_err)
    }

    pub fn finish(self) -> Result<(), ExportError> {
        self.zip.finish().map(drop).map_err(encode_err)
    }
}
