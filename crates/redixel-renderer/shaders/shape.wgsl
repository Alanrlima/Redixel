// Redixel — Shape / Sprite Batch Shader
//
// Draws coloured triangles/quads from a per-vertex colour attribute. The
// shader itself is agnostic to which camera projection is bound — the caller
// binds the orthographic uniform before flushing 2D geometry and the
// perspective uniform before flushing 3D geometry, both against this same
// pipeline.
struct CameraUniforms {
    projection: mat4x4<f32>,
}

@group(0) @binding(0)
var<uniform> camera: CameraUniforms;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) color: vec4<f32>,
}

struct VertexOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) color: vec4<f32>,
}

@vertex
fn vs_main(in: VertexIn) -> VertexOut {
    var out: VertexOut;
    out.clip_pos = camera.projection * vec4<f32>(in.position, 1.0);
    out.color = in.color;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    return in.color;
}
