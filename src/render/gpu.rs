use super::cmd::SAMPLE_BIAS;
use super::{Blend, DrawCmd, Paint, Style, Transform, font};
use crate::ecs::fx::FxMap;
use crate::{Sprite, Vec2};
use std::sync::Arc;

const SHADER: &str = "
struct Globals { size: vec2<f32> };
@group(0) @binding(0) var<uniform> globals: Globals;
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;

struct Out {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) flash: vec4<f32>,
};

@vertex
fn vs(
    @location(0) p: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) flash: vec4<f32>,
) -> Out {
    var o: Out;
    o.pos = vec4<f32>(p.x / globals.size.x * 2.0 - 1.0, 1.0 - p.y / globals.size.y * 2.0, 0.0, 1.0);
    o.uv = uv;
    o.color = color;
    o.flash = flash;
    return o;
}

@fragment
fn fs(i: Out) -> @location(0) vec4<f32> {
    let t = textureSample(tex, samp, i.uv) * i.color;
    return vec4<f32>(mix(t.rgb, i.flash.rgb, i.flash.a), t.a);
}

// Multiplying: the blend state multiplies the output by what is behind, so white (transparent) leaves it alone.
@fragment
fn fs_mul(i: Out) -> @location(0) vec4<f32> {
    let t = textureSample(tex, samp, i.uv) * i.color;
    return vec4<f32>(mix(vec3<f32>(1.0), mix(t.rgb, i.flash.rgb, i.flash.a), t.a), 1.0);
}
";

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    pos: [f32; 2],
    uv: [f32; 2],
    color: [f32; 4],
    // The colour to mix the texel towards and by how much (alpha), or all zero.
    flash: [f32; 4],
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Tex {
    White,
    Font,
    Sprite(usize),
}

// How a batch joins the picture behind it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Alpha,
    Add,
    Multiply,
}

impl From<Blend> for Mode {
    fn from(b: Blend) -> Mode {
        match b {
            Blend::Alpha => Mode::Alpha,
            Blend::Add => Mode::Add,
            Blend::Multiply => Mode::Multiply,
        }
    }
}

struct Batch {
    tex: Tex,
    mode: Mode,
    start: u32,
    end: u32,
}

struct Uploaded {
    _keep: Arc<Sprite>,
    tex: wgpu::Texture,
    size: (u32, u32),
    bind: wgpu::BindGroup,
    version: u64,
}

#[derive(Debug)]
struct Display(wgpu::rwh::RawDisplayHandle);

// The handle is only a pointer to the window system connection, which outlives the Gpu (see `run`).
unsafe impl Send for Display {}
unsafe impl Sync for Display {}

impl wgpu::rwh::HasDisplayHandle for Display {
    fn display_handle(&self) -> Result<wgpu::rwh::DisplayHandle<'_>, wgpu::rwh::HandleError> {
        Ok(unsafe { wgpu::rwh::DisplayHandle::borrow_raw(self.0) })
    }
}

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

fn rgba(c: u32, a: f32) -> [f32; 4] {
    [(c >> 16 & 255) as f32 / 255.0, (c >> 8 & 255) as f32 / 255.0, (c & 255) as f32 / 255.0, a]
}

// The colour a flat paint shows with after the style's flash and fade.
fn flat_color(col: u32, alpha: f32, st: &Style) -> [f32; 4] {
    let ([r, g, b, _], [fr, fg, fb, _]) = (rgba(col, 1.0), rgba(st.flash, 1.0));
    let k = st.flash_amount as f32 / 255.0;
    [r + (fr - r) * k, g + (fg - g) * k, b + (fb - b) * k, alpha * st.opacity as f32 / 255.0]
}

// The pixels of a sprite written over a texture of its size.
fn write_pixels(queue: &wgpu::Queue, tex: &wgpu::Texture, px: &[u32], (w, h): (u32, u32)) {
    let bytes: Vec<u8> = px.iter().flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, *p as u8, (p >> 24) as u8]).collect();
    queue.write_texture(
        tex.as_image_copy(),
        &bytes,
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * w), rows_per_image: Some(h) },
        wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
    );
}

fn upload(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    px: &[u32],
    (w, h): (u32, u32),
) -> (wgpu::Texture, wgpu::BindGroup) {
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    write_pixels(queue, &tex, px, (w, h));
    let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
    let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
        ],
    });
    (tex, bind)
}

fn vertex_buffer(device: &wgpu::Device, cap: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: (cap * std::mem::size_of::<Vertex>()) as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn best(adapters: impl IntoIterator<Item = wgpu::Adapter>, allow_cpu: bool) -> Result<wgpu::Adapter, String> {
    adapters
        .into_iter()
        .filter(|a| allow_cpu || a.get_info().device_type != wgpu::DeviceType::Cpu)
        .min_by_key(|a| match a.get_info().device_type {
            wgpu::DeviceType::DiscreteGpu => 0,
            wgpu::DeviceType::IntegratedGpu => 1,
            wgpu::DeviceType::Cpu => 3,
            _ => 2,
        })
        .ok_or("no usable GPU adapter".to_string())
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
        let target = unsafe { wgpu::SurfaceTargetUnsafe::from_window(win) }.map_err(|e| e.to_string())?;
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
                    let bits = font::bits(ch as u8)[row];
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

    fn quad(
        &mut self,
        tex: Tex,
        (x0, y0, x1, y1): (f32, f32, f32, f32),
        (u0, v0, u1, v1): (f32, f32, f32, f32),
        color: [f32; 4],
    ) {
        self.quad4(tex, [[x0, y0], [x1, y0], [x0, y1], [x1, y1]], (u0, v0, u1, v1), color);
    }

    fn quad4(
        &mut self,
        tex: Tex,
        [tl, tr, bl, br]: [[f32; 2]; 4],
        (u0, v0, u1, v1): (f32, f32, f32, f32),
        color: [f32; 4],
    ) {
        let at = self.verts.len() as u32;
        let flash = self.flash;
        let v = |pos, uv| Vertex { pos, uv, color, flash };
        self.verts.extend([
            v(tl, [u0, v0]),
            v(tr, [u1, v0]),
            v(bl, [u0, v1]),
            v(tr, [u1, v0]),
            v(br, [u1, v1]),
            v(bl, [u0, v1]),
        ]);
        let mode = self.mode;
        match self.batches.last_mut() {
            Some(b) if b.tex == tex && b.mode == mode => b.end = at + 6,
            _ => self.batches.push(Batch { tex, mode, start: at, end: at + 6 }),
        }
    }

    fn warped(&mut self, tex: Tex, quad: crate::Rect, t: &Transform, uv: (f32, f32, f32, f32), color: [f32; 4]) {
        let Vec2 { x: cx, y: cy } = quad.center();
        let (s, c) = t.angle.sin_cos();
        let corner = |ux: f32, uy: f32| {
            let (lx, ly) = (ux * quad.w / 2.0 * t.sx, uy * quad.h / 2.0 * t.sy);
            [cx + lx * c - ly * s, cy + lx * s + ly * c]
        };
        self.quad4(tex, [corner(-1.0, -1.0), corner(1.0, -1.0), corner(-1.0, 1.0), corner(1.0, 1.0)], uv, color);
    }

    fn solid(&mut self, r: &crate::Rect, color: [f32; 4]) {
        let (w, h) = (self.size.0 as f32, self.size.1 as f32);
        let (x0, y0) = (r.x.max(0.0).trunc(), r.y.max(0.0).trunc());
        let (x1, y1) = ((r.x + r.w).max(0.0).trunc().min(w), (r.y + r.h).max(0.0).trunc().min(h));
        if x1 > x0 && y1 > y0 {
            self.quad(Tex::White, (x0, y0, x1, y1), (0.0, 0.0, 1.0, 1.0), color);
        }
    }

    fn texture(&mut self, s: &Arc<Sprite>) -> (Tex, (f32, f32)) {
        let key = Arc::as_ptr(s) as usize;
        let version = s.version();
        if self.textures.get(&key).is_none_or(|t| t.version != version) {
            let img = s.image();
            let size = (img.w as u32, img.h as u32);
            // A sprite remade every frame (a light map) keeps its texture and only gets new pixels.
            match self.textures.get_mut(&key).filter(|t| t.size == size) {
                Some(t) => {
                    write_pixels(&self.queue, &t.tex, &img.px, size);
                    t.version = version;
                }
                None => {
                    let (tex, bind) = upload(&self.device, &self.queue, &self.tex_layout, &self.sampler, &img.px, size);
                    self.textures.insert(key, Uploaded { _keep: s.clone(), tex, size, bind, version });
                }
            }
        }
        let (w, h) = s.size();
        (Tex::Sprite(key), (w as f32, h as f32))
    }

    fn build(&mut self, cmds: &[DrawCmd]) {
        // The cache holds the only reference once a game drops a sprite; keeping it would leak one texture per sprite.
        self.textures.retain(|_, t| Arc::strong_count(&t._keep) > 1);
        self.verts.clear();
        self.batches.clear();
        let full = crate::Rect::new(0.0, 0.0, self.size.0 as f32, self.size.1 as f32);
        for c in cmds {
            let r = &c.rect;
            let t = &c.xform;
            let st = &c.style;
            (self.flash, self.mode) = ([0.0; 4], Mode::Alpha);
            let fill = match &c.paint {
                Paint::Color(col) => Some((*col, 1.0)),
                Paint::Alpha(col, a) => Some((*col, *a as f32 / 255.0)),
                _ => None,
            };
            if let Some((col, a)) = fill {
                self.mode = st.blend.into();
                let color = flat_color(col, a, st);
                match t.is_identity() {
                    true => self.solid(r, color),
                    false => self.warped(Tex::White, *r, t, (0.0, 0.0, 1.0, 1.0), color),
                }
                continue;
            }
            if let Some((sheet, src, flip)) = c.paint.region() {
                self.mode = st.blend.into();
                let color = [1.0, 1.0, 1.0, st.opacity as f32 / 255.0];
                if st.flash_amount > 0 {
                    let [r, g, b, _] = rgba(st.flash, 1.0);
                    self.flash = [r, g, b, st.flash_amount as f32 / 255.0];
                }
                let (tex, (sw, sh)) = self.texture(sheet);
                let (u0, u1) = (src.x / sw, (src.x + src.w) / sw);
                let (u0, u1) = if flip { (u1, u0) } else { (u0, u1) };
                let uv = (u0, src.y / sh, u1, (src.y + src.h) / sh);
                if t.is_identity() {
                    let (x, y) = (r.x.trunc(), r.y.trunc());
                    self.quad(tex, (x, y, x + src.w, y + src.h), uv, color);
                } else {
                    let nudged = (
                        uv.0 + SAMPLE_BIAS / sw,
                        uv.1 + SAMPLE_BIAS / sh,
                        uv.2 + SAMPLE_BIAS / sw,
                        uv.3 + SAMPLE_BIAS / sh,
                    );
                    self.warped(tex, crate::Rect::new(r.x, r.y, src.w, src.h), t, nudged, color);
                }
                continue;
            }
            match &c.paint {
                Paint::Clear(col) => self.solid(&full, rgba(*col, 1.0)),
                Paint::Outline(col) => {
                    for edge in r.edges() {
                        self.solid(&edge, rgba(*col, 1.0));
                    }
                }
                Paint::Glyph { ch, color, scale } => {
                    let (x, y, size) = (r.x.trunc(), r.y.trunc(), 8.0 * *scale as f32);
                    let (u0, u1) = (*ch as f32 / font::GLYPHS as f32, (*ch as f32 + 1.0) / font::GLYPHS as f32);
                    self.quad(Tex::Font, (x, y, x + size, y + size), (u0, 0.0, u1, 1.0), rgba(*color, 1.0));
                }
                Paint::Tiles { sheet, size, tiles } => {
                    let (tex, (sw, sh)) = self.texture(sheet);
                    let sz = *size as f32;
                    for &(gx, gy, id) in tiles {
                        let (x, y) = (r.x.trunc() + (gx * size) as f32, r.y.trunc() + (gy * size) as f32);
                        let src = Paint::tile_src(id, *size, sw as usize);
                        let uv = (src.x / sw, src.y / sh, (src.x + sz) / sw, (src.y + sz) / sh);
                        self.quad(tex, (x, y, x + sz, y + sz), uv, [1.0; 4]);
                    }
                }
                Paint::Mask { sheet, src, color } => {
                    let (tex, (sw, sh)) = self.texture(sheet);
                    let (x, y) = (r.x.trunc(), r.y.trunc());
                    let uv = (src.x / sw, src.y / sh, (src.x + src.w) / sw, (src.y + src.h) / sh);
                    self.quad(tex, (x, y, x + r.w, y + r.h), uv, rgba(*color, 1.0));
                }
                Paint::Color(_) | Paint::Alpha(..) | Paint::Sprite(_) | Paint::Region { .. } => unreachable!(),
            }
        }
    }

    fn encode(&mut self, view: &wgpu::TextureView) -> wgpu::CommandEncoder {
        if self.verts.len() > self.vcap {
            self.vcap = self.verts.len().next_power_of_two();
            self.vbuf = vertex_buffer(&self.device, self.vcap);
        }
        if !self.verts.is_empty() {
            self.queue.write_buffer(&self.vbuf, 0, bytemuck::cast_slice(&self.verts));
        }
        let mut enc = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            let v = &self.viewport;
            pass.set_viewport(v.x, v.y, v.w, v.h, 0.0, 1.0);
            pass.set_bind_group(0, &self.globals_bind, &[]);
            pass.set_vertex_buffer(0, self.vbuf.slice(..));
            let mut current = None;
            for b in &self.batches {
                if current != Some(b.mode) {
                    pass.set_pipeline(match b.mode {
                        Mode::Alpha => &self.pipeline,
                        Mode::Add => &self.additive,
                        Mode::Multiply => &self.multiply,
                    });
                    current = Some(b.mode);
                }
                let bind = match b.tex {
                    Tex::White => &self.white,
                    Tex::Font => &self.font,
                    Tex::Sprite(k) => &self.textures[&k].bind,
                };
                pass.set_bind_group(1, bind, &[]);
                pass.draw(b.start..b.end, 0..1);
            }
        }
        enc
    }

    pub(crate) fn batches(&self) -> usize {
        self.batches.len()
    }

    pub(crate) fn render(&mut self, cmds: &[DrawCmd]) {
        self.build(cmds);
        let Some((surface, config)) = &self.surface else { return };
        let frame = match surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t) => t,
            wgpu::CurrentSurfaceTexture::Suboptimal(_)
            | wgpu::CurrentSurfaceTexture::Outdated
            | wgpu::CurrentSurfaceTexture::Lost => return surface.configure(&self.device, config),
            _ => return,
        };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let enc = self.encode(&view);
        self.queue.submit([enc.finish()]);
        self.queue.present(frame);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::Soft;
    use crate::{Gfx, Rect, Style, Tile, Tilemap};

    fn scene() -> Gfx {
        let sprite = |p: &str| Arc::new(Sprite::load(p).unwrap());
        let mut g = Gfx::new(640, 360);
        g.cam = Vec2::new(120.0, 0.0);
        g.clear(0x6fa8e0);
        let map = Tilemap::load("assets/levels/level1.txt", 16, sprite("assets/images/tiles.png")).unwrap();
        g.draw(map.draw_list(g.view()));
        let player = sprite("assets/images/player.png");
        let frame = Rect::new(12.0, 24.0, 12.0, 24.0);
        g.blit_region(&player, frame, 50, 40, false);
        g.blit_region(&player, frame, 80, 40, true);
        g.blit(&sprite("assets/images/ball.png"), -4, 100);
        g.fill(Rect::new(300.0, 200.0, 50.0, 30.0), 0xff0000);
        g.fill_alpha(Rect::new(320.0, 210.0, 50.0, 30.0), 0x0000ff, 128);
        g.outline(Rect::new(400.0, 50.0, 40.0, 20.0), 0x00ff00);
        g.fill(Rect::new(-5.0, 350.0, 30.0, 30.0), 0xffff00);
        g.text("HELLO 123", 10, 10, 2, 0xffffff);
        g
    }

    fn dump(px: &[u32], path: &str) {
        let mut img = image::RgbImage::new(640, 360);
        for (i, p) in px.iter().enumerate() {
            img.put_pixel(i as u32 % 640, i as u32 / 640, image::Rgb([(p >> 16) as u8, (p >> 8) as u8, *p as u8]));
        }
        img.save(path).unwrap();
    }

    // Renders on both backends and fails if any pixel differs by more than the blending rounding.
    fn assert_same(gpu: &mut Gpu, g: &Gfx, name: &str) {
        let mut soft = Soft::new(640, 360, 1);
        soft.render(g.cmds());
        let gpu_px = gpu.render_to_pixels(g.cmds());
        if let Ok(dir) = std::env::var("ENGINE_DUMP") {
            dump(&soft.buf, &format!("{dir}/soft{name}.png"));
            dump(&gpu_px, &format!("{dir}/gpu{name}.png"));
        }
        let channel = |p: u32, sh: u32| (p >> sh & 255) as i32;
        let off: Vec<usize> = (0..soft.buf.len())
            .filter(|&i| [16, 8, 0].iter().any(|&sh| (channel(soft.buf[i], sh) - channel(gpu_px[i], sh)).abs() > 2))
            .collect();
        assert!(
            off.is_empty(),
            "{name}: {} of {} pixels differ on {}, in x {:?} y {:?}, first at {:?}",
            off.len(),
            gpu_px.len(),
            gpu.adapter,
            off.iter().map(|i| i % 640).min().zip(off.iter().map(|i| i % 640).max()),
            off.iter().map(|i| i / 640).min().zip(off.iter().map(|i| i / 640).max()),
            off.first().map(|i| (i % 640, i / 640))
        );
    }

    #[test]
    fn tiles_cut_from_a_sheet_taller_than_a_tile_match_between_the_backends() {
        let Ok(mut gpu) = Gpu::offscreen((640, 360), true) else {
            eprintln!("no GPU adapter, skipping");
            return;
        };
        let px = (0..64 * 32).map(|i| {
            let (x, y) = (i % 64, i / 64);
            let shade = (x as u32 / 16 + 1) * 60;
            if y < 16 { 0xFF00_0000 | (shade << 16) | ((shade / 2) << 8) } else { 0xFF00_FF00 }
        });
        let sheet = Arc::new(Sprite::new(64, 32, px.collect()));
        let map = Tilemap::from_ascii(&["#+#+", "+#+#"], 16, sheet);
        let mut g = Gfx::new(640, 360);
        g.draw(map.draw_list(g.view()));
        assert_same(&mut gpu, &g, "");
    }

    #[test]
    fn gpu_output_matches_the_software_rasterizer() {
        let Ok(mut gpu) = Gpu::offscreen((640, 360), true) else {
            eprintln!("no GPU adapter, skipping");
            return;
        };
        assert_same(&mut gpu, &scene(), "");
    }

    // Everything the camera zooms: tiles, sprites (some mirrored, some turned), flat and blended rects, an outline and
    // world text, drawn through `Gfx::draw` the way a game does, plus one screen-space line that must not zoom.
    fn zoomed_scene(zoom: f32) -> Gfx {
        let sprite = |p: &str| Arc::new(Sprite::load(p).unwrap());
        let mut world = Gfx::new(640, 360);
        let player = sprite("assets/images/player.png");
        let frame = Rect::new(12.0, 24.0, 12.0, 24.0);
        world.blit_region(&player, frame, 150, 100, false);
        world.blit_region(&player, frame, 180, 100, true);
        world.blit(&sprite("assets/images/ball.png"), 130, 160);
        world.fill(Rect::new(200.0, 150.0, 50.0, 30.0), 0xff0000);
        world.fill(Rect::new(250.0, 150.0, 17.0, 30.0), 0x00ff00);
        world.fill_alpha(Rect::new(220.0, 160.0, 50.0, 30.0), 0x0000ff, 128);
        world.outline(Rect::new(300.0, 120.0, 40.0, 20.0), 0x00ff00);
        world.text("HELLO 123", 160, 80, 1, 0xffffff);
        let mut turned = Gfx::new(640, 360);
        turned.blit_region(&player, frame, 120, 130, false);
        turned.fill(Rect::new(280.0, 170.0, 30.0, 14.0), 0xffaa00);
        let spin = Transform { angle: 0.7, sx: 1.0, sy: 1.0 };
        let spun: Vec<DrawCmd> = turned.cmds().iter().map(|c| DrawCmd { xform: spin, ..c.clone() }).collect();
        let mut g = Gfx::new(640, 360);
        (g.zoom, g.cam) = (zoom, Vec2::new(60.0, 40.0));
        g.clear(0x6fa8e0);
        let map = Tilemap::load("assets/levels/level1.txt", 16, sprite("assets/images/tiles.png")).unwrap();
        g.draw(map.draw_list(g.view()));
        g.draw(world.cmds().to_vec());
        g.draw(spun);
        g.text("HUD", 10, 10, 2, 0xffffff);
        g
    }

    #[test]
    fn a_zoomed_scene_matches_between_the_software_and_gpu_rasterizers() {
        let Ok(mut gpu) = Gpu::offscreen((640, 360), true) else {
            eprintln!("no GPU adapter, skipping");
            return;
        };
        for (zoom, name) in [(2.0, "_zoom2"), (1.5, "_zoom1_5"), (0.5, "_zoom0_5")] {
            assert_same(&mut gpu, &zoomed_scene(zoom), name);
        }
    }

    #[test]
    fn a_reloaded_sprite_is_uploaded_again_and_still_matches_the_software_output() {
        let Ok(mut gpu) = Gpu::offscreen((64, 64), true) else {
            eprintln!("no GPU adapter, skipping");
            return;
        };
        let dir = std::env::temp_dir().join(format!("engine-gpu-reload-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("s.png");
        let path = path.to_str().unwrap();
        std::fs::copy("assets/images/ball.png", path).unwrap();
        let sprite = Arc::new(Sprite::load(path).unwrap());
        let draw = |gpu: &mut Gpu| {
            let mut g = Gfx::new(64, 64);
            g.blit(&sprite, 4, 4);
            let mut soft = Soft::new(64, 64, 1);
            soft.render(g.cmds());
            let rgb: Vec<u32> = soft.buf.iter().map(|p| p & 0x00ff_ffff).collect();
            (rgb, gpu.render_to_pixels(g.cmds()))
        };
        let (soft, hard) = draw(&mut gpu);
        assert!(soft == hard, "first upload differs from the software output");
        std::fs::copy("assets/images/player.png", path).unwrap();
        sprite.reload(path).unwrap();
        let (soft2, hard2) = draw(&mut gpu);
        assert!(soft != soft2, "the new image really looks different");
        assert!(soft2 == hard2, "re-uploaded texture differs from the software output");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn textures_of_dropped_sprites_are_released() {
        let Ok(mut gpu) = Gpu::offscreen((64, 64), true) else {
            eprintln!("no GPU adapter, skipping");
            return;
        };
        let kept = Sprite::test_ball();
        for _ in 0..20 {
            let temp = Arc::new(Sprite::load("assets/images/player.png").unwrap());
            let mut g = Gfx::new(64, 64);
            g.blit(&kept, 0, 0);
            g.blit(&temp, 20, 0);
            gpu.render_to_pixels(g.cmds());
        }
        gpu.render_to_pixels(Gfx::new(64, 64).cmds());
        assert!(gpu.textures.len() <= 1, "{} textures cached", gpu.textures.len());
    }

    fn transformed_scene() -> Gfx {
        let sprite = |p: &str| Arc::new(Sprite::load(p).unwrap());
        let (ball, player) = (sprite("assets/images/ball.png"), sprite("assets/images/player.png"));
        let mut g = Gfx::new(160, 120);
        g.clear(0x203050);
        let cmd =
            |rect, paint, angle, sx, sy| DrawCmd { xform: Transform { angle, sx, sy }, ..DrawCmd::new(rect, paint) };
        let at = |x, y, w, h| Rect { x, y, w, h };
        let frame = Rect::new(12.0, 24.0, 12.0, 24.0);
        g.draw([
            cmd(at(10.0, 10.0, 16.0, 16.0), Paint::Sprite(ball.clone()), 0.5, 1.0, 1.0),
            cmd(at(40.0, 10.0, 16.0, 16.0), Paint::Sprite(ball.clone()), 0.0, 3.0, 2.0),
            cmd(at(90.0, 10.0, 16.0, 16.0), Paint::Sprite(ball), 2.0, -1.5, 1.5),
            cmd(
                at(20.0, 50.0, 12.0, 24.0),
                Paint::Region { sheet: player.clone(), src: frame, flip: false },
                1.0,
                1.0,
                1.0,
            ),
            cmd(
                at(50.0, 50.0, 12.0, 24.0),
                Paint::Region { sheet: player.clone(), src: frame, flip: true },
                0.7,
                2.0,
                2.0,
            ),
            cmd(at(100.0, 50.0, 30.0, 12.0), Paint::Color(0xff4040), 0.4, 1.0, 1.0),
            cmd(at(110.0, 60.0, 30.0, 30.0), Paint::Alpha(0x40ff80, 128), -0.8, 1.0, 0.6),
        ]);
        let font = crate::Font::load("assets/fonts/galmuri9.bdf").unwrap();
        g.text_font(&font, "한글 text", 4, 100, 0xffee88);
        g.text_font_scaled(&font, "큰 글", 80, 98, 2, 0x88eeff);
        g
    }

    #[test]
    fn rotated_and_scaled_drawing_agrees_between_the_two_backends() {
        let Ok(mut gpu) = Gpu::offscreen((160, 120), true) else {
            eprintln!("no GPU adapter, skipping");
            return;
        };
        let g = transformed_scene();
        let mut soft = Soft::new(160, 120, 1);
        soft.render(g.cmds());
        let soft_px: Vec<u32> = soft.buf.iter().map(|p| p & 0x00ff_ffff).collect();
        let gpu_px = gpu.render_to_pixels(g.cmds());
        if let Ok(dir) = std::env::var("ENGINE_DUMP") {
            let save = |px: &[u32], name: &str| {
                let mut img = image::RgbImage::new(160, 120);
                for (i, p) in px.iter().enumerate() {
                    img.put_pixel(
                        i as u32 % 160,
                        i as u32 / 160,
                        image::Rgb([(p >> 16) as u8, (p >> 8) as u8, *p as u8]),
                    );
                }
                img.save(format!("{dir}/{name}")).unwrap();
            };
            save(&soft_px, "xform_soft.png");
            save(&gpu_px, "xform_gpu.png");
        }
        let channel = |p: u32, sh: u32| (p >> sh & 255) as i32;
        let differ =
            |i: usize| [16, 8, 0].iter().any(|&sh| (channel(soft_px[i], sh) - channel(gpu_px[i], sh)).abs() > 2);
        let drawn = soft_px.iter().filter(|&&p| p != 0x203050).count();
        let off = (0..soft_px.len()).filter(|&i| differ(i)).count();
        eprintln!("transform parity: {off} of {drawn} drawn pixels differ on {}", gpu.adapter);
        assert!(off * 100 <= drawn * 3, "{off} of {drawn} drawn pixels differ");
    }

    #[test]
    fn a_window_of_another_shape_gets_the_picture_centred_with_black_around_it() {
        let Ok(mut gpu) = Gpu::offscreen((64, 32), true) else {
            eprintln!("no GPU adapter, skipping");
            return;
        };
        let mut g = crate::Gfx::new(64, 32);
        g.clear(0xff0000);
        gpu.fit((128, 128));
        let px = gpu.render_to_pixels(g.cmds());
        assert_eq!(px.len(), 128 * 128);
        let at = |x: usize, y: usize| px[y * 128 + x];
        // 2:1 picture at 2x in a square window: 32 rows of bar above and below it.
        assert_eq!((at(64, 10), at(64, 120)), (0, 0), "the bars stay black");
        assert_eq!(
            (at(0, 40), at(127, 90), at(64, 64)),
            (0xff0000, 0xff0000, 0xff0000),
            "the picture fills the middle"
        );
        assert_eq!(
            (at(5, 31), at(5, 32), at(5, 95), at(5, 96)),
            (0, 0xff0000, 0xff0000, 0),
            "edges fall on whole rows"
        );
        gpu.fit((0, 0));
        assert_eq!(gpu.target, (128, 128), "a minimized window changes nothing");
    }

    #[test]
    fn styled_sprites_and_flat_colours_match_between_the_software_and_gpu_rasterizers() {
        let Ok(mut gpu) = Gpu::offscreen((640, 360), true) else {
            eprintln!("no GPU adapter, skipping");
            return;
        };
        let sprite = |p: &str| Arc::new(Sprite::load(p).unwrap());
        let (ball, player) = (sprite("assets/images/ball.png"), sprite("assets/images/player.png"));
        let frame = Rect::new(12.0, 24.0, 12.0, 24.0);
        let mut g = Gfx::new(640, 360);
        g.clear(0x203050);
        g.fill(Rect::new(0.0, 60.0, 640.0, 40.0), 0x806040);
        let styles = [
            Style::PLAIN,
            Style::PLAIN.flashed(0xffffff, 128),
            Style::PLAIN.flashed(0xff0000, 255),
            Style::PLAIN.faded(100),
            Style::PLAIN.additive(),
            Style::PLAIN.additive().faded(120),
            Style::PLAIN.flashed(0x00ff80, 90).faded(200),
            Style::PLAIN.multiplied(),
            Style::PLAIN.multiplied().faded(120),
            Style::PLAIN.multiplied().flashed(0xff8000, 100),
        ];
        let turns = [
            Transform::IDENTITY,
            Transform { angle: 0.0, sx: 2.0, sy: 2.0 },
            Transform { angle: 0.5, sx: 1.0, sy: 1.0 },
        ];
        let mut cmds = Vec::new();
        for (row, turn) in turns.iter().enumerate() {
            for (i, style) in styles.iter().enumerate() {
                let (x, y) = (10.0 + i as f32 * 60.0, 10.0 + row as f32 * 90.0);
                let sheet_cmd = |at: Rect, paint| DrawCmd { xform: *turn, style: *style, ..DrawCmd::new(at, paint) };
                cmds.push(sheet_cmd(Rect { x, y, w: 16.0, h: 16.0 }, Paint::Sprite(ball.clone())));
                let region = Paint::Region { sheet: player.clone(), src: frame, flip: i % 2 == 1 };
                cmds.push(sheet_cmd(Rect { x: x + 20.0, y, w: 12.0, h: 24.0 }, region));
                cmds.push(sheet_cmd(Rect { x, y: y + 40.0, w: 20.0, h: 14.0 }, Paint::Color(0xc08040)));
                cmds.push(sheet_cmd(Rect::new(x + 24.0, y + 40.0, 20.0, 14.0), Paint::Alpha(0x4080c0, 150)));
            }
        }
        g.draw(cmds);
        assert_same(&mut gpu, &g, "_styles");
    }

    #[test]
    fn a_lit_scene_with_a_vignette_and_scanlines_matches_between_the_backends_and_follows_a_moving_light() {
        let Ok(mut gpu) = Gpu::offscreen((640, 360), true) else {
            eprintln!("no GPU adapter, skipping");
            return;
        };
        let mut map = crate::LightMap::new(640, 360);
        let (vignette, lines) = (crate::vignette(640, 360, 0.8), crate::scanlines(640, 360, 50));
        for (frame, x) in [200.0, 420.0].into_iter().enumerate() {
            let mut g = zoomed_scene(1.0);
            map.clear(0x303850);
            map.add(crate::Light::new((x, 150.0), 90.0).colored(0xffd080));
            map.add(crate::Light::new((300.0, 250.0), 60.0).colored(0x4080ff).intensity(0.7));
            map.draw(&mut g);
            g.multiply(&vignette);
            g.multiply(&lines);
            g.text("HUD after the light", 10, 340, 1, 0xffffff);
            assert_same(&mut gpu, &g, &format!("_lit{frame}"));
        }
    }

    #[test]
    fn tiles_from_the_second_row_of_a_sheet_match_between_the_backends() {
        let Ok(mut gpu) = Gpu::offscreen((640, 360), true) else {
            eprintln!("no GPU adapter, skipping");
            return;
        };
        let px = (0..64 * 32).map(|i| {
            let (x, y) = (i % 64, i / 64);
            let id = (y / 16) * 4 + x / 16;
            0xFF00_0000 | ((id as u32 * 31) << 16) | ((255 - id as u32 * 30) << 8) | ((x as u32 % 7) * 20)
        });
        let sheet = Arc::new(Sprite::new(64, 32, px.collect()));
        let legend: Vec<Tile> = b"12345678"
            .iter()
            .enumerate()
            .map(|(i, &ch)| Tile {
                ch,
                id: i as u8 + 1,
                solid: false,
                front: false,
                auto: false,
                one_way: false,
                slope: None,
            })
            .collect();
        let map = Tilemap::try_from_ascii_with(&["1234", "5678", "8765"], 16, sheet, &legend).unwrap();
        let mut g = Gfx::new(640, 360);
        g.draw(map.draw_list(g.view()));
        assert_same(&mut gpu, &g, "_rows");
        (g.zoom, g.cam) = (1.5, Vec2::new(4.0, 3.0));
        g.reset();
        g.draw(map.draw_list(g.view()));
        assert_same(&mut gpu, &g, "_rows_zoom");
    }
}
