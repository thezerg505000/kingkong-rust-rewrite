//! AMD FidelityFX Super Resolution 1.0 for the world camera (see `fsr.wgsl`).
//!
//! The main pass renders a smaller image (`MainPassResolutionOverride`, the same mechanism Bevy's DLSS uses)
//! into the top-left corner of the full-size view target; right after the transparent pass EASU rebuilds the
//! full-size frame from it and RCAS sharpens it, so bloom, the god ray, fog post-effects and tonemapping all
//! work on a full-resolution image.

use crate::graphics::FsrUpscale;
use bevy::camera::MainPassResolutionOverride;
use bevy::core_pipeline::core_3d::main_transparent_pass_3d;
use bevy::core_pipeline::schedule::{Core3d, Core3dSystems};
use bevy::core_pipeline::FullscreenShader;
use bevy::prelude::*;
use bevy::render::camera::ExtractedCamera;
use bevy::render::render_resource::binding_types::{sampler, texture_2d, uniform_buffer};
use bevy::render::render_resource::*;
use bevy::render::renderer::{RenderContext, RenderDevice, ViewQuery};
use bevy::render::sync_world::RenderEntity;
use bevy::render::view::ViewTarget;
use bevy::render::{ExtractSchedule, MainWorld, RenderApp, RenderStartup};

pub struct FsrPlugin;

impl Plugin for FsrPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "fsr.wgsl");
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        render_app
            .add_systems(RenderStartup, init_pipeline)
            .add_systems(ExtractSchedule, extract_fsr)
            .add_systems(Core3d, fsr_pass.after(main_transparent_pass_3d).in_set(Core3dSystems::MainPass));
    }
}

/// Render-world copy of the camera's FSR settings with the resolved sizes.
#[derive(Component, Clone, Copy)]
struct FsrView {
    render: UVec2,
    output: UVec2,
    sharpness: f32,
}

#[derive(Clone, Copy, ShaderType)]
struct FsrUniform {
    sizes: Vec4,
    rcas: Vec4,
}

fn extract_fsr(mut commands: Commands, mut main_world: ResMut<MainWorld>, had: Query<Has<FsrView>>) {
    let mut q = main_world.query::<(RenderEntity, &Camera, Option<&FsrUpscale>)>();
    for (re, cam, fsr) in q.iter(&main_world) {
        let Ok(mut ec) = commands.get_entity(re) else { continue };
        match (fsr, cam.physical_viewport_size()) {
            (Some(f), Some(out)) if cam.is_active && f.scale < 0.999 => {
                let render = (out.as_vec2() * f.scale.clamp(0.25, 1.0)).round().as_uvec2().max(UVec2::ONE);
                ec.insert((FsrView { render, output: out, sharpness: f.sharpness }, MainPassResolutionOverride(render)));
            }
            _ => {
                if had.get(re) == Ok(true) {
                    ec.remove::<(FsrView, MainPassResolutionOverride)>();
                }
            }
        }
    }
}

#[derive(Resource)]
struct FsrPipelines {
    layout: BindGroupLayoutDescriptor,
    sampler: Sampler,
    easu: CachedRenderPipelineId,
    rcas: CachedRenderPipelineId,
}

fn init_pipeline(mut commands: Commands, device: Res<RenderDevice>, fullscreen: Res<FullscreenShader>, server: Res<AssetServer>, cache: Res<PipelineCache>) {
    let layout = BindGroupLayoutDescriptor::new(
        "fsr_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (texture_2d(TextureSampleType::Float { filterable: true }), sampler(SamplerBindingType::Filtering), uniform_buffer::<FsrUniform>(false)),
        ),
    );
    let sampler = device.create_sampler(&SamplerDescriptor { mag_filter: FilterMode::Linear, min_filter: FilterMode::Linear, ..default() });
    let shader: Handle<Shader> = server.load("embedded://kk_fps/fsr.wgsl");
    let mk = |entry: &'static str| RenderPipelineDescriptor {
        label: Some(format!("fsr_{entry}").into()),
        layout: vec![layout.clone()],
        vertex: fullscreen.to_vertex_state(),
        fragment: Some(FragmentState {
            shader: shader.clone(),
            shader_defs: vec![],
            entry_point: Some(entry.into()),
            targets: vec![Some(ColorTargetState { format: TextureFormat::Rgba16Float, blend: None, write_mask: ColorWrites::ALL })],
        }),
        ..default()
    };
    let easu = cache.queue_render_pipeline(mk("easu"));
    let rcas = cache.queue_render_pipeline(mk("rcas"));
    commands.insert_resource(FsrPipelines { layout, sampler, easu, rcas });
}

fn fsr_pass(view: ViewQuery<(&ViewTarget, &FsrView, &ExtractedCamera)>, pipes: Option<Res<FsrPipelines>>, cache: Res<PipelineCache>, mut ctx: RenderContext) {
    let Some(pipes) = pipes else { return };
    let (target, fsr, _cam) = view.into_inner();
    let (Some(easu), Some(rcas)) = (cache.get_render_pipeline(pipes.easu), cache.get_render_pipeline(pipes.rcas)) else { return };
    let layout = cache.get_bind_group_layout(&pipes.layout);
    let u = FsrUniform {
        sizes: Vec4::new(fsr.render.x as f32, fsr.render.y as f32, fsr.output.x as f32, fsr.output.y as f32),
        // RCAS attenuation: 0 stops = sharpest; sharpness 1 -> 0 stops, 0 -> 2 stops
        rcas: Vec4::new((-(2.0 * (1.0 - fsr.sharpness.clamp(0.0, 1.0)))).exp2(), 0.0, 0.0, 0.0),
    };
    let mut buf = encase::UniformBuffer::new(Vec::<u8>::new());
    if buf.write(&u).is_err() {
        return;
    }
    let ubuf = ctx.render_device().create_buffer_with_data(&BufferInitDescriptor { label: Some("fsr_uniform"), contents: buf.as_ref(), usage: BufferUsages::UNIFORM });
    let mut run = |ctx: &mut RenderContext, pipeline: &RenderPipeline, label: &'static str| {
        let post = target.post_process_write();
        let bg = ctx.render_device().create_bind_group(label, &layout, &BindGroupEntries::sequential((post.source, &pipes.sampler, ubuf.as_entire_binding())));
        let mut pass = ctx.command_encoder().begin_render_pass(&RenderPassDescriptor {
            label: Some(label),
            color_attachments: &[Some(RenderPassColorAttachment { view: post.destination, depth_slice: None, resolve_target: None, ops: Operations::default() })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &bg, &[]);
        pass.draw(0..3, 0..1);
    };
    run(&mut ctx, easu, "fsr_easu");
    if fsr.sharpness > 0.01 {
        run(&mut ctx, rcas, "fsr_rcas");
    }
}
