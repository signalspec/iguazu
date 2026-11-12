struct VertexOut {
    @location(0) tex_coord: vec2<f32>,
    @builtin(position) position: vec4<f32>,
};

struct Uniforms {
    rect: vec4<f32>,
    matrix: mat3x3<f32>,
    color1: vec4<f32>,
    color2: vec4<f32>,
    alpha: f32,
    colormap: u32,
    vmin: f32,
    vmax: f32,
};

const COLORMAP_LINEAR: u32 = 1;
const COLORMAP_VIRIDIS: u32 = 16;
const COLORMAP_PLASMA: u32 = 17;
const COLORMAP_MAGMA: u32 = 18;
const COLORMAP_INFERNO: u32 = 19;

@group(0) @binding(0)
var tex: texture_2d<f32>;

@group(0) @binding(1)
var samp: sampler;

@group(1) @binding(0)
var<uniform> uniforms: Uniforms;

@vertex
fn vs_main(@builtin(vertex_index) v_idx: u32) -> VertexOut {
    let coord = vec2<f32>(f32(v_idx / 2u), f32(v_idx % 2u));

    var out: VertexOut;
    out.position = vec4<f32>(
        mix(uniforms.rect.x, uniforms.rect.y, coord.x),
        mix(uniforms.rect.z, uniforms.rect.w, coord.y),
        0.0,
        1.0
    );
    out.tex_coord = (uniforms.matrix * vec3f(coord, 1.0)).xy;

    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let val = textureSample(tex, samp, in.tex_coord).r;
    let scaled_val = saturate((val - uniforms.vmin) / (uniforms.vmax - uniforms.vmin));

    if uniforms.colormap == COLORMAP_LINEAR {
        return mix(uniforms.color1, uniforms.color2, scaled_val) * uniforms.alpha;
    } else if uniforms.colormap == COLORMAP_VIRIDIS {
        return colormap_viridis_srgb(scaled_val) * uniforms.alpha;
    } else if uniforms.colormap == COLORMAP_PLASMA {
        return colormap_plasma_srgb(scaled_val) * uniforms.alpha;
    } else if uniforms.colormap == COLORMAP_MAGMA {
        return colormap_magma_srgb(scaled_val) * uniforms.alpha;
    } else if uniforms.colormap == COLORMAP_INFERNO {
        return colormap_inferno_srgb(scaled_val) * uniforms.alpha;
    } else {
        return vec4f(uniforms.alpha, 0.0, 0.0, uniforms.alpha);
    }
}


// Polynomials fitted to matplotlib colormaps, taken from https://www.shadertoy.com/view/WlfXRN.
//
// License CC0 (public domain)
//   https://creativecommons.org/share-your-work/public-domain/cc0/
fn colormap_viridis_srgb(t: f32) -> vec4f {
    let c0 = vec3f(0.2777273272234177, 0.005407344544966578, 0.3340998053353061);
    let c1 = vec3f(0.1050930431085774, 1.404613529898575, 1.384590162594685);
    let c2 = vec3f(-0.3308618287255563, 0.214847559468213, 0.09509516302823659);
    let c3 = vec3f(-4.634230498983486, -5.799100973351585, -19.33244095627987);
    let c4 = vec3f(6.228269936347081, 14.17993336680509, 56.69055260068105);
    let c5 = vec3f(4.776384997670288, -13.74514537774601, -65.35303263337234);
    let c6 = vec3f(-5.435455855934631, 4.645852612178535, 26.3124352495832);
    return vec4f(c0 + t * (c1 + t * (c2 + t * (c3 + t * (c4 + t * (c5 + t * c6))))), 1.0);
}

fn colormap_plasma_srgb(t: f32) -> vec4f {
    let c0 = vec3f(0.05873234392399702, 0.02333670892565664, 0.5433401826748754);
    let c1 = vec3f(2.176514634195958, 0.2383834171260182, 0.7539604599784036);
    let c2 = vec3f(-2.689460476458034, -7.455851135738909, 3.110799939717086);
    let c3 = vec3f(6.130348345893603, 42.3461881477227, -28.51885465332158);
    let c4 = vec3f(-11.10743619062271, -82.66631109428045, 60.13984767418263);
    let c5 = vec3f(10.02306557647065, 71.41361770095349, -54.07218655560067);
    let c6 = vec3f(-3.658713842777788, -22.93153465461149, 18.19190778539828);
    return vec4f(c0 + t * (c1 + t * (c2 + t * (c3 + t * (c4 + t * (c5 + t * c6))))), 1.0);
}

fn colormap_magma_srgb(t: f32) -> vec4f {
    let c0 = vec3f(-0.002136485053939582, -0.000749655052795221, -0.005386127855323933);
    let c1 = vec3f(0.2516605407371642, 0.6775232436837668, 2.494026599312351);
    let c2 = vec3f(8.353717279216625, -3.577719514958484, 0.3144679030132573);
    let c3 = vec3f(-27.66873308576866, 14.26473078096533, -13.64921318813922);
    let c4 = vec3f(52.17613981234068, -27.94360607168351, 12.94416944238394);
    let c5 = vec3f(-50.76852536473588, 29.04658282127291, 4.23415299384598);
    let c6 = vec3f(18.65570506591883, -11.48977351997711, -5.601961508734096);
    return vec4f(c0 + t * (c1 + t * (c2 + t * (c3 + t * (c4 + t * (c5 + t * c6))))), 1.0);
}

fn colormap_inferno_srgb(t: f32) -> vec4f {
    let c0 = vec3f(0.0002189403691192265, 0.001651004631001012, -0.01948089843709184);
    let c1 = vec3f(0.1065134194856116, 0.5639564367884091, 3.932712388889277);
    let c2 = vec3f(11.60249308247187, -3.972853965665698, -15.9423941062914);
    let c3 = vec3f(-41.70399613139459, 17.43639888205313, 44.35414519872813);
    let c4 = vec3f(77.162935699427, -33.40235894210092, -81.80730925738993);
    let c5 = vec3f(-71.31942824499214, 32.62606426397723, 73.20951985803202);
    let c6 = vec3f(25.13112622477341, -12.24266895238567, -23.07032500287172);
    return vec4f(c0 + t * (c1 + t * (c2 + t * (c3 + t * (c4 + t * (c5 + t * c6))))), 1.0);
}
