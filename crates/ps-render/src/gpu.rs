//! The GPU renderer: triangle meshes in millimetres, drawn with a view of pixels per
//! millimetre, to an offscreen image or, in the app, into egui's own render pass.
//!
//! A [`Scene`] holds draws (a mesh, a transform and a colour); [`GpuContext::render`] draws it to
//! an RGBA image and reads the pixels back, which is how the renderer is tested. Fills come from
//! lyon meshes (`crate::mesh`), and so do strokes, with dashes cut on the CPU.

use std::borrow::Cow;

use ps_geometry::Affine;
use wgpu::util::DeviceExt;

use crate::mesh::Mesh;
use crate::viewport::Viewport;

/// The format offscreen images are drawn in: exact bytes, no gamma curve.
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
/// Row pitch alignment for texture-to-buffer copies.
const ROW_ALIGN: u32 = 256;

const SHADER: &str = r"
struct Uniforms {
    m: vec4<f32>,      // the linear part of the transform: m00, m10, m01, m11
    t: vec4<f32>,      // the translation, then the view centre
    view: vec4<f32>,   // pixels per millimetre, width, height
    colour: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u: Uniforms;

@vertex
fn vs_main(@location(0) p: vec2<f32>) -> @builtin(position) vec4<f32> {
    let world = vec2<f32>(u.m.x * p.x + u.m.z * p.y + u.t.x, u.m.y * p.x + u.m.w * p.y + u.t.y);
    let px = (world - vec2<f32>(u.t.z, u.t.w)) * u.view.x + vec2<f32>(u.view.y, u.view.z) * 0.5;
    return vec4<f32>(px.x / u.view.y * 2.0 - 1.0, 1.0 - px.y / u.view.z * 2.0, 0.0, 1.0);
}

@fragment
fn fs_main() -> @location(0) vec4<f32> {
    return u.colour;
}
";

const LINE_SHADER: &str = r"
struct Uniforms {
    m: vec4<f32>,      // m.x is the line width in millimetres
    t: vec4<f32>,      // unused, then the view centre
    view: vec4<f32>,   // pixels per millimetre, width, height
    colour: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u: Uniforms;

struct LineOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) px: vec2<f32>,
    @location(1) a: vec2<f32>,
    @location(2) b: vec2<f32>,
    @location(3) hw: f32,
};

fn to_px(world: vec2<f32>) -> vec2<f32> {
    return (world - vec2<f32>(u.t.z, u.t.w)) * u.view.x + vec2<f32>(u.view.y, u.view.z) * 0.5;
}

@vertex
fn vs_line(@builtin(vertex_index) vi: u32, @location(0) seg: vec4<f32>) -> LineOut {
    let a = to_px(seg.xy);
    let b = to_px(seg.zw);
    let hw = max(u.m.x * u.view.x * 0.5, 0.5);
    let ext = hw + 1.0;
    var dir = vec2<f32>(1.0, 0.0);
    let span = b - a;
    if (length(span) > 0.0001) {
        dir = normalize(span);
    }
    let nrm = vec2<f32>(-dir.y, dir.x);
    var side = -1.0;
    if ((vi & 1u) == 1u) {
        side = 1.0;
    }
    var end = a - dir * ext;
    if (vi >= 2u) {
        end = b + dir * ext;
    }
    let p = end + nrm * ext * side;
    var o: LineOut;
    o.pos = vec4<f32>(p.x / u.view.y * 2.0 - 1.0, 1.0 - p.y / u.view.z * 2.0, 0.0, 1.0);
    o.px = p;
    o.a = a;
    o.b = b;
    o.hw = hw;
    return o;
}

@fragment
fn fs_line(i: LineOut) -> @location(0) vec4<f32> {
    let ab = i.b - i.a;
    let len2 = dot(ab, ab);
    var t = 0.0;
    if (len2 > 0.0) {
        t = clamp(dot(i.px - i.a, ab) / len2, 0.0, 1.0);
    }
    let d = length(i.px - (i.a + ab * t));
    let cover = clamp(0.5 + i.hw - d, 0.0, 1.0);
    return vec4<f32>(u.colour.rgb, u.colour.a * cover);
}
";

/// Why a graphics device could not be opened.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GpuError {
    /// No graphics adapter, not even a software one.
    #[error("no graphics adapter is available")]
    NoAdapter,
    /// The adapter refused to open.
    #[error("the graphics device could not be opened: {0}")]
    Device(String),
}

/// A rendered picture: RGBA bytes, row by row from the top.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Four bytes per pixel.
    pub rgba: Vec<u8>,
}

/// What one draw puts on the target.
enum Shape {
    /// An indexed triangle mesh.
    Mesh {
        vertices: Vec<[f32; 2]>,
        indices: Vec<u32>,
        transform: Affine,
    },
    /// Instanced line segments with round ends: `(x0, y0, x1, y1)` in millimetres.
    Lines {
        segments: Vec<[f32; 4]>,
        width_mm: f32,
    },
}

struct Draw {
    shape: Shape,
    colour: [f32; 4],
}

/// What to draw: a view, a background colour and the meshes in order.
pub struct Scene {
    viewport: Viewport,
    clear: [f32; 4],
    draws: Vec<Draw>,
    srgb: bool,
}

impl std::fmt::Debug for Scene {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Scene")
            .field("draws", &self.draws.len())
            .finish_non_exhaustive()
    }
}

impl Scene {
    /// An empty scene seen through `viewport`, cleared to `clear` (straight RGBA).
    #[must_use]
    pub fn new(viewport: Viewport, clear: [f32; 4]) -> Self {
        Self {
            viewport,
            clear,
            draws: Vec::new(),
            srgb: false,
        }
    }

    /// Makes colours given in sRGB come out right on a target that applies the sRGB curve when
    /// it is written (an `*Srgb` surface format): they are converted to linear light first.
    #[must_use]
    pub fn for_srgb_target(mut self) -> Self {
        self.srgb = true;
        self.clear = to_linear(self.clear);
        self
    }

    /// How many draws the scene holds, for checking that something was or was not drawn.
    #[must_use]
    pub fn draw_count(&self) -> usize {
        self.draws.len()
    }

    /// Adds a mesh drawn with `transform` (mesh space to millimetres on the mat) in `colour`.
    /// Later draws cover earlier ones, and a colour with alpha blends over what is below.
    pub fn fill(&mut self, mesh: &Mesh, transform: Affine, colour: [f32; 4]) {
        if mesh.indices.is_empty() {
            return;
        }
        self.draws.push(Draw {
            shape: Shape::Mesh {
                vertices: mesh.vertices.clone(),
                indices: mesh.indices.clone(),
                transform,
            },
            colour: if self.srgb { to_linear(colour) } else { colour },
        });
    }

    /// Adds a batch of line segments, `width_mm` wide with round ends, drawn by the line shader:
    /// one instanced quad per segment, with its edges and ends shaped by a distance function so
    /// they stay smooth at any zoom. A line is never drawn thinner than a pixel.
    pub fn lines(&mut self, segments: &[[f32; 4]], width_mm: f32, colour: [f32; 4]) {
        if segments.is_empty() {
            return;
        }
        self.draws.push(Draw {
            shape: Shape::Lines {
                segments: segments.to_vec(),
                width_mm,
            },
            colour: if self.srgb { to_linear(colour) } else { colour },
        });
    }

    /// How many draws the scene holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.draws.len()
    }

    /// Whether the scene holds no draws.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.draws.is_empty()
    }
}

/// An sRGB-encoded channel as linear light.
#[must_use]
pub fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.040_45 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn to_linear(c: [f32; 4]) -> [f32; 4] {
    [
        srgb_to_linear(c[0]),
        srgb_to_linear(c[1]),
        srgb_to_linear(c[2]),
        c[3],
    ]
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "the GPU works in f32; millimetre and pixel values fit with room to spare"
)]
fn f32_of(v: f64) -> f32 {
    v as f32
}

fn uniforms(draw: &Draw, viewport: &Viewport, width: u32, height: u32) -> [f32; 16] {
    let [m00, m10, m01, m11, tx, ty] = match &draw.shape {
        Shape::Mesh { transform, .. } => transform.as_coeffs(),
        Shape::Lines { width_mm, .. } => [f64::from(*width_mm), 0.0, 0.0, 0.0, 0.0, 0.0],
    };
    let centre = viewport.center();
    [
        f32_of(m00),
        f32_of(m10),
        f32_of(m01),
        f32_of(m11),
        f32_of(tx),
        f32_of(ty),
        f32_of(centre.x),
        f32_of(centre.y),
        f32_of(viewport.zoom()),
        f32::from(u16::try_from(width).unwrap_or(u16::MAX)),
        f32::from(u16::try_from(height).unwrap_or(u16::MAX)),
        0.0,
        draw.colour[0],
        draw.colour[1],
        draw.colour[2],
        draw.colour[3],
    ]
}

/// What differs between the pipelines: the shader entry points, the vertex layout and the shape.
struct PipelineSpec<'a> {
    label: &'a str,
    module: &'a wgpu::ShaderModule,
    vertex: &'a str,
    fragment: &'a str,
    buffer: wgpu::VertexBufferLayout<'a>,
    topology: wgpu::PrimitiveTopology,
}

fn build_pipeline(
    device: &wgpu::Device,
    spec: &PipelineSpec<'_>,
    layout: &wgpu::PipelineLayout,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(spec.label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: spec.module,
            entry_point: Some(spec.vertex),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: std::slice::from_ref(&spec.buffer),
        },
        fragment: Some(wgpu::FragmentState {
            module: spec.module,
            entry_point: Some(spec.fragment),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            topology: spec.topology,
            ..wgpu::PrimitiveState::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    })
}

/// A graphics device with the mesh pipeline built.
pub struct GpuContext {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    line_pipeline: wgpu::RenderPipeline,
    bind_layout: wgpu::BindGroupLayout,
}

impl std::fmt::Debug for GpuContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GpuContext").finish_non_exhaustive()
    }
}

impl GpuContext {
    /// Opens a device with no window: the best adapter, or a software one if there is none.
    ///
    /// # Errors
    ///
    /// [`GpuError`] when no adapter can be opened.
    pub fn headless() -> Result<Self, GpuError> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let adapter = [false, true]
            .into_iter()
            .find_map(|force_fallback_adapter| {
                pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::default(),
                    force_fallback_adapter,
                    compatible_surface: None,
                }))
                .ok()
            })
            .ok_or(GpuError::NoAdapter)?;
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
                .map_err(|e| GpuError::Device(e.to_string()))?;
        Ok(Self::from_device(device, queue, FORMAT))
    }

    /// Builds the pipeline on an existing device (the app's own) for a target `format`.
    #[must_use]
    pub fn from_device(
        device: wgpu::Device,
        queue: wgpu::Queue,
        format: wgpu::TextureFormat,
    ) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ps-render mesh shader"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(SHADER)),
        });
        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ps-render uniforms"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ps-render pipeline layout"),
            bind_group_layouts: &[&bind_layout],
            push_constant_ranges: &[],
        });
        let pipeline = build_pipeline(
            &device,
            &PipelineSpec {
                label: "ps-render meshes",
                module: &module,
                vertex: "vs_main",
                fragment: "fs_main",
                buffer: wgpu::VertexBufferLayout {
                    array_stride: 8,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x2,
                        offset: 0,
                        shader_location: 0,
                    }],
                },
                topology: wgpu::PrimitiveTopology::TriangleList,
            },
            &layout,
            format,
        );
        let line_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ps-render line shader"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(LINE_SHADER)),
        });
        let line_pipeline = build_pipeline(
            &device,
            &PipelineSpec {
                label: "ps-render lines",
                module: &line_module,
                vertex: "vs_line",
                fragment: "fs_line",
                buffer: wgpu::VertexBufferLayout {
                    array_stride: 16,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &[wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x4,
                        offset: 0,
                        shader_location: 0,
                    }],
                },
                topology: wgpu::PrimitiveTopology::TriangleStrip,
            },
            &layout,
            format,
        );
        Self {
            device,
            queue,
            pipeline,
            line_pipeline,
            bind_layout,
        }
    }

    /// Draws a scene into a render pass the caller owns (the app's, for the canvas).
    pub fn draw_into(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        scene: &Scene,
        width: u32,
        height: u32,
    ) {
        for draw in &scene.draws {
            let uniform = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("ps-render draw uniforms"),
                    contents: bytemuck::cast_slice(&uniforms(draw, &scene.viewport, width, height)),
                    usage: wgpu::BufferUsages::UNIFORM,
                });
            let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("ps-render draw bind group"),
                layout: &self.bind_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                }],
            });
            match &draw.shape {
                Shape::Mesh {
                    vertices, indices, ..
                } => self.draw_mesh(pass, &bind, vertices, indices),
                Shape::Lines { segments, .. } => self.draw_lines(pass, &bind, segments),
            }
        }
    }

    fn draw_mesh(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        bind: &wgpu::BindGroup,
        vertices: &[[f32; 2]],
        indices: &[u32],
    ) {
        pass.set_pipeline(&self.pipeline);
        let vertex_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("ps-render vertices"),
                contents: bytemuck::cast_slice(vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
        let index_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("ps-render indices"),
                contents: bytemuck::cast_slice(indices),
                usage: wgpu::BufferUsages::INDEX,
            });
        pass.set_bind_group(0, bind, &[]);
        pass.set_vertex_buffer(0, vertex_buffer.slice(..));
        pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        let count = u32::try_from(indices.len()).unwrap_or(u32::MAX);
        pass.draw_indexed(0..count, 0, 0..1);
    }

    fn draw_lines(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        bind: &wgpu::BindGroup,
        segments: &[[f32; 4]],
    ) {
        pass.set_pipeline(&self.line_pipeline);
        let instances = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("ps-render line segments"),
                contents: bytemuck::cast_slice(segments),
                usage: wgpu::BufferUsages::VERTEX,
            });
        pass.set_bind_group(0, bind, &[]);
        pass.set_vertex_buffer(0, instances.slice(..));
        let count = u32::try_from(segments.len()).unwrap_or(u32::MAX);
        pass.draw(0..4, 0..count);
    }

    /// Draws a scene to an offscreen image of `width` by `height` pixels and reads it back.
    #[must_use]
    pub fn render(&self, scene: &Scene, width: u32, height: u32) -> Image {
        if width == 0 || height == 0 {
            return Image {
                width,
                height,
                rgba: Vec::new(),
            };
        }
        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("ps-render target"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let padded_row = (width * 4).div_ceil(ROW_ALIGN) * ROW_ALIGN;
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ps-render readback"),
            size: u64::from(padded_row) * u64::from(height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("ps-render offscreen"),
            });
        {
            let [r, g, b, a] = scene.clear.map(f64::from);
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ps-render pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color { r, g, b, a }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            self.draw_into(&mut pass, scene, width, height);
        }
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_row),
                    rows_per_image: Some(height),
                },
            },
            size,
        );
        self.queue.submit(Some(encoder.finish()));

        let (tx, rx) = std::sync::mpsc::channel();
        readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = tx.send(result);
            });
        // Waiting for the device to finish is how the mapping completes; a failure leaves the
        // image blank, which the callers' checks would then show.
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
        let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
        if rx.recv().is_ok_and(|r| r.is_ok()) {
            let data = readback.slice(..).get_mapped_range();
            for row in 0..height as usize {
                let start = row * padded_row as usize;
                rgba.extend_from_slice(&data[start..start + width as usize * 4]);
            }
        }
        Image {
            width,
            height,
            rgba,
        }
    }
}

#[cfg(test)]
mod tests;
