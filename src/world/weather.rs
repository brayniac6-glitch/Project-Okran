//! The weather (port_map `weather`): per BIOME_GROUP a region cycling through its SEASONs over the
//! year, each season picking WEATHERs by chance, a weather instance holding its strength, end and
//! wind; the current region's weather drives the sky (cloud density, sky colour, wind).
//!
//! Spec: F051033 Weather (a WEATHER record and its season ref's values: chance, duration min /
//! max in game minutes), F051034 Season (its "weathers"; one weather alone lasts ceil(2618.18 x
//! days / 60) minutes; with any timed weather a filler (chance 0, 5 .. 60 min) is added last),
//! F051035 WeatherRegion (a BIOME_GROUP's "seasons" sorted by their first value, each lasting
//! roundHalf(its days value / their sum x days per year) days, a days value under 1 meaning an
//! equal share; "weather strength multiplier min/max"), F051027 (choose: weighted by chance,
//! timed weathers only within their hours; nothing chosen -> the last), F051024 (start: strength
//! F051014, end = now + randInt(min, max) minutes, cut at the weather's end hour; the first wind
//! random, later winds blended over 5% of the update time), F051012 (wind: every "wind update
//! time" minutes the speed moves "wind update limit" / 360 of the way to lerp(min, max, strength)
//! and the angle by randRange(-pi, pi) x limit / 360), F051029 (region update: a new season when
//! its days run out, a new weather when the instance ends), F051016 (the current region tells the
//! sky its colour multiplier and cloud density over 30 time units, and the wind).

use kenshi_data::{GameData, GameDataContainer, ItemType};

/// A source of uniform randoms in [0, 1] (Kenshi: rand() / RAND_MAX).
pub trait Random {
    fn unit(&mut self) -> f32;
}

/// randRange (F050502): a + rand x (b - a).
fn rand_range(r: &mut dyn Random, a: f32, b: f32) -> f32 {
    a + r.unit() * (b - a)
}

/// randInt (F050548): an integer in [a, b].
fn rand_int(r: &mut dyn Random, a: i32, b: i32) -> i32 {
    if b <= a {
        return a;
    }
    a + ((r.unit() * (b - a + 1) as f32) as i32).min(b - a)
}

/// roundHalf (F050533).
fn round_half(v: f32) -> i32 {
    (v + 0.5).floor() as i32
}

/// F051033: a WEATHER as a season holds it.
#[derive(Debug, Clone, PartialEq)]
pub struct Weather {
    pub record: String,
    pub affect_type: i32,
    pub affect_strength: f32,
    /// (EFFECT record, its values).
    pub effects: Vec<(String, [i32; 3])>,
    pub wind_speed: [f32; 2],
    /// Game minutes between wind changes (0 in the record: 1 000 000).
    pub wind_update_time: i32,
    pub wind_update_limit: i32,
    pub fog_enabled: bool,
    pub fog_distance: [f32; 2],
    pub fog_wind: [f32; 2],
    /// rgb (alpha 1).
    pub fog_colour: [f32; 3],
    pub clouds_density: f32,
    pub sky_colour_mult: [f32; 3],
    /// The season ref's values: chance, duration min and max (minutes).
    pub chance: f32,
    pub duration: [i32; 2],
    /// Hours the weather may start in (equal: any time).
    pub start_time: f32,
    pub end_time: f32,
    pub rain_intensity: f32,
    pub wind_intensity: f32,
    pub wetness: f32,
    pub dust: f32,
    pub dust_inside: f32,
    pub dust_slope: f32,
    pub heat_haze: f32,
    /// "effect strength min" (read twice by F051024: the instance's effect strength).
    pub effect_strength: f32,
}

fn argb(v: i32) -> [f32; 3] {
    let v = v as u32;
    [
        ((v >> 16) & 255) as f32 / 255.0,
        ((v >> 8) & 255) as f32 / 255.0,
        (v & 255) as f32 / 255.0,
    ]
}

impl Weather {
    /// @fn F051033
    pub fn from_record(r: &GameData, chance: f32, duration: [i32; 2]) -> Weather {
        let f = |k: &str| r.floats.get(k).copied().unwrap_or(0.0);
        let i = |k: &str| r.ints.get(k).copied().unwrap_or(0);
        let wind_update_time = match i("wind update time") {
            0 => 1_000_000,
            t => t,
        };
        let mut fog_distance = [f("fog distance min"), f("fog distance max")];
        let fog_wind = [f("fog wind min"), f("fog wind max")];
        if fog_wind[0] == fog_wind[1] {
            fog_distance[0] = fog_distance[0].max(fog_distance[1]);
        }
        Weather {
            record: r.string_id.clone(),
            affect_type: i("affect type"),
            affect_strength: f("affect strength"),
            effects: r
                .refs("effects")
                .iter()
                .map(|(sid, v)| (sid.clone(), *v))
                .collect(),
            wind_speed: [f("wind speed min"), f("wind speed max")],
            wind_update_time,
            wind_update_limit: i("wind update limit"),
            fog_enabled: r.bools.get("fog enabled").copied().unwrap_or(false),
            fog_distance,
            fog_wind,
            fog_colour: argb(i("fog color")),
            clouds_density: f("clouds density"),
            sky_colour_mult: argb(i("sky color mult")),
            chance,
            duration,
            start_time: f("start time"),
            end_time: f("end time"),
            rain_intensity: f("rain intensity"),
            wind_intensity: f("wind intensity"),
            wetness: f("wetness"),
            dust: f("dust"),
            dust_inside: f("dust inside"),
            dust_slope: f("dust slope"),
            heat_haze: f("heat haze"),
            effect_strength: f("effect strength min"),
        }
    }

    fn timed(&self) -> bool {
        self.start_time != self.end_time
    }
}

/// F051034: a SEASON.
#[derive(Debug, Clone, PartialEq)]
pub struct Season {
    pub record: Option<String>,
    pub weathers: Vec<Weather>,
    pub days: i32,
    pub timed: bool,
    pub strength_limit: [f32; 2],
    pub sunlight: [f32; 3],
}

/// Minutes F040787 gives a day for a lone weather.
const LONE_WEATHER_DAY: f32 = 2_618.182;

impl Season {
    /// @fn F051034
    pub fn from_record(data: &GameDataContainer, r: Option<&GameData>, days: i32) -> Season {
        let mut weathers = vec![];
        let mut timed = false;
        let mut last: Option<&GameData> = None;
        if let Some(r) = r {
            let refs = r.refs("weathers");
            for (sid, v) in refs {
                let Some(w) = data.get(sid) else { continue };
                let (mut lo, mut hi) = (v[1], v[2]);
                if refs.len() == 1 {
                    let d = (LONE_WEATHER_DAY * days as f32 / 60.0).ceil() as i32;
                    (lo, hi) = (d, d);
                }
                let weather = Weather::from_record(w, v[0] as f32, [lo, hi]);
                timed |= weather.timed();
                weathers.push(weather);
                last = Some(w);
            }
            if timed && let Some(w) = last {
                weathers.push(Weather::from_record(w, 0.0, [5, 60]));
            }
        }
        let f = |k: &str| r.and_then(|r| r.floats.get(k).copied()).unwrap_or(0.0);
        let mut season = Season {
            record: r.map(|r| r.string_id.clone()),
            weathers,
            days,
            timed,
            strength_limit: [f("weather strength limit min"), f("weather strength limit max")],
            sunlight: argb(r.and_then(|r| r.ints.get("sunlight color").copied()).unwrap_or(-1)),
        };
        if season.weathers.is_empty() {
            // no weather: the default WEATHER record for a day or two, at full strength range
            if let Some(w) = data.get("5460-weather.mod") {
                season.weathers.push(Weather::from_record(w, 1.0, [1440, 2880]));
            }
            season.strength_limit = [0.0, 1.0];
            season.sunlight = [1.0, 1.0, 1.0];
        }
        season
    }

    /// @fn F051027
    ///
    /// Pick a weather (index) at game hour `hour`.
    pub fn choose(&self, hour: f32, rng: &mut dyn Random) -> Option<usize> {
        let mut cumulative: Vec<(f32, usize)> = vec![];
        let mut total = 0.0;
        for (k, w) in self.weathers.iter().enumerate() {
            let allowed = if !self.timed || !w.timed() {
                true
            } else {
                let (s, e) = (w.start_time, w.end_time);
                (s < e && s <= hour && hour <= e) || (e < s && (hour <= s || e <= hour))
            };
            if allowed && w.chance > 0.0 {
                total += w.chance;
                cumulative.push((total, k));
            }
        }
        let chosen = match cumulative.len() {
            0 => None,
            1 => Some(cumulative[0].1),
            _ => {
                let r = rand_range(rng, 0.0, total);
                cumulative.iter().find(|(key, _)| r < *key).map(|(_, k)| *k)
            }
        };
        chosen.or(if self.weathers.is_empty() {
            None
        } else {
            Some(self.weathers.len() - 1)
        })
    }
}

/// F051024 / F051012: a weather playing in a region.
#[derive(Debug, Clone, PartialEq)]
pub struct WeatherInstance {
    pub weather: usize,
    pub effect_strength: f32,
    pub strength: f32,
    pub wind_speed: f32,
    /// The wind's direction (x, z).
    pub wind_dir: [f32; 2],
    pub blend_done: bool,
    blend_start: i32,
    blend_end: i32,
    speed_from: f32,
    speed_to: f32,
    angle_from: f32,
    angle_to: f32,
    pub start: i32,
    pub end: i32,
    next_wind: i32,
}

/// F051035: a BIOME_GROUP's weather.
#[derive(Debug, Clone, PartialEq)]
pub struct WeatherRegion {
    pub biome_group: String,
    pub seasons: Vec<Season>,
    pub strength_mult: [f32; 2],
    pub season: usize,
    pub season_end_day: i32,
    pub instance: Option<WeatherInstance>,
    /// A new weather began (F051029 +0xb1): the sky is told again.
    pub changed: bool,
}

impl WeatherRegion {
    /// @fn F051035
    pub fn from_record(data: &GameDataContainer, group: &GameData, days_per_year: i32) -> WeatherRegion {
        let refs = group.refs("seasons");
        let mut seasons = vec![];
        let mut strength_mult = [1.0, 1.0];
        if refs.is_empty() {
            seasons.push(Season::from_record(data, None, days_per_year));
        } else {
            let n = refs.len() as i32;
            let mut entries: Vec<(i32, &GameData, i32)> = vec![];
            let mut sum = 0.0;
            for (sid, v) in refs {
                let Some(s) = data.get(sid) else { continue };
                let days = if v[1] < 1 { days_per_year / n } else { v[1] };
                sum += days as f32;
                entries.push((v[0], s, days));
            }
            let mut left = days_per_year;
            let count = entries.len();
            for (k, e) in entries.iter_mut().enumerate() {
                let d = round_half(e.2 as f32 / sum * days_per_year as f32).min(left);
                left -= d;
                e.2 = d;
                if k + 1 == count && left > 0 {
                    e.2 += left;
                    left = 0;
                }
            }
            entries.sort_by_key(|e| (e.0, e.1.string_id.clone()));
            for (_, s, days) in entries {
                seasons.push(Season::from_record(data, Some(s), days));
            }
            let f = |k: &str| group.floats.get(k).copied().unwrap_or(0.0);
            strength_mult = [
                f("weather strength multiplier min"),
                f("weather strength multiplier max"),
            ];
        }
        WeatherRegion {
            biome_group: group.string_id.clone(),
            seasons,
            strength_mult,
            season: 0,
            season_end_day: 0,
            instance: None,
            changed: false,
        }
    }

    pub fn weather(&self) -> Option<&Weather> {
        let i = self.instance.as_ref()?;
        self.seasons.get(self.season)?.weathers.get(i.weather)
    }

    /// F051014: a new weather's strength.
    fn strength(&self, rng: &mut dyn Random) -> f32 {
        let s = &self.seasons[self.season];
        (rand_range(rng, s.strength_limit[0], s.strength_limit[1])
            * rand_range(rng, self.strength_mult[0], self.strength_mult[1]))
        .clamp(0.0, 1.0)
    }

    /// @fn F051024
    ///
    /// Start weather `w` of the current season at game time `hours`.
    fn start(&mut self, w: usize, hours: f64, rng: &mut dyn Random) {
        let strength = self.strength(rng);
        let weather = self.seasons[self.season].weathers[w].clone();
        let now = (hours * 60.0) as i32;
        let mut end = rand_int(rng, weather.duration[0], weather.duration[1]) + now;
        if weather.start_time != weather.end_time {
            let cut = (((hours / 24.0).floor() * 24.0 + weather.end_time as f64) * 60.0) as f32;
            if cut < end as f32 {
                end = cut as i32;
            }
        }
        let speed = (weather.wind_speed[1] - weather.wind_speed[0]) * strength + weather.wind_speed[0];
        let next = match &self.instance {
            None => {
                let (x, z) = (rand_range(rng, -1.0, 1.0), rand_range(rng, -1.0, 1.0));
                let l = (x * x + z * z).sqrt().max(1e-6);
                WeatherInstance {
                    weather: w,
                    effect_strength: weather.effect_strength,
                    strength,
                    wind_speed: speed,
                    wind_dir: [x / l, z / l],
                    blend_done: true,
                    blend_start: 0,
                    blend_end: 0,
                    speed_from: 0.0,
                    speed_to: 0.0,
                    angle_from: 0.0,
                    angle_to: 0.0,
                    start: now,
                    end,
                    next_wind: now + weather.wind_update_time,
                }
            }
            Some(old) => {
                let angle = old.wind_dir[1].atan2(old.wind_dir[0]).abs();
                WeatherInstance {
                    weather: w,
                    effect_strength: weather.effect_strength,
                    strength,
                    wind_speed: old.wind_speed,
                    wind_dir: old.wind_dir,
                    blend_done: false,
                    blend_start: now,
                    blend_end: now + round_half(weather.wind_update_time as f32 * 0.05),
                    speed_from: old.wind_speed,
                    speed_to: speed,
                    angle_from: angle,
                    angle_to: rand_range(rng, -std::f32::consts::PI, std::f32::consts::PI),
                    start: now,
                    end,
                    next_wind: now + weather.wind_update_time,
                }
            }
        };
        self.instance = Some(next);
    }

    /// @fn F051027
    fn choose(&mut self, hours: f64, rng: &mut dyn Random) {
        let hour = (hours.rem_euclid(24.0)).floor() as f32;
        if let Some(w) = self.seasons[self.season].choose(hour, rng) {
            self.start(w, hours, rng);
        }
    }

    /// The next season (F051028 / F051029).
    fn next_season(&mut self, day: i32, hours: f64, rng: &mut dyn Random) {
        if self.seasons.is_empty() {
            return;
        }
        self.season = (self.season + 1) % self.seasons.len();
        self.season_end_day = self.seasons[self.season].days + day;
        self.choose(hours, rng);
    }

    /// Begin at game day `day`, game time `hours` (F051031's fresh start: the first season).
    pub fn begin(&mut self, day: i32, hours: f64, rng: &mut dyn Random) {
        if self.seasons.is_empty() {
            return;
        }
        self.season = 0;
        self.season_end_day = self.seasons[0].days + day;
        self.choose(hours, rng);
        self.changed = true;
    }

    /// Start `weather` in the current season now (testing: a chosen weather).
    pub fn force(&mut self, weather: Weather, hours: f64, rng: &mut dyn Random) {
        if self.seasons.is_empty() {
            return;
        }
        let s = &mut self.seasons[self.season];
        s.weathers.push(weather);
        let k = s.weathers.len() - 1;
        self.start(k, hours, rng);
        self.changed = true;
    }

    /// @fn F051012
    ///
    /// The instance's wind at game time `hours`; true while it moves.
    fn wind(&mut self, hours: f64, rng: &mut dyn Random) -> bool {
        let Some(weather) = self.weather().cloned() else {
            return false;
        };
        let Some(i) = self.instance.as_mut() else {
            return false;
        };
        let now = (hours * 60.0) as i32;
        if now >= i.next_wind {
            i.blend_start = now;
            i.next_wind = now + weather.wind_update_time;
            i.blend_end = round_half(weather.wind_update_time as f32 * 0.05) + i.blend_start;
            i.speed_from = i.wind_speed;
            let limit = weather.wind_update_limit as f32 / 360.0;
            let target = (weather.wind_speed[1] - weather.wind_speed[0]) * i.strength
                + weather.wind_speed[0];
            i.speed_to = i.wind_speed + (target - i.wind_speed) * limit;
            let angle = i.wind_dir[1].atan2(i.wind_dir[0]).abs();
            i.angle_from = angle;
            let pi = std::f32::consts::PI;
            let mut to = rand_range(rng, -pi, pi) * limit + angle;
            if to > pi {
                to -= 2.0 * pi;
            } else if to < -pi {
                to += 2.0 * pi;
            }
            i.angle_to = to;
            i.blend_done = false;
        }
        if i.blend_done {
            return false;
        }
        let span = (i.blend_end - i.blend_start).max(1) as f32;
        let t = ((now - i.blend_start) as f32 / span).clamp(0.0, 1.0);
        i.wind_speed = i.speed_from + (i.speed_to - i.speed_from) * t;
        let a = i.angle_from + (i.angle_to - i.angle_from) * t;
        // UNIT_X turned about Y by a
        i.wind_dir = [a.cos(), -a.sin()];
        if i.blend_end <= now {
            i.blend_done = true;
        }
        true
    }

    /// @fn F051029
    ///
    /// The region at game day `day`, time `hours`.
    pub fn update(&mut self, day: i32, hours: f64, rng: &mut dyn Random) {
        if self.instance.is_none() || self.seasons.is_empty() {
            return;
        }
        if day < self.season_end_day {
            self.wind(hours, rng);
            let ended = self.instance.as_ref().is_some_and(|i| (i.end as f64) < hours * 60.0);
            if ended {
                self.changed = true;
                self.choose(hours, rng);
            }
        } else {
            self.next_season(day, hours, rng);
            self.changed = true;
        }
    }
}

/// Every BIOME_GROUP's weather region (WeatherManager).
pub fn weather_regions(data: &GameDataContainer, days_per_year: i32) -> Vec<WeatherRegion> {
    data.of_type(ItemType::BiomeGroup)
        .map(|g| WeatherRegion::from_record(data, g, days_per_year))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixed(f32);
    impl Random for Fixed {
        fn unit(&mut self) -> f32 {
            self.0
        }
    }

    #[test]
    fn weighted_choice_takes_the_first_key_above_the_roll() {
        let w = |chance: f32| Weather {
            record: String::new(),
            affect_type: 0,
            affect_strength: 0.0,
            effects: vec![],
            wind_speed: [0.0, 0.0],
            wind_update_time: 10,
            wind_update_limit: 0,
            fog_enabled: false,
            fog_distance: [0.0; 2],
            fog_wind: [0.0; 2],
            fog_colour: [1.0; 3],
            clouds_density: 0.0,
            sky_colour_mult: [1.0; 3],
            chance,
            duration: [60, 60],
            start_time: 0.0,
            end_time: 0.0,
            rain_intensity: 0.0,
            wind_intensity: 0.0,
            wetness: 0.0,
            dust: 0.0,
            dust_inside: 0.0,
            dust_slope: 0.0,
            heat_haze: 0.0,
            effect_strength: 0.0,
        };
        let s = Season {
            record: None,
            weathers: vec![w(10.0), w(5.0), w(5.0)],
            days: 10,
            timed: false,
            strength_limit: [0.0, 1.0],
            sunlight: [1.0; 3],
        };
        assert_eq!(s.choose(12.0, &mut Fixed(0.0)), Some(0));
        assert_eq!(s.choose(12.0, &mut Fixed(0.6)), Some(1));
        assert_eq!(s.choose(12.0, &mut Fixed(0.99)), Some(2));
    }
}

/// F051179: one region slot's fog in the weather manager, moving from the last weather's to
/// the current's over the first 2.5% of the weather.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct FogSlot {
    /// The region (index into the regions) the slot follows.
    pub region: Option<usize>,
    old: (f32, f32, [f32; 3]),
    new: (f32, f32, [f32; 3]),
    progress: f32,
    pub blending: bool,
}

impl WeatherRegion {
    /// F051006: how far the instance is through its weather (0..1).
    pub fn progress(&self, hours: f64) -> f32 {
        let Some(i) = &self.instance else { return 1.0 };
        let now = (hours * 60.0) as f32;
        ((now - i.start as f32) / (i.end - i.start).max(1) as f32).clamp(0.0, 1.0)
    }

    /// F051009 / F051013 / F051010: the weather's fog (enabled, distance, colour); the distance
    /// between its min and max by the wind speed's place between the fog wind min and max.
    pub fn fog(&self) -> (f32, f32, [f32; 3]) {
        let (Some(w), Some(i)) = (self.weather(), &self.instance) else {
            return (0.0, 0.0, [1.0; 3]);
        };
        let distance = if w.fog_wind[1] != w.fog_wind[0] {
            let t = ((i.wind_speed - w.fog_wind[0]) / (w.fog_wind[1] - w.fog_wind[0])).clamp(0.0, 1.0);
            (w.fog_distance[1] - w.fog_distance[0]) * t + w.fog_distance[0]
        } else {
            w.fog_distance[0]
        };
        (if w.fog_enabled { 1.0 } else { 0.0 }, distance, w.fog_colour)
    }
}

impl FogSlot {
    /// A slot newly given to `region`.
    pub fn new(region: usize) -> FogSlot {
        FogSlot {
            region: Some(region),
            ..Default::default()
        }
    }

    /// @fn F051179
    pub fn update(&mut self, region: &WeatherRegion, hours: f64) {
        let t = region.progress(hours) / 0.025;
        if t < self.progress {
            self.old = self.new;
        }
        self.blending = t < 1.0;
        self.new = region.fog();
        self.progress = t;
    }

    /// F051182: the slot's fog now.
    pub fn value(&self) -> (f32, f32, [f32; 3]) {
        if !self.blending {
            return self.new;
        }
        let t = self.progress;
        let l = |a: f32, b: f32| (b - a) * t + a;
        (
            l(self.old.0, self.new.0),
            l(self.old.1, self.new.1),
            [
                l(self.old.2[0], self.new.2[0]),
                l(self.old.2[1], self.new.2[1]),
                l(self.old.2[2], self.new.2[2]),
            ],
        )
    }
}

/// F051184 / F051194: what the weather manager hands the renderer each frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WeatherOutputs {
    /// The global fog (MainFog): Σ weight x enabled (its colour's alpha), Σ weight x distance
    /// (density 1 / distance), Σ weight x colour.
    pub fog_enabled: f32,
    pub fog_distance: f32,
    pub fog_colour: [f32; 3],
    /// rain intensity x strength (rainAmount = it / 50).
    pub rain: f32,
    /// The shaders' wetness.
    pub wetness: f32,
    /// dustAmount: (dust, dust inside x dust, dust slope).
    pub dust: [f32; 3],
    pub heat_haze: f32,
    pub wind_intensity: f32,
}

impl Default for WeatherOutputs {
    fn default() -> Self {
        WeatherOutputs {
            fog_enabled: 0.0,
            fog_distance: 0.0,
            fog_colour: [1.0; 3],
            rain: 0.0,
            wetness: 0.0,
            dust: [0.0; 3],
            heat_haze: 0.0,
            wind_intensity: 0.0,
        }
    }
}

impl WeatherOutputs {
    /// @fn F051184
    ///
    /// One frame: `weights` the region slots' weights (normalised), `current` the region the
    /// camera is in, `dt` the frame time, `sun_y` the sun's height, `jump` whether the camera
    /// moved far (values set at once).
    #[allow(clippy::needless_range_loop)]
    pub fn update(
        &mut self,
        slots: &[(FogSlot, f32)],
        current: &WeatherRegion,
        dt: f32,
        sun_y: f32,
        jump: bool,
    ) {
        self.fog_enabled = 0.0;
        self.fog_distance = 0.0;
        self.fog_colour = [0.0; 3];
        for (slot, w) in slots {
            if *w > 0.0 {
                let (e, d, c) = slot.value();
                self.fog_enabled += w * e;
                self.fog_distance += w * d;
                for k in 0..3 {
                    self.fog_colour[k] += w * c[k];
                }
            }
        }
        let (Some(weather), Some(i)) = (current.weather(), &current.instance) else {
            return;
        };
        let strength = i.strength;
        self.rain = weather.rain_intensity * strength;
        self.wind_intensity = weather.wind_intensity * i.wind_speed * 0.01;
        // heat haze: toward haze x strength x saturate(sun y x 6) at dt / 3
        let haze = weather.heat_haze * strength * (sun_y * 6.0).clamp(0.0, 1.0);
        if haze != self.heat_haze {
            let step = dt / 3.0;
            if step <= (haze - self.heat_haze).abs() {
                self.heat_haze += if haze < self.heat_haze { -step } else { step };
            } else {
                self.heat_haze = haze;
            }
        }
        // dust: up at dust x dt x 0.017 to the weather's, down at min(dust, -0.001) x dt
        let dust = weather.dust * strength;
        if dust > 0.0 && self.dust[0] < dust {
            self.dust[0] = (dust * dt * 0.017 + self.dust[0]).min(dust);
        } else if dust < self.dust[0] {
            self.dust[0] += dt * dust.min(-0.001);
        }
        if jump {
            self.wetness = weather.wetness + if weather.wetness != 0.0 { -0.01 } else { 0.01 };
            self.dust[0] = dust;
            self.heat_haze = haze;
            self.dust[2] = weather.dust_slope;
        }
        self.dust[1] = weather.dust_inside * self.dust[0];
        // dust slope toward the weather's at 0.1 / s
        let (to, step) = (weather.dust_slope, dt * 0.1);
        self.dust[2] = if self.dust[2] < to - step {
            self.dust[2] + step
        } else if to + step < self.dust[2] {
            self.dust[2] - step
        } else {
            to
        };
        // F051194: wetness up at 0.01 / s to the weather's, dried at 0.005 / s when it is dry
        let target = weather.wetness;
        if target > self.wetness {
            self.wetness += dt * 0.01;
        } else if target <= 0.0 && self.wetness > 0.0 {
            self.wetness -= dt * 0.005 * (1.0 - (target * 2.0).max(0.0));
        }
    }

    /// MainFog's density (F023570): 1 / distance, 1 when there is no distance.
    pub fn fog_density(&self) -> f32 {
        if self.fog_distance > 0.0 {
            1.0 / self.fog_distance
        } else {
            1.0
        }
    }
}
