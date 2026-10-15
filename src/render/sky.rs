//! Kenshi's sky light (port_map `sky`): the sun's path (Kenshi's SkyXController) and colour (SkyX's
//! atmospheric scattering, SkyX_x64.dll), as the main light shader takes them.
//!
//! Spec: F040798 SkyXController::update (the hour against GLOBAL CONSTANTS "sunrise" / "sunset":
//! by day f = (t - sunrise) / (sunset - sunrise), by night f = 1 + the night's fraction gone; the
//! sun at angle f * PI: (cos a, sin a cos tilt, sin a sin tilt), tilt = "latitude" in radians),
//! F040804 (the SkyX atmosphere options: inner radius 9.77501, outer 10.2963, height 0.01,
//! Rayleigh 0.0022, Mie 0.000675, sun intensity 30, wavelengths (0.57, 0.48, 0.44), G -0.991,
//! exposure 1.4, 4 samples; HDR lighting mode; "exposure min" / "exposure max" / "night darkness"),
//! F040805 (per frame: sunColour = getColorAt(sun + (0, 0, 0.04)) / (exposure x 4), its strength w
//! lerp(0.5, 1, sun y) by day and falling to 0 at y = -1 / 10.8, none below y = -0.2;
//! sunDirection = the sun with y clamped to 0; MIN_LUMINANCE = exposure min x lerp(night darkness,
//! 1, saturate(sun y x 5)); the fog's horizonClouds and pFogParams, below), SkyX::AtmosphereManager::
//! getColorAt (SkyX_x64.dll 0x1800038e0, its HDR branch) and ::_scale (0x180003860).
//!
//! The fog (F040805): pFogParams = (D, D x 0.06, D x 0.6, 0) with D = the "View Distance" option
//! (settings.cfg, F032960) x 10, the 0.06 / 0.6 GLOBAL CONSTANTS defaults F046688 sets and nothing
//! loads; horizonClouds = (saturate((sunColour x (1 - c x 0.1) + max(getColorAt(UNIT_Z) x skyMult,
//! C) x sunColour.g) x (1 - darkness)) x sqrt(exposure), saturate(offset + 0.5)) with
//! c = (offset + 0.2) x multiplier, where multiplier / offset / darkness are the cloud layer's
//! DensityMultiplier / DensityOffset / Darkness (the controller's +0x30 is its CloudLayer, F040804)
//! and C the controller's minimum colour (F040804); skyMult the weather's sky colour multiplier.

use bevy::math::Vec3;

/// The controller's minimum sky colour (F040804: (0.001, 0.001, 0.0015)).
const MIN_COLOUR: Vec3 = Vec3::new(0.001, 0.001, 0.0015);

/// SkyX AtmosphereManager::Options as Kenshi sets them (F040804).
#[derive(Debug, Clone, Copy)]
pub struct Atmosphere {
    pub inner_radius: f32,
    pub outer_radius: f32,
    pub height_position: f32,
    pub rayleigh: f32,
    pub mie: f32,
    pub sun_intensity: f32,
    pub wavelength: Vec3,
    pub g: f32,
    pub exposure: f32,
    pub samples: i32,
}

impl Default for Atmosphere {
    fn default() -> Self {
        Atmosphere {
            inner_radius: 9.77501,
            outer_radius: 10.2963,
            height_position: 0.01,
            rayleigh: 0.0022,
            mie: 0.000675,
            sun_intensity: 30.0,
            wavelength: Vec3::new(0.57, 0.48, 0.44),
            g: -0.991,
            exposure: 1.4,
            samples: 4,
        }
    }
}

/// SkyX AtmosphereManager::_scale: the optical depth scale at a cosine.
fn scale(cos: f32, scale_depth: f32) -> f32 {
    let x = (1.0 - cos) as f64;
    let e = (((x * 5.25 - 6.8) * x + 3.83) * x + 0.459) * x - 0.00287;
    (e as f32).exp() * scale_depth
}

impl Atmosphere {
    /// SkyX AtmosphereManager::getColorAt, HDR lighting mode: the sky's colour toward `direction`
    /// with the sun toward `sun` (both from the ground; RGB).
    pub fn color_at(&self, direction: Vec3, sun: Vec3) -> Vec3 {
        let (inner, outer) = (self.inner_radius, self.outer_radius);
        let scale_depth = (outer - inner) * 0.5;
        let scale_f = 1.0 / (outer - inner);
        let scale_over_depth = scale_f / scale_depth;
        let kr4pi = self.rayleigh * 4.0 * std::f32::consts::PI;
        let kr_esun = self.sun_intensity * self.rayleigh;
        let km4pi = self.mie * 4.0 * std::f32::consts::PI;
        let km_esun = self.mie * self.sun_intensity;
        let dir = direction.normalize_or_zero();
        let cam_height = (outer - inner) * self.height_position + inner;
        let inv_wl = Vec3::new(
            1.0 / self.wavelength.x.powf(4.0),
            1.0 / self.wavelength.y.powf(4.0),
            1.0 / self.wavelength.z.powf(4.0),
        );
        let pos = Vec3::new(dir.x, dir.y + inner, dir.z);
        let start = Vec3::new(0.0, cam_height, 0.0);
        let mut ray = pos - start;
        let far = ray.length();
        ray /= far;
        let depth0 = (scale_over_depth * (inner - cam_height)).exp();
        let start_angle = ray.dot(start) / cam_height;
        let start_offset = depth0 * scale(start_angle, scale_depth);
        let sample_length = far / self.samples as f32;
        let scaled_length = sample_length * scale_f;
        let sample_ray = ray * sample_length;
        let mut point = start + sample_ray * 0.5;
        let mut front = Vec3::ZERO;
        for _ in 0..self.samples.max(0) {
            let height = point.length();
            let depth = (scale_over_depth * (inner - height)).exp();
            let light_angle = sun.dot(point) / height;
            let camera_angle = ray.dot(point) / height;
            let scatter = start_offset
                + depth * (scale(light_angle, scale_depth) - scale(camera_angle, scale_depth));
            let attenuate = Vec3::new(
                (-scatter * (inv_wl.x * kr4pi + km4pi)).exp(),
                (-scatter * (inv_wl.y * kr4pi + km4pi)).exp(),
                (-scatter * (inv_wl.z * kr4pi + km4pi)).exp(),
            );
            front += attenuate * (depth * scaled_length);
            point += sample_ray;
        }
        let rayleigh = front * inv_wl * kr_esun;
        let mie = front * km_esun;
        let to_cam = start - pos;
        let cos = sun.dot(to_cam) / to_cam.length();
        let g2 = self.g * self.g;
        let ray_phase = (cos * cos * 0.5 + 1.0) * 0.75;
        let mie_phase = ((1.0 - g2) / (g2 + 2.0)) * 1.5 * (cos * cos + 1.0)
            / (g2 + 1.0 - 2.0 * self.g * cos).powf(1.5);
        let mut c = (rayleigh * ray_phase + mie * mie_phase) * self.exposure;
        // the night sky's glow toward the horizon
        let m = c.x.max(c.y).max(c.z);
        let fade = (1.0 - m * 10.0).clamp(0.0, 1.0);
        let below = (-sun.y).clamp(0.0, 1.0);
        let k = 2.0 - below * 0.75;
        let p = (1.0 - dir.y).powf(3.0);
        c.x += fade * (k * 0.05 * p).powf(2.2);
        c.y += fade * (k * 0.05 * p).powf(2.2);
        c.z += fade * (k * 0.1 * p).powf(2.2);
        c
    }
}

/// Kenshi's sky settings (GLOBAL CONSTANTS, F040804).
#[derive(Debug, Clone, Copy)]
pub struct SkySettings {
    pub sunrise: f32,
    pub sunset: f32,
    /// Radians.
    pub tilt: f32,
    pub exposure_min: f32,
    pub exposure_max: f32,
    pub night_darkness: f32,
    pub atmosphere: Atmosphere,
    /// The "View Distance" option (settings.cfg).
    pub view_distance: f32,
    /// The "Shadow Range" option (settings.cfg).
    pub shadow_range: f32,
    /// The "shadow mode" (0 none, 1 CSM, 2 RTW) and "shadow quality" options (settings.cfg).
    pub shadow_mode: i32,
    pub shadow_quality: i32,
    /// The "water reflection" level and "reflection range" options (settings.cfg).
    pub water_reflection: i32,
    pub reflection_range: f32,
    /// The "npc range" (0x1421334c8), "objects view range" (0x1421334cc) and "Fast zone hopping"
    /// (0x142133518) options (settings.cfg, F032951).
    pub npc_range: f32,
    pub objects_view_range: f32,
    pub fast_zone_hopping: bool,
    /// The HeatHaze option (settings.cfg).
    pub heat_haze: bool,
    /// The FXAA option (settings.cfg).
    pub fxaa: bool,
    /// The cloud layer's DensityMultiplier, DensityOffset, Darkness (F040804's, until the weather
    /// changes them, F040805).
    pub clouds: Vec3,
    /// The weather's sky colour multiplier (the controller's +0x48, F040805), multiplying the
    /// sky's colour in zenithLight / nadirLight.
    pub sky_mult: Vec3,
    /// The weather manager's outputs (fog, wetness, rain, dust).
    pub weather: kenshi_world::weather::WeatherOutputs,
}

impl SkySettings {
    pub fn from_data(data: &kenshi_data::GameDataContainer) -> SkySettings {
        let gc = data.get("110-gamedata.quack");
        let f = |k: &str, d: f32| gc.and_then(|g| g.floats.get(k).copied()).unwrap_or(d);
        let (mut sunrise, mut sunset) = (f("sunrise", 5.0), f("sunset", 23.0));
        if sunset <= sunrise {
            (sunrise, sunset) = (6.0, 20.0);
        }
        SkySettings {
            sunrise,
            sunset,
            tilt: f("latitude", 54.0) * std::f32::consts::PI / 180.0,
            exposure_min: f("exposure min", 0.8),
            exposure_max: f("exposure max", 1.2),
            night_darkness: f("night darkness", 0.35),
            atmosphere: Atmosphere::default(),
            view_distance: 6000.0,
            shadow_range: 1500.0,
            shadow_mode: 1,
            shadow_quality: 0,
            water_reflection: 1,
            reflection_range: 0.6,
            npc_range: 6000.0,
            objects_view_range: 4000.0,
            fast_zone_hopping: false,
            heat_haze: true,
            fxaa: true,
            clouds: Vec3::new(3.0, -0.8, 0.0),
            sky_mult: Vec3::ONE,
            weather: Default::default(),
        }
    }

    /// The cloud layer under a weather's cloud density (F040805: DensityOffset = density x 1.4 -
    /// 0.8, Darkness = clamp(density - 0.5, 0, 1)^0.3).
    pub fn set_cloud_density(&mut self, density: f32) {
        self.clouds.y = density * 1.4 - 0.8;
        self.clouds.z = (density - 0.5).clamp(0.0, 1.0).powf(0.3);
    }

    /// The "View Distance" and "Shadow Range" options from the install's settings.cfg
    /// ((assumption) 6000 and 1500 without one).
    pub fn with_install(mut self, install: &std::path::Path) -> SkySettings {
        if let Ok(cfg) = Ok::<String, ()>(kenshi_io::cfg::text(install, "settings.cfg")) {
            for line in cfg.lines() {
                if let Some((k, v)) = line.split_once('=')
                    && let Ok(v) = v.trim().parse::<f32>()
                {
                    match k.trim().to_ascii_lowercase().as_str() {
                        "view distance" => self.view_distance = v,
                        "shadow range" => self.shadow_range = v,
                        "shadow mode" => self.shadow_mode = v as i32,
                        "shadow quality" => self.shadow_quality = v as i32,
                        "water reflection" => self.water_reflection = v as i32,
                        "reflection range" => self.reflection_range = v,
                        "npc range" => self.npc_range = v,
                        "objects view range" => self.objects_view_range = v,
                        "fast zone hopping" => self.fast_zone_hopping = v != 0.0,
                        "heathaze" => self.heat_haze = v != 0.0,
                        "fxaa" => self.fxaa = v != 0.0,
                        _ => {}
                    }
                }
            }
        }
        self
    }

    /// pFogParams (F040805).
    pub fn fog_params(&self) -> bevy::math::Vec4 {
        let d = self.view_distance * 10.0;
        bevy::math::Vec4::new(d, d * 0.06, (d * 0.6).min(d), 0.0)
    }

    /// zenithLight at an hour (F040805): getColorAt(UNIT_Z) x the weather's sky colour
    /// multiplier, at least the controller's minimum colour, x the sun colour's green.
    pub fn zenith_light(&self, hour: f32) -> Vec3 {
        let real = self.sun_direction(hour);
        let (_, colour, _, _) = self.main_light(hour);
        (self.atmosphere.color_at(Vec3::Z, real) * self.sky_mult).max(MIN_COLOUR) * colour.y
    }

    /// horizonClouds at an hour (F040805, see the module notes).
    pub fn horizon_clouds(&self, hour: f32) -> bevy::math::Vec4 {
        let real = self.sun_direction(hour);
        let (_, colour, _, _) = self.main_light(hour);
        let (n, m, k) = (self.clouds.x, self.clouds.y, self.clouds.z);
        let c = (m + 0.2) * n;
        let zenith = (self.atmosphere.color_at(Vec3::Z, real) * self.sky_mult).max(MIN_COLOUR);
        let h = ((colour * (1.0 - c * 0.1) + zenith * colour.y) * (1.0 - k))
            .clamp(Vec3::ZERO, Vec3::ONE)
            * self.atmosphere.exposure.powf(0.5);
        h.extend((m + 0.5).clamp(0.0, 1.0))
    }

    /// @fn F040798
    ///
    /// The sun's direction at an hour (0..24).
    pub fn sun_direction(&self, hour: f32) -> Vec3 {
        let (rise, set) = (self.sunrise, self.sunset);
        let mut t = hour.rem_euclid(24.0);
        let f = if t < rise || set < t {
            if t < rise {
                t += 24.0;
            }
            (t - set) / (24.0 - (set - rise)) + 1.0
        } else {
            (t - rise) / (set - rise)
        };
        let a = f * std::f32::consts::PI;
        Vec3::new(
            a.cos(),
            a.sin() * self.tilt.cos(),
            a.sin() * self.tilt.sin(),
        )
    }

    /// @fn F040805
    ///
    /// The main light at an hour: (sunDirection, sunColour rgb and strength, MIN_LUMINANCE).
    pub fn main_light(&self, hour: f32) -> (Vec3, Vec3, f32, f32) {
        let real = self.sun_direction(hour);
        let mut colour = Vec3::ZERO;
        if real.y >= -0.2 {
            colour = self
                .atmosphere
                .color_at(real + Vec3::new(0.0, 0.0, 0.04), real)
                / (self.atmosphere.exposure * 4.0);
        }
        let base = 1.0f64 * 0.5;
        let w = if real.y <= 0.0 {
            (0.0 - base) * real.y as f64 * -10.8 + base
        } else {
            (1.0 - base) * real.y as f64 + base
        }
        .max(0.0) as f32;
        let mut dir = real;
        if dir.y < 0.0 {
            dir.y = 0.0;
            dir = dir.normalize_or_zero();
        }
        let day = (real.y * 5.0).clamp(0.0, 1.0);
        let min_lum = ((1.0 - self.night_darkness) * day + self.night_darkness) * self.exposure_min;
        (dir, colour, w, min_lum)
    }
}

impl SkySettings {
    /// The terrain's main light at an hour (`MainLight` for the terrain material). Its
    /// `exposure` is an estimate the shaders do not use (`to_screen` passes colours through):
    /// the scene is exposed by the HDR chain (kenshi_render::hdr), which measures and adapts the
    /// luminance as Kenshi's hdrfp4.hlsl does.
    pub fn terrain_light(&self, hour: f32) -> crate::terrain_material::MainLight {
        let (dir, colour, w, min_lum) = self.main_light(hour);
        let mut fog = self.fog_params();
        // the global fog (MainFog): the weather manager's blend (F051194 -> F023570 / F023571)
        fog.w = self.weather.fog_density();
        let day = (dir.y * 5.0).clamp(0.0, 1.0);
        let lum = min_lum + (self.exposure_max - min_lum) * day;
        crate::terrain_material::MainLight {
            sun_direction: dir,
            sun_colour: colour.extend(w),
            // AmbientParams' defaults (common.program): never set by the game's code
            env_colour: bevy::math::Vec4::ONE,
            ambient_params: bevy::math::Vec4::new(0.89, 0.66, 0.16, 1.0),
            exposure: 0.55 / lum,
            sun_real: self.sun_direction(hour),
            horizon: self.horizon_clouds(hour),
            fog,
            fog_colour: bevy::math::Vec4::new(
                self.weather.fog_colour[0],
                self.weather.fog_colour[1],
                self.weather.fog_colour[2],
                self.weather.fog_enabled,
            ),
            zenith: self.zenith_light(hour),
            clouds: self.clouds,
            wetness: self.weather.wetness,
            rain_amount: (self.weather.rain / 50.0).clamp(0.0, 1.0),
            dust: Vec3::from(self.weather.dust),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sun_rises_and_sets() {
        let s = SkySettings {
            sunrise: 5.0,
            sunset: 23.0,
            tilt: 54f32.to_radians(),
            exposure_min: 0.8,
            exposure_max: 1.2,
            night_darkness: 0.35,
            atmosphere: Atmosphere::default(),
            view_distance: 6000.0,
            shadow_range: 1500.0,
            shadow_mode: 1,
            shadow_quality: 0,
            water_reflection: 1,
            reflection_range: 0.6,
            npc_range: 6000.0,
            objects_view_range: 4000.0,
            fast_zone_hopping: false,
            heat_haze: true,
            fxaa: true,
            clouds: Vec3::new(3.0, -0.8, 0.0),
            sky_mult: Vec3::ONE,
            weather: Default::default(),
        };
        assert!(s.sun_direction(5.0).y.abs() < 1e-5);
        assert!(s.sun_direction(14.0).y > 0.5);
        assert!(s.sun_direction(2.0).y < 0.0);
        let (_, noon, w, _) = s.main_light(14.0);
        let (_, dusk, _, _) = s.main_light(22.8);
        assert!(noon.x > 0.0 && noon.z > 0.0 && w > 0.5);
        // a low sun is redder than a high one
        assert!(dusk.x / dusk.z.max(1e-6) > noon.x / noon.z);
    }
}
