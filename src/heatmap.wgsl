struct VertexOut {
    @location(0) tex_coord: vec2<f32>,
    @builtin(position) position: vec4<f32>,
};

struct Uniforms {
    // Texture coordinate to display at top left
    top_left: vec2<f32>,

    // Texture coordinate to display at bottom right 
    bottom_right: vec2<f32>,
};

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

@group(0) @binding(1)
var tex: texture_2d<f32>;

@group(0) @binding(2)
var samp: sampler;

@vertex
fn vs_main(@builtin(vertex_index) v_idx: u32) -> VertexOut {
    let coord = vec2<f32>(f32(v_idx / 2u), f32(v_idx % 2u));
    
    var out: VertexOut;
    out.position = vec4<f32>(-1.0 + 2.0 * coord.x, 1.0 - 2.0 * coord.y, 0.0, 1.0);
    out.tex_coord = uniforms.top_left + coord * (uniforms.bottom_right - uniforms.top_left);

    return out;
}

fn colormap_viridis_srgb(t: f32) -> vec3f {
    let c0 = vec3f(0.2777273272234177, 0.005407344544966578, 0.3340998053353061);
    let c1 = vec3f(0.1050930431085774, 1.404613529898575, 1.384590162594685);
    let c2 = vec3f(-0.3308618287255563, 0.214847559468213, 0.09509516302823659);
    let c3 = vec3f(-4.634230498983486, -5.799100973351585, -19.33244095627987);
    let c4 = vec3f(6.228269936347081, 14.17993336680509, 56.69055260068105);
    let c5 = vec3f(4.776384997670288, -13.74514537774601, -65.35303263337234);
    let c6 = vec3f(-5.435455855934631, 4.645852612178535, 26.3124352495832);
    return c0 + t * (c1 + t * (c2 + t * (c3 + t * (c4 + t * (c5 + t * c6)))));
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let val = textureSample(tex, samp, in.tex_coord).r;
    let color = vec4f(colormap_viridis_srgb(val), 1.0);
    return color;
}