//! Optional detailed timestamps for the offline production-path profiler.
#[derive(Clone, Copy)]
pub struct RenderTiming<'a> {
    pub queries: &'a wgpu::QuerySet,
    pub offset: u32,
}
impl RenderTiming<'_> {
    pub const NAMES: [&str; 9] = [
        "sky_lut",
        "height_pyramid",
        "caustic_trace",
        "caustic_raster",
        "opaque_prepass",
        "screen_depth_pyramid",
        "sky_and_opaque_draw",
        "water_shading",
        "bounds_and_spray_draw",
    ];
    pub const QUERY_COUNT: u32 = Self::NAMES.len() as u32 + 1;
    pub const FEATURES: wgpu::Features = wgpu::Features::TIMESTAMP_QUERY
        .union(wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS)
        .union(wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES);
    pub(super) fn mark(self, encoder: &mut wgpu::CommandEncoder, boundary: u32) {
        encoder.write_timestamp(self.queries, self.offset + boundary);
    }
    pub(super) fn draw_mark(self, pass: &mut wgpu::RenderPass<'_>, boundary: u32) {
        pass.write_timestamp(self.queries, self.offset + boundary);
    }
}
