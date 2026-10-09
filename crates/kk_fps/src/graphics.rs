//! Remaster graphics: every modern rendering feature of the rebuild as a setting on top of the
//! recovered 2005 look.
//!
//! * `Preset::Original` keeps the look matched to the user's reference frames (the recovered lights, fog,
//!   god ray, Kong's shell fur) and turns every modern feature off.
//! * `Preset::Remaster` keeps the same scenes and data and adds modern rendering: upscaling (AMD FSR 1 or
//!   NVIDIA DLSS), screen-space ambient occlusion, image-based global illumination from the sky, hardware ray
//!   tracing (Bevy Solari), contact shadows, sharper shadows, filmic tonemapping, bloom, depth of field, motion
//!   blur, vignette, chromatic aberration and sharpening.
//!
//! Every feature can also be switched one by one in the in-game menu (F10) and is saved to
//! `kk_settings.json` next to the executable (`KK_SETTINGS` overrides the path). RTX Remix cannot hook this
//! renderer (it only intercepts DirectX 8/9 fixed-function games); the ray-traced path here is Bevy's own
//! Solari renderer instead (`raytracing` cargo feature), and DLSS is Bevy's DLSS integration (`dlss` feature).
//!
//! Nothing here changes game data or mechanics; all values are presentation choices [G].

use crate::player::MainCam;
use bevy::anti_alias::contrast_adaptive_sharpening::ContrastAdaptiveSharpening;
use bevy::anti_alias::fxaa::Fxaa;
use bevy::anti_alias::smaa::Smaa;
use bevy::anti_alias::taa::TemporalAntiAliasing;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::light::{DirectionalLightShadowMap, GeneratedEnvironmentMapLight};
use bevy::pbr::{ContactShadows, ScreenSpaceAmbientOcclusion, ScreenSpaceAmbientOcclusionQualityLevel};
use bevy::post_process::dof::{DepthOfField, DepthOfFieldMode};
use bevy::post_process::effect_stack::{ChromaticAberration, Vignette};
use bevy::post_process::motion_blur::MotionBlur;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// ------------------------------------------------------------------------------------------------ settings

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Preset {
    /// the recovered 2005 look, modern features off
    Original,
    /// modern rendering on top of the same scenes
    Remaster,
    /// the user changed individual settings
    Custom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Upscaler {
    Native,
    /// AMD FidelityFX Super Resolution 1.0 (spatial, any GPU); value = render scale preset
    Fsr(FsrQuality),
    /// NVIDIA DLSS Super Resolution (RTX GPUs, Vulkan, `dlss` build feature)
    Dlss(DlssQuality),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FsrQuality {
    UltraQuality,
    Quality,
    Balanced,
    Performance,
}

impl FsrQuality {
    /// AMD's per-axis scale factors for FSR 1.0 (1.3x, 1.5x, 1.7x, 2.0x)
    pub fn scale(self) -> f32 {
        match self {
            FsrQuality::UltraQuality => 1.0 / 1.3,
            FsrQuality::Quality => 1.0 / 1.5,
            FsrQuality::Balanced => 1.0 / 1.7,
            FsrQuality::Performance => 0.5,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DlssQuality {
    Auto,
    Dlaa,
    Quality,
    Balanced,
    Performance,
    UltraPerformance,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AntiAliasing {
    Off,
    Fxaa,
    Smaa,
    Taa,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Quality {
    Off,
    Low,
    Medium,
    High,
    Ultra,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GlobalIllumination {
    /// the original constant ambient term
    Off,
    /// image-based diffuse + specular light from a sky probe generated from the level's sky colours
    SkyProbe,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToneMapper {
    /// what the reference-matched look uses
    Original,
    AgX,
    AcesFitted,
    BlenderFilmic,
    KhronosPbrNeutral,
}

#[derive(Resource, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GraphicsSettings {
    pub preset: Preset,
    pub upscaler: Upscaler,
    /// RCAS / CAS sharpening strength 0..1 (FSR, DLSS, TAA)
    pub sharpness: f32,
    pub anti_aliasing: AntiAliasing,
    pub ambient_occlusion: Quality,
    pub global_illumination: GlobalIllumination,
    /// hardware ray-traced direct + indirect light (Bevy Solari); needs a restart
    pub ray_tracing: bool,
    pub contact_shadows: bool,
    pub shadow_quality: Quality,
    pub tonemapper: ToneMapper,
    pub bloom: bool,
    pub depth_of_field: bool,
    pub motion_blur: bool,
    pub vignette: bool,
    pub chromatic_aberration: bool,
}

impl Default for GraphicsSettings {
    fn default() -> Self {
        // the software-GL test runs always render the original look (most modern passes need compute)
        if software_gl() {
            Self::original()
        } else {
            Self::remaster()
        }
    }
}

impl GraphicsSettings {
    pub fn original() -> Self {
        Self {
            preset: Preset::Original,
            upscaler: Upscaler::Native,
            sharpness: 0.0,
            anti_aliasing: AntiAliasing::Off,
            ambient_occlusion: Quality::Off,
            global_illumination: GlobalIllumination::Off,
            ray_tracing: false,
            contact_shadows: false,
            shadow_quality: Quality::Medium,
            tonemapper: ToneMapper::Original,
            bloom: true,
            depth_of_field: false,
            motion_blur: false,
            vignette: false,
            chromatic_aberration: false,
        }
    }

    pub fn remaster() -> Self {
        Self {
            preset: Preset::Remaster,
            upscaler: Upscaler::Native,
            sharpness: 0.35,
            anti_aliasing: AntiAliasing::Taa,
            ambient_occlusion: Quality::High,
            global_illumination: GlobalIllumination::SkyProbe,
            ray_tracing: false,
            contact_shadows: true,
            shadow_quality: Quality::High,
            tonemapper: ToneMapper::AgX,
            bloom: true,
            depth_of_field: false,
            motion_blur: false,
            vignette: true,
            chromatic_aberration: false,
        }
    }

    pub fn path() -> PathBuf {
        if let Ok(p) = std::env::var("KK_SETTINGS") {
            return PathBuf::from(p);
        }
        std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.join("kk_settings.json"))).unwrap_or_else(|| PathBuf::from("kk_settings.json"))
    }

    /// The saved file's `graphics` section, or the defaults.
    pub fn load() -> Self {
        if software_gl() {
            return Self::original();
        }
        let v: Option<serde_json::Value> = std::fs::read_to_string(Self::path()).ok().and_then(|t| serde_json::from_str(&t).ok());
        v.and_then(|v| serde_json::from_value(v["graphics"].clone()).ok()).unwrap_or_default()
    }
}

pub fn software_gl() -> bool {
    std::env::var("KK_SOFTWARE_GL").is_ok()
}

/// Write one section of `kk_settings.json`, keeping the others (graphics / audio / mods share the file).
pub fn save_section(name: &str, value: serde_json::Value) {
    if software_gl() {
        return;
    }
    let path = GraphicsSettings::path();
    let mut root: serde_json::Value = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_else(|| serde_json::json!({}));
    if !root.is_object() {
        root = serde_json::json!({});
    }
    root[name] = value;
    if let Err(e) = std::fs::write(&path, serde_json::to_string_pretty(&root).unwrap_or_default()) {
        warn!("could not save {}: {e}", path.display());
    }
}

/// What this build / GPU can do (greys out menu entries).
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct GraphicsCaps {
    pub dlss_built: bool,
    pub dlss_supported: bool,
    pub raytracing_built: bool,
    /// ray tracing enabled for this session (decided at start-up from the settings and the GPU)
    pub raytracing_active: bool,
    pub compute: bool,
}

/// FSR 1.0 on the world camera: render scale and RCAS strength (read by `fsr.rs`).
#[derive(Component, Clone, Copy, Debug)]
pub struct FsrUpscale {
    pub scale: f32,
    pub sharpness: f32,
}

// ------------------------------------------------------------------------------------------------ plugin

pub struct GraphicsPlugin;

impl Plugin for GraphicsPlugin {
    fn build(&self, app: &mut App) {
        let settings = GraphicsSettings::load();
        info!("graphics: {:?} preset (settings {})", settings.preset, GraphicsSettings::path().display());
        let caps = GraphicsCaps {
            dlss_built: cfg!(feature = "dlss"),
            raytracing_built: cfg!(feature = "raytracing"),
            raytracing_active: cfg!(feature = "raytracing") && settings.ray_tracing && !software_gl(),
            compute: !software_gl(),
            ..default()
        };
        app.insert_resource(settings)
            .insert_resource(caps)
            .init_resource::<SkyProbe>()
            .add_plugins(crate::fsr::FsrPlugin)
            .add_systems(Update, (detect_caps, apply_camera, apply_lights, crate::settings_menu::menu).chain());
    }
}

#[cfg(feature = "dlss")]
fn detect_caps(mut caps: ResMut<GraphicsCaps>, sup: Option<Res<bevy::anti_alias::dlss::DlssSuperResolutionSupported>>) {
    let s = sup.is_some();
    if caps.dlss_supported != s {
        caps.dlss_supported = s;
    }
}

#[cfg(not(feature = "dlss"))]
fn detect_caps() {}

#[derive(Resource, Default)]
struct SkyProbe(Option<Handle<Image>>);

/// A small cube map of the overcast sky + ground bounce, built from the scene's fog colour and sun direction; the
/// GPU filters it into diffuse / specular environment light (`GeneratedEnvironmentMapLight`).
fn sky_probe_image(images: &mut Assets<Image>, sky: Color, ground: Color, sun: Vec3) -> Handle<Image> {
    use bevy::asset::RenderAssetUsages;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureViewDescriptor, TextureViewDimension};
    const N: u32 = 32;
    let s = sky.to_linear();
    let g = ground.to_linear();
    let mut data = Vec::with_capacity((N * N * 6 * 8) as usize);
    // +X, -X, +Y, -Y, +Z, -Z
    let face = |f: u32, u: f32, v: f32| -> Vec3 {
        match f {
            0 => Vec3::new(1.0, -v, -u),
            1 => Vec3::new(-1.0, -v, u),
            2 => Vec3::new(u, 1.0, v),
            3 => Vec3::new(u, -1.0, -v),
            4 => Vec3::new(u, -v, 1.0),
            _ => Vec3::new(-u, -v, -1.0),
        }
        .normalize()
    };
    for f in 0..6 {
        for y in 0..N {
            for x in 0..N {
                let u = (x as f32 + 0.5) / N as f32 * 2.0 - 1.0;
                let v = (y as f32 + 0.5) / N as f32 * 2.0 - 1.0;
                let d = face(f, u, v);
                let up = d.y.clamp(-1.0, 1.0);
                // sky above the horizon, darker ground bounce below, a soft bright gap toward the sun
                let k = (up * 4.0).clamp(-1.0, 1.0) * 0.5 + 0.5;
                let base = Vec3::new(g.red, g.green, g.blue).lerp(Vec3::new(s.red, s.green, s.blue) * 1.6, k);
                let glow = d.dot(sun).max(0.0).powf(16.0) * 6.0;
                let c = base + Vec3::splat(glow) * Vec3::new(1.0, 0.97, 0.9);
                for ch in [c.x, c.y, c.z, 1.0] {
                    data.extend_from_slice(&half::f16::from_f32(ch).to_le_bytes());
                }
            }
        }
    }
    let mut img = Image::new(
        Extent3d { width: N, height: N, depth_or_array_layers: 6 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba16Float,
        RenderAssetUsages::RENDER_WORLD,
    );
    img.texture_view_descriptor = Some(TextureViewDescriptor { dimension: Some(TextureViewDimension::Cube), ..default() });
    images.add(img)
}

#[allow(clippy::type_complexity)]
fn apply_camera(
    mut commands: Commands,
    settings: Res<GraphicsSettings>,
    caps: Res<GraphicsCaps>,
    cams: Query<(Entity, Ref<MainCam>, Option<&DistanceFog>, Has<bevy::post_process::bloom::Bloom>), With<Camera3d>>,
    mut probe: ResMut<SkyProbe>,
    mut images: ResMut<Assets<Image>>,
    mut last: Local<Option<GraphicsSettings>>,
) {
    let changed = last.as_ref() != Some(&*settings);
    for (e, added, fog, has_bloom) in &cams {
        if !changed && !added.is_added() {
            continue;
        }
        let s = &*settings;
        let mut c = commands.entity(e);
        // --- upscaling / anti-aliasing (mutually exclusive temporal passes)
        let mut use_taa = s.anti_aliasing == AntiAliasing::Taa && caps.compute;
        c.remove::<FsrUpscale>();
        #[cfg(feature = "dlss")]
        c.remove::<bevy::anti_alias::dlss::Dlss>();
        match s.upscaler {
            Upscaler::Fsr(q) => {
                c.insert(FsrUpscale { scale: q.scale(), sharpness: s.sharpness });
                use_taa = false;
            }
            Upscaler::Dlss(_q) => {
                #[cfg(feature = "dlss")]
                if caps.dlss_supported {
                    use bevy::anti_alias::dlss::{Dlss, DlssPerfQualityMode};
                    let mode = match _q {
                        DlssQuality::Auto => DlssPerfQualityMode::Auto,
                        DlssQuality::Dlaa => DlssPerfQualityMode::Dlaa,
                        DlssQuality::Quality => DlssPerfQualityMode::Quality,
                        DlssQuality::Balanced => DlssPerfQualityMode::Balanced,
                        DlssQuality::Performance => DlssPerfQualityMode::Performance,
                        DlssQuality::UltraPerformance => DlssPerfQualityMode::UltraPerformance,
                    };
                    c.insert(Dlss { perf_quality_mode: mode, ..default() });
                    use_taa = false;
                }
            }
            Upscaler::Native => {}
        }
        // test hook: FSR at a forced scale even in the Original preset (software-GL batches)
        if let Some(sc) = std::env::var("KK_FORCE_FSR").ok().and_then(|v| v.parse::<f32>().ok()) {
            c.insert(FsrUpscale { scale: sc, sharpness: 0.3 });
            use_taa = false;
        }
        c.remove::<(Fxaa, Smaa, TemporalAntiAliasing)>();
        if use_taa {
            c.insert(TemporalAntiAliasing::default());
        } else {
            match s.anti_aliasing {
                AntiAliasing::Fxaa => {
                    c.insert(Fxaa::default());
                }
                AntiAliasing::Smaa => {
                    c.insert(Smaa::default());
                }
                _ => {}
            }
        }
        // CAS after TAA / DLSS (FSR has its own RCAS)
        let cas = s.sharpness > 0.01 && !matches!(s.upscaler, Upscaler::Fsr(_)) && (use_taa || matches!(s.upscaler, Upscaler::Dlss(_)));
        if cas {
            c.insert(ContrastAdaptiveSharpening { enabled: true, sharpening_strength: s.sharpness.clamp(0.0, 1.0), denoise: false });
        } else {
            c.remove::<ContrastAdaptiveSharpening>();
        }
        // --- ambient occlusion
        let ao = match s.ambient_occlusion {
            Quality::Off => None,
            Quality::Low => Some(ScreenSpaceAmbientOcclusionQualityLevel::Low),
            Quality::Medium => Some(ScreenSpaceAmbientOcclusionQualityLevel::Medium),
            Quality::High => Some(ScreenSpaceAmbientOcclusionQualityLevel::High),
            Quality::Ultra => Some(ScreenSpaceAmbientOcclusionQualityLevel::Ultra),
        };
        match ao.filter(|_| caps.compute) {
            Some(q) => {
                c.insert(ScreenSpaceAmbientOcclusion { quality_level: q, constant_object_thickness: 0.5 });
            }
            None => {
                c.remove::<ScreenSpaceAmbientOcclusion>();
            }
        }
        // --- global illumination: a sky probe filtered into environment light
        if s.global_illumination == GlobalIllumination::SkyProbe && caps.compute {
            let sky = fog.map(|f| f.color).unwrap_or(Color::srgb(0.6, 0.66, 0.63));
            let h = probe.0.get_or_insert_with(|| sky_probe_image(&mut images, sky, Color::srgb(0.10, 0.11, 0.09), crate::sky::sun_dir())).clone();
            c.insert(GeneratedEnvironmentMapLight { environment_map: h, intensity: 900.0, ..default() });
        } else {
            c.remove::<GeneratedEnvironmentMapLight>();
        }
        // --- contact shadows (the lights opt in, see apply_lights)
        if s.contact_shadows && caps.compute {
            c.insert(ContactShadows { linear_steps: 24, thickness: 0.12, length: 0.45 });
        } else {
            c.remove::<ContactShadows>();
        }
        // --- tonemapping
        c.insert(match s.tonemapper {
            ToneMapper::Original => Tonemapping::TonyMcMapface,
            ToneMapper::AgX => Tonemapping::AgX,
            ToneMapper::AcesFitted => Tonemapping::AcesFitted,
            ToneMapper::BlenderFilmic => Tonemapping::BlenderFilmic,
            ToneMapper::KhronosPbrNeutral => Tonemapping::KhronosPbrNeutral,
        });
        // --- lens effects
        if s.depth_of_field && caps.compute {
            c.insert(DepthOfField { mode: DepthOfFieldMode::Bokeh, focal_distance: 12.0, aperture_f_stops: 2.8, ..default() });
        } else {
            c.remove::<DepthOfField>();
        }
        if s.motion_blur && caps.compute {
            c.insert(MotionBlur { shutter_angle: 0.35, samples: 4 });
        } else {
            c.remove::<MotionBlur>();
        }
        if s.vignette {
            c.insert(Vignette { intensity: 0.22, radius: 0.85, smoothness: 0.6, ..default() });
        } else {
            c.remove::<Vignette>();
        }
        if s.chromatic_aberration {
            c.insert(ChromaticAberration { intensity: 0.012, max_samples: 8, ..default() });
        } else {
            c.remove::<ChromaticAberration>();
        }
        if !s.bloom {
            c.remove::<bevy::post_process::bloom::Bloom>();
        } else if !has_bloom && caps.compute && !added.is_added() {
            // switched back on: the scene's bloom (atmos.rs values)
            c.insert(bevy::post_process::bloom::Bloom {
                intensity: 0.2,
                prefilter: bevy::post_process::bloom::BloomPrefilter { threshold: 0.35, threshold_softness: 0.4 },
                ..bevy::post_process::bloom::Bloom::NATURAL
            });
        }
        #[cfg(feature = "raytracing")]
        if caps.raytracing_active {
            crate::raytrace::camera(&mut c);
        }
    }
    if changed {
        *last = Some(settings.clone());
    }
}

fn apply_lights(
    settings: Res<GraphicsSettings>,
    caps: Res<GraphicsCaps>,
    mut lights: Query<&mut DirectionalLight>,
    mut map: ResMut<DirectionalLightShadowMap>,
) {
    if !settings.is_changed() && lights.iter().all(|l| l.contact_shadows_enabled == (settings.contact_shadows && caps.compute)) {
        return;
    }
    for mut l in &mut lights {
        let want = settings.contact_shadows && caps.compute;
        if l.contact_shadows_enabled != want {
            l.contact_shadows_enabled = want;
        }
        // Solari replaces shadow maps with ray-traced visibility
        if caps.raytracing_active && l.shadow_maps_enabled {
            l.shadow_maps_enabled = false;
        }
    }
    let size = match settings.shadow_quality {
        Quality::Off | Quality::Low => 1024,
        Quality::Medium => 2048,
        Quality::High => 4096,
        Quality::Ultra => 8192,
    };
    if map.size != size {
        map.size = size;
    }
}
