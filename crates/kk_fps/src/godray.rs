//! Screen-space god ray, a port of King Kong's XeGodRayEffect (see godray.wgsl).
//!
//! Level 03E data [C]: the only god-ray source is LD_03E_GodRay.gao, Jade position
//! (35.0, 46.4, 7.1) -> glTF (35.0, 7.1, -46.4), forward axis (-0.595, 0.801, 0) -> glTF
//! (-0.595, 0, -0.801). The per-frame AI code (fn@0x006ee3e0) sets AFX effect 0x12 with
//! intensity = min(var0, max(0, var1 - dot(camera_dir, ray_dir))). var0/var1 are not stored
//! in the 03E stream; (1.0, 0.35) are the des_f_max_intensity / des_f_max_angle candidates [G].
//! Exec fn@0x00a2ccc0 [C]: view-angle fade = 1 under 60 deg, cos((angle-60)*0.0333) up to 90,
//! skipped beyond; zoom sign c = -0.4 when the light is in front; final factor A*(A+1).
//! Tint colour is not recovered: white [G].

use crate::player::MainCam;
use bevy::core_pipeline::schedule::{Core3d, Core3dSystems};
use bevy::core_pipeline::tonemapping::tonemapping;
use bevy::core_pipeline::FullscreenShader;
use bevy::prelude::*;
use bevy::render::extract_component::{ExtractComponent, ExtractComponentPlugin};
use bevy::render::render_resource::binding_types::{sampler, texture_2d, texture_depth_2d, uniform_buffer};
use bevy::render::render_resource::*;
use bevy::render::renderer::{RenderContext, RenderDevice, ViewQuery};
use bevy::render::uniform::{ComponentUniforms, DynamicUniformIndex, UniformComponentPlugin};
use bevy::render::view::{ViewDepthTexture, ViewTarget};
use bevy::render::{RenderApp, RenderStartup};
use bevy::shader::ShaderDefVal;

pub const GODRAY_POS: Vec3 = Vec3::new(35.0, 7.1, -46.4);
pub const GODRAY_RAY: Vec3 = Vec3::new(-0.595, 0.0, -0.801);
/// des_f_max_intensity / des_f_max_angle candidates [G]
pub const GODRAY_VAR0: f32 = 1.0;
pub const GODRAY_VAR1: f32 = 0.35;

/// Per-camera god-ray parameters, recomputed every frame on the CPU like fn@0x00a2ccc0.
#[derive(Component, Clone, Copy, Default, ExtractComponent, ShaderType)]
pub struct GodRay {
    pub light_uv: Vec2,
    pub zoom_c: f32,
    pub factor: f32,
    pub tint: Vec4,
    /// xy: depth uv scale (FSR renders the main pass into the top-left corner of the depth buffer)
    pub depth_scale: Vec4,
}

#[derive(Component)]
pub struct GodRaySource {
    pub pos: Vec3,
    pub ray: Vec3,
}

/// Current god-ray intensity (tests / HUD).
#[derive(Resource, Default)]
pub struct GodRayStats {
    pub intensity: f32,
    pub fade: f32,
}

/// KK_GODRAY_DEBUG: "nodepth" (no occlusion mask, works on software GL) or "pass" (pass-through)
fn debug_mode() -> String {
    std::env::var("KK_GODRAY_DEBUG").unwrap_or_default()
}
fn no_depth() -> bool {
    std::env::var("KK_SOFTWARE_GL").is_ok() || debug_mode().contains("nodepth") || debug_mode().contains("pass")
}

pub fn enabled() -> bool {
    std::env::var("KK_NO_GODRAY").is_err() && (std::env::var("KK_SOFTWARE_GL").is_err() || !debug_mode().is_empty())
}

pub struct GodRayPlugin;

impl Plugin for GodRayPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "godray.wgsl");
        app.init_resource::<GodRayStats>()
            .add_plugins((ExtractComponentPlugin::<GodRay>::default(), UniformComponentPlugin::<GodRay>::default()))
            .add_systems(Update, update_godray);
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        render_app
            .add_systems(RenderStartup, init_godray_pipeline)
            .add_systems(Core3d, godray_pass.before(tonemapping).in_set(Core3dSystems::PostProcess));
    }
}

fn update_godray(
    mut cams: Query<(&Camera, &GlobalTransform, &mut GodRay, Option<&crate::graphics::FsrUpscale>), With<MainCam>>,
    sources: Query<&GodRaySource>,
    mut stats: ResMut<GodRayStats>,
) {
    let Ok((cam, gt, mut gr, fsr)) = cams.single_mut() else { return };
    let ds = fsr.map(|f| f.scale.clamp(0.25, 1.0)).unwrap_or(1.0);
    gr.depth_scale = Vec4::new(ds, ds, 0.0, 0.0);
    let Ok(src) = sources.single() else {
        gr.factor = 0.0;
        return;
    };
    let fwd = gt.forward().as_vec3();
    let eye = gt.translation();
    // AI side (fn@0x006ee3e0): intensity from how directly the camera faces the ray
    let intensity = GODRAY_VAR0.min((GODRAY_VAR1 - fwd.dot(src.ray)).max(0.0));
    // effect side (fn@0x00a2ccc0): angle between view direction and the light
    let to_light = (src.pos - eye).normalize_or_zero();
    let angle = fwd.dot(to_light).clamp(-1.0, 1.0).acos().to_degrees();
    let fade = if angle < 60.0 { 1.0 } else if angle <= 90.0 { ((angle - 60.0) * 0.0333).cos() } else { 0.0 };
    let in_front = fwd.dot(src.pos - eye) >= 0.0;
    let ndc = cam.world_to_ndc(gt, src.pos).unwrap_or(Vec3::ZERO);
    let a = fade * intensity;
    gr.light_uv = Vec2::new(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    gr.zoom_c = if in_front { -0.4 } else { 0.4 };
    gr.factor = a * (a + 1.0);
    gr.tint = Vec4::new(intensity, intensity, intensity, intensity);
    stats.intensity = intensity;
    stats.fade = fade;
}

fn godray_pass(
    view: ViewQuery<(&ViewTarget, Option<&ViewDepthTexture>, &DynamicUniformIndex<GodRay>)>,
    pipeline_res: Option<Res<GodRayPipeline>>,
    cache: Res<PipelineCache>,
    uniforms: Res<ComponentUniforms<GodRay>>,
    mut ctx: RenderContext,
) {
    let Some(pipeline_res) = pipeline_res else { return };
    let (view_target, prepass, uniform_index) = view.into_inner();
    let Some(pipeline) = cache.get_render_pipeline(pipeline_res.pipeline_id) else { return };
    let Some(binding) = uniforms.uniforms().binding() else { return };
    let depth = prepass.map(|d| d.view());
    if !pipeline_res.no_depth && depth.is_none() {
        return;
    }
    let post = view_target.post_process_write();
    let layout = cache.get_bind_group_layout(&pipeline_res.layout);
    let bind_group = if pipeline_res.no_depth {
        ctx.render_device().create_bind_group("godray_bind_group", &layout, &BindGroupEntries::sequential((post.source, &pipeline_res.sampler, binding.clone())))
    } else {
        ctx.render_device().create_bind_group(
            "godray_bind_group",
            &layout,
            &BindGroupEntries::sequential((post.source, &pipeline_res.sampler, depth.unwrap(), binding.clone())),
        )
    };
    let mut pass = ctx.command_encoder().begin_render_pass(&RenderPassDescriptor {
        label: Some("godray_pass"),
        color_attachments: &[Some(RenderPassColorAttachment { view: post.destination, depth_slice: None, resolve_target: None, ops: Operations::default() })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, &bind_group, &[uniform_index.index()]);
    pass.draw(0..3, 0..1);
}

#[derive(Resource)]
struct GodRayPipeline {
    no_depth: bool,
    layout: BindGroupLayoutDescriptor,
    sampler: Sampler,
    pipeline_id: CachedRenderPipelineId,
}

fn init_godray_pipeline(
    mut commands: Commands,
    device: Res<RenderDevice>,
    fullscreen: Res<FullscreenShader>,
    server: Res<AssetServer>,
    cache: Res<PipelineCache>,
) {
    let no_depth = no_depth();
    let layout = if no_depth {
        BindGroupLayoutDescriptor::new(
            "godray_layout",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::FRAGMENT,
                (texture_2d(TextureSampleType::Float { filterable: true }), sampler(SamplerBindingType::Filtering), uniform_buffer::<GodRay>(true)),
            ),
        )
    } else {
        BindGroupLayoutDescriptor::new(
            "godray_layout",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::FRAGMENT,
                (
                    texture_2d(TextureSampleType::Float { filterable: true }),
                    sampler(SamplerBindingType::Filtering),
                    texture_depth_2d(),
                    uniform_buffer::<GodRay>(true),
                ),
            ),
        )
    };
    let mut defs: Vec<ShaderDefVal> = vec![];
    if no_depth {
        defs.push("NO_DEPTH".into());
    }
    if debug_mode().contains("pass") {
        defs.push("PASSTHROUGH".into());
    }
    let sampler = device.create_sampler(&SamplerDescriptor { mag_filter: FilterMode::Linear, min_filter: FilterMode::Linear, ..default() });
    let shader = server.load("embedded://kk_fps/godray.wgsl");
    let pipeline_id = cache.queue_render_pipeline(RenderPipelineDescriptor {
        label: Some("godray_pipeline".into()),
        layout: vec![layout.clone()],
        vertex: fullscreen.to_vertex_state(),
        fragment: Some(FragmentState {
            shader,
            shader_defs: defs,
            entry_point: Some("fragment".into()),
            targets: vec![Some(ColorTargetState { format: ViewTarget::TEXTURE_FORMAT_HDR, blend: None, write_mask: ColorWrites::ALL })],
        }),
        ..default()
    });
    commands.insert_resource(GodRayPipeline { no_depth, layout, sampler, pipeline_id });
}
