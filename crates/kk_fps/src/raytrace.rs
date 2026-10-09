//! Hardware ray tracing through Bevy Solari (`raytracing` cargo feature, "Ray tracing" in the F10 menu).
//!
//! RTX Remix only hooks DirectX 8/9 fixed-function renderers, so the rebuild uses Bevy's own real-time ray
//! tracer instead: ReSTIR direct + indirect light, ray-traced shadows and specular, on GPUs with ray queries
//! (NVIDIA RTX, AMD RDNA2+, Intel Arc) under Vulkan or DX12. Experimental, like Solari itself:
//! * the scene renders deferred while it is on (decided at start-up: the setting applies after a restart);
//! * only static level meshes with standard materials enter the ray-tracing scene (Solari has no skinned
//!   meshes yet): Kong, the rex and Jack's arms are lit by it but do not cast ray-traced shadows or bounce;
//! * every level mesh gets a stripped copy with the attributes Solari wants (position, normal, uv, tangent,
//!   u32 indices).

use bevy::camera::CameraMainTextureUsages;
use bevy::ecs::system::EntityCommands;
use bevy::mesh::{Indices, VertexAttributeValues};
use bevy::pbr::DefaultOpaqueRendererMethod;
use bevy::prelude::*;
use bevy::render::render_resource::TextureUsages;
use bevy::solari::prelude::{RaytracingMesh3d, SolariLighting, SolariPlugins};
use std::collections::HashMap;

pub struct RaytracePlugin;

impl Plugin for RaytracePlugin {
    fn build(&self, app: &mut App) {
        let s = crate::graphics::GraphicsSettings::load();
        if !s.ray_tracing || crate::graphics::software_gl() {
            return;
        }
        info!("ray tracing: Bevy Solari on (deferred renderer)");
        app.insert_resource(DefaultOpaqueRendererMethod::deferred()).add_plugins(SolariPlugins).add_systems(Update, prepare_meshes);
    }
}

/// Camera components Solari needs.
pub fn camera(c: &mut EntityCommands) {
    c.insert((SolariLighting::default(), CameraMainTextureUsages::default().with(TextureUsages::STORAGE_BINDING), Msaa::Off));
}

#[derive(Component)]
struct RtDone;

/// Give every static level mesh a ray-tracing copy.
#[allow(clippy::type_complexity)]
fn prepare_meshes(
    mut commands: Commands,
    q: Query<(Entity, &Mesh3d), (With<MeshMaterial3d<StandardMaterial>>, Without<RtDone>, Without<bevy::mesh::skinning::SkinnedMesh>)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut cache: Local<HashMap<AssetId<Mesh>, Option<Handle<Mesh>>>>,
) {
    for (e, m) in &q {
        let id = m.0.id();
        let rt = match cache.get(&id) {
            Some(h) => h.clone(),
            None => {
                let Some(src) = meshes.get(&m.0) else { continue };
                let h = rt_copy(src).map(|c| meshes.add(c));
                cache.insert(id, h.clone());
                h
            }
        };
        let mut ec = commands.entity(e);
        ec.insert(RtDone);
        if let Some(h) = rt {
            ec.insert(RaytracingMesh3d(h));
        }
    }
}

fn rt_copy(src: &Mesh) -> Option<Mesh> {
    if src.primitive_topology() != bevy::mesh::PrimitiveTopology::TriangleList {
        return None;
    }
    let pos = match src.attribute(Mesh::ATTRIBUTE_POSITION)? {
        VertexAttributeValues::Float32x3(v) => v.clone(),
        _ => return None,
    };
    let n = pos.len();
    let nrm = match src.attribute(Mesh::ATTRIBUTE_NORMAL) {
        Some(VertexAttributeValues::Float32x3(v)) => v.clone(),
        _ => vec![[0.0, 1.0, 0.0]; n],
    };
    let uv = match src.attribute(Mesh::ATTRIBUTE_UV_0) {
        Some(VertexAttributeValues::Float32x2(v)) => v.clone(),
        _ => vec![[0.0, 0.0]; n],
    };
    let idx: Vec<u32> = match src.indices() {
        Some(Indices::U32(v)) => v.clone(),
        Some(Indices::U16(v)) => v.iter().map(|&i| i as u32).collect(),
        None => (0..n as u32).collect(),
    };
    let mut m = Mesh::new(bevy::mesh::PrimitiveTopology::TriangleList, bevy::asset::RenderAssetUsages::RENDER_WORLD | bevy::asset::RenderAssetUsages::MAIN_WORLD);
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, nrm);
    m.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
    m.insert_indices(Indices::U32(idx));
    match src.attribute(Mesh::ATTRIBUTE_TANGENT) {
        Some(VertexAttributeValues::Float32x4(v)) => m.insert_attribute(Mesh::ATTRIBUTE_TANGENT, v.clone()),
        _ => {
            if m.generate_tangents().is_err() {
                m.insert_attribute(Mesh::ATTRIBUTE_TANGENT, vec![[1.0, 0.0, 0.0, 1.0]; n]);
            }
        }
    }
    m.enable_raytracing = true;
    Some(m)
}
