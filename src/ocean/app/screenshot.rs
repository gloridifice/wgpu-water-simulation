//! Copy the live viewport on demand and encode its PNG off the window thread.
use crate::ocean::{Error, gpu::context::Gpu};
use std::{
    fs::{self, File},
    io::BufWriter,
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, TryRecvError},
    time::{SystemTime, UNIX_EPOCH},
};

pub(super) struct Screenshot(Receiver<Result<PathBuf, String>>);

impl Screenshot {
    pub(super) fn encode(
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        texture: &wgpu::Texture,
    ) -> Result<Self, Error> {
        let bgra = match texture.format().remove_srgb_suffix() {
            wgpu::TextureFormat::Rgba8Unorm => false,
            wgpu::TextureFormat::Bgra8Unorm => true,
            format => return Err(format!("Unsupported screenshot format: {format:?}").into()),
        };
        let size = texture.size();
        let row_bytes = size.width * 4;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let stride = row_bytes.div_ceil(align) * align;
        let staging = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("viewport screenshot readback"),
            size: u64::from(stride) * u64::from(size.height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let path = std::env::current_dir()?
            .join("screenshots")
            .join(format!("ocean-{stamp}.png"));
        let (map_tx, map_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        let worker_buffer = staging.clone();
        std::thread::Builder::new()
            .name("ocean-screenshot".into())
            .spawn(move || {
                let save = || -> Result<PathBuf, Error> {
                    map_rx.recv()??;
                    let mut pixels = Vec::with_capacity((row_bytes * size.height) as usize);
                    {
                        let data = worker_buffer.slice(..).get_mapped_range();
                        for row in data.chunks_exact(stride as usize) {
                            pixels.extend_from_slice(&row[..row_bytes as usize]);
                        }
                    }
                    worker_buffer.unmap();
                    if bgra {
                        for pixel in pixels.as_chunks_mut::<4>().0 {
                            pixel.swap(0, 2);
                        }
                    }
                    write_png(&path, size, &pixels)
                        .map_err(|e| format!("{}: {e}", path.display()))?;
                    Ok(path)
                };
                let _ = done_tx.send(save().map_err(|e| e.to_string()));
            })?;
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &staging,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(stride),
                    rows_per_image: Some(size.height),
                },
            },
            size,
        );
        encoder.map_buffer_on_submit(&staging, wgpu::MapMode::Read, .., move |result| {
            let _ = map_tx.send(result);
        });
        Ok(Self(done_rx))
    }

    pub(super) fn poll(&self, gpu: &Gpu) -> Option<Result<PathBuf, String>> {
        if let Err(error) = gpu.device.poll(wgpu::PollType::Poll) {
            return Some(Err(error.to_string()));
        }
        match self.0.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => None,
            Err(error) => Some(Err(error.to_string())),
        }
    }

    pub(super) fn finish(self, gpu: &Gpu) -> Result<PathBuf, Error> {
        gpu.device.poll(wgpu::PollType::wait_indefinitely())?;
        Ok(self.0.recv()??)
    }
}

fn write_png(path: &Path, size: wgpu::Extent3d, pixels: &[u8]) -> Result<(), Error> {
    // A failure creating the file must not remove a pre-existing screenshot.
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let file = File::create_new(path)?;
    let result = (|| -> Result<(), png::EncodingError> {
        let mut png = png::Encoder::new(BufWriter::new(file), size.width, size.height);
        png.set_color(png::ColorType::Rgba);
        png.set_depth(png::BitDepth::Eight);
        png.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
        let mut writer = png.write_header()?;
        writer.write_image_data(pixels)?;
        writer.finish()
    })();
    if result.is_err() {
        let _ = fs::remove_file(path);
    }
    Ok(result?)
}
