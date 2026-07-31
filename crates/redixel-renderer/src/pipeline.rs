use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayout, BindGroupLayoutDescriptor, BindGroupLayoutEntry,
    BindingResource, BindingType, BlendState, Buffer, BufferBindingType, BufferDescriptor, BufferUsages,
    ColorTargetState, ColorWrites, CompareFunction, DepthBiasState, DepthStencilState, Device, FragmentState,
    FrontFace, MultisampleState, PipelineLayout, PipelineLayoutDescriptor, PolygonMode, PrimitiveState,
    PrimitiveTopology, Queue, RenderPipeline, RenderPipelineDescriptor, ShaderModule, ShaderModuleDescriptor,
    ShaderSource, ShaderStages, StencilState, TextureFormat, VertexAttribute, VertexBufferLayout, VertexState,
    VertexStepMode,
};

use crate::device::DEPTH_FORMAT;

const SHADER_SRC: &str = include_str!("../shaders/shape.wgsl");

/// A single vertex in the shape batch: 3D position + RGBA colour.
///
/// 2D draw calls (`draw_rect`/`draw_triangle`) carry `z = 0.0` through; only
/// the 3D path (`draw_triangle_3d`) supplies a real z.
///
/// `repr(C)` + packed fields → safe to cast to `&[u8]` via `bytemuck`.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub color: [f32; 4],
}

impl Vertex {
    const ATTRIBUTES: [VertexAttribute; 2] = wgpu::vertex_attr_array![
        0 => Float32x3,
        1 => Float32x4,
    ];

    pub fn layout() -> VertexBufferLayout<'static> {
        VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as u64,
            step_mode: VertexStepMode::Vertex,
            attributes: &Self::ATTRIBUTES,
        }
    }
}

/// The uniform buffer fed to `group(0) binding(0)` in the shader.
/// Contains a column-major 4×4 orthographic projection matrix.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CameraUniform {
    pub projection: [[f32; 4]; 4],
}

impl CameraUniform {
    pub fn from_mat4(m: [[f32; 4]; 4]) -> Self {
        Self { projection: m }
    }
}

/// Owns the WGPU render pipeline and the two camera uniforms bound to it —
/// one orthographic (2D content), one perspective (3D content). Both share
/// one `BindGroupLayout`, since the binding shape is identical; only one is
/// bound at a time, immediately before the draw call that needs it.
pub struct ShapePipeline {
    pub pipeline: RenderPipeline,
    pub bind_group_layout: BindGroupLayout,
    pub camera_buffer: Buffer,
    pub camera_bind_group: BindGroup,
    pub camera_buffer_3d: Buffer,
    pub camera_bind_group_3d: BindGroup,
}

impl ShapePipeline {
    pub fn new(device: &Device, surface_format: TextureFormat) -> Self {
        let shader: ShaderModule = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("REDIXEL_SHAPE_SHADER"),
            source: ShaderSource::Wgsl(SHADER_SRC.into()),
        });

        let bind_group_layout: BindGroupLayout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("REDIXEL_CAMERA_BIND_GROUP_LAYOUT"),
            entries: &[BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::VERTEX,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let (camera_buffer, camera_bind_group): (Buffer, BindGroup) = Self::create_camera(
            device,
            &bind_group_layout,
            "REDIXEL_CAMERA_BUFFER_2D",
            "REDIXEL_CAMERA_BIND_GROUP_2D",
        );

        let (camera_buffer_3d, camera_bind_group_3d): (Buffer, BindGroup) = Self::create_camera(
            device,
            &bind_group_layout,
            "REDIXEL_CAMERA_BUFFER_3D",
            "REDIXEL_CAMERA_BIND_GROUP_3D",
        );

        let pipeline_layout: PipelineLayout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("REDIXEL_SHAPE_PIPELINE_LAYOUT"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            ..Default::default()
        });

        let pipeline: RenderPipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("REDIXEL_SHAPE_PIPELINE"),
            layout: Some(&pipeline_layout),
            vertex: VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[Some(Vertex::layout())],
                compilation_options: Default::default(),
            },
            fragment: Some(FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(ColorTargetState {
                    format: surface_format,
                    blend: Some(BlendState::ALPHA_BLENDING),
                    write_mask: ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: PrimitiveState {
                topology: PrimitiveTopology::TriangleList,
                front_face: FrontFace::Ccw,
                polygon_mode: PolygonMode::Fill,
                ..Default::default()
            },
            depth_stencil: Some(DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(CompareFunction::LessEqual),
                stencil: StencilState::default(),
                bias: DepthBiasState::default(),
            }),
            multisample: MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        Self {
            pipeline,
            bind_group_layout,
            camera_buffer,
            camera_bind_group,
            camera_buffer_3d,
            camera_bind_group_3d,
        }
    }

    /// Builds a camera uniform buffer and its bind group against `layout`.
    fn create_camera(
        device: &Device,
        layout: &BindGroupLayout,
        buffer_label: &str,
        group_label: &str,
    ) -> (Buffer, BindGroup) {
        let buffer: Buffer = device.create_buffer(&BufferDescriptor {
            label: Some(buffer_label),
            size: std::mem::size_of::<CameraUniform>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_group: BindGroup = device.create_bind_group(&BindGroupDescriptor {
            label: Some(group_label),
            layout,
            entries: &[BindGroupEntry {
                binding: 0,
                resource: BindingResource::Buffer(buffer.as_entire_buffer_binding()),
            }],
        });

        (buffer, bind_group)
    }

    /// Uploads the current orthographic matrix to the 2D camera uniform buffer.
    pub fn update_camera(&self, queue: &Queue, projection: [[f32; 4]; 4]) {
        let uniform: CameraUniform = CameraUniform::from_mat4(projection);
        queue.write_buffer(&self.camera_buffer, 0, bytemuck::bytes_of(&uniform));
    }

    /// Uploads the current perspective matrix to the 3D camera uniform buffer.
    pub fn update_camera_3d(&self, queue: &Queue, projection: [[f32; 4]; 4]) {
        let uniform: CameraUniform = CameraUniform::from_mat4(projection);
        queue.write_buffer(&self.camera_buffer_3d, 0, bytemuck::bytes_of(&uniform));
    }
}
