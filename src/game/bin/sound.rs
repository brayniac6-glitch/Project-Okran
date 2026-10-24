//! Kenshi's sound (AudioSystemGlobal, F028264 - F028322): its Wwise banks (data/audio) loaded,
//! their events posted as the game does, their sounds played through Bevy's audio.
//!
//! - Banks: Init, SFX_General_Bank, VO_Bank, SFX_Amb_Indoors_Bank, SFX_Amb_Weather_Bank and
//!   MT_Music_Bank at the start (F028319); a region's "SFX_Amb_<ambience>" and
//!   "<music>_Music_Bank" when the camera is in it (F028312, F028318), the one left unloaded.
//! - An event's actions: play a node (a sound, or what its containers pick by the switches set
//!   and their weights; a continuous container its children one after another), stop the nodes
//!   it started, set a state.
//! - The regions (BIOME_GROUP "ambience": enum Ambience; "music": enum BiomeMusic, 4 none):
//!   "Leave_<old>" and "Enter_<new>_Day" or "_Night" (F028313); the state Time_in_Game day or
//!   night.
//! - The menu: "Boot_Up_Game" (the main theme), a game begun "Create_or_Load_Game".
//! - Volumes: settings.cfg's Music, Ambient, Footstep, Sfx, UI and VO volume (the game's
//!   "User_Volume_*" game parameters, F028291 - F028296), by the event's kind.
//!
//! - Music (F028312, F028318): the region's music sets the Biomes switch and the event to post,
//!   Music_<music>_Play, which is posted every 0.8 to 1.4 x the "Music frequency" minutes of
//!   real time (whether or not a piece still plays); a region's music bank goes 60 s after it is
//!   left.
//!
//! - Interactive music (kenshi_audio::music; MusicPlay): a music node an event plays is played
//!   as Wwise plays it: a switch its child by the switches set (its decision tree), changed as
//!   they change (its transition rule: at once with its fades, or at the exit cue); a playlist its
//!   segments one after another as its items' types, loops, weights and avoid-repeat walk them;
//!   a segment its tracks' clips at their places, the next segment's entry cue at its exit cue;
//!   the play and stop actions' fades. A region's music event plays three such nodes: the
//!   day / night pieces (Time_in_Game) and two layers of shorter ones, each a step at random (a
//!   piece or a silence) each time it is posted. (not ported) Stingers, the transition
//!   segments, a rule's fade offsets and curves (linear), the sync to a bar or beat (taken as
//!   the exit cue); a clip's end trim.
//!
//! Buses, effects, RTPCs and attenuation curves are not read: a
//! sound plays at its nodes' summed volume, panned and fading with distance by Bevy's spatial
//! audio (1/100 of a unit a metre). Day is from "sunrise" to "sunset" (F040780).
use super::{Game, MainCam, WorldReady};
use bevy::audio::{AudioPlayer, AudioSource, PlaybackMode, PlaybackSettings, SpatialListener, SpatialScale, Volume};
use bevy::prelude::*;
use bevy::audio::{AudioSink, AudioSinkPlayback};
use kenshi_audio::bnk::{name_id, Bank};
use kenshi_audio::music::{Music, MusicKind, PlaylistWalk};
use kenshi_audio::tree::{Action, Kind, Library, Switches};
use std::collections::HashMap;
use std::sync::Arc;

/// Which user volume a sound is under.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Channel {
    Music,
    Ambient,
    Footstep,
    Sfx,
    Ui,
    Vo,
}

/// An event to post: its id, where (scene coordinates; none: everywhere), the switches it
/// plays with (group, value), its channel.
pub struct Post {
    pub event: u32,
    pub at: Option<Vec3>,
    pub switches: Vec<(u32, u32)>,
    pub channel: Channel,
    /// What started it (an emitter's building), to stop its sounds by.
    pub tag: Option<u64>,
}

struct Voice {
    entity: Entity,
    tag: Option<u64>,
    /// The node the playing started from (an action's target) and the sound's own.
    root: u32,
    /// The user volume it plays under.
    channel: Channel,
}

/// A continuous container playing its children one after another.
struct Seq {
    node: u32,
    root: u32,
    at: Option<Vec3>,
    switches: Switches,
    channel: Channel,
    next: f32,
    left: Option<u32>,
    last: Option<u32>,
    tag: Option<u64>,
}

/// A music node playing (kenshi_audio::music): the action's target (a switch, playlist or
/// segment), what it plays now and when the next segment starts.
struct MusicPlay {
    root: u32,
    channel: Channel,
    tag: Option<u64>,
    /// A switch root's child playing, and the one to change to at the next segment.
    child: Option<u32>,
    switch_to: Option<u32>,
    /// The playlist walked (and its walk); a segment root played once.
    playlist: Option<u32>,
    walk: PlaylistWalk,
    played: bool,
    /// When the next segment's entry cue comes (s); the clips waiting (start, source, gain dB,
    /// seek s); the voices (entity, gain).
    next_at: f32,
    pending: Vec<(f32, u32, f32, f32)>,
    voices: Vec<(Entity, f32)>,
    /// The fade (start, length, from, to) on the play's gain; stopping when it reaches 0.
    fade: (f32, f32, f32, f32),
    stopping: bool,
    /// No segment is left: gone once its voices end.
    done: bool,
}

impl MusicPlay {
    fn gain(&self, now: f32) -> f32 {
        let (t0, len, a, b) = self.fade;
        if len <= 0.0 || now >= t0 + len {
            b
        } else {
            a + (b - a) * ((now - t0) / len).clamp(0.0, 1.0)
        }
    }
}

/// A sound waiting its initial delay.
struct Delayed {
    at_time: f32,
    source: u32,
    node: u32,
    root: u32,
    at: Option<Vec3>,
    channel: Channel,
    tag: Option<u64>,
}

#[derive(Resource)]
pub struct Sound {
    dir: std::path::PathBuf,
    lib: Library,
    banks: Vec<(String, Arc<Bank>)>,
    /// Event -> the bank it came from.
    event_bank: HashMap<u32, usize>,
    cache: HashMap<u32, Handle<AudioSource>>,
    /// Sound (WEM) -> its length (s).
    lengths: HashMap<u32, f32>,
    pub switches: Switches,
    posts: Vec<Post>,
    voices: Vec<Voice>,
    seqs: Vec<Seq>,
    delayed: Vec<Delayed>,
    /// The banks' interactive music, and the music playing (MusicPlay) and fading out.
    imusic: Music,
    music_plays: Vec<MusicPlay>,
    music_fading: Vec<(Entity, f32, f32, f32)>,
    /// Each playlist's step and avoid-repeat memories (kept between plays), each sequence step
    /// track's next sub-track, the late clips to seek once their sinks are there.
    music_memory: HashMap<u32, (HashMap<u32, usize>, HashMap<u32, Vec<usize>>)>,
    music_memory_tracks: HashMap<u32, u32>,
    music_seek: Vec<(Entity, f32)>,
    rng: u64,
    volumes: HashMap<&'static str, f32>,
    /// Volume changes not yet put on the sounds playing: the channel and the factor.
    rescale: Vec<(Channel, f32)>,
    music_frequency: f32,
    /// The region's ambience event suffix ("Desert") and day, its music bank.
    ambience: Option<(String, bool)>,
    music: Option<String>,
    music_next: f32,
    /// Music banks left, unloaded at the time (F028318: 60 s after).
    music_leaving: Vec<(String, f32)>,
    /// When the piece playing ends (the next is counted from then).
    music_until: f32,
    in_world: bool,
    pub enabled: bool,
    /// The tag of the post being started, and the tags to stop.
    cur_tag: Option<u64>,
    stops: Vec<u64>,
    /// Each character's Surface_Material switch value (Character +0x688, F039207).
    surfaces: HashMap<usize, &'static str>,
}

/// The banks loaded at the start (F028319, F028314, F028312).
const START_BANKS: [&str; 6] = ["Init.bnk", "SFX_General_Bank.bnk", "VO_Bank.bnk", "SFX_Amb_Indoors_Bank.bnk", "SFX_Amb_Weather_Bank.bnk", "MT_Music_Bank.bnk"];
/// enum Ambience (fcs_enums.def) -> the bank's and events' name.
const AMBIENCE: [&str; 10] = ["Ashlands", "BoneYard", "Canyons", "Coast", "Desert", "Forest", "Jungle", "Plains", "Swamplands", "Tarsands"];
const AMBIENCE_BANK: [&str; 10] = ["Ashlands", "Boneyard", "Canyons", "Coast", "Desert", "Forest", "Jungle", "Plains", "Swamplands", "Tarsands"];
/// enum BiomeMusic -> its bank (4 none).
const MUSIC: [&str; 4] = ["Dunes", "Savannah", "Swamplands", "Tarsands"];
/// One unit of distance in Bevy's spatial audio.
const SPATIAL_SCALE: f32 = 0.01;

impl Sound {
    /// Set a volume option (settings.cfg's "Music volume" and the like, 0 .. 1) while playing
    /// (F032926: the bus volumes at once, the sounds playing too).
    pub fn set_volume(&mut self, key: &str, v: f32) {
        let channels = [
            ("Music volume", Channel::Music),
            ("Ambient volume", Channel::Ambient),
            ("Footstep volume", Channel::Footstep),
            ("Sfx volume", Channel::Sfx),
            ("UI volume", Channel::Ui),
            ("VO volume", Channel::Vo),
        ];
        for (k, c) in channels {
            if k == key {
                let old = self.volumes.get(k).copied().unwrap_or(0.5);
                let new = v.clamp(0.0, 1.0);
                self.volumes.insert(k, new);
                // (the music's own volume is set each frame)
                if c != Channel::Music && old > 0.0 {
                    self.rescale.push((c, new / old));
                }
            }
        }
    }

    /// Set the "Music frequency" option (minutes, at least half a minute) while playing
    /// (F032926): the next piece's wait is drawn from it.
    pub fn set_music_frequency(&mut self, minutes: f32) {
        self.music_frequency = minutes.max(0.5) * 60.0;
    }

    pub fn new(root: &std::path::Path) -> Sound {
        let cfg = kenshi_io::cfg::text(root, "settings.cfg");
        let get = |k: &str, d: f32| cfg.lines().find_map(|l| l.strip_prefix(&format!("{k}=")).and_then(|v| v.trim().parse().ok())).unwrap_or(d);
        let mut volumes = HashMap::new();
        for k in ["Music volume", "Ambient volume", "Footstep volume", "Sfx volume", "UI volume", "VO volume"] {
            volumes.insert(k, get(k, 0.5));
        }
        let mut s = Sound {
            dir: root.join("data/audio"),
            lib: Library::default(),
            banks: Vec::new(),
            event_bank: HashMap::new(),
            cache: HashMap::new(),
            lengths: HashMap::new(),
            switches: Switches::new(),
            posts: Vec::new(),
            voices: Vec::new(),
            seqs: Vec::new(),
            delayed: Vec::new(),
            imusic: Music::default(),
            music_plays: Vec::new(),
            music_fading: Vec::new(),
            music_memory: HashMap::new(),
            music_memory_tracks: HashMap::new(),
            music_seek: Vec::new(),
            rng: 0x9e37_79b9_7f4a_7c15,
            volumes,
            rescale: Vec::new(),
            music_frequency: get("Music frequency", 5.0).max(0.5) * 60.0,
            ambience: None,
            music: None,
            music_next: 0.0,
            music_leaving: Vec::new(),
            music_until: -1.0e9,
            in_world: false,
            enabled: std::env::var("KENSHI_NO_SOUND").is_err(),
            cur_tag: None,
            stops: Vec::new(),
            surfaces: HashMap::new(),
        };
        for b in START_BANKS {
            s.load(b);
        }
        s
    }

    fn load(&mut self, file: &str) -> bool {
        if self.banks.iter().any(|(n, _)| n == file) {
            return true;
        }
        let Ok(bytes) = std::fs::read(self.dir.join(file)) else { return false };
        let Some(bank) = Bank::parse(bytes) else { return false };
        self.lib.add(&bank);
        self.imusic.add(&bank);
        let k = self.banks.len();
        for o in bank.objects.iter().filter(|o| o.kind == 4) {
            self.event_bank.insert(o.id, k);
        }
        self.banks.push((file.to_string(), Arc::new(bank)));
        true
    }

    /// (assumption) A bank left: its sounds no longer cached (its nodes stay known).
    fn unload(&mut self, file: &str) {
        if let Some(k) = self.banks.iter().position(|(n, _)| n == file) {
            let (_, bank) = &self.banks[k];
            for id in bank.sounds.keys() {
                self.cache.remove(id);
            }
            // keep the index stable: an empty bank in its place
            self.banks[k].1 = Arc::new(Bank::default());
            self.banks[k].0 = String::new();
        }
    }

    fn rand(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng % 1_000_000) as f32 / 1_000_000.0
    }

    /// Post event `name` everywhere.
    pub fn post(&mut self, name: &str, channel: Channel) {
        self.post_with(name, None, &[], channel);
    }

    /// Post event `name` at a place with switches (group, value names).
    pub fn post_with(&mut self, name: &str, at: Option<Vec3>, switches: &[(&str, &str)], channel: Channel) {
        if !self.enabled {
            return;
        }
        self.posts.push(Post { event: name_id(name), at, switches: switches.iter().map(|(g, v)| (name_id(g), name_id(v))).collect(), channel, tag: None });
    }

    /// [`Sound::post_with`] its sounds tagged (stopped with [`Sound::stop_tag`]).
    pub fn post_tagged(&mut self, name: &str, at: Option<Vec3>, switches: &[(&str, &str)], channel: Channel, tag: u64) {
        if !self.enabled {
            return;
        }
        self.posts.push(Post { event: name_id(name), at, switches: switches.iter().map(|(g, v)| (name_id(g), name_id(v))).collect(), channel, tag: Some(tag) });
    }

    /// Stop the sounds tagged `tag` (and their sequences).
    pub fn stop_tag(&mut self, tag: u64) {
        self.stops.push(tag);
    }

    /// The game is unloaded (super::unload_world): what was to play dropped, the region's
    /// ambience and music banks unloaded; the voices playing are returned to be despawned.
    pub fn leave_world(&mut self) -> Vec<Entity> {
        self.posts.clear();
        self.seqs.clear();
        let mut gone: Vec<Entity> = self.music_plays.drain(..).flat_map(|m| m.voices.into_iter().map(|v| v.0)).collect();
        gone.extend(self.music_fading.drain(..).map(|f| f.0));
        self.delayed.clear();
        self.stops.clear();
        if let Some((old, _)) = self.ambience.take()
            && let Some(k) = AMBIENCE.iter().position(|x| *x == old)
        {
            self.unload(&format!("SFX_Amb_{}.bnk", AMBIENCE_BANK[k]));
        }
        if let Some(old) = self.music.take() {
            self.unload(&format!("{old}_Music_Bank.bnk"));
        }
        for (old, _) in std::mem::take(&mut self.music_leaving) {
            self.unload(&format!("{old}_Music_Bank.bnk"));
        }
        self.music_until = -1.0e9;
        gone.extend(self.voices.drain(..).map(|v| v.entity));
        gone
    }

    /// The sounds and music started from these action targets stopped now.
    fn stop_roots(&mut self, commands: &mut Commands, roots: &[u32]) {
        let stop: Vec<Entity> = self.voices.iter().filter(|v| roots.contains(&v.root)).map(|v| v.entity).collect();
        for e in stop {
            commands.entity(e).despawn();
        }
        self.voices.retain(|v| !roots.contains(&v.root));
        for m in self.music_plays.iter_mut().filter(|m| roots.contains(&m.root)) {
            for (e, _) in m.voices.drain(..) {
                commands.entity(e).despawn();
            }
            m.pending.clear();
            m.done = true;
        }
    }

    fn volume(&self, c: Channel) -> f32 {
        let k = match c {
            Channel::Music => "Music volume",
            Channel::Ambient => "Ambient volume",
            Channel::Footstep => "Footstep volume",
            Channel::Sfx => "Sfx volume",
            Channel::Ui => "UI volume",
            Channel::Vo => "VO volume",
        };
        // (assumption) the settings' 0.5 is the mix as made: twice it full
        self.volumes.get(k).copied().unwrap_or(0.5) * 2.0
    }

    /// A sound's audio, made once.
    fn audio(&mut self, source: u32, assets: &mut Assets<AudioSource>) -> Option<Handle<AudioSource>> {
        if let Some(h) = self.cache.get(&source) {
            return Some(h.clone());
        }
        let bank = self.banks.iter().find(|(_, b)| b.sounds.contains_key(&source))?.1.clone();
        let wem = bank.sound(source)?;
        if let Ok(info) = kenshi_audio::wem::info(wem) {
            self.lengths.insert(source, info.samples as f32 / info.sample_rate.max(1) as f32);
        }
        let ogg = match kenshi_audio::wem::to_ogg(wem) {
            Ok(o) => o,
            Err(e) => {
                eprintln!("sound: {source}: {e}");
                return None;
            }
        };
        let h = assets.add(AudioSource { bytes: ogg.into() });
        self.cache.insert(source, h.clone());
        Some(h)
    }
}

/// Spawn one sound.
#[allow(clippy::too_many_arguments)]
fn spawn_sound(commands: &mut Commands, s: &mut Sound, assets: &mut Assets<AudioSource>, source: u32, node: u32, root: u32, at: Option<Vec3>, channel: Channel) {
    let Some(handle) = s.audio(source, assets) else { return };
    let (db, cents) = s.lib.mix(node);
    let gain = 10f32.powf(db / 20.0) * s.volume(channel);
    let looped = s.lib.looped(node);
    let settings = PlaybackSettings {
        mode: if looped { PlaybackMode::Loop } else { PlaybackMode::Despawn },
        volume: Volume::Linear(gain),
        speed: 2f32.powf(cents / 1200.0),
        spatial: at.is_some(),
        spatial_scale: at.map(|_| SpatialScale::new(SPATIAL_SCALE)),
        ..PlaybackSettings::ONCE
    };
    if std::env::var("KENSHI_SOUND_DEBUG").is_ok() {
        eprintln!("sound: play {source} (node {node}) gain {gain:.2}{}{}", if looped { " looped" } else { "" }, at.map_or(String::new(), |a| format!(" at {a:.0}")));
    }
    let e = commands.spawn((AudioPlayer::new(handle), settings, Transform::from_translation(at.unwrap_or_default()))).id();
    s.voices.push(Voice { entity: e, root, tag: s.cur_tag, channel });
}

/// Start node `id` (from the action's target `root`): a continuous container as a sequence,
/// else what it picks now.
#[allow(clippy::too_many_arguments)]
fn start(commands: &mut Commands, s: &mut Sound, assets: &mut Assets<AudioSource>, time: f32, id: u32, root: u32, at: Option<Vec3>, sw: &Switches, channel: Channel, depth: u32) {
    if depth > 16 {
        return;
    }
    let Some(n) = s.lib.nodes.get(&id).cloned() else { return };
    let delay = n.props.delay;
    match &n.kind {
        Kind::Sound { source, silent } => {
            if *silent {
                return;
            }
            if delay > 0.05 {
                s.delayed.push(Delayed { at_time: time + delay, source: *source, node: id, root, at, channel, tag: s.cur_tag });
            } else {
                spawn_sound(commands, s, assets, *source, id, root, at, channel);
            }
        }
        Kind::RanSeq { continuous: true, loops, .. } => {
            s.seqs.push(Seq { node: id, root, at, switches: sw.clone(), channel, next: time + delay, left: if *loops == 0 { None } else { Some(*loops as u32) }, last: None, tag: s.cur_tag });
        }
        Kind::RanSeq { .. } => {
            if let Some(c) = choose(s, id, None) {
                start(commands, s, assets, time, c, root, at, sw, channel, depth + 1);
            }
        }
        Kind::Switch { group, default, map, .. } => {
            let want = sw.get(group).copied().unwrap_or(*default);
            let items = map.iter().find(|(v, _)| *v == want).or_else(|| map.iter().find(|(v, _)| v == default)).map(|(_, i)| i.clone()).unwrap_or_default();
            for c in items {
                start(commands, s, assets, time, c, root, at, sw, channel, depth + 1);
            }
        }
        Kind::Layer => {
            for c in n.children.clone() {
                start(commands, s, assets, time, c, root, at, sw, channel, depth + 1);
            }
        }
        Kind::Mixer => {}
    }
}

/// A random / sequence container's child: by weight, not the one just played.
fn choose(s: &mut Sound, id: u32, last: Option<u32>) -> Option<u32> {
    let Some(Kind::RanSeq { random, playlist, .. }) = s.lib.nodes.get(&id).map(|n| n.kind.clone()) else { return None };
    if playlist.is_empty() {
        return None;
    }
    if !random {
        let i = last.and_then(|l| playlist.iter().position(|p| p.0 == l)).map_or(0, |i| (i + 1) % playlist.len());
        return Some(playlist[i].0);
    }
    let pool: Vec<(u32, i32)> = if playlist.len() > 1 { playlist.iter().copied().filter(|p| Some(p.0) != last).collect() } else { playlist.clone() };
    let total: f32 = pool.iter().map(|p| p.1.max(1) as f32).sum();
    let mut x = s.rand() * total;
    pool.iter().find(|p| {
        x -= p.1.max(1) as f32;
        x <= 0.0
    }).or(pool.last()).map(|p| p.0)
}

/// The longest sound a node would play (s), for a sequence's next start.
fn length_of(s: &Sound, id: u32, depth: u32) -> f32 {
    if depth > 8 {
        return 0.0;
    }
    let Some(n) = s.lib.nodes.get(&id) else { return 0.0 };
    match &n.kind {
        Kind::Sound { source, .. } => s.lengths.get(source).copied().unwrap_or(2.0) + n.props.delay,
        _ => n.children.iter().map(|&c| length_of(s, c, depth + 1)).fold(0.0, f32::max),
    }
}

/// Start the posted events, the sequences' next sounds and the delayed ones; drop the voices
/// done.
pub fn play(mut commands: Commands, mut s: ResMut<Sound>, mut assets: ResMut<Assets<AudioSource>>, time: Res<Time>, live: Query<Entity, With<AudioPlayer>>, mut sinks: Query<&mut AudioSink>) {
    let now = time.elapsed_secs();
    let s = &mut *s;
    s.voices.retain(|v| live.get(v.entity).is_ok());
    // a volume changed: the sounds playing under it (a sound muted at 0 comes back with its
    // next playing)
    for (c, k) in std::mem::take(&mut s.rescale) {
        for v in s.voices.iter().filter(|v| v.channel == c) {
            if let Ok(mut sink) = sinks.get_mut(v.entity) {
                let g = sink.volume().to_linear();
                sink.set_volume(Volume::Linear(g * k));
            }
        }
    }
    // tags stopped: their sounds, sequences and waiting sounds
    for tag in std::mem::take(&mut s.stops) {
        for v in s.voices.iter().filter(|v| v.tag == Some(tag)) {
            commands.entity(v.entity).despawn();
        }
        s.voices.retain(|v| v.tag != Some(tag));
        s.seqs.retain(|q| q.tag != Some(tag));
        s.delayed.retain(|d| d.tag != Some(tag));
        for m in s.music_plays.iter_mut().filter(|m| m.tag == Some(tag)) {
            for (e, _) in m.voices.drain(..) {
                commands.entity(e).despawn();
            }
            m.pending.clear();
            m.done = true;
        }
    }
    let posts = std::mem::take(&mut s.posts);
    for p in posts {
        s.cur_tag = p.tag;
        let mut sw = s.switches.clone();
        sw.extend(p.switches.iter().copied());
        let Some(actions) = s.lib.events.get(&p.event).cloned() else { continue };
        if std::env::var("KENSHI_SOUND_DEBUG").is_ok() {
            eprintln!("sound: event {} ({} actions)", p.event, actions.len());
        }
        for a in actions {
            match a {
                Action::Play(t) => {
                    if s.lib.nodes.contains_key(&t) {
                        start(&mut commands, s, &mut assets, now, t, t, p.at, &sw, p.channel, 0);
                    } else if s.imusic.nodes.contains_key(&t) {
                        // interactive music: from now, faded in over the action's fade
                        let fade = s.lib.fades.get(&(p.event, t)).copied().unwrap_or(0.0);
                        if std::env::var("KENSHI_SOUND_DEBUG").is_ok() {
                            eprintln!("sound: music {t} starts (fade {fade} s)");
                        }
                        s.music_plays.push(MusicPlay {
                            root: t,
                            channel: p.channel,
                            tag: p.tag,
                            child: None,
                            switch_to: None,
                            playlist: None,
                            walk: PlaylistWalk::default(),
                            played: false,
                            next_at: now,
                            pending: Vec::new(),
                            voices: Vec::new(),
                            fade: (now, fade, if fade > 0.0 { 0.0 } else { 1.0 }, 1.0),
                            stopping: false,
                            done: false,
                        });
                    }
                }
                Action::Stop(t) => {
                    // interactive music: faded out over the action's fade, then gone
                    let fade = s.lib.fades.get(&(p.event, t)).copied().unwrap_or(0.0);
                    for k in 0..s.music_plays.len() {
                        if s.music_plays[k].root != t || s.music_plays[k].stopping {
                            continue;
                        }
                        let g = s.music_plays[k].gain(now);
                        let m = &mut s.music_plays[k];
                        m.stopping = true;
                        m.pending.clear();
                        m.fade = (now, fade, g, 0.0);
                    }
                    let under = |s: &Sound, mut n: u32| {
                        for _ in 0..32 {
                            if n == t {
                                return true;
                            }
                            match s.lib.nodes.get(&n) {
                                Some(x) => n = x.parent,
                                None => return false,
                            }
                        }
                        false
                    };
                    let stop: Vec<Entity> = s.voices.iter().filter(|v| v.root == t || under(s, v.root)).map(|v| v.entity).collect();
                    for e in stop {
                        commands.entity(e).despawn();
                    }
                    s.voices.retain(|v| !(v.root == t || under_static(&s.lib, v.root, t)));
                    s.seqs.retain(|q| !(q.root == t || under_static(&s.lib, q.root, t)));
                }
                Action::SetState { group, state } => {
                    s.switches.insert(group, state);
                }
                Action::Other(_) => {}
            }
        }
    }
    // the sequences
    let mut k = 0;
    while k < s.seqs.len() {
        if s.seqs[k].next > now {
            k += 1;
            continue;
        }
        let (node, root, at, sw, channel, last) = {
            let q = &s.seqs[k];
            (q.node, q.root, q.at, q.switches.clone(), q.channel, q.last)
        };
        s.cur_tag = s.seqs[k].tag;
        if let Some(l) = s.seqs[k].left.as_mut() {
            if *l == 0 {
                s.seqs.remove(k);
                continue;
            }
            *l -= 1;
        }
        let Some(c) = choose(s, node, last) else {
            s.seqs.remove(k);
            continue;
        };
        start(&mut commands, s, &mut assets, now, c, root, at, &sw, channel, 0);
        let (mode, t, lo, hi) = match s.lib.nodes.get(&node).map(|n| &n.kind) {
            Some(Kind::RanSeq { transition, .. }) => *transition,
            _ => (0, 0.0, 0.0, 0.0),
        };
        let gap = if mode == 2 { (t + lo + (hi - lo) * s.rand()) / 1000.0 } else { 0.0 };
        let next = now + length_of(s, c, 0).max(0.5) + gap.max(0.0);
        let q = &mut s.seqs[k];
        q.last = Some(c);
        q.next = next;
        k += 1;
    }
    // the delayed
    let due: Vec<Delayed> = {
        let (a, b): (Vec<_>, Vec<_>) = std::mem::take(&mut s.delayed).into_iter().partition(|d| d.at_time <= now);
        s.delayed = b;
        a
    };
    for d in due {
        s.cur_tag = d.tag;
        spawn_sound(&mut commands, s, &mut assets, d.source, d.node, d.root, d.at, d.channel);
    }
    s.cur_tag = None;
}

/// The next segment music play `k` plays (its switch's child, playlist walk or segment); None:
/// none left.
fn next_segment(s: &mut Sound, k: usize) -> Option<u32> {
    let switches = s.switches.clone();
    let mut node = {
        let m = &mut s.music_plays[k];
        if let Some(c) = m.switch_to.take() {
            m.child = Some(c);
            m.playlist = None;
            m.walk = PlaylistWalk::default();
            m.played = false;
        }
        m.child.unwrap_or(m.root)
    };
    for _ in 0..8 {
        let kind = s.imusic.nodes.get(&node)?.kind.clone();
        match kind {
            MusicKind::Segment { .. } => {
                let m = &mut s.music_plays[k];
                if m.played {
                    return None;
                }
                m.played = true;
                return Some(node);
            }
            MusicKind::Playlist { items, .. } => {
                if s.music_plays[k].playlist != Some(node) {
                    let m = &mut s.music_plays[k];
                    m.playlist = Some(node);
                    m.walk = PlaylistWalk::default();
                }
                // the step and avoid-repeat memories are the playlist's own, kept between plays
                let mut walk = std::mem::take(&mut s.music_plays[k].walk);
                if walk.step.is_empty() && walk.recent.is_empty() {
                    if let Some((st, re)) = s.music_memory.get(&node) {
                        walk.step = st.clone();
                        walk.recent = re.clone();
                    }
                }
                let mut rng = s.rng;
                let mut rand = || {
                    rng ^= rng << 13;
                    rng ^= rng >> 7;
                    rng ^= rng << 17;
                    (rng % 1_000_000) as f32 / 1_000_000.0
                };
                let next = walk.next(&items, &mut rand);
                s.rng = rng;
                s.music_memory.insert(node, (walk.step.clone(), walk.recent.clone()));
                s.music_plays[k].walk = walk;
                return next;
            }
            MusicKind::Switch { .. } => {
                let c = s.imusic.switch_child(node, &|g| switches.get(&g).copied());
                if c == 0 {
                    return None;
                }
                node = c;
            }
            MusicKind::Track { .. } => return None,
        }
    }
    None
}

/// Interactive music, each frame: switches following their values, segments started at their
/// entry cues, clips at their places, fades.
pub fn music(mut commands: Commands, mut s: ResMut<Sound>, mut assets: ResMut<Assets<AudioSource>>, time: Res<Time>, mut sinks: Query<&mut AudioSink>, live: Query<Entity, With<AudioPlayer>>) {
    let now = time.elapsed_secs();
    let s = &mut *s;
    let debug = std::env::var("KENSHI_SOUND_DEBUG").is_ok();
    // the voices fading out after a switch's immediate change
    s.music_fading.retain(|&(e, t0, len, g)| {
        let left = if len > 0.0 { 1.0 - (now - t0) / len } else { 0.0 };
        if left <= 0.0 || live.get(e).is_err() {
            if live.get(e).is_ok() {
                commands.entity(e).despawn();
            }
            return false;
        }
        if let Ok(mut sink) = sinks.get_mut(e) {
            sink.set_volume(Volume::Linear(g * left));
        }
        true
    });
    for k in 0..s.music_plays.len() {
        let root = s.music_plays[k].root;
        // a switch following its value (its transition rule: at once with its fades, else at the
        // exit cue)
        if !s.music_plays[k].stopping
            && !s.music_plays[k].done
            && let Some(MusicKind::Switch { rules, .. }) = s.imusic.nodes.get(&root).map(|n| n.kind.clone())
        {
            let switches = s.switches.clone();
            let want = s.imusic.switch_child(root, &|g| switches.get(&g).copied());
            let m = &s.music_plays[k];
            let cur = m.child;
            if m.child.is_some() && Some(want) != cur && m.switch_to != Some(want) {
                let fits = |ids: &[u32], id: Option<u32>| ids.iter().any(|&x| x == u32::MAX || Some(x) == id);
                let rule = rules
                    .iter()
                    .filter(|r| fits(&r.src, cur) && fits(&r.dst, Some(want)))
                    .max_by_key(|r| (!r.src.contains(&u32::MAX)) as u8 + (!r.dst.contains(&u32::MAX)) as u8)
                    .cloned()
                    .unwrap_or_default();
                if debug {
                    eprintln!("sound: music {root} switches {cur:?} -> {want} (sync {}, fades {} / {} ms)", rule.sync, rule.fade_out, rule.fade_in);
                }
                if rule.sync == 0 {
                    let g = m.gain(now);
                    let out = rule.fade_out.max(0) as f32 / 1000.0;
                    let m = &mut s.music_plays[k];
                    for (e, base) in m.voices.drain(..) {
                        s.music_fading.push((e, now, out, base * g));
                    }
                    m.pending.clear();
                    m.switch_to = Some(want);
                    m.next_at = now;
                    let inn = rule.fade_in.max(0) as f32 / 1000.0;
                    m.fade = (now, inn, if inn > 0.0 { 0.0 } else { 1.0 }, 1.0);
                } else {
                    s.music_plays[k].switch_to = Some(want);
                }
            }
        }
        // the next segment at its time
        if !s.music_plays[k].stopping && !s.music_plays[k].done && s.music_plays[k].next_at <= now {
            if s.music_plays[k].child.is_none()
                && let Some(MusicKind::Switch { .. }) = s.imusic.nodes.get(&root).map(|n| &n.kind)
            {
                let switches = s.switches.clone();
                let c = s.imusic.switch_child(root, &|g| switches.get(&g).copied());
                s.music_plays[k].child = Some(c);
            }
            match next_segment(s, k) {
                Some(seg) => {
                    let Some(n) = s.imusic.nodes.get(&seg).cloned() else { continue };
                    let MusicKind::Segment { entry, exit, .. } = n.kind else { continue };
                    let at = s.music_plays[k].next_at;
                    if debug {
                        eprintln!("sound: music {root} plays segment {seg} ({:.1} s{})", (exit - entry) / 1000.0, if n.children.is_empty() { ", silent" } else { "" });
                    }
                    for t in &n.children {
                        let Some(MusicKind::Track { clips, kind, sub_tracks, switch }) = s.imusic.nodes.get(t).map(|x| x.kind.clone()) else { continue };
                        // the sub-track: a random step track's at random, a switch track's by its
                        // switch, a sequence step track's in turn (assumption: from the first)
                        let sub = match kind {
                            1 => Some((s.rand() * sub_tracks.max(1) as f32) as u32),
                            2 => {
                                let c = s.music_memory_tracks.entry(*t).or_insert(0);
                                let v = *c % sub_tracks.max(1);
                                *c += 1;
                                Some(v)
                            }
                            3 => switch.as_ref().map(|(g, d, ids)| {
                                let v = s.switches.get(g).copied().unwrap_or(*d);
                                ids.iter().position(|x| *x == v).unwrap_or(0) as u32
                            }),
                            _ => None,
                        };
                        let db = s.imusic.volume(*t);
                        for c in clips.iter().filter(|c| sub.is_none_or(|v| c.sub_track == v)) {
                            let start = at + ((c.play_at + c.begin_trim - entry) / 1000.0) as f32;
                            let seek = (c.begin_trim / 1000.0) as f32;
                            s.music_plays[k].pending.push((start, c.source, db, seek));
                        }
                    }
                    s.music_plays[k].next_at = at + ((exit - entry) / 1000.0).max(0.05) as f32;
                    s.music_until = s.music_plays[k].next_at;
                }
                None => s.music_plays[k].done = true,
            }
        }
        // the clips due (a late one seeks to where it should be)
        let due: Vec<(f32, u32, f32, f32)> = {
            let m = &mut s.music_plays[k];
            let (a, b): (Vec<_>, Vec<_>) = std::mem::take(&mut m.pending).into_iter().partition(|c| c.0 <= now);
            m.pending = b;
            a
        };
        for (start, source, db, seek) in due {
            let Some(h) = s.audio(source, &mut assets) else { continue };
            let base = 10f32.powf(db / 20.0) * s.volume(s.music_plays[k].channel);
            let g = s.music_plays[k].gain(now);
            let e = commands.spawn((AudioPlayer::new(h), PlaybackSettings { mode: PlaybackMode::Despawn, volume: Volume::Linear(base * g), ..PlaybackSettings::ONCE })).id();
            let late = (now - start).max(0.0) + seek;
            if late > 0.05 {
                s.music_seek.push((e, late));
            }
            s.music_plays[k].voices.push((e, base));
        }
        // fades
        let g = s.music_plays[k].gain(now);
        let m = &mut s.music_plays[k];
        m.voices.retain(|(e, _)| live.get(*e).is_ok());
        for (e, base) in &m.voices {
            if let Ok(mut sink) = sinks.get_mut(*e) {
                sink.set_volume(Volume::Linear(base * g));
            }
        }
        if m.stopping && g <= 0.0 {
            for (e, _) in m.voices.drain(..) {
                commands.entity(e).despawn();
            }
            m.done = true;
        }
    }
    // seeks once the sinks are there
    s.music_seek.retain(|&(e, at)| match sinks.get_mut(e) {
        Ok(sink) => {
            let _ = sink.try_seek(std::time::Duration::from_secs_f32(at));
            false
        }
        Err(_) => live.get(e).is_ok(),
    });
    s.music_plays.retain(|m| !(m.done && m.voices.is_empty() && m.pending.is_empty()));
}

fn under_static(lib: &Library, mut n: u32, t: u32) -> bool {
    for _ in 0..32 {
        if n == t {
            return true;
        }
        match lib.nodes.get(&n) {
            Some(x) => n = x.parent,
            None => return false,
        }
    }
    false
}

/// The listener on the camera.
pub fn listener(mut commands: Commands, cams: Query<Entity, (MainCam, Without<SpatialListener>)>) {
    for e in &cams {
        commands.entity(e).insert(SpatialListener::new(4.0));
    }
}

/// The menu's theme until a game begins (MainMenu: "Boot_Up_Game"; "Create_or_Load_Game").
pub fn menu_music(mut commands: Commands, mut s: ResMut<Sound>, ready: Option<Res<WorldReady>>, mut started: Local<bool>) {
    if !*started {
        *started = true;
        if ready.is_none() {
            s.post("Boot_Up_Game", Channel::Music);
        }
    }
    if ready.is_some() && !s.in_world {
        s.in_world = true;
        // "Create_or_Load_Game": its own sound and the theme stopped (its stop action)
        s.post("Create_or_Load_Game", Channel::Music);
        let _ = &mut commands;
    }
}

/// The region's ambience and music: on a change of region (or of day and night) "Leave_" the
/// old and "Enter_<new>_<Day|Night>"; music every "Music frequency" minutes.
pub fn regions(
    mut commands: Commands,
    mut s: ResMut<Sound>,
    game: Res<Game>,
    weather: Option<Res<super::WeatherState>>,
    time: Res<Time>,
    _ready: Res<WorldReady>,
) {
    let Some(w) = weather else { return };
    let Some(r) = w.current else { return };
    let Some(group) = game.sim.data.get(&w.regions[r].biome_group) else { return };
    let int = |k: &str| group.ints.get(k).copied();
    // Sky's day test (F040780): the hour between the GLOBAL CONSTANTS "sunrise" and "sunset"
    let hour = game.sim.hour();
    let day = (game.sim.sunrise as f64) < hour && hour < game.sim.sunset as f64;
    // the state the ambience switches on
    s.switches.insert(name_id("Time_in_Game"), name_id(if day { "day" } else { "night" }));
    if let Some(a) = int("ambience").and_then(|a| AMBIENCE.get(a as usize)) {
        let k = AMBIENCE.iter().position(|x| x == a).unwrap_or(0);
        let want = (a.to_string(), day);
        if s.ambience.as_ref() != Some(&want) {
            if let Some((old, _)) = s.ambience.clone() {
                s.post(&format!("Leave_{old}"), Channel::Ambient);
                // a new region: the notification (the banner's "New_Biome")
                if old != *a {
                    s.post_with("Notifications", None, &[("Notifications", "New_Biome")], Channel::Ui);
                }
                if old != *a {
                    let ok = AMBIENCE.iter().position(|x| *x == old).unwrap_or(0);
                    s.unload(&format!("SFX_Amb_{}.bnk", AMBIENCE_BANK[ok]));
                }
            }
            s.load(&format!("SFX_Amb_{}.bnk", AMBIENCE_BANK[k]));
            s.post(&format!("Enter_{a}_{}", if day { "Day" } else { "Night" }), Channel::Ambient);
            if std::env::var("KENSHI_SOUND_DEBUG").is_ok() {
                eprintln!("sound: ambience {a} {}", if day { "day" } else { "night" });
            }
            s.ambience = Some(want);
        }
    }
    // music: the region's (F028312): the Biomes switch and Music_<music>_Play to post; the one
    // left plays on until its bank goes 60 s later (F028318)
    let now = time.elapsed_secs();
    let music = int("music").and_then(|m| MUSIC.get(m as usize)).map(|m| m.to_string());
    if music != s.music {
        if let Some(old) = s.music.take() {
            s.music_leaving.push((old, now + 60.0));
        }
        if let Some(m) = &music {
            s.music_leaving.retain(|(o, _)| o != m);
            s.load(&format!("{m}_Music_Bank.bnk"));
            s.switches.insert(name_id("Biomes"), name_id(m));
        }
        s.music = music.clone();
    }
    for (old, _) in s.music_leaving.iter().filter(|(_, at)| now > *at).cloned().collect::<Vec<_>>() {
        // (stand-in for UnloadBank stopping its sounds) the music playing from it stops
        let roots: Vec<u32> = s.lib.events.get(&name_id(&format!("Music_{old}_Play"))).map(|a| a.iter().filter_map(|x| if let Action::Play(t) = x { Some(*t) } else { None }).collect()).unwrap_or_default();
        s.stop_roots(&mut commands, &roots);
        s.unload(&format!("{old}_Music_Bank.bnk"));
    }
    s.music_leaving.retain(|(_, at)| now <= *at);
    // F028318: on the real-time clock, every 0.8 to 1.4 x the "Music frequency" (the unit random),
    // the region's music posted when the music volume is above 0 (none posted without one)
    if now > s.music_next {
        let u = s.rand();
        let (lo, hi) = (s.music_frequency * 0.8, s.music_frequency * 1.4);
        s.music_next = u * (hi - lo) + lo + now;
        if let Some(m) = music
            && s.volume(Channel::Music) > 0.0
        {
            s.post(&format!("Music_{m}_Play"), Channel::Music);
        }
    }
}

/// Kenshi's RaceSound (fcs_enums.def; a RACE's "sounds"): its voice (VO_Races, by gender for
/// humans and shek) or creature (Creature_VO, WEAPONS, Armor_Worn).
fn race_sound(game: &Game, id: usize) -> (Option<&'static str>, Option<&'static str>) {
    let p = &game.sim.folk[id];
    let race = p.race.clone().or_else(|| game.looks.get(&id).map(|l| l.race.clone()));
    let rec = race.as_deref().and_then(|r| game.sim.data.get(r).or_else(|| game.sim.data.of_type(kenshi_data::ItemType::Race).find(|x| x.name == r)));
    let female = game.looks.get(&id).is_some_and(|l| l.female);
    let k = rec.and_then(|r| r.ints.get("sounds").copied()).unwrap_or(0);
    match k {
        0 => (Some(if female { "human_female" } else { "human_male" }), None),
        1 => (Some(if female { "shek_female" } else { "shek_male" }), None),
        2 => (Some("Hive"), None),
        3 => (Some("SKELETON"), None),
        4 => (None, Some("beak")),
        5 => (None, Some("Bull")),
        6 => (None, Some("cage")),
        7 => (None, Some("dog")),
        8 => (None, Some("duck")),
        9 => (None, Some("Gar")),
        10 => (None, Some("goat")),
        11 => (None, Some("Gorillo")),
        12 => (None, Some("IronSpider")),
        13 => (None, Some("Leviathan")),
        14 => (None, Some("pack")),
        15 => (None, Some("Spider")),
        16 => (None, Some("Turtle")),
        _ => (Some("human_male"), None),
    }
}

fn place(game: &Game, id: usize) -> Vec3 {
    Vec3::from(game.sim.folk[id].mover.position_units()) - game.zone.origin + Vec3::Y * 10.0
}

/// The fights: a swing "Attack" (WEAPONS: the weapon kind, a creature attack, unarmed
/// "HtH_Punch" or "HtH_Kick" by the striking limb, "_Robot" for a robot or a robotic limb), now
/// and then the attacker "VO_ATTACK"; a hit "Impact" (Impacts: "Heavy_Hit"
/// from 25 damage, else "Light_Hit"; unarmed "HtH_Impacts") and the victim "VO_Get_Hit"; a block
/// "Impact" "Parry"; someone falling "VO_Drop" (a creature dying "VO_Creature_Die").
/// (assumption) The timing within the swing and the voice chance are not decompiled: the swing
/// start, one voice in three.
pub fn fights(mut s: ResMut<Sound>, mut game: ResMut<Game>, mut down: Local<Vec<bool>>, _ready: Res<WorldReady>) {
    let cues = std::mem::take(&mut game.sim.arena.sh.cues);
    let near = |game: &Game, id: usize| place(game, id).distance(game.focus) < 1500.0;
    for c in cues {
        match c {
            kenshi_combat::arena::Cue::Swing(me) => {
                if !near(&game, me) {
                    continue;
                }
                use kenshi_combat::weapon_category::WeaponCategory as W;
                let (voice, creature) = race_sound(&game, me);
                // AttackState::tryAttack (F029615): an unarmed attack's striking limb (its last
                // frame's) picks a punch (an arm, 0 / 1) or a kick; a robot's, or a robotic limb's
                // (MedicalSystem limb state 2), the robot one
                let limb = game.sim.arena.f[me].last_attack.and_then(|k| game.sim.arena.sh.attacks.get(k)).and_then(|t| t.frame(i32::MAX)).map(|f| f.limb);
                let kick = limb.is_some_and(|l| l != 0 && l != 1);
                let robotic = limb.and_then(|l| game.sim.folk[me].limbs.get(l as usize)).is_some_and(|x| x.is_some());
                let robot = voice == Some("SKELETON") || robotic;
                let weapon = match (creature, Some(game.sim.arena.f[me].wielded.category)) {
                    (Some("Gorillo"), _) => "Gorillo_Fhand",
                    (Some(c), _) => c,
                    // drawWeapon (F039274) sets Weapons by the weapon's category (the table at
                    // 0x141e60c50; unarmed sets none): polearms sound blunt
                    (None, Some(W::SkillKatanas)) => "Katanas",
                    (None, Some(W::SkillSabres)) => "Sabres",
                    (None, Some(W::SkillBlunt | W::AttackPolearms)) => "Blunt",
                    (None, Some(W::SkillHeavy)) => "Heavy",
                    (None, Some(W::SkillHackers)) => "Hackers",
                    (None, Some(W::SkillBow)) => "Bow",
                    (None, Some(W::SkillTurret)) => "Turret",
                    _ => match (kick, robot) {
                        (false, false) => "HtH_Punch",
                        (false, true) => "HtH_Punch_Robot",
                        (true, false) => "HtH_Kick",
                        (true, true) => "HtH_Kick_Robot",
                    },
                };
                let at = Some(place(&game, me));
                s.post_with("Attack", at, &[("WEAPONS", weapon)], Channel::Sfx);
                if let Some(v) = voice
                    && s.rand() < 0.33
                {
                    s.post_with("VO_ATTACK", at, &[("VO_Races", v)], Channel::Vo);
                }
            }
            kenshi_combat::arena::Cue::Hit(a, v, dmg) => {
                if !near(&game, v) {
                    continue;
                }
                let unarmed = game.sim.arena.f[a].wielded.category == kenshi_combat::weapon_category::WeaponCategory::SkillUnarmed && race_sound(&game, a).1.is_none();
                let impact = if unarmed {
                    if race_sound(&game, v).0 == Some("SKELETON") { "HtH_Impacts_Robot" } else { "HtH_Impacts" }
                } else if dmg >= 25.0 {
                    "Heavy_Hit"
                } else {
                    "Light_Hit"
                };
                let at = Some(place(&game, v));
                s.post_with("Impact", at, &[("Impacts", impact)], Channel::Sfx);
                match race_sound(&game, v) {
                    (Some(voice), _) => s.post_with("VO_Get_Hit", at, &[("VO_Races", voice)], Channel::Vo),
                    (_, Some(c)) => s.post_with("VO_Creature_Alert", at, &[("Creature_VO", c)], Channel::Vo),
                    _ => {}
                }
            }
            kenshi_combat::arena::Cue::Block(d, _, dodged) => {
                if !dodged && near(&game, d) {
                    s.post_with("Impact", Some(place(&game, d)), &[("Impacts", "Parry")], Channel::Sfx);
                }
            }
            // a crossbow shot: "Attack" with WEAPONS its "shoot sound" (F033867, F033877)
            kenshi_combat::arena::Cue::Shoot(me, weapon) => {
                if near(&game, me) {
                    s.post_with("Attack", Some(place(&game, me)), &[("WEAPONS", weapon)], Channel::Sfx);
                }
            }
            // (assumption) the reload clip's "Reload Crossbow" event posts "Reload" (WEAPONS
            // Crossbow) as the reload begins
            kenshi_combat::arena::Cue::Reload(me) => {
                if near(&game, me) {
                    s.post_with("Reload", Some(place(&game, me)), &[("WEAPONS", "Crossbow")], Channel::Sfx);
                }
            }
            // a bolt in someone (F033863): "Impact", or off armour "Deflection" with Armor_Worn
            // the material; the victim's voice as for any hit
            kenshi_combat::arena::Cue::Bolt(_, v, material) => {
                if !near(&game, v) {
                    continue;
                }
                let at = Some(place(&game, v));
                match ["Cloth", "Leather", "Chain", "Plate"].get(material as usize).filter(|_| material >= 0) {
                    Some(m) => s.post_with("Deflection", at, &[("Armor_Worn", m)], Channel::Sfx),
                    None => s.post_with("Impact", at, &[], Channel::Sfx),
                }
                match race_sound(&game, v) {
                    (Some(voice), _) => s.post_with("VO_Get_Hit", at, &[("VO_Races", voice)], Channel::Vo),
                    (_, Some(c)) => s.post_with("VO_Creature_Alert", at, &[("Creature_VO", c)], Channel::Vo),
                    _ => {}
                }
            }
            // a turret's head turning (F033784)
            kenshi_combat::arena::Cue::TurretMove(at, start) => {
                let at = Vec3::from(at) - game.zone.origin;
                if at.distance(game.focus) < 1500.0 {
                    s.post_with(if start { "Turret_Mvt_Start" } else { "Turret_Mvt_Stop" }, Some(at), &[], Channel::Sfx);
                }
            }
            // a bolt into the ground (F033856): "Impact", Impacts "Stone"
            kenshi_combat::arena::Cue::BoltGround(at) => {
                let at = Vec3::from(at) - game.zone.origin;
                if at.distance(game.focus) < 1500.0 {
                    s.post_with("Impact", Some(at), &[("Impacts", "Stone")], Channel::Sfx);
                }
            }
        }
    }
    let n = game.sim.folk.len();
    // a game loaded in the program: those down already make no sound
    if game.is_added() {
        *down = game.sim.folk.iter().map(|p| p.down || p.dead).collect();
    }
    down.resize(n, false);
    for id in 0..n {
        let p = &game.sim.folk[id];
        let now = p.down || p.dead;
        if now && !down[id] && near(&game, id) {
            let at = Some(place(&game, id));
            match race_sound(&game, id) {
                (Some(voice), _) => s.post_with("VO_Drop", at, &[("VO_Races", voice)], Channel::Vo),
                (_, Some(c)) if p.dead => s.post_with("VO_Creature_Die", at, &[("Creature_VO", c)], Channel::Vo),
                _ => {}
            }
        }
        down[id] = now;
    }
}

/// GroundType (fcs_enums: sand, grass, concrete, wood, metal, water, mud, snow, dirt): the
/// Surface_Material values F051548 gives, shod and barefoot (its two tables; past them, 9 as
/// some biomes have, a null name: the switch is left as it was).
const SURFACES: [&str; 9] = ["Sand", "Grass", "Concrete", "Wood", "Metal", "Water", "Mud", "Snow", "Dirt"];
const SURFACES_BAREFOOT: [&str; 9] =
    ["BFoot_Sand", "BFoot_Grass", "Concrete", "BFoot_Wood", "BFoot_Metal", "Water", "Mud", "Snow", "BFoot_Dirt"];
/// F040611 works the terrain's surface out only this near the camera's target (squared).
const SURFACE_NEAR: f32 = 120_000.0;
/// Standing lower than this is in water (F040611).
const SURFACE_WATER: f32 = 100.0;

/// @fn F051635
///
/// The ground type of the terrain under a point: its BIOMES' "ground type", "slope type",
/// "grass type", "dirt type" or "road type" (F051626's five, by the terrain layer there,
/// F051634); off the map (no layer) concrete.
pub fn terrain_surface(data: &kenshi_data::GameDataContainer, biome: Option<&str>, layer: Option<usize>) -> i32 {
    const KEYS: [&str; 5] = ["ground type", "slope type", "grass type", "dirt type", "road type"];
    let Some(layer) = layer else { return 2 };
    let b = biome.and_then(|b| data.get(b));
    KEYS.get(layer).and_then(|k| b.and_then(|b| b.ints.get(*k).copied())).unwrap_or(0)
}

/// A footstep's switches (the animations' "Footsteps" events, `kenshi.rs` footsteps): the
/// Surface_Material and Armor_Worn of person `id` at `at` (scene).
///
/// Surface (F040611, kept per character as F039207 does): standing below 100 water; on the
/// terrain, within ~346 of the camera's target, `terrain` (`terrain_surface`), farther the
/// value it had; inside a building its "interior material" (Building +0x21c), on one's floor
/// outside its "exterior material" (+0x220). Barefoot (no boots, not a creature; (assumption)
/// the original asks the character's sound object) takes the BFoot_ names where there are
/// some.
/// (stand-in) The building of a floor outside is the nearest placed one (the original takes
/// the physics object under the feet).
///
/// Armour: the heaviest worn (cloth, leather, chain, plate), a creature its own body.
pub fn footstep_switches(s: &mut Sound, game: &Game, id: usize, at: Vec3, terrain: i32) -> (&'static str, &'static str) {
    let p = &game.sim.folk[id];
    let (_, creature) = race_sound(game, p.id);
    let ground = game.ground(at.x, at.z);
    let floor = at.y > ground + 3.0;
    let boots = p.inventory.equipped().any(|i| i.kind == Some(kenshi_data::ItemType::Armour) && i.slot == 9);
    let barefoot = !(boots || creature.is_some());
    let stands = p.mover.position_units()[1];
    let near = (at - game.focus).length_squared() < SURFACE_NEAR;
    let index = if stands < SURFACE_WATER {
        Some(5)
    } else if let Some(b) = p.inside {
        let rec = game.sim.placed.get(b).and_then(|pl| game.sim.data.get(&pl.building));
        Some(rec.and_then(|r| r.ints.get("interior material").copied()).unwrap_or(0))
    } else if floor {
        // (stand-in for the physics object under the feet) the nearest building's
        let me = p.mover.position_units();
        let nearest = game.sim.placed.iter().min_by(|a, b| {
            let d = |q: &[f32; 3]| (q[0] - me[0]).powi(2) + (q[2] - me[2]).powi(2);
            d(&a.position).total_cmp(&d(&b.position))
        });
        let rec = nearest.and_then(|pl| game.sim.data.get(&pl.building));
        Some(rec.and_then(|r| r.ints.get("exterior material").copied()).unwrap_or(0))
    } else if near || !s.surfaces.contains_key(&id) {
        Some(terrain)
    } else {
        None
    };
    let table = if barefoot { &SURFACES_BAREFOOT } else { &SURFACES };
    if let Some(name) = index.and_then(|i| usize::try_from(i).ok()).and_then(|i| table.get(i)) {
        s.surfaces.insert(id, name);
    }
    let surface = s.surfaces.get(&id).copied().unwrap_or(table[0]);
    let armour = match creature {
        Some(c) => c,
        None => {
            let heaviest = p
                .inventory
                .equipped()
                .filter(|i| i.kind == Some(kenshi_data::ItemType::Armour))
                .filter_map(|i| game.sim.data.get(&i.record).and_then(|r| r.ints.get("material type").copied()))
                .max()
                .unwrap_or(0);
            ["Cloth", "Leather", "Chain", "Plate"][heaviest.clamp(0, 3) as usize]
        }
    };
    (surface, armour)
}

/// The interface: a button "General_Click"; a right click with people selected "Right_Click";
/// the inventory, the map and the stats window opening and closing; the game paused, resumed
/// and sped up.
#[allow(clippy::too_many_arguments)]
pub fn interface(
    mut s: ResMut<Sound>,
    game: Option<Res<Game>>,
    mut clicks: MessageReader<kenshi_gui::GuiClick>,
    buttons: Res<ButtonInput<MouseButton>>,
    windows: (Option<Res<super::inventory_ui::InventoryWindow>>, Option<Res<super::stats_ui::StatsWindow>>, Option<Res<super::research_ui::ResearchWindow>>),
    mut was: Local<(bool, bool, bool, bool, f32)>,
) {
    if clicks.read().count() > 0 {
        s.post("General_Click", Channel::Ui);
    }
    // (the menu: no game yet)
    let (Some(game), (Some(inv), Some(stats), Some(overview))) = (game, windows) else { return };
    if buttons.just_pressed(MouseButton::Right) && !game.selected.is_empty() {
        s.post("Right_Click", Channel::Ui);
    }
    let (i, st, m) = (inv.is_open(), stats.is_open(), overview.tab() == Some(2));
    if i != was.0 {
        s.post(if i { "Inventory_Open" } else { "Inventory_Close" }, Channel::Ui);
    }
    if st != was.1 {
        s.post(if st { "Stats_Open" } else { "Stats_Close" }, Channel::Ui);
    }
    if m != was.2 {
        s.post(if m { "Map_Open" } else { "Map_Close" }, Channel::Ui);
    }
    if game.paused != was.3 {
        s.post(if game.paused { "Pause_Game" } else { "Resume_Game" }, Channel::Ui);
    }
    if game.speed != was.4 && !game.paused {
        let e = if game.speed >= 4.0 { "Game_Speed_x4" } else if game.speed >= 2.0 { "Game_Speed_x2" } else { "Game_Speed_Normal" };
        s.post(e, Channel::Ui);
    }
    *was = (i, st, m, game.paused, game.speed);
}

/// The weather: rain begun and ended (the region rain over 0.1), the wind always
/// ("Weather_Wind_Start" once).
pub fn weather(mut s: ResMut<Sound>, weather: Option<Res<super::WeatherState>>, mut was: Local<(bool, bool)>, _ready: Res<WorldReady>) {
    let Some(w) = weather else { return };
    if !was.1 {
        was.1 = true;
        s.post("Weather_Wind_Start", Channel::Ambient);
    }
    let rain = w.outputs.rain > 0.1;
    if rain != was.0 {
        s.post(if rain { "Weather_Rain_Start" } else { "Weather_Rain_End" }, Channel::Ambient);
        was.0 = rain;
    }
}

/// Items picked up and put down by the hand ("Item_Lift", "Item_Drop"; Inventory_Items: an
/// ITEM's "inventory sound" (enum InventorySound), a weapon's "material" (Metal, Wood), armour
/// by its material).
/// (assumption) Armour without an "inventory sound" sounds as its material: cloth "Fabric",
/// leather "Leather", chain "Steel_Bar", plate "Armor_Plating"; anything else "Tools".
pub fn items(mut s: ResMut<Sound>, mut inv: ResMut<super::inventory_ui::InventoryWindow>, game: Res<Game>) {
    for (lift, record) in std::mem::take(&mut inv.cues) {
        let Some(r) = game.sim.data.get(&record) else { continue };
        s.post_with(if lift { "Item_Lift" } else { "Item_Drop" }, None, &[("Inventory_Items", item_sound(r))], Channel::Ui);
    }
}

/// An item's Inventory_Items switch: an ITEM's "inventory sound" (enum InventorySound), a
/// weapon's "material" (Metal, Wood), armour by its material (see `items`).
fn item_sound(r: &kenshi_data::GameData) -> &'static str {
    const INVENTORY: [&str; 20] = [
        "Backpack", "Building_Material", "Fabric", "Flour", "Food", "Kits", "Leather", "Luxury", "Narcotics", "Ore", "Potato",
        "Robotic_Component", "Rum", "Steel_Bar", "Tools", "Water", "Wheat", "Armor_Plating", "BLUEPRINTS", "Meat",
    ];
    let int = |k: &str| r.ints.get(k).copied();
    match r.item_type() {
        Some(kenshi_data::ItemType::Weapon) => if int("material") == Some(1) { "Weapon_Wood" } else { "Weapon_Metal" },
        _ => match int("inventory sound") {
            Some(k) => INVENTORY.get(k as usize).copied().unwrap_or("Tools"),
            None => match int("material type") {
                Some(0) => "Fabric",
                Some(1) => "Leather",
                Some(2) => "Steel_Bar",
                Some(3) => "Armor_Plating",
                _ => "Tools",
            },
        },
    }
}

/// A loose item picked up by Task_Pickup (F031289 -> F039184 / F039186): the picker's
/// "Character_Pick_Up_Ground" with the item's Inventory_Items.
pub fn ground_pickups(mut s: ResMut<Sound>, mut game: ResMut<Game>, _ready: Res<WorldReady>) {
    if game.sim.ground_pickups.is_empty() {
        return;
    }
    for (who, record) in std::mem::take(&mut game.sim.ground_pickups) {
        let (Some(p), Some(r)) = (game.sim.folk.get(who), game.sim.data.get(&record)) else { continue };
        let at = Some(Vec3::from(p.mover.position_units()) - game.zone.origin);
        s.post_with("Character_Pick_Up_Ground", at, &[("Inventory_Items", item_sound(r))], Channel::Sfx);
    }
}

/// Inside a building (a selected character in it, the building opened): "Enter_BUILDING" with
/// Indoors its "interior ambience" (enum InteriorSound: none, Metal, Stone, Wicker, Wood),
/// "Leave_BUILDING" on going out (F028313). A building of interior ambience none plays none.
pub fn indoors(mut s: ResMut<Sound>, open: Res<super::OpenBuildings>, views: Query<&kenshi_render::zone::BuildingView>, game: Res<Game>, mut inside: Local<Option<String>>) {
    let now = open.open.iter().next().and_then(|key| views.iter().find(|v| (v.zone, v.building) == *key)).and_then(|v| {
        let kind = game.sim.data.get(&v.record)?.ints.get("interior ambience").copied()?;
        ["", "METAL", "stone", "Wicker", "wood"].get(kind as usize).filter(|k| !k.is_empty()).map(|k| k.to_string())
    });
    if now != *inside {
        if inside.is_some() {
            s.post("Leave_BUILDING", Channel::Ambient);
        }
        if let Some(k) = &now {
            s.post_with("Enter_BUILDING", None, &[("Indoors", k)], Channel::Ambient);
        }
        *inside = now;
    }
}

/// The buildings sounding near the camera (BUILDING "sounds" -> AMBIENT_SOUND: its "emitter"
/// played by "Start_Emitter" with Emitters, at the building): started within its "cutoff" x 10
/// units of the camera's focus, stopped past it.
/// The emitter (F028321) keeps "cutoff" squared x 100 (units squared: the cutoff is in metres),
/// "intensity" and "efficiency multiplier" x 100; the intensity goes to the emitter's RTPC
/// (F028308), which is not read here (no RTPCs are). (assumption) A building needing power
/// sounds anyway (the power grid of others' towns is not kept); "chance" is not read.
pub fn emitters(mut s: ResMut<Sound>, game: Res<Game>, time: Res<Time>, mut on: Local<HashMap<u64, ()>>, mut every: Local<f32>, _ready: Res<WorldReady>) {
    *every -= time.delta_secs();
    if *every > 0.0 {
        return;
    }
    *every = 1.0;
    // a game loaded in the program: its emitters start afresh
    if game.is_added() {
        on.clear();
    }
    let focus = game.focus + game.zone.origin;
    let mut want = HashMap::new();
    for (k, p) in game.sim.placed.iter().enumerate() {
        let Some(b) = game.sim.data.get(&p.building) else { continue };
        let Some(snd) = b.refs("sounds").first().and_then(|(sid, _)| game.sim.data.get(sid)) else { continue };
        let Some(emitter) = snd.strings.get("emitter").filter(|e| !e.is_empty()) else { continue };
        let cutoff = snd.floats.get("cutoff").copied().unwrap_or(50.0) * 10.0;
        let at = Vec3::from(game.sim.placed_at[k]);
        if at.distance(focus) > cutoff {
            continue;
        }
        let tag = 0x1000_0000_0000 + k as u64;
        want.insert(tag, ());
        if !on.contains_key(&tag) {
            s.post_tagged("Start_Emitter", Some(at - game.zone.origin), &[("Emitters", emitter)], Channel::Ambient, tag);
        }
    }
    for tag in on.keys().copied().collect::<Vec<_>>() {
        if !want.contains_key(&tag) {
            s.stop_tag(tag);
        }
    }
    *on = want;
}

/// The squad at work and carrying: first aid "Character_Medic_Start" / "_Stop", building
/// "Character_Build_Loop_Start" / "_Stop" (as their work begins and ends, staying put), lifting
/// someone "Character_Pick_Up_Person", putting them down "Character_Drop_Person"; a site begun
/// "Building_Place" (Outdoors), a building finished the notification "Building_Complete".
/// (assumption) Kenshi starts these from its tasks (not decompiled): here from the state.
pub fn work(mut s: ResMut<Sound>, game: Res<Game>, mut was: Local<HashMap<usize, (bool, bool, bool)>>, mut sites: Local<(usize, usize)>, _ready: Res<WorldReady>) {
    if game.is_added() {
        was.clear();
        *sites = (0, 0);
    }
    for p in game.sim.folk.iter().filter(|p| p.player) {
        let at = Some(Vec3::from(p.mover.position_units()) - game.zone.origin);
        let still = p.speed < 2.0 && !p.down;
        let now = (p.first_aid.is_some() && still, p.build.is_some() && still, p.carrying.is_some());
        let before = was.get(&p.id).copied().unwrap_or((false, false, false));
        if now.0 != before.0 {
            s.post_with(if now.0 { "Character_Medic_Start" } else { "Character_Medic_Stop" }, at, &[], Channel::Sfx);
        }
        if now.1 != before.1 {
            s.post_with(if now.1 { "Character_Build_Loop_Start" } else { "Character_Build_Loop_Stop" }, at, &[], Channel::Sfx);
        }
        if now.2 != before.2 {
            s.post_with(if now.2 { "Character_Pick_Up_Person" } else { "Character_Drop_Person" }, at, &[], Channel::Sfx);
        }
        was.insert(p.id, now);
    }
    let count = game.sim.buildings.len();
    let done = game.sim.buildings.iter().filter(|b| b.done).count();
    if count > sites.0 && sites.0 > 0 {
        s.post_with("Building_Place", None, &[("Building_Place", "Outdoors")], Channel::Ui);
    }
    if done > sites.1 && sites.0 > 0 {
        s.post_with("Notifications", None, &[("Notifications", "Building_Complete")], Channel::Ui);
    }
    *sites = (count.max(1), done);
}

/// A building of the player's dismantled ("Thing_Complete", F029409).
pub fn dismantled(mut s: ResMut<Sound>, game: Res<Game>, mut was: Local<u32>, _ready: Res<WorldReady>) {
    if game.sim.dismantled > *was {
        s.post_with("Thing_Complete", None, &[], Channel::Ui);
    }
    *was = game.sim.dismantled;
}

/// Doors as they move (F029271, F029334, F029337): "Door_Open" as one starts opening,
/// "Door_Close" as it starts closing, "Door_Shut" as it comes to a stop either way; "Break" as
/// one is bashed in (F029386).
pub fn doors(mut s: ResMut<Sound>, game: Res<Game>, mut was: Local<Vec<(i32, bool)>>, _ready: Res<WorldReady>) {
    use kenshi_game::sim::{DOOR_CLOSED, DOOR_CLOSING, DOOR_OPEN, DOOR_OPENING};
    let doors = &game.sim.door_states;
    if was.len() != doors.len() || game.is_added() {
        *was = doors.iter().map(|d| (d.phase, d.broken)).collect();
        return;
    }
    for (d, (before, broken)) in doors.iter().zip(was.iter_mut()) {
        if d.broken && !*broken {
            s.post_with("Break", Some(Vec3::from(d.door.position) - game.zone.origin), &[], Channel::Sfx);
        }
        *broken = d.broken;
        if d.phase == *before {
            continue;
        }
        let event = match (*before, d.phase) {
            (_, DOOR_OPENING) => Some("Door_Open"),
            (_, DOOR_CLOSING) => Some("Door_Close"),
            (DOOR_OPENING, DOOR_OPEN) | (DOOR_CLOSING, DOOR_CLOSED) => Some("Door_Shut"),
            _ => None,
        };
        *before = d.phase;
        if let Some(e) = event {
            let at = Some(Vec3::from(d.door.position) - game.zone.origin);
            s.post_with(e, at, &[], Channel::Sfx);
        }
    }
}

/// @fn F039241
///
/// CharacterAnimal's update (vtable slot 29): each animal, once its timer is past (the real-time
/// clock at 0x1421349b0, F028343: microseconds since it started, running while paused too),
/// unless it is down, calls out "VO_Creature_Passive" (its Creature_VO) when not fighting
/// (F02f220) and within 1000 of the camera's target (F039186: squared 1e6), and its next call
/// comes 20 to 60 s later (20 + the unit random x 40).
/// (assumption) The timer starts at 0: its first call comes at once.
pub fn creatures(mut s: ResMut<Sound>, game: Res<Game>, time: Res<Time>, mut next: Local<HashMap<usize, f32>>, _ready: Res<WorldReady>) {
    if game.is_added() {
        next.clear();
    }
    let now = time.elapsed_secs();
    for p in &game.sim.folk {
        if p.dead || p.down {
            continue;
        }
        let (_, Some(c)) = race_sound(&game, p.id) else { continue };
        if now <= next.get(&p.id).copied().unwrap_or(0.0) {
            continue;
        }
        if !game.sim.arena.f.get(p.id).is_some_and(|f| f.active) {
            let at = place(&game, p.id);
            if at.distance_squared(game.focus) < 1.0e6 {
                s.post_with("VO_Creature_Passive", Some(at), &[("Creature_VO", c)], Channel::Vo);
            }
        }
        let u = s.rand();
        next.insert(p.id, now + u * 40.0 + 20.0);
    }
}

/// @fn F029299
///
/// @fn F037659
///
/// A door's lock turned (the building panel's "Security:" button): "Building_Lock_Unlock" with
/// its "Build_Lock" switch set first by the lock wanted before the turn, "Lock_Building" while
/// it was wanted, else "Unlock_Building".
pub fn door_locks(mut s: ResMut<Sound>, game: Res<Game>, mut was: Local<Vec<bool>>, _ready: Res<WorldReady>) {
    let now: Vec<bool> = game.sim.door_states.iter().map(|d| d.wants_lock).collect();
    if was.len() == now.len() {
        for (before, after) in was.iter().zip(&now) {
            if before != after {
                s.post_with("Building_Lock_Unlock", None, &[("Build_Lock", if *before { "Lock_Building" } else { "Unlock_Building" })], Channel::Ui);
            }
        }
    }
    *was = now;
}

/// A machine's power switched on or off (the building panel's "Power" tick): "UI_Generator"
/// with its "Generator" switch "On" or "Off".
pub fn machines(mut s: ResMut<Sound>, game: Res<Game>, mut was: Local<Vec<(String, [f32; 3], bool)>>, _ready: Res<WorldReady>) {
    let now: Vec<(String, [f32; 3], bool)> = game.sim.buildings.iter().map(|b| (b.record.clone(), b.at, b.switched_on)).collect();
    for (r, at, on) in &now {
        if let Some((_, _, before)) = was.iter().find(|(r2, a2, _)| r2 == r && a2 == at)
            && before != on
        {
            s.post_with("UI_Generator", None, &[("Generator", if *on { "On" } else { "Off" })], Channel::Ui);
        }
    }
    *was = now;
}
