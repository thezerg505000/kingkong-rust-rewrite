//! In-game remaster menu (F10): graphics, audio and mods, one row per setting.
//!
//! Up / Down pick a row, Left / Right (or Enter) change it, F10 / Escape close. Changes apply at once (ray
//! tracing and mod changes on the next start) and are saved to `kk_settings.json`.

use crate::audio_engine::AudioSettings;
use crate::graphics::*;
use crate::mods::ModList;
use bevy::prelude::*;

#[derive(Component)]
pub struct MenuRoot;
#[derive(Component)]
pub struct MenuText;

#[derive(Default)]
pub struct MenuState {
    open: bool,
    row: usize,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Row {
    Header(&'static str),
    Preset,
    Upscaler,
    Sharpness,
    AntiAliasing,
    AmbientOcclusion,
    GlobalIllumination,
    RayTracing,
    ContactShadows,
    Shadows,
    Tonemapper,
    Bloom,
    DepthOfField,
    MotionBlur,
    Vignette,
    ChromaticAberration,
    VolumetricFog,
    AudioEngine,
    Hrtf,
    Reverb,
    Occlusion,
    Volume,
    Mod(usize),
}

fn rows(mods: &ModList) -> Vec<Row> {
    use Row::*;
    let mut v = vec![
        Header("GRAPHICS"),
        Preset,
        Upscaler,
        Sharpness,
        AntiAliasing,
        AmbientOcclusion,
        GlobalIllumination,
        RayTracing,
        ContactShadows,
        Shadows,
        Tonemapper,
        Bloom,
        DepthOfField,
        MotionBlur,
        Vignette,
        ChromaticAberration,
    VolumetricFog,
        Header("AUDIO"),
        AudioEngine,
        Hrtf,
        Reverb,
        Occlusion,
        Volume,
        Header("MODS (applied on the next start)"),
    ];
    v.extend((0..mods.mods.len()).map(Mod));
    v
}

fn on(b: bool) -> &'static str {
    if b {
        "On"
    } else {
        "Off"
    }
}

fn label(r: Row, g: &GraphicsSettings, a: &AudioSettings, m: &ModList, caps: &GraphicsCaps) -> (String, String) {
    use Row::*;
    let s = |x: &str| x.to_string();
    match r {
        Header(h) => (s(h), String::new()),
        Preset => (s("Preset"), format!("{:?}", g.preset)),
        Upscaler => (
            s("Upscaling"),
            match g.upscaler {
                crate::graphics::Upscaler::Native => s("Native"),
                crate::graphics::Upscaler::Fsr(q) => format!("AMD FSR 1.0 {q:?} ({:.0}%)", q.scale() * 100.0),
                crate::graphics::Upscaler::Dlss(q) => {
                    let note = if !caps.dlss_built {
                        " (not in this build)"
                    } else if !caps.dlss_supported {
                        " (GPU/driver not supported)"
                    } else {
                        ""
                    };
                    format!("NVIDIA DLSS {q:?}{note}")
                }
            },
        ),
        Sharpness => (s("Sharpening"), format!("{:.0}%", g.sharpness * 100.0)),
        AntiAliasing => (s("Anti-aliasing"), format!("{:?}", g.anti_aliasing)),
        AmbientOcclusion => (s("Ambient occlusion (SSAO)"), format!("{:?}", g.ambient_occlusion)),
        GlobalIllumination => (
            s("Global illumination"),
            match g.global_illumination {
                crate::graphics::GlobalIllumination::Off => s("Off (original ambient)"),
                crate::graphics::GlobalIllumination::SkyProbe => s("Sky probe (image-based)"),
            },
        ),
        RayTracing => {
            let note = if !caps.raytracing_built {
                " (not in this build)"
            } else if g.ray_tracing != caps.raytracing_active {
                " (restart to apply)"
            } else {
                ""
            };
            (s("Ray tracing (Solari, experimental)"), format!("{}{note}", on(g.ray_tracing)))
        }
        ContactShadows => (s("Contact shadows"), s(on(g.contact_shadows))),
        Shadows => (s("Shadow quality"), format!("{:?}", g.shadow_quality)),
        Tonemapper => (s("Tonemapping"), format!("{:?}", g.tonemapper)),
        Bloom => (s("Bloom"), s(on(g.bloom))),
        DepthOfField => (s("Depth of field"), s(on(g.depth_of_field))),
        MotionBlur => (s("Motion blur"), s(on(g.motion_blur))),
        Vignette => (s("Vignette"), s(on(g.vignette))),
        ChromaticAberration => (s("Chromatic aberration"), s(on(g.chromatic_aberration))),
        VolumetricFog => (s("Volumetric fog + sun shafts"), s(on(g.volumetric_fog))),
        AudioEngine => (s("Audio engine"), format!("{:?}", a.engine)),
        Hrtf => (s("3D audio (HRTF)"), s(on(a.hrtf))),
        Reverb => (s("Environmental reverb"), s(on(a.reverb))),
        Occlusion => (s("Sound occlusion"), s(on(a.occlusion))),
        Volume => (s("Master volume"), format!("{:.0}%", a.master * 100.0)),
        Mod(i) => {
            let md = &m.mods[i];
            (format!("{} {}", md.name, md.version), s(on(md.enabled)))
        }
    }
}

fn cycle<T: Copy + PartialEq>(all: &[T], cur: T, dir: i32) -> T {
    let i = all.iter().position(|x| *x == cur).unwrap_or(0) as i32;
    let n = all.len() as i32;
    all[((i + dir).rem_euclid(n)) as usize]
}

fn change(r: Row, dir: i32, g: &mut GraphicsSettings, a: &mut AudioSettings, m: &mut ModList) {
    use crate::graphics::{AntiAliasing as A, DlssQuality as D, FsrQuality as F, GlobalIllumination as Gi, Quality as Q, ToneMapper as T, Upscaler as U};
    use Row::*;
    let before = g.clone();
    match r {
        Header(_) => {}
        Preset => {
            *g = match g.preset {
                crate::graphics::Preset::Remaster => GraphicsSettings::original(),
                _ => GraphicsSettings::remaster(),
            };
            return;
        }
        Upscaler => {
            let all = [
                U::Native,
                U::Fsr(F::UltraQuality),
                U::Fsr(F::Quality),
                U::Fsr(F::Balanced),
                U::Fsr(F::Performance),
                U::Dlss(D::Auto),
                U::Dlss(D::Dlaa),
                U::Dlss(D::Quality),
                U::Dlss(D::Balanced),
                U::Dlss(D::Performance),
                U::Dlss(D::UltraPerformance),
            ];
            g.upscaler = cycle(&all, g.upscaler, dir);
        }
        Sharpness => g.sharpness = ((g.sharpness * 10.0).round() / 10.0 + 0.1 * dir as f32).clamp(0.0, 1.0),
        AntiAliasing => g.anti_aliasing = cycle(&[A::Off, A::Fxaa, A::Smaa, A::Taa], g.anti_aliasing, dir),
        AmbientOcclusion => g.ambient_occlusion = cycle(&[Q::Off, Q::Low, Q::Medium, Q::High, Q::Ultra], g.ambient_occlusion, dir),
        GlobalIllumination => g.global_illumination = cycle(&[Gi::Off, Gi::SkyProbe], g.global_illumination, dir),
        RayTracing => g.ray_tracing = !g.ray_tracing,
        ContactShadows => g.contact_shadows = !g.contact_shadows,
        Shadows => g.shadow_quality = cycle(&[Q::Low, Q::Medium, Q::High, Q::Ultra], g.shadow_quality, dir),
        Tonemapper => g.tonemapper = cycle(&[T::Original, T::AgX, T::AcesFitted, T::BlenderFilmic, T::KhronosPbrNeutral], g.tonemapper, dir),
        Bloom => g.bloom = !g.bloom,
        DepthOfField => g.depth_of_field = !g.depth_of_field,
        MotionBlur => g.motion_blur = !g.motion_blur,
        Vignette => g.vignette = !g.vignette,
        ChromaticAberration => g.chromatic_aberration = !g.chromatic_aberration,
        VolumetricFog => g.volumetric_fog = !g.volumetric_fog,
        AudioEngine => {
            use crate::audio_engine::AudioEngine as E;
            a.engine = cycle(&[E::Original, E::Remaster], a.engine, dir);
        }
        Hrtf => a.hrtf = !a.hrtf,
        Reverb => a.reverb = !a.reverb,
        Occlusion => a.occlusion = !a.occlusion,
        Volume => a.master = ((a.master * 10.0).round() / 10.0 + 0.1 * dir as f32).clamp(0.0, 1.0),
        Mod(i) => m.mods[i].enabled = !m.mods[i].enabled,
    }
    if *g != before && matches!(r, Upscaler | Sharpness | AntiAliasing | AmbientOcclusion | GlobalIllumination | RayTracing | ContactShadows | Shadows | Tonemapper | Bloom | DepthOfField | MotionBlur | Vignette | ChromaticAberration | VolumetricFog) {
        g.preset = crate::graphics::Preset::Custom;
    }
}

#[allow(clippy::too_many_arguments)]
pub fn menu(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    mut state: Local<MenuState>,
    mut g: ResMut<GraphicsSettings>,
    mut a: ResMut<AudioSettings>,
    mut m: ResMut<ModList>,
    caps: Res<GraphicsCaps>,
    root: Query<Entity, With<MenuRoot>>,
    mut text: Query<&mut Text, With<MenuText>>,
) {
    if keys.just_pressed(KeyCode::F10) || (state.open && keys.just_pressed(KeyCode::Escape)) {
        state.open = !state.open;
        if state.open {
            commands
                .spawn((
                    MenuRoot,
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(40.0),
                        top: Val::Px(40.0),
                        padding: UiRect::all(Val::Px(14.0)),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.02, 0.03, 0.03, 0.86)),
                    GlobalZIndex(100),
                ))
                .with_children(|p| {
                    p.spawn((MenuText, Text::new(""), TextFont { font_size: FontSize::Px(15.0), ..default() }, TextColor(Color::srgb(0.88, 0.9, 0.86))));
                });
        } else {
            for e in &root {
                commands.entity(e).despawn();
            }
        }
    }
    if !state.open {
        return;
    }
    let list = rows(&m);
    let selectable: Vec<usize> = list.iter().enumerate().filter(|(_, r)| !matches!(r, Row::Header(_))).map(|(i, _)| i).collect();
    if selectable.is_empty() {
        return;
    }
    let mut pos = selectable.iter().position(|&i| i == state.row).unwrap_or(0);
    if keys.just_pressed(KeyCode::ArrowDown) {
        pos = (pos + 1) % selectable.len();
    }
    if keys.just_pressed(KeyCode::ArrowUp) {
        pos = (pos + selectable.len() - 1) % selectable.len();
    }
    state.row = selectable[pos];
    let dir = if keys.just_pressed(KeyCode::ArrowRight) || keys.just_pressed(KeyCode::Enter) {
        1
    } else if keys.just_pressed(KeyCode::ArrowLeft) {
        -1
    } else {
        0
    };
    if dir != 0 {
        let (mut g2, mut a2, mut m2) = (g.clone(), a.clone(), m.clone());
        change(list[state.row], dir, &mut g2, &mut a2, &mut m2);
        if g2 != *g {
            *g = g2;
            save_section("graphics", serde_json::to_value(&*g).unwrap_or_default());
        }
        if a2 != *a {
            *a = a2;
            save_section("audio", serde_json::to_value(&*a).unwrap_or_default());
        }
        if m2 != *m {
            *m = m2;
            m.save();
        }
    }
    let Ok(mut t) = text.single_mut() else { return };
    let mut out = String::from("REMASTER SETTINGS   (F10 / Esc close, arrows change)\n\n");
    for (i, r) in list.iter().enumerate() {
        let (l, v) = label(*r, &g, &a, &m, &caps);
        if matches!(r, Row::Header(_)) {
            out.push_str(&format!("\n{l}\n"));
        } else {
            let mark = if i == state.row { ">" } else { " " };
            out.push_str(&format!("{mark} {l:<36} {v}\n"));
        }
    }
    if m.mods.is_empty() {
        out.push_str(&format!("  (no mods in {})\n", crate::mods::mods_dir().display()));
    }
    **t = out;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preset_switch_and_custom_marking() {
        let (mut g, mut a, mut m) = (GraphicsSettings::original(), AudioSettings::default(), ModList::default());
        change(Row::Preset, 1, &mut g, &mut a, &mut m);
        assert_eq!(g, GraphicsSettings::remaster());
        change(Row::Bloom, 1, &mut g, &mut a, &mut m);
        assert!(!g.bloom);
        assert_eq!(g.preset, crate::graphics::Preset::Custom);
        // upscaler cycles through FSR qualities and DLSS modes and back to native
        let mut seen = 0;
        for _ in 0..11 {
            change(Row::Upscaler, 1, &mut g, &mut a, &mut m);
            seen += 1;
        }
        assert_eq!(seen, 11);
        assert_eq!(g.upscaler, crate::graphics::Upscaler::Native);
    }

    #[test]
    fn settings_round_trip_through_json() {
        let g = GraphicsSettings::remaster();
        let v = serde_json::to_value(&g).unwrap();
        let back: GraphicsSettings = serde_json::from_value(v).unwrap();
        assert_eq!(g, back);
        // missing keys fall back to defaults instead of failing
        let partial: GraphicsSettings = serde_json::from_value(serde_json::json!({"bloom": false})).unwrap();
        assert!(!partial.bloom);
    }

    #[test]
    fn fsr_scales_match_amd_presets() {
        use crate::graphics::FsrQuality::*;
        let r = |x: f32| (x * 1000.0).round() / 1000.0;
        assert_eq!([r(UltraQuality.scale()), r(Quality.scale()), r(Balanced.scale()), r(Performance.scale())], [0.769, 0.667, 0.588, 0.5]);
    }
}
