pub(super) const SHADER: &str = "
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
