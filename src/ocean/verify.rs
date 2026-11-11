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
    super::simulation::animated_lake::verification::run(&gpu)?;
    verification::lake::run(&gpu)?;
    offshore::verification::verify_fft(&gpu)?;
    sand::verify(&gpu)?;
    offshore::verification::verify_coupling(&gpu)?;
    spray::verify(&gpu)?;
    offshore::foam::verify(&gpu)?;
    nearshore::verify_terrain(&gpu)?;
    nearshore::verify(&gpu)?;
    verification::post_process::run(&gpu)?;
    verification::optics::water_lighting(&gpu)?;
    optics::verify(&gpu)?;
    verification::optics::run(&gpu)?;
    verification::skylight::run(&gpu)?;
    Ok(())
}

pub async fn lake() -> Result<(), Error> {
    let gpu = Gpu::new(&wgpu::Instance::default(), None).await?;
    eprintln!("GPU lake verification: {}", gpu.name);
    super::simulation::animated_lake::verification::run(&gpu)?;
    verification::lake::run(&gpu)
}

/// Focused production raster checks for planar and model water lighting.
pub async fn water_lighting() -> Result<(), Error> {
    let gpu = Gpu::new(&wgpu::Instance::default(), None).await?;
    eprintln!("GPU water lighting: {}", gpu.name);
    verification::post_process::run(&gpu)?;
    verification::optics::water_lighting(&gpu)
}
