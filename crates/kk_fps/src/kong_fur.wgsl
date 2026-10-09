// Kong's shell fur: Jade's MDF FUR modifier (MDFmodifier_FUR.c, KingKong8.exe fn@0x00a1c990 / fn@0x00a4c090).
// The engine re-draws the skinned mesh N times; shell i (1..N) is pushed out along the vertex normal by i * len / N,
// its texture coordinates are shifted by i * (du, dv), and it is alpha-tested with ref i / N (GL_GEQUAL) against the
// alpha of the material's fur layer (a strand height map) sampled through that layer's UV matrix. See kong_fur.rs.

#import bevy_pbr::{
    mesh_functions,
    skinning,
    forward_io::{Vertex, VertexOutput, FragmentOutput},
    view_transformations::position_world_to_clip,
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
}

struct FurUniform {
    // x: offset of this shell along the normal (m), y: alpha-test ref (i/N), z: shade (root darkening), w: unused
    shell: vec4<f32>,
    // fur layer UV matrix rows (m0, m1), (m2, m3) and offset (m4, m5) - Jade layout u' = m0 u + m2 v + m4
    m: vec4<f32>,
    // xy: (m4, m5), zw: texcoord shift of this shell (i * du, i * dv)
    off_shift: vec4<f32>,
}

@group(2) @binding(100) var<uniform> fur: FurUniform;
@group(2) @binding(101) var fur_texture: texture_2d<f32>;
@group(2) @binding(102) var fur_sampler: sampler;

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;
#ifdef SKINNED
    var world_from_local = skinning::skin_model(vertex.joint_indices, vertex.joint_weights, vertex.instance_index);
#else
    var world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
#endif

#ifdef VERTEX_NORMALS
#ifdef SKINNED
    out.world_normal = skinning::skin_normals(world_from_local, vertex.normal);
#else
    out.world_normal = mesh_functions::mesh_normal_local_to_world(vertex.normal, vertex.instance_index);
#endif
#endif

#ifdef VERTEX_POSITIONS
    out.world_position = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(vertex.position, 1.0));
#ifdef VERTEX_NORMALS
    // per-vertex length: 1 - RLI.a, carried in the vertex colour alpha (vsfur.hlsl); 1 without a mask
    var len_k = 1.0;
#ifdef VERTEX_COLORS
    len_k = vertex.color.a;
#endif
    out.world_position = vec4<f32>(out.world_position.xyz + normalize(out.world_normal) * fur.shell.x * len_k, out.world_position.w);
#endif
    out.position = position_world_to_clip(out.world_position.xyz);
#endif

#ifdef VERTEX_UVS_A
    out.uv = vertex.uv + fur.off_shift.zw;
#endif
#ifdef VERTEX_UVS_B
    out.uv_b = vertex.uv_b;
#endif
#ifdef VERTEX_TANGENTS
    out.world_tangent = mesh_functions::mesh_tangent_local_to_world(world_from_local, vertex.tangent, vertex.instance_index);
#endif
#ifdef VERTEX_COLORS
    out.color = vertex.color;
#endif
#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    out.instance_index = vertex.instance_index;
#endif
    return out;
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
#ifdef VERTEX_UVS_A
    let uv = in.uv;
#else
    let uv = vec2<f32>(0.0);
#endif
    let fuv = vec2<f32>(fur.m.x * uv.x + fur.m.z * uv.y + fur.off_shift.x, fur.m.y * uv.x + fur.m.w * uv.y + fur.off_shift.y);
    let a = textureSample(fur_texture, fur_sampler, fuv).a;
    if (a < fur.shell.y) {
        discard;
    }
#ifdef VERTEX_COLORS
    // bare skin (face, chest, palms): no shells at all, they would only z-fight the base surface
    if (in.color.a < 0.04) {
        discard;
    }
#endif
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    pbr_input.material.base_color = vec4<f32>(pbr_input.material.base_color.rgb * fur.shell.z, 1.0);
    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
