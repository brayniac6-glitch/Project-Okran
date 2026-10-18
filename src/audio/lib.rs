//! Kenshi's sound (port_map `audio`): its Wwise sound banks (data/audio/*.bnk) read, their
//! sounds (Wwise Vorbis) made into Ogg Vorbis to be played.
//!
//! - [`wem`]: a Wwise Vorbis sound (a RIFF "WEM") rebuilt as an Ogg Vorbis stream, after ww2ogg
//!   (BSD, see data/ww2ogg_COPYING), with its packed external codebooks
//!   (data/packed_codebooks_aoTuV_603.bin);
//! - [`bnk`]: a sound bank's sounds and its hierarchy (events, actions, containers);
//! - [`music`]: its interactive music (segments, tracks, switches, playlists).

pub mod bnk;
pub mod music;
pub mod tree;
pub mod wem;
