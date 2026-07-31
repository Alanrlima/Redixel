use wgpu::{Buffer, BufferDescriptor, BufferUsages, Device, IndexFormat, Queue, RenderPass};

use redixel_math::{Color, Vec2, Vec3};

use crate::pipeline::Vertex;

/// Index pattern for a rectangle, relative to its own first vertex.
///
/// Clockwise in world space, which the camera's y-flipping projection turns
/// into the `FrontFace::Ccw` the shape pipeline declares.
const RECT_INDICES: [u32; 6] = [0, 2, 1, 1, 2, 3];

/// Index pattern for a standalone triangle, relative to its own first vertex.
const TRIANGLE_INDICES: [u32; 3] = [0, 1, 2];

/// Builds the four corners of a rectangle in
/// `[top-left, top-right, bottom-left, bottom-right]` order, the order
/// [`RECT_INDICES`] expects.
fn rect_vertices(position: Vec2, size: Vec2, color: Color) -> [Vertex; 4] {
    let x0: f32 = position.x;
    let y0: f32 = position.y;
    let x1: f32 = position.x + size.x;
    let y1: f32 = position.y + size.y;
    let c: [f32; 4] = color.to_array();

    [
        Vertex {
            position: [x0, y0, 0.0],
            color: c,
        },
        Vertex {
            position: [x1, y0, 0.0],
            color: c,
        },
        Vertex {
            position: [x0, y1, 0.0],
            color: c,
        },
        Vertex {
            position: [x1, y1, 0.0],
            color: c,
        },
    ]
}

/// Builds the three corners of a triangle in the order they were given.
fn triangle_vertices(p1: Vec2, p2: Vec2, p3: Vec2, color: Color) -> [Vertex; 3] {
    let c: [f32; 4] = color.to_array();

    [
        Vertex {
            position: [p1.x, p1.y, 0.0],
            color: c,
        },
        Vertex {
            position: [p2.x, p2.y, 0.0],
            color: c,
        },
        Vertex {
            position: [p3.x, p3.y, 0.0],
            color: c,
        },
    ]
}

/// Builds the three corners of a 3D triangle in the order they were given.
///
/// Unlike `triangle_vertices`, the caller supplies a real z — this is the
/// only vertex-building path whose depth isn't hardcoded to zero.
fn triangle_vertices_3d(p1: Vec3, p2: Vec3, p3: Vec3, color: Color) -> [Vertex; 3] {
    let c: [f32; 4] = color.to_array();

    [
        Vertex {
            position: [p1.x, p1.y, p1.z],
            color: c,
        },
        Vertex {
            position: [p2.x, p2.y, p2.z],
            color: c,
        },
        Vertex {
            position: [p3.x, p3.y, p3.z],
            color: c,
        },
    ]
}

/// Geometry accumulated on the CPU during a frame.
///
/// Owns no GPU resources, so index rebasing can be exercised without a device.
#[derive(Default)]
struct Geometry {
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
}

impl Geometry {
    /// Appends `vertices` and the `indices` that reference them, rebasing each
    /// index onto the geometry already queued.
    ///
    /// Indices are given relative to the primitive's own first vertex, so a
    /// primitive never has to know how much geometry precedes it. This is the
    /// only place that offset is computed.
    fn push(&mut self, vertices: &[Vertex], indices: &[u32]) {
        let base: u32 = self.vertices.len() as u32;
        self.vertices.extend_from_slice(vertices);
        self.indices.extend(indices.iter().map(|i: &u32| base + i));
    }

    fn clear(&mut self) {
        self.vertices.clear();
        self.indices.clear();
    }
}

fn create_vertex_buffer(device: &Device, vertices: usize) -> Buffer {
    device.create_buffer(&BufferDescriptor {
        label: Some("REDIXEL_SPRITE_BATCH_VB"),
        size: (vertices * std::mem::size_of::<Vertex>()) as u64,
        usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn create_index_buffer(device: &Device, indices: usize) -> Buffer {
    device.create_buffer(&BufferDescriptor {
        label: Some("REDIXEL_SPRITE_BATCH_IB"),
        size: (indices * std::mem::size_of::<u32>()) as u64,
        usage: BufferUsages::INDEX | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// Accumulates `draw_rect` calls per frame and submits them to the GPU in a
/// single indexed draw call on `flush()`.
///
/// This is the standard 2D batch-rendering pattern: minimise draw calls by
/// grouping same-pipeline geometry together.
///
/// The batch imposes no ceiling on how much geometry a frame may queue, and
/// reserves nothing up front: its buffers start empty and grow to fit whatever
/// is actually drawn, so nothing is ever silently dropped.
pub struct SpriteBatch {
    vertex_buffer: Buffer,
    index_buffer: Buffer,
    vertex_capacity: usize,
    index_capacity: usize,
    geometry: Geometry,
}

impl SpriteBatch {
    /// Creates a batch with empty GPU buffers.
    ///
    /// The first `flush()` sizes them to the frame that is actually drawn, so
    /// a game that draws little pays for little.
    pub fn new(device: &Device) -> Self {
        Self {
            vertex_buffer: create_vertex_buffer(device, 0),
            index_buffer: create_index_buffer(device, 0),
            vertex_capacity: 0,
            index_capacity: 0,
            geometry: Geometry::default(),
        }
    }

    /// Queues a filled rectangle for drawing.
    ///
    /// - `position` — top-left corner in world coordinates (y-down)
    /// - `size`     — width × height in world units
    /// - `color`    — RGBA fill colour
    pub fn draw_rect(&mut self, position: Vec2, size: Vec2, color: Color) {
        self.geometry.push(&rect_vertices(position, size, color), &RECT_INDICES);
    }

    /// Queues a filled triangle for drawing.
    ///
    /// - `p1`, `p2`, `p3` — The three corners of the triangle in world coordinates
    /// - `color`          — RGBA fill colour
    pub fn draw_triangle(&mut self, p1: Vec2, p2: Vec2, p3: Vec2, color: Color) {
        self.geometry
            .push(&triangle_vertices(p1, p2, p3, color), &TRIANGLE_INDICES);
    }

    /// Queues a filled triangle in 3D view space.
    ///
    /// - `p1`, `p2`, `p3` — the three vertices, in the perspective camera's
    ///   view space (see [`redixel_math::Mat4::perspective`])
    /// - `color`          — RGBA fill colour
    pub fn draw_triangle_3d(&mut self, p1: Vec3, p2: Vec3, p3: Vec3, color: Color) {
        self.geometry
            .push(&triangle_vertices_3d(p1, p2, p3, color), &TRIANGLE_INDICES);
    }

    /// Returns the number of unique vertices currently queued — four per
    /// rectangle, three per triangle.
    ///
    /// Since the batch draws indexed, this is *not* the size of the draw call;
    /// see [`SpriteBatch::index_count`].
    pub fn unique_vertex_count(&self) -> usize {
        self.geometry.vertices.len()
    }

    /// Returns the number of indices currently queued, which is the vertex
    /// count the next `flush()` will submit.
    pub fn index_count(&self) -> usize {
        self.geometry.indices.len()
    }

    /// Uploads queued vertices and indices to the GPU and records the indexed
    /// draw call.
    ///
    /// Must be called **inside** an active `RenderPass`.
    /// Clears the internal queues after submission.
    pub fn flush(&mut self, device: &Device, queue: &Queue, pass: &mut RenderPass<'_>) {
        let count: usize = self.geometry.indices.len();
        if count == 0 {
            return;
        }

        self.reserve(device);

        queue.write_buffer(&self.vertex_buffer, 0, bytemuck::cast_slice(&self.geometry.vertices));
        queue.write_buffer(&self.index_buffer, 0, bytemuck::cast_slice(&self.geometry.indices));

        pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        pass.set_index_buffer(self.index_buffer.slice(..), IndexFormat::Uint32);
        pass.draw_indexed(0..count as u32, 0, 0..1);

        self.geometry.clear();
    }

    /// Reallocates the GPU buffers when the queued geometry outgrows them.
    ///
    /// Capacity climbs straight to the next power of two and never shrinks, so
    /// a scene whose sprite count oscillates settles after a handful of frames
    /// and stops reallocating.
    fn reserve(&mut self, device: &Device) {
        let vertices: usize = self.geometry.vertices.len();
        if vertices > self.vertex_capacity {
            self.vertex_capacity = vertices.next_power_of_two();
            self.vertex_buffer = create_vertex_buffer(device, self.vertex_capacity);
        }

        let indices: usize = self.geometry.indices.len();
        if indices > self.index_capacity {
            self.index_capacity = indices.next_power_of_two();
            self.index_buffer = create_index_buffer(device, self.index_capacity);
        }
    }
}

#[cfg(test)]
mod tests {
    use redixel_math::Mat4;

    use super::*;

    fn positions(vertices: &[Vertex]) -> Vec<[f32; 3]> {
        vertices.iter().map(|v: &Vertex| v.position).collect()
    }

    fn project(vertices: &[Vertex]) -> [Vertex; 4] {
        let projection: Mat4 = Mat4::orthographic(0.0, 800.0, 600.0, 0.0, -1.0, 1.0);

        std::array::from_fn(|i: usize| Vertex {
            position: [
                projection.cols[0][0] * vertices[i].position[0] + projection.cols[3][0],
                projection.cols[1][1] * vertices[i].position[1] + projection.cols[3][1],
                0.0,
            ],
            color: vertices[i].color,
        })
    }

    fn signed_area(vertices: &[Vertex], indices: &[u32], triangle: usize) -> f32 {
        let i: usize = triangle * 3;
        let a: [f32; 3] = vertices[indices[i] as usize].position;
        let b: [f32; 3] = vertices[indices[i + 1] as usize].position;
        let c: [f32; 3] = vertices[indices[i + 2] as usize].position;

        (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
    }

    #[test]
    fn push_leaves_indices_alone_on_empty_geometry() {
        let mut geometry: Geometry = Geometry::default();
        geometry.push(&rect_vertices(Vec2::ZERO, Vec2::ONE, Color::WHITE), &RECT_INDICES);

        assert_eq!(geometry.indices, RECT_INDICES.to_vec());
    }

    #[test]
    fn push_rebases_indices_onto_queued_vertices() {
        let mut geometry: Geometry = Geometry::default();
        geometry.push(&triangle_vertices(Vec2::ZERO, Vec2::X, Vec2::Y, Color::WHITE), &[0, 1, 2]);
        geometry.push(&triangle_vertices(Vec2::ONE, Vec2::X, Vec2::Y, Color::RED), &[0, 1, 2]);
        geometry.push(&triangle_vertices(Vec2::ZERO, Vec2::Y, Vec2::X, Color::BLUE), &[0, 1, 2]);

        assert_eq!(geometry.vertices.len(), 9);
        assert_eq!(geometry.indices, vec![0, 1, 2, 3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn rect_vertices_are_ordered_tl_tr_bl_br() {
        let vertices: [Vertex; 4] = rect_vertices(Vec2::new(10.0, 20.0), Vec2::new(3.0, 4.0), Color::WHITE);

        assert_eq!(
            positions(&vertices),
            vec![
                [10.0, 20.0, 0.0],
                [13.0, 20.0, 0.0],
                [10.0, 24.0, 0.0],
                [13.0, 24.0, 0.0]
            ]
        );
    }

    #[test]
    fn rect_index_pattern_is_pinned() {
        assert_eq!(RECT_INDICES, [0, 2, 1, 1, 2, 3]);
    }

    #[test]
    fn rect_triangles_wind_counter_clockwise_in_ndc() {
        let vertices: [Vertex; 4] = project(&rect_vertices(Vec2::ZERO, Vec2::ONE, Color::WHITE));

        let first: f32 = signed_area(&vertices, &RECT_INDICES, 0);
        let second: f32 = signed_area(&vertices, &RECT_INDICES, 1);

        assert!(
            first > 0.0,
            "first triangle is clockwise in NDC; the shape pipeline declares FrontFace::Ccw"
        );
        assert!(
            second > 0.0,
            "second triangle is clockwise in NDC; the shape pipeline declares FrontFace::Ccw"
        );
    }

    #[test]
    fn consecutive_rects_offset_their_base_vertex() {
        let mut geometry: Geometry = Geometry::default();
        geometry.push(&rect_vertices(Vec2::ZERO, Vec2::ONE, Color::WHITE), &RECT_INDICES);
        geometry.push(&rect_vertices(Vec2::ONE, Vec2::ONE, Color::RED), &RECT_INDICES);

        assert_eq!(geometry.vertices.len(), 8);
        assert_eq!(geometry.indices, vec![0, 2, 1, 1, 2, 3, 4, 6, 5, 5, 6, 7]);
    }

    #[test]
    fn triangle_pushes_three_sequential_indices() {
        let mut geometry: Geometry = Geometry::default();
        geometry.push(
            &triangle_vertices(Vec2::ZERO, Vec2::X, Vec2::Y, Color::WHITE),
            &TRIANGLE_INDICES,
        );

        assert_eq!(geometry.vertices.len(), 3);
        assert_eq!(geometry.indices, vec![0, 1, 2]);
    }

    #[test]
    fn mixing_rects_and_triangles_keeps_indices_in_range() {
        let mut geometry: Geometry = Geometry::default();
        geometry.push(
            &triangle_vertices(Vec2::ZERO, Vec2::X, Vec2::Y, Color::WHITE),
            &TRIANGLE_INDICES,
        );
        geometry.push(&rect_vertices(Vec2::ZERO, Vec2::ONE, Color::RED), &RECT_INDICES);
        geometry.push(&triangle_vertices(Vec2::ONE, Vec2::X, Vec2::Y, Color::BLUE), &TRIANGLE_INDICES);

        assert_eq!(geometry.vertices.len(), 10);
        assert_eq!(geometry.indices.len(), 12);

        let highest: u32 = *geometry.indices.iter().max().unwrap();
        assert!(highest < geometry.vertices.len() as u32);
    }

    #[test]
    fn geometry_has_no_ceiling() {
        const QUADS: usize = 50_000;

        let mut geometry: Geometry = Geometry::default();
        for _ in 0..QUADS {
            geometry.push(&rect_vertices(Vec2::ZERO, Vec2::ONE, Color::WHITE), &RECT_INDICES);
        }

        assert_eq!(geometry.vertices.len(), QUADS * 4);
        assert_eq!(geometry.indices.len(), QUADS * 6);

        let highest: u32 = *geometry.indices.iter().max().unwrap();
        assert_eq!(highest, geometry.vertices.len() as u32 - 1);
    }

    #[test]
    fn clear_resets_both_queues() {
        let mut geometry: Geometry = Geometry::default();
        geometry.push(&rect_vertices(Vec2::ZERO, Vec2::ONE, Color::WHITE), &RECT_INDICES);
        geometry.clear();

        assert!(geometry.vertices.is_empty());
        assert!(geometry.indices.is_empty());
    }

    #[test]
    fn triangle_vertices_3d_carries_real_depth() {
        let vertices: [Vertex; 3] = triangle_vertices_3d(
            Vec3::new(0.0, 1.0, 2.0),
            Vec3::new(-1.0, -1.0, 2.0),
            Vec3::new(1.0, -1.0, 2.0),
            Color::WHITE,
        );

        assert_eq!(positions(&vertices), vec![[0.0, 1.0, 2.0], [-1.0, -1.0, 2.0], [1.0, -1.0, 2.0]]);
    }

    #[test]
    fn triangle_3d_pushes_three_sequential_indices() {
        let mut geometry: Geometry = Geometry::default();
        geometry.push(
            &triangle_vertices_3d(Vec3::ZERO, Vec3::X, Vec3::Y, Color::WHITE),
            &TRIANGLE_INDICES,
        );

        assert_eq!(geometry.vertices.len(), 3);
        assert_eq!(geometry.indices, vec![0, 1, 2]);
    }
}
