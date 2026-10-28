//! Headless verification entry point; checks remain with their owning modules.
use super::{
    Error,
    gpu::context::Gpu,
    render::{optics, textures::sand, verification},
    simulation::{nearshore, offshore, spray},
};

pub async fn run() -> Result<(), Error> {
    let gpu = Gpu::new(&wgpu::Instance::default(), None).await?;
    eprintln!("GPU verification: {}", gpu.name);
    offshore::verification::verify_fft(&gpu)?;
    sand::verify(&gpu)?;
    offshore::verification::verify_coupling(&gpu)?;
    spray::verify(&gpu)?;
    offshore::foam::verify(&gpu)?;
    nearshore::verify_terrain(&gpu)?;
    nearshore::verify(&gpu)?;
    optics::verify(&gpu)?;
    verification::optics::run(&gpu)?;
    verification::skylight::run(&gpu)?;
    Ok(())
}
