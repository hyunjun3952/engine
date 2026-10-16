use super::{
    Arc, Batch, DrawCmd, Gpu, Mode, Paint, SAMPLE_BIAS, Sprite, Tex, Transform, Uploaded, Vec2, Vertex, flat_color,
    font, rgba, upload, vertex_buffer, write_pixels,
};

impl Gpu {
    pub(super) fn quad(
        &mut self,
        tex: Tex,
        (x0, y0, x1, y1): (f32, f32, f32, f32),
        (u0, v0, u1, v1): (f32, f32, f32, f32),
        color: [f32; 4],
    ) {
        self.quad4(tex, [[x0, y0], [x1, y0], [x0, y1], [x1, y1]], (u0, v0, u1, v1), color);
    }

    pub(super) fn quad4(
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

    pub(super) fn warped(
        &mut self,
        tex: Tex,
        quad: crate::Rect,
        t: &Transform,
        uv: (f32, f32, f32, f32),
        color: [f32; 4],
    ) {
        let Vec2 { x: cx, y: cy } = quad.center();
        let (s, c) = t.angle.sin_cos();
        let corner = |ux: f32, uy: f32| {
            let (lx, ly) = (ux * quad.w / 2.0 * t.sx, uy * quad.h / 2.0 * t.sy);
            [cx + lx * c - ly * s, cy + lx * s + ly * c]
        };
        self.quad4(tex, [corner(-1.0, -1.0), corner(1.0, -1.0), corner(-1.0, 1.0), corner(1.0, 1.0)], uv, color);
    }

    pub(super) fn solid(&mut self, r: &crate::Rect, color: [f32; 4]) {
        let (w, h) = (self.size.0 as f32, self.size.1 as f32);
        let (x0, y0) = (r.x.max(0.0).trunc(), r.y.max(0.0).trunc());
        let (x1, y1) = ((r.x + r.w).max(0.0).trunc().min(w), (r.y + r.h).max(0.0).trunc().min(h));
        if x1 > x0 && y1 > y0 {
            self.quad(Tex::White, (x0, y0, x1, y1), (0.0, 0.0, 1.0, 1.0), color);
        }
    }

    pub(super) fn texture(&mut self, s: &Arc<Sprite>) -> Option<(Tex, (f32, f32))> {
        let key = Arc::as_ptr(s) as usize;
        let version = s.version();
        if self.textures.get(&key).is_none_or(|t| t.version != version) {
            let img = s.image();
            let size = (img.w as u32, img.h as u32);
            // wgpu panics on an empty texture or one past the device's limit; Soft draws nothing for these either.
            if size.0 == 0 || size.1 == 0 || size.0.max(size.1) > self.device.limits().max_texture_dimension_2d {
                return None;
            }
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
        Some((Tex::Sprite(key), (w as f32, h as f32)))
    }

    pub(super) fn build(&mut self, cmds: &[DrawCmd]) {
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
                let Some((tex, (sw, sh))) = self.texture(sheet) else { continue };
                let (u0, u1) = (src.x / sw, (src.x + src.w) / sw);
                let (u0, u1) = if flip { (u1, u0) } else { (u0, u1) };
                let uv = (u0, src.y / sh, u1, (src.y + src.h) / sh);
                if t.is_identity() {
                    let (x, y) = (r.x.floor(), r.y.floor());
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
                    let Some((tex, (sw, sh))) = self.texture(sheet) else { continue };
                    let sz = *size as f32;
                    for &(gx, gy, id) in tiles {
                        let (x, y) = (r.x.floor() + (gx * size) as f32, r.y.floor() + (gy * size) as f32);
                        let src = Paint::tile_src(id, *size, sw as usize);
                        let uv = (src.x / sw, src.y / sh, (src.x + sz) / sw, (src.y + sz) / sh);
                        self.quad(tex, (x, y, x + sz, y + sz), uv, [1.0; 4]);
                    }
                }
                Paint::Mask { sheet, src, color } => {
                    let Some((tex, (sw, sh))) = self.texture(sheet) else { continue };
                    let (x, y) = (r.x.floor(), r.y.floor());
                    let uv = (src.x / sw, src.y / sh, (src.x + src.w) / sw, (src.y + src.h) / sh);
                    self.quad(tex, (x, y, x + r.w, y + r.h), uv, rgba(*color, 1.0));
                }
                Paint::Color(_) | Paint::Alpha(..) | Paint::Sprite(_) | Paint::Region { .. } => unreachable!(),
            }
        }
    }

    pub(super) fn encode(&mut self, view: &wgpu::TextureView) -> wgpu::CommandEncoder {
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
}
