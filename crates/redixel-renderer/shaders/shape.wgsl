// Redixel — Shape / Sprite Batch Shader
//
// Draws triangles/quads from a per-vertex colour attribute, modulated by a
// texture sample. Agnostic to which camera projection is bound: the caller
// binds the orthographic uniform before flushing 2D geometry and the
// perspective uniform before 3D, both against this same pipeline.
//
// Equally agnostic to whether anything is actually textured. Untextured
// geometry is bound against a 1x1 opaque white texture, making the multiply an
// identity, which is what lets solid shapes and sprites share one pipeline and
// one painter order.
struct CameraUniforms {
    projection: mat4x4<f32>,
}

@group(0) @binding(0)
var<uniform> camera: CameraUniforms;

@group(1) @binding(0)
var sprite_texture: texture_2d<f32>;

@group(1) @binding(1)
var sprite_sampler: sampler;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) color: vec4<f32>,
    @location(2) uv: vec2<f32>,
}

struct VertexOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) uv: vec2<f32>,
}

@vertex
fn vs_main(in: VertexIn) -> VertexOut {
    var out: VertexOut;
    out.clip_pos = camera.projection * vec4<f32>(in.position, 1.0);
    out.color = in.color;
    out.uv = in.uv;
    return out;
}

// The texture is an `Rgba8UnormSrgb`, so the sample arrives linearised. No
// gamma correction belongs here: the sRGB surface encodes on write-out.
@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    return textureSample(sprite_texture, sprite_sampler, in.uv) * in.color;
}
