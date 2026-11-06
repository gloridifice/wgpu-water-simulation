// AgX/allenwp SDR approximation adapted from Godot Engine 4.6 (MIT):
// https://github.com/godotengine/godot/blob/4.6/servers/rendering/renderer_rd/shaders/effects/tonemap.glsl
// https://github.com/godotengine/godot/blob/4.6/servers/rendering/storage/environment_storage.cpp
// AgX inset/outset derived there from EaryChow's Blender AgX implementation.
// allenwp curve: https://allenwp.com/blog/2025/05/29/allenwp-tonemapping-curve/
//
// Copyright (c) 2014-present Godot Engine contributors (see upstream AUTHORS.md).
// Copyright (c) 2007-2014 Juan Linietsky, Ariel Manzur.
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.

fn agx_curve(x: vec3<f32>) -> vec3<f32> {
    // Godot defaults: contrast=1.25, middle gray=0.18, SDR white=16.29.
    // toe_a=(1/middle-1)*pow(middle,contrast); slope=contrast*(1-middle);
    // shoulder_w=pow(white-middle,2)*slope/(1-middle).
    // Fixed parameters are precomputed, not evaluated per pixel or tuned to water.
    let toe_power = pow(x, vec3(1.25));
    let toe = toe_power / (toe_power + vec3(0.53411156));
    let shoulder_x = max(x - vec3(0.18), vec3(0.0));
    let shoulder = vec3(0.18) + 1.025 * shoulder_x
        * (vec3(1.0) + shoulder_x / 324.41513)
        / (vec3(1.0) + 1.025 * shoulder_x / 0.82);
    return select(shoulder, toe, x < vec3(0.18));
}

// Linear Rec.709/sRGB primaries in, display-linear Rec.709 out. The target
// attachment, NOT this function, performs the final sRGB transfer function.
fn agx_tonemap(color: vec3<f32>) -> vec3<f32> {
    // WGSL mat3 constructors, like GLSL, list COLUMNS. Mixing channels before
    // the curve is essential: applying a scalar S-curve to raw RGB is not AgX.
    const inset = mat3x3<f32>(
        vec3(0.544814746488245, 0.140416948464053, 0.0888104196149096),
        vec3(0.373787398372697, 0.754137554567394, 0.178871756420858),
        vec3(0.0813978551390581, 0.105445496968552, 0.732317823964232));
    const outset = mat3x3<f32>(
        vec3(1.96488741169489, -0.299313364904742, -0.164352742528393),
        vec3(-0.855988495690215, 1.32639796461980, -0.238183969428088),
        vec3(-0.108898916004672, -0.0270845997150571, 1.40253671195648));
    let stimulus = inset * max(color, vec3(0.0));
    // The curve is monotonic and reaches 1 at white. Limiting its input there
    // is equivalent to the upstream post-curve cap, and bounds intermediate math.
    let mapped = min(agx_curve(min(stimulus, vec3(16.29))), vec3(1.0));
    // No further color grading follows, so clip out-of-display-gamut residuals
    // here. Do not clip channels before inset or add scene-specific desaturation.
    return clamp(outset * mapped, vec3(0.0), vec3(1.0));
}
