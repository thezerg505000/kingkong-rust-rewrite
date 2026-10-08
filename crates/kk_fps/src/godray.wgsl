// King Kong (2005) XeGodRayEffect, ported from the game's own vsaftereffects/psaftereffects
// (iGodRayShaderId 1..6) and fn@0x00a2ccc0.
//
// Original: mask pass (geometry -> black, open sky kept), then 7 zoom-blur stages toward the
// projected light L, run as 3 two-stage passes + 1 final stage on render targets:
//   stage k: blend the image with a copy scaled about L by f_k = 1 + c*0.5^k (c = -0.4 when
//   the light is in front), alpha a_k = 0.5 - 1/(6*2^k).
// Pass 0 (stages 0,1) is drawn on a 17x17 grid whose vertex colour is a vignette
// max(0, 1 - (x^2+y^2)) and is tinted by g_vGodRayAdjust (= colour * intensity).
// Final: out = scene + blur * A*(A+1), A = view-fade * intensity.
// The 2^7 = 128 tap combinations are summed here in one pass (identical maths, no RTs).

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

struct GodRay {
    light_uv: vec2<f32>,
    zoom_c: f32,
    factor: f32,
    tint: vec4<f32>,
};

@group(0) @binding(0) var scene_tex: texture_2d<f32>;
@group(0) @binding(1) var scene_smp: sampler;
#ifdef NO_DEPTH
@group(0) @binding(2) var<uniform> gr: GodRay;
#else
@group(0) @binding(2) var depth_tex: texture_depth_2d;
@group(0) @binding(3) var<uniform> gr: GodRay;
#endif

// mask pass: only open sky feeds the rays. The sky is geometry (sky.rs cloud sphere at 900 m),
// so "sky" = anything farther than 400 m: reverse-Z infinite depth = near / view_z, near 0.05.
const SKY_DEPTH: f32 = 0.05 / 400.0;
fn masked(uv: vec2<f32>) -> vec3<f32> {
#ifndef NO_DEPTH
    let dims = vec2<f32>(textureDimensions(depth_tex));
    let p = clamp(vec2<i32>(uv * dims), vec2<i32>(0), vec2<i32>(dims) - vec2<i32>(1));
    let d = textureLoad(depth_tex, p, 0);
    if (d > SKY_DEPTH) {
        return vec3<f32>(0.0);
    }
#endif
    // full-strength mask, as in the original (user-confirmed match, GPU run_23872): the whole
    // open sky feeds the rays and they may wash over the Rex
    return textureSampleLevel(scene_tex, scene_smp, clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0)), 0.0).rgb;
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let scene = textureSampleLevel(scene_tex, scene_smp, in.uv, 0.0);
#ifdef PASSTHROUGH
    return scene;
#endif
    if (gr.factor <= 0.0) {
        return scene;
    }
    let L = gr.light_uv;
    var f: array<f32, 7>;
    var a: array<f32, 7>;
    for (var k = 0u; k < 7u; k++) {
        let h = pow(0.5, f32(k));
        f[k] = 1.0 + gr.zoom_c * h;
        a[k] = 0.5 - h / 6.0;
    }
    var acc = vec3<f32>(0.0);
    for (var bits = 0u; bits < 128u; bits++) {
        var w = 1.0;
        var s_outer = 1.0; // stages 2..6: applied before the vignette (pass 0 output position)
        var s_inner = 1.0; // stages 0..1: pass 0's own taps into the masked scene
        for (var k = 0u; k < 7u; k++) {
            let on = ((bits >> k) & 1u) == 1u;
            if (on) {
                w *= a[k];
                if (k >= 2u) { s_outer *= f[k]; } else { s_inner *= f[k]; }
            } else {
                w *= 1.0 - a[k];
            }
        }
        if (w < 1e-5) { continue; }
        let q = L + (in.uv - L) * s_outer;
        let ndc = q * 2.0 - 1.0;
        let vig = max(0.0, 1.0 - dot(ndc, ndc));
        if (vig <= 0.0) { continue; }
        let p = L + (q - L) * s_inner;
        acc += w * vig * masked(p);
    }
    let blur = acc * gr.tint.rgb;
    let outc = scene.rgb + blur * gr.factor;
    // never let a bad sample blank the frame
    if (any(outc != outc) || any(outc > vec3<f32>(1e6))) {
        return scene;
    }
    return vec4<f32>(outc, scene.a);
}
