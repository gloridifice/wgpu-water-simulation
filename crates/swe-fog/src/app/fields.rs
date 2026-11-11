//! Transactional construction of independently simulated bake volumes.
use crate::{
    Error,
    parameters::Session,
    render::Renderer,
    simulation::Simulation,
    terrain::{BakeSource, Terrain},
};
use common::gpu::Gpu;

pub(super) struct Field {
    pub simulation: Simulation,
    pub renderer: Renderer,
}
pub(super) type BuiltFields = (Terrain, Simulation, Renderer, Vec<Field>);
pub(super) fn build(
    gpu: &Gpu,
    native: &Terrain,
    source: &BakeSource,
    session: &Session,
    format: wgpu::TextureFormat,
) -> Result<BuiltFields, Error> {
    let terrains = crate::volumes::bake_fields(native, source, session)?;
    let mut fields = Vec::new();
    for terrain in &terrains {
        let simulation = Simulation::new(gpu, terrain)?;
        let renderer = Renderer::new(gpu, &simulation, terrain, source, format)?;
        fields.push(Field {
            simulation,
            renderer,
        });
    }
    let primary = fields.remove(0);
    Ok((
        terrains.into_iter().next().unwrap(),
        primary.simulation,
        primary.renderer,
        fields,
    ))
}
pub(super) fn renderers<'a>(
    primary: &'a Renderer,
    domain: [f32; 4],
    extra: &'a [Field],
) -> Vec<(&'a Renderer, [f32; 4])> {
    std::iter::once((primary, domain))
        .chain(extra.iter().map(|f| (&f.renderer, f.simulation.domain)))
        .collect()
}
