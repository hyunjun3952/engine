use super::{Arc, Blend, Sprite, Style};

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(super) struct Vertex {
    pub(super) pos: [f32; 2],
    pub(super) uv: [f32; 2],
    pub(super) color: [f32; 4],
    // The colour to mix the texel towards and by how much (alpha), or all zero.
    pub(super) flash: [f32; 4],
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Tex {
    White,
    Font,
    Sprite(usize),
}

// How a batch joins the picture behind it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Mode {
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

pub(super) struct Batch {
    pub(super) tex: Tex,
    pub(super) mode: Mode,
    pub(super) start: u32,
    pub(super) end: u32,
}

pub(super) struct Uploaded {
    pub(super) _keep: Arc<Sprite>,
    pub(super) tex: wgpu::Texture,
    pub(super) size: (u32, u32),
    pub(super) bind: wgpu::BindGroup,
    pub(super) version: u64,
}

pub(super) fn rgba(c: u32, a: f32) -> [f32; 4] {
    [(c >> 16 & 255) as f32 / 255.0, (c >> 8 & 255) as f32 / 255.0, (c & 255) as f32 / 255.0, a]
}

// The colour a flat paint shows with after the style's flash and fade.
pub(super) fn flat_color(col: u32, alpha: f32, st: &Style) -> [f32; 4] {
    let ([r, g, b, _], [fr, fg, fb, _]) = (rgba(col, 1.0), rgba(st.flash, 1.0));
    let k = st.flash_amount as f32 / 255.0;
    [r + (fr - r) * k, g + (fg - g) * k, b + (fb - b) * k, alpha * st.opacity as f32 / 255.0]
}
