// Shared with raster.rs::Camera (208 bytes, all fields 16-byte aligned).
struct RasterCamera { view_projection:mat4x4f, inverse:mat4x4f, eye:vec4f, domain:vec4f, player:vec4f, params:vec4f, viewport:vec4f }
// World vertex xyz and nonnegative extinction-free density w.
struct TetData { p:array<vec4f,4> }
// Projected tetrahedron fan, linearly interpolated reciprocal front/back W,
// affine world density. 112 bytes per proxy, 64 bytes per world tetrahedron.
struct ProjectedTet { hull:array<vec4f,4>, center:vec4f, density:vec4f, flags:vec4u }
