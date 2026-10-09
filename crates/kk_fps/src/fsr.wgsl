// AMD FidelityFX Super Resolution 1.0 (EASU upscale + RCAS sharpen), ported to WGSL.
// Algorithm: AMD FidelityFX FSR 1.0 (ffx_fsr1.h, MIT licence, Copyright (c) 2021 Advanced Micro Devices, Inc.).
// The main pass renders into the top-left `input_size` pixels of the view target (MainPassResolutionOverride);
// EASU reconstructs the full-size image from that region, RCAS sharpens it. Both run on HDR colour encoded with
// the invertible "max" tonemap c / (1 + max(c)) that AMD recommends for HDR inputs, and decode afterwards.

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

struct FsrUniform {
    // xy: rendered size (px), zw: output size (px)
    sizes: vec4<f32>,
    // x: RCAS sharpness attenuation exp2(-stops), yzw: unused
    rcas: vec4<f32>,
}

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var src_sampler: sampler;
@group(0) @binding(2) var<uniform> fsr: FsrUniform;

fn encode(c: vec3<f32>) -> vec3<f32> {
    return c / (1.0 + max(max(c.r, c.g), c.b));
}

fn decode(c: vec3<f32>) -> vec3<f32> {
    return c / max(1.0 - max(max(c.r, c.g), c.b), 1.0e-4);
}

fn luma(c: vec3<f32>) -> f32 {
    return c.b * 0.5 + (c.r * 0.5 + c.g);
}

// ------------------------------------------------------------------------------------------------ EASU

struct Acc {
    dir: vec2<f32>,
    len: f32,
}

fn easu_set(acc: ptr<function, Acc>, w: f32, la: f32, lb: f32, lc: f32, ld: f32, le: f32) {
    //    a
    //  b c d
    //    e
    let dc = ld - lc;
    let cb = lc - lb;
    var len_x = max(abs(dc), abs(cb));
    len_x = 1.0 / max(len_x, 1.0e-6);
    let dir_x = ld - lb;
    (*acc).dir.x += dir_x * w;
    len_x = clamp(abs(dir_x) * len_x, 0.0, 1.0);
    len_x *= len_x;
    (*acc).len += len_x * w;
    let ec = le - lc;
    let ca = lc - la;
    var len_y = max(abs(ec), abs(ca));
    len_y = 1.0 / max(len_y, 1.0e-6);
    let dir_y = le - la;
    (*acc).dir.y += dir_y * w;
    len_y = clamp(abs(dir_y) * len_y, 0.0, 1.0);
    len_y *= len_y;
    (*acc).len += len_y * w;
}

struct Tap {
    c: vec3<f32>,
    w: f32,
}

fn easu_tap(t: ptr<function, Tap>, off: vec2<f32>, dir: vec2<f32>, len: vec2<f32>, lob: f32, clp: f32, c: vec3<f32>) {
    var v = vec2<f32>(off.x * dir.x + off.y * dir.y, off.x * (-dir.y) + off.y * dir.x);
    v *= len;
    var d2 = v.x * v.x + v.y * v.y;
    d2 = min(d2, clp);
    var wb = (2.0 / 5.0) * d2 - 1.0;
    var wa = lob * d2 - 1.0;
    wb *= wb;
    wa *= wa;
    wb = (25.0 / 16.0) * wb - (25.0 / 16.0 - 1.0);
    let w = wb * wa;
    (*t).c += c * w;
    (*t).w += w;
}

// texel fetch inside the rendered region (clamped), encoded
fn px(p: vec2<i32>) -> vec3<f32> {
    let hi = vec2<i32>(fsr.sizes.xy) - vec2<i32>(1);
    return encode(textureLoad(src, clamp(p, vec2<i32>(0), hi), 0).rgb);
}

@fragment
fn easu(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let in_size = fsr.sizes.xy;
    let out_size = fsr.sizes.zw;
    // output pixel -> input position (pixel centres)
    let ip = floor(in.position.xy);
    var pp = (ip + 0.5) * (in_size / out_size) - 0.5;
    let fp = floor(pp);
    pp -= fp;
    let f0 = vec2<i32>(fp);
    //    b c
    //  e f g h
    //  i j k l
    //    n o
    let b = px(f0 + vec2<i32>(0, -1));
    let c = px(f0 + vec2<i32>(1, -1));
    let e = px(f0 + vec2<i32>(-1, 0));
    let f = px(f0 + vec2<i32>(0, 0));
    let g = px(f0 + vec2<i32>(1, 0));
    let h = px(f0 + vec2<i32>(2, 0));
    let i = px(f0 + vec2<i32>(-1, 1));
    let j = px(f0 + vec2<i32>(0, 1));
    let k = px(f0 + vec2<i32>(1, 1));
    let l = px(f0 + vec2<i32>(2, 1));
    let n = px(f0 + vec2<i32>(0, 2));
    let o = px(f0 + vec2<i32>(1, 2));
    let bl = luma(b);
    let cl = luma(c);
    let el = luma(e);
    let fl = luma(f);
    let gl = luma(g);
    let hl = luma(h);
    let il = luma(i);
    let jl = luma(j);
    let kl = luma(k);
    let ll = luma(l);
    let nl = luma(n);
    let ol = luma(o);
    var acc: Acc;
    acc.dir = vec2<f32>(0.0);
    acc.len = 0.0;
    easu_set(&acc, (1.0 - pp.x) * (1.0 - pp.y), bl, el, fl, gl, jl);
    easu_set(&acc, pp.x * (1.0 - pp.y), cl, fl, gl, hl, kl);
    easu_set(&acc, (1.0 - pp.x) * pp.y, fl, il, jl, kl, nl);
    easu_set(&acc, pp.x * pp.y, gl, jl, kl, ll, ol);
    var dir = acc.dir;
    let dir2 = dir * dir;
    var dir_r = dir2.x + dir2.y;
    let zro = dir_r < (1.0 / 32768.0);
    dir_r = inverseSqrt(max(dir_r, 1.0e-12));
    if (zro) {
        dir_r = 1.0;
        dir.x = 1.0;
    }
    dir *= dir_r;
    var len = acc.len * 0.5;
    len *= len;
    let stretch = (dir.x * dir.x + dir.y * dir.y) / max(max(abs(dir.x), abs(dir.y)), 1.0e-6);
    let len2 = vec2<f32>(1.0 + (stretch - 1.0) * len, 1.0 - 0.5 * len);
    let lob = 0.5 + ((1.0 / 4.0 - 0.04) - 0.5) * len;
    let clp = 1.0 / lob;
    let mn = min(min(f, g), min(j, k));
    let mx = max(max(f, g), max(j, k));
    var t: Tap;
    t.c = vec3<f32>(0.0);
    t.w = 0.0;
    easu_tap(&t, vec2<f32>(0.0, -1.0) - pp, dir, len2, lob, clp, b);
    easu_tap(&t, vec2<f32>(1.0, -1.0) - pp, dir, len2, lob, clp, c);
    easu_tap(&t, vec2<f32>(-1.0, 1.0) - pp, dir, len2, lob, clp, i);
    easu_tap(&t, vec2<f32>(0.0, 1.0) - pp, dir, len2, lob, clp, j);
    easu_tap(&t, vec2<f32>(0.0, 0.0) - pp, dir, len2, lob, clp, f);
    easu_tap(&t, vec2<f32>(-1.0, 0.0) - pp, dir, len2, lob, clp, e);
    easu_tap(&t, vec2<f32>(1.0, 1.0) - pp, dir, len2, lob, clp, k);
    easu_tap(&t, vec2<f32>(2.0, 1.0) - pp, dir, len2, lob, clp, l);
    easu_tap(&t, vec2<f32>(2.0, 0.0) - pp, dir, len2, lob, clp, h);
    easu_tap(&t, vec2<f32>(1.0, 0.0) - pp, dir, len2, lob, clp, g);
    easu_tap(&t, vec2<f32>(1.0, 2.0) - pp, dir, len2, lob, clp, o);
    easu_tap(&t, vec2<f32>(0.0, 2.0) - pp, dir, len2, lob, clp, n);
    let col = min(mx, max(mn, t.c / max(t.w, 1.0e-6)));
    return vec4<f32>(decode(col), 1.0);
}

// ------------------------------------------------------------------------------------------------ RCAS

fn full(p: vec2<i32>) -> vec3<f32> {
    let hi = vec2<i32>(fsr.sizes.zw) - vec2<i32>(1);
    return encode(textureLoad(src, clamp(p, vec2<i32>(0), hi), 0).rgb);
}

@fragment
fn rcas(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let ip = vec2<i32>(floor(in.position.xy));
    //    b
    //  d e f
    //    h
    let b = full(ip + vec2<i32>(0, -1));
    let d = full(ip + vec2<i32>(-1, 0));
    let e = full(ip);
    let f = full(ip + vec2<i32>(1, 0));
    let h = full(ip + vec2<i32>(0, 1));
    let bl = luma(b);
    let dl = luma(d);
    let el = luma(e);
    let fl = luma(f);
    let hl = luma(h);
    // noise detection
    var nz = 0.25 * bl + 0.25 * dl + 0.25 * fl + 0.25 * hl - el;
    let rng = max(max(max(bl, dl), max(el, fl)), hl) - min(min(min(bl, dl), min(el, fl)), hl);
    nz = clamp(abs(nz) / max(rng, 1.0e-6), 0.0, 1.0);
    nz = -0.5 * nz + 1.0;
    let mn4 = min(min(b, d), min(f, h));
    let mx4 = max(max(b, d), max(f, h));
    let peak = vec2<f32>(1.0, -4.0);
    let hit_min = mn4 / max(4.0 * mx4, vec3<f32>(1.0e-6));
    let hit_max = (peak.x - mx4) / min(4.0 * mn4 + peak.y, vec3<f32>(-1.0e-6));
    let lobe_rgb = max(-hit_min, hit_max);
    let limit = 0.25 - 1.0 / 16.0;
    var lobe = max(-limit, min(max(max(lobe_rgb.r, lobe_rgb.g), lobe_rgb.b), 0.0)) * fsr.rcas.x;
    lobe *= nz;
    let rcp_l = 1.0 / (4.0 * lobe + 1.0);
    let col = (lobe * b + lobe * d + lobe * h + lobe * f + e) * rcp_l;
    return vec4<f32>(decode(clamp(col, vec3<f32>(0.0), vec3<f32>(0.999))), 1.0);
}
