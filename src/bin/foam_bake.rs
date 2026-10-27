//! Offline, deterministic foam-mask to NSH baker. It never updates simulation settings.
use std::{
    error::Error,
    fs,
    io::Cursor,
    path::{Path, PathBuf},
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[derive(Clone, Copy, Debug, serde::Serialize)]
#[serde(rename_all = "lowercase")]
enum InputSpace {
    Linear,
    Srgb,
}

#[derive(Clone, Copy, Debug, serde::Serialize)]
struct BakeSettings {
    size: usize,
    soft_gamma: f32,
    height_gamma: f32,
    blur_radius: usize,
    /// Relative height per texture tile, used to scale the UV-space gradient.
    normal_height: f32,
}
impl Default for BakeSettings {
    fn default() -> Self {
        Self {
            size: 1024,
            soft_gamma: 1.0,
            height_gamma: 1.35,
            blur_radius: 2,
            normal_height: 0.004,
        }
    }
}
impl BakeSettings {
    fn validate(self) -> Result<()> {
        if !(32..=4096).contains(&self.size) || !self.size.is_power_of_two() {
            return Err("--size must be a power of two in 32..4096".into());
        }
        for (name, value, low, high) in [
            ("soft gamma", self.soft_gamma, 0.1, 8.0),
            ("height gamma", self.height_gamma, 0.1, 8.0),
            ("normal height", self.normal_height, 0.0, 0.1),
        ] {
            if !value.is_finite() || !(low..=high).contains(&value) {
                return Err(format!("{name} must be finite and in {low}..{high}").into());
            }
        }
        if self.blur_radius > 32 {
            return Err("--blur-radius must be in 0..32 pixels".into());
        }
        Ok(())
    }
}

struct Mask {
    width: usize,
    height: usize,
    values: Vec<f32>,
}
struct Baked {
    size: usize,
    rgba: Vec<u8>,
}
struct Job {
    input: PathBuf,
    output: PathBuf,
    space: InputSpace,
}
struct Options {
    settings: BakeSettings,
    jobs: Vec<Job>,
    preview: Option<PathBuf>,
}

const HELP: &str = "NSH foam baker (offline)\n\
Usage: cargo run --features ocean --bin foam-bake -- [OPTIONS]\n\
With no --input, bake the bundled Shorewaves and two pinned Crest masks.\n\
  --input PATH          Custom PNG; first channel is the foam mask\n\
  --output PATH         Custom RGBA PNG output (required with --input)\n\
  --input-space SPACE   linear or srgb (default linear for custom input)\n\
  --size N              Square output size, power of two 32..4096 (1024)\n\
  --soft-gamma X        Soft curve exponent, 0.1..8 (1)\n\
  --height-gamma X      Designed height curve exponent, 0.1..8 (1.35)\n\
  --blur-radius N       Periodic height blur radius in output pixels, 0..32 (2)\n\
  --normal-height X     Height/tile-size ratio for the UV gradient, 0..0.1 (0.004)\n\
  --preview-dir PATH    Preview PNG directory (target/foam-bake-preview)\n\
  --no-preview          Skip preview images\n\
  --help                Print this help\n\
Output: linear RGBA8 PNG (RG signed tangent normal encoded to 0..1, B Soft, A Height),\n\
plus .recipe.json. Height is a designed shading field, not measured water depth.\n\
All interpolation, blur and derivatives wrap periodically. Inputs may be rectangular.\n\
Default source/output paths are relative to the repository root.\n";

fn options() -> Result<Option<Options>> {
    let mut settings = BakeSettings::default();
    let mut input = None;
    let mut output = None;
    let mut input_space = None;
    let mut preview = Some(PathBuf::from("target/foam-bake-preview"));
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--help" || arg == "-h" {
            print!("{HELP}");
            return Ok(None);
        }
        if arg == "--no-preview" {
            preview = None;
            continue;
        }
        let value = args
            .next()
            .ok_or_else(|| format!("missing value for {arg}"))?;
        match arg.as_str() {
            "--input" => input = Some(PathBuf::from(value)),
            "--output" => output = Some(PathBuf::from(value)),
            "--input-space" => {
                input_space = Some(match value.as_str() {
                    "linear" => InputSpace::Linear,
                    "srgb" => InputSpace::Srgb,
                    _ => return Err("--input-space must be linear or srgb".into()),
                })
            }
            "--size" => settings.size = value.parse()?,
            "--soft-gamma" => settings.soft_gamma = value.parse()?,
            "--height-gamma" => settings.height_gamma = value.parse()?,
            "--blur-radius" => settings.blur_radius = value.parse()?,
            "--normal-height" => settings.normal_height = value.parse()?,
            "--preview-dir" => preview = Some(PathBuf::from(value)),
            _ => return Err(format!("unknown option: {arg}").into()),
        }
    }
    settings.validate()?;
    let jobs = match (input, output) {
        (Some(input), Some(output)) => vec![Job {
            input,
            output,
            space: input_space.unwrap_or(InputSpace::Linear),
        }],
        (None, None) => {
            if input_space.is_some() {
                return Err("--input-space requires --input".into());
            }
            [
                (
                    "assets/shorewaves/voronoi_lace.png",
                    "shorewaves_nsh.png",
                    InputSpace::Linear,
                ),
                (
                    "assets/foam/crest/foam.png",
                    "crest_foam_nsh.png",
                    InputSpace::Srgb,
                ),
                (
                    "assets/foam/crest/Foam2.png",
                    "crest_foam2_nsh.png",
                    InputSpace::Srgb,
                ),
            ]
            .into_iter()
            .map(|(src, dst, space)| Job {
                input: src.into(),
                output: Path::new("assets/foam/baked").join(dst),
                space,
            })
            .collect()
        }
        _ => return Err("--input and --output must be used together".into()),
    };
    for job in &jobs {
        if job.input == job.output
            || (job.output.exists()
                && fs::canonicalize(&job.input)? == fs::canonicalize(&job.output)?)
        {
            return Err("input and output must be different files".into());
        }
        if job.output.extension().and_then(|s| s.to_str()) != Some("png") {
            return Err("--output must have a .png extension".into());
        }
    }
    Ok(Some(Options {
        settings,
        jobs,
        preview,
    }))
}

fn decode(value: f32, space: InputSpace) -> f32 {
    match space {
        InputSpace::Linear => value,
        InputSpace::Srgb if value <= 0.04045 => value / 12.92,
        InputSpace::Srgb => ((value + 0.055) / 1.055).powf(2.4),
    }
}
fn read_mask(bytes: &[u8], space: InputSpace) -> Result<Mask> {
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info()?;
    let info = reader.info();
    if info.width == 0 || info.height == 0 || info.width > 8192 || info.height > 8192 {
        return Err("input dimensions must be in 1..8192".into());
    }
    let mut pixels = vec![0; reader.output_buffer_size().ok_or("PNG output too large")?];
    let info = reader.next_frame(&mut pixels)?;
    if info.bit_depth != png::BitDepth::Eight {
        return Err("failed to decode PNG as 8-bit samples".into());
    }
    let channels = info.color_type.samples();
    let values = pixels[..info.buffer_size()]
        .chunks_exact(channels)
        .map(|p| decode(f32::from(p[0]) / 255.0, space))
        .collect();
    Ok(Mask {
        width: info.width as usize,
        height: info.height as usize,
        values,
    })
}
fn at(values: &[f32], width: usize, height: usize, x: isize, y: isize) -> f32 {
    values[y.rem_euclid(height as isize) as usize * width + x.rem_euclid(width as isize) as usize]
}
fn resample_value(from: usize, to: usize, index: usize, get: impl Fn(usize) -> f32) -> f32 {
    if from > to {
        // Area filtering avoids turning fine 4K masks into aliased 1K stipple.
        let start = index as f64 * from as f64 / to as f64;
        let end = (index + 1) as f64 * from as f64 / to as f64;
        let mut sum = 0.0;
        for i in start.floor() as usize..end.ceil() as usize {
            let weight = (end.min((i + 1) as f64) - start.max(i as f64)).max(0.0);
            sum += f64::from(get(i.min(from - 1))) * weight;
        }
        return (sum / (end - start)) as f32;
    }
    let p = (index as f32 + 0.5) * from as f32 / to as f32 - 0.5;
    let i = p.floor() as isize;
    let f = p - p.floor();
    let a = get(i.rem_euclid(from as isize) as usize);
    let b = get((i + 1).rem_euclid(from as isize) as usize);
    a + (b - a) * f
}
fn resample(mask: &Mask, size: usize) -> Vec<f32> {
    let mut horizontal = vec![0.0; size * mask.height];
    for y in 0..mask.height {
        for x in 0..size {
            horizontal[y * size + x] =
                resample_value(mask.width, size, x, |i| mask.values[y * mask.width + i]);
        }
    }
    let mut output = vec![0.0; size * size];
    for y in 0..size {
        for x in 0..size {
            output[y * size + x] =
                resample_value(mask.height, size, y, |i| horizontal[i * size + x]);
        }
    }
    output
}
fn periodic_blur(values: &[f32], size: usize, radius: usize) -> Vec<f32> {
    if radius == 0 {
        return values.to_vec();
    }
    let mut horizontal = vec![0.0; values.len()];
    let mut result = vec![0.0; values.len()];
    let r = radius as isize;
    let divisor = (2 * radius + 1) as f32;
    for y in 0..size {
        for x in 0..size {
            horizontal[y * size + x] = (-r..=r)
                .map(|d| at(values, size, size, x as isize + d, y as isize))
                .sum::<f32>()
                / divisor;
        }
    }
    for y in 0..size {
        for x in 0..size {
            result[y * size + x] = (-r..=r)
                .map(|d| at(&horizontal, size, size, x as isize, y as isize + d))
                .sum::<f32>()
                / divisor;
        }
    }
    result
}
fn normal(height: &[f32], size: usize, x: usize, y: usize, strength: f32) -> [f32; 3] {
    let x = x as isize;
    let y = y as isize;
    let dx = (at(height, size, size, x + 1, y) - at(height, size, size, x - 1, y))
        * 0.5
        * size as f32
        * strength;
    let dy = (at(height, size, size, x, y + 1) - at(height, size, size, x, y - 1))
        * 0.5
        * size as f32
        * strength;
    let length = (dx * dx + dy * dy + 1.0).sqrt();
    [-dx / length, -dy / length, 1.0 / length]
}
fn byte(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}
fn bake(mask: &Mask, settings: BakeSettings) -> Baked {
    let size = settings.size;
    let soft: Vec<_> = resample(mask, size)
        .into_iter()
        .map(|v| v.clamp(0.0, 1.0).powf(settings.soft_gamma))
        .collect();
    let shape: Vec<_> = soft.iter().map(|v| v.powf(settings.height_gamma)).collect();
    let height = periodic_blur(&shape, size, settings.blur_radius);
    let mut rgba = Vec::with_capacity(size * size * 4);
    for y in 0..size {
        for x in 0..size {
            let n = normal(&height, size, x, y, settings.normal_height);
            let i = y * size + x;
            rgba.extend_from_slice(&[
                byte(n[0] * 0.5 + 0.5),
                byte(n[1] * 0.5 + 0.5),
                byte(soft[i]),
                byte(height[i]),
            ]);
        }
    }
    Baked { size, rgba }
}
fn encode_png(width: usize, height: usize, pixels: &[u8]) -> Result<Vec<u8>> {
    if pixels.len() != width * height * 4 {
        return Err("RGBA pixel count mismatch".into());
    }
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, width as u32, height as u32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        // No sRGB/gAMA chunk: these are four independent linear data channels.
        let mut writer = encoder.write_header()?;
        writer.write_image_data(pixels)?;
    }
    Ok(bytes)
}
fn save_png(path: &Path, width: usize, height: usize, pixels: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, encode_png(width, height, pixels)?)?;
    Ok(())
}
fn smoothstep(low: f32, high: f32, value: f32) -> f32 {
    let t = ((value - low) / (high - low)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
fn coverage(soft: f32, amount: f32) -> f32 {
    if amount <= 0.0 {
        return 0.0;
    }
    if amount >= 1.0 {
        return 1.0;
    }
    let threshold = 1.05 - 1.1 * amount;
    smoothstep(threshold - 0.05, threshold + 0.05, soft)
}
fn preview(baked: &Baked, dir: &Path, stem: &str) -> Result<()> {
    // Eight labeled-by-position 256px panels, in two rows. Avoid preview files in assets.
    let tile = 256;
    let width = tile * 4;
    let height = tile * 2;
    let mut pixels = vec![255; width * height * 4];
    for panel in 0..8 {
        for y in 0..tile {
            for x in 0..tile {
                let repeat = if panel == 7 { 3 } else { 1 };
                let sx = x * baked.size * repeat / tile % baked.size;
                let sy = y * baked.size * repeat / tile % baked.size;
                let p = &baked.rgba[(sy * baked.size + sx) * 4..][..4];
                let rgb = match panel {
                    0 => [p[2]; 3],
                    1 => [p[3]; 3],
                    2 => {
                        let nx = f32::from(p[0]) / 255.0 * 2.0 - 1.0;
                        let ny = f32::from(p[1]) / 255.0 * 2.0 - 1.0;
                        [
                            p[0],
                            p[1],
                            byte((1.0 - nx * nx - ny * ny).max(0.0).sqrt() * 0.5 + 0.5),
                        ]
                    }
                    3 => [p[0], p[1], p[2]],
                    4..=6 => {
                        [byte(coverage(
                            f32::from(p[2]) / 255.0,
                            [0.2, 0.5, 0.8][panel - 4],
                        )); 3]
                    }
                    _ => [p[2]; 3],
                };
                let i = ((panel / 4 * tile + y) * width + panel % 4 * tile + x) * 4;
                pixels[i..i + 3].copy_from_slice(&rgb);
            }
        }
    }
    save_png(
        &dir.join(format!("{stem}_preview.png")),
        width,
        height,
        &pixels,
    )?;
    fs::create_dir_all(dir)?;
    fs::write(
        dir.join("README.txt"),
        "Preview panel order (left to right):\nTop: Soft, Height, decoded normal RGB, packed RGB (alpha omitted for inspection).\nBottom: coverage at display density 0.2, 0.5, 0.8, Soft tiled 3x3.\nPreview coverage is an illustrative curve, not a claim of identical runtime shading.\nAll preview outputs are opaque.\n",
    )?;
    Ok(())
}
fn fingerprint(bytes: &[u8]) -> String {
    let hash = bytes.iter().fold(0xcbf29ce484222325u64, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
    });
    format!("{hash:016x}")
}
fn run() -> Result<()> {
    let Some(options) = options()? else {
        return Ok(());
    };
    for job in options.jobs {
        let bytes = fs::read(&job.input)?;
        let mask = read_mask(&bytes, job.space)?;
        let baked = bake(&mask, options.settings);
        save_png(&job.output, baked.size, baked.size, &baked.rgba)?;
        let recipe = serde_json::json!({
            "algorithm": "periodic-mask-nsh-v1", "input": job.input.to_string_lossy().replace('\\', "/"),
            "input_fnv1a64": fingerprint(&bytes), "input_space": job.space,
            "source_size": [mask.width, mask.height], "settings": options.settings,
            "output": job.output.to_string_lossy().replace('\\', "/"),
            "channels": {"R": "tangent normal X encoded 0..1", "G": "tangent normal Y encoded 0..1; positive along increasing texture V", "B": "linear Soft", "A": "designed relative Height, not physical meters"},
            "normal_convention": "normal = normalize(-dH/du * normal_height, -dH/dv * normal_height, 1)",
            "output_color_space": "linear data; no gamma or alpha premultiplication",
            "height_model": "periodic box blur of Soft raised to height_gamma; artistic approximation"
        });
        fs::write(
            job.output.with_extension("recipe.json"),
            serde_json::to_string_pretty(&recipe)? + "\n",
        )?;
        if let Some(dir) = &options.preview {
            let stem = job
                .output
                .file_stem()
                .and_then(|s| s.to_str())
                .ok_or("invalid output filename")?;
            preview(&baked, dir, stem)?;
        }
        println!(
            "{} ({}x{}, {:?}) -> {} ({}x{} RGBA8 linear)",
            job.input.display(),
            mask.width,
            mask.height,
            job.space,
            job.output.display(),
            baked.size,
            baked.size
        );
    }
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("foam-bake: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn downsampling_preserves_subpixel_coverage() {
        let mask = Mask {
            width: 96,
            height: 96,
            values: (0..96 * 96)
                .map(|i| if (i % 96) % 3 == 0 { 1.0 } else { 0.0 })
                .collect(),
        };
        assert!(
            resample(&mask, 32)
                .iter()
                .all(|v| (v - 1.0 / 3.0).abs() < 1e-6)
        );
    }
    #[test]
    fn flat_height_has_upward_normal_and_preserves_height_alpha() {
        let mask = Mask {
            width: 4,
            height: 4,
            values: vec![0.25; 16],
        };
        let settings = BakeSettings {
            size: 32,
            ..Default::default()
        };
        let result = bake(&mask, settings);
        for p in result.rgba.as_chunks::<4>().0 {
            assert_eq!(&p[..3], &[128, 128, 64]);
            assert_eq!(p[3], byte(0.25f32.powf(settings.height_gamma)));
        }
        assert_eq!(result.rgba, bake(&mask, settings).rgba);
    }
    #[test]
    fn gradient_wraps_across_boundary_instead_of_clamping() {
        let size = 64;
        let values: Vec<_> = (0..size * size)
            .map(|i| 0.5 + 0.25 * (std::f32::consts::TAU * (i % size) as f32 / size as f32).sin())
            .collect();
        let n = normal(&values, size, 0, 0, 0.1);
        let derivative = 0.25 * (std::f32::consts::TAU / size as f32).sin() * size as f32 * 0.1;
        assert!((n[0] + derivative / (1.0 + derivative * derivative).sqrt()).abs() < 1e-6);
        assert!(n.iter().all(|v| v.is_finite()));
        assert!(n[1].abs() < 1e-6);
    }
    #[test]
    fn periodic_blur_preserves_mean_and_wraps_impulse() {
        let mut values = vec![0.0; 64];
        values[0] = 1.0;
        let blur = periodic_blur(&values, 8, 1);
        assert!((blur.iter().sum::<f32>() - 1.0).abs() < 1e-6);
        assert_eq!(blur[7], blur[1]);
        assert_eq!(blur[56], blur[8]);
    }
    #[test]
    fn rgba_png_roundtrip_keeps_alpha_as_data_and_no_srgb_tag() {
        let rgba = [128, 128, 25, 10, 200, 50, 100, 240];
        let bytes = encode_png(2, 1, &rgba).unwrap();
        let mut reader = png::Decoder::new(Cursor::new(bytes)).read_info().unwrap();
        assert!(reader.info().srgb.is_none());
        let mut actual = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut actual).unwrap();
        assert_eq!(info.color_type, png::ColorType::Rgba);
        assert_eq!(actual, rgba);
    }
    #[test]
    fn srgb_decode_and_zero_full_coverage_are_explicit() {
        assert!((decode(0.5, InputSpace::Srgb) - 0.21404114).abs() < 1e-6);
        for i in 0..256 {
            let v = i as f32 / 255.0;
            assert_eq!(coverage(v, 0.0), 0.0);
            assert_eq!(coverage(v, 1.0), 1.0);
        }
        assert!(
            BakeSettings {
                normal_height: f32::NAN,
                ..Default::default()
            }
            .validate()
            .is_err()
        );
    }
}
