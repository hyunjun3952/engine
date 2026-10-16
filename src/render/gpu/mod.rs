use super::cmd::SAMPLE_BIAS;
use super::{Blend, DrawCmd, Paint, Style, Transform, font};
use crate::base::fx::FxMap;
use crate::{Sprite, Vec2};
use std::sync::Arc;

mod adapter;
mod build;
mod shader;
#[cfg(test)]
mod tests;
mod types;
mod upload;

use adapter::*;
use shader::SHADER;
use types::*;
use upload::*;

pub(crate) struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    additive: wgpu::RenderPipeline,
    multiply: wgpu::RenderPipeline,
    // The style of the command being built, which every quad of it carries.
    flash: [f32; 4],
    mode: Mode,
    globals_bind: wgpu::BindGroup,
    tex_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    white: wgpu::BindGroup,
    font: wgpu::BindGroup,
    textures: FxMap<usize, Uploaded>,
    verts: Vec<Vertex>,
    batches: Vec<Batch>,
    vbuf: wgpu::Buffer,
    vcap: usize,
    size: (u32, u32),
    // What the picture is drawn into (the window), and where in it the picture goes.
    target: (u32, u32),
    viewport: crate::Rect,
    surface: Option<(wgpu::Surface<'static>, wgpu::SurfaceConfiguration)>,
    pub(crate) adapter: String,
}

impl Gpu {
    pub(crate) fn for_window<W: wgpu::rwh::HasWindowHandle + wgpu::rwh::HasDisplayHandle>(
        win: &W,
        size: (u32, u32),
        scale: u32,
        allow_cpu: bool,
    ) -> Result<Gpu, String> {
        let raw = win.display_handle().map_err(|e| e.to_string())?.as_raw();
        let desc = wgpu::InstanceDescriptor::new_with_display_handle(Box::new(Display(raw)));
        let instance = wgpu::Instance::new(desc);
        // SAFETY: `run` declares the window before the backend that holds this surface, so the surface drops first.
        let target = unsafe { wgpu::SurfaceTargetUnsafe::from_window(win) }.map_err(|e| e.to_string())?;
        // SAFETY: `target` comes from that same window.
        let surface = unsafe { instance.create_surface_unsafe(target) }.map_err(|e| e.to_string())?;
        let adapters = pollster::block_on(instance.enumerate_adapters(wgpu::Backends::all()));
        let usable = adapters.into_iter().filter(|a| a.is_surface_supported(&surface));
        let adapter = best(usable, allow_cpu)?;
        let caps = surface.get_capabilities(&adapter);
        let format = caps.formats.iter().copied().find(|f| !f.is_srgb()).or(caps.formats.first().copied());
        let format = format.ok_or("surface reports no formats")?;
        let mut gpu = Gpu::new(adapter, format, size)?;
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.0 * scale,
            height: size.1 * scale,
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            color_space: wgpu::SurfaceColorSpace::default(),
            view_formats: vec![],
        };
        surface.configure(&gpu.device, &config);
        gpu.surface = Some((surface, config));
        gpu.resize((size.0 * scale, size.1 * scale));
        Ok(gpu)
    }

    /// Follows a window resize: the picture keeps its shape, centred, and the rest of the window stays black.
    pub(crate) fn resize(&mut self, (w, h): (u32, u32)) {
        let Some((surface, config)) = &mut self.surface else { return };
        if w == 0 || h == 0 {
            return;
        }
        (config.width, config.height) = (w, h);
        surface.configure(&self.device, config);
        self.fit((w, h));
    }

    // The picture keeps its shape, centred in a `target`-sized area; a zero size (minimized) changes nothing.
    fn fit(&mut self, (w, h): (u32, u32)) {
        if w == 0 || h == 0 {
            return;
        }
        self.target = (w, h);
        let canvas = (self.size.0 as f32, self.size.1 as f32);
        self.viewport = super::letterbox((w as f32, h as f32), canvas);
    }

    #[cfg(test)]
    pub(crate) fn offscreen(size: (u32, u32), allow_cpu: bool) -> Result<Gpu, String> {
        // Debug builds name every object through VK_EXT_debug_utils, and creating devices on several test threads at
        // once crashed inside the Vulkan loader's name call (about one run in ten); labels are not needed here.
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.flags = wgpu::InstanceFlags::empty();
        let instance = wgpu::Instance::new(desc);
        let adapters = pollster::block_on(instance.enumerate_adapters(wgpu::Backends::all()));
        let adapter = best(adapters, allow_cpu)?;
        Gpu::new(adapter, wgpu::TextureFormat::Rgba8Unorm, size)
    }

    fn new(adapter: wgpu::Adapter, format: wgpu::TextureFormat, size: (u32, u32)) -> Result<Gpu, String> {
        let info = adapter.get_info();
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
            .map_err(|e| e.to_string())?;

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("engine"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let tex_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&globals_layout), Some(&tex_layout)],
            immediate_size: 0,
        });
        let attrs = wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32x4, 3 => Float32x4];
        let make = |blend: wgpu::BlendState, fs: &str| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: None,
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs"),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<Vertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &attrs,
                    })],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(blend),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let pipeline = make(wgpu::BlendState::ALPHA_BLENDING, "fs");
        let add = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::SrcAlpha,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        };
        let keep_alpha = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::Zero,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        };
        let additive = make(wgpu::BlendState { color: add, alpha: keep_alpha }, "fs");
        let times = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::Dst,
            dst_factor: wgpu::BlendFactor::Zero,
            operation: wgpu::BlendOperation::Add,
        };
        let multiply = make(wgpu::BlendState { color: times, alpha: keep_alpha }, "fs_mul");

        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&globals, 0, bytemuck::cast_slice(&[size.0 as f32, size.1 as f32, 0.0, 0.0]));
        let globals_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &globals_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: globals.as_entire_binding() }],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let (_, white) = upload(&device, &queue, &tex_layout, &sampler, &[0xffff_ffff], (1, 1));
        let atlas: Vec<u32> = (0..8)
            .flat_map(|row| {
                (0..font::GLYPHS).flat_map(move |ch| {
                    let bits = font::glyph_rows(ch as u8)[row];
                    (0..8).map(move |col| if bits >> col & 1 == 1 { 0xffff_ffff } else { 0x00ff_ffff })
                })
            })
            .collect();
        let (_, font) = upload(&device, &queue, &tex_layout, &sampler, &atlas, (font::GLYPHS as u32 * 8, 8));
        let vcap = 4096;
        let vbuf = vertex_buffer(&device, vcap);
        Ok(Gpu {
            device,
            queue,
            pipeline,
            additive,
            multiply,
            flash: [0.0; 4],
            mode: Mode::Alpha,
            globals_bind,
            tex_layout,
            sampler,
            white,
            font,
            textures: FxMap::default(),
            verts: Vec::new(),
            batches: Vec::new(),
            vbuf,
            vcap,
            size,
            target: size,
            viewport: crate::Rect::new(0.0, 0.0, size.0 as f32, size.1 as f32),
            surface: None,
            adapter: format!("{} ({:?})", info.name, info.backend),
        })
    }

    pub(crate) fn batches(&self) -> usize {
        self.batches.len()
    }

    pub(crate) fn render(&mut self, cmds: &[DrawCmd]) {
        self.build(cmds);
        let Some((surface, config)) = &self.surface else { return };
        // A suboptimal frame still shows; dropping it would show nothing on a surface that stays suboptimal.
        let (frame, stale) = match surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t) => (t, false),
            wgpu::CurrentSurfaceTexture::Suboptimal(t) => (t, true),
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                return surface.configure(&self.device, config);
            }
            _ => return,
        };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let enc = self.encode(&view);
        self.queue.submit([enc.finish()]);
        self.queue.present(frame);
        if let (true, Some((surface, config))) = (stale, &self.surface) {
            surface.configure(&self.device, config);
        }
    }

    #[cfg(test)]
    pub(crate) fn render_to_pixels(&mut self, cmds: &[DrawCmd]) -> Vec<u32> {
        self.build(cmds);
        let (w, h) = self.target;
        let extent = wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 };
        let tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        let mut enc = self.encode(&view);
        let row = (4 * w).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let out = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: (row * h) as u64,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        enc.copy_texture_to_buffer(
            tex.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &out,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(h) },
            },
            extent,
        );
        self.queue.submit([enc.finish()]);
        out.slice(..).map_async(wgpu::MapMode::Read, |r| r.expect("map"));
        self.device.poll(wgpu::PollType::wait_indefinitely()).expect("poll");
        let data = out.slice(..).get_mapped_range().expect("mapped");
        let mut px = Vec::with_capacity((w * h) as usize);
        for y in 0..h as usize {
            for x in 0..w as usize {
                let i = y * row as usize + x * 4;
                px.push((data[i] as u32) << 16 | (data[i + 1] as u32) << 8 | data[i + 2] as u32);
            }
        }
        px
    }
}
