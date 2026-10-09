//! A port of ParticleUniverse (the particle plugin Kenshi ships, Plugin_ParticleUniverse_x64.dll;
//! port_map `render`), from its published source (OGRECave/particleuniverse, MIT): the scripts in
//! data/particles/scripts, their systems simulated as PU does and drawn as Ogre's BillboardSet
//! would with the materials in data/particles/materials.
//!
//! (assumption) Kenshi's DLL behaves as that source (it carries no version; spot checks only).
//! Not ported: what Kenshi's scripts do not use (other emitters, affectors, observers, handlers,
//! renderers; emitted affectors and systems; spatial hashing; camera dependencies; externs).

pub mod affector;
pub mod billboard;
pub mod dynamic;
pub mod effects;
pub mod emitter;
pub mod kenshi;
pub mod library;
pub mod math;
pub mod observer;
pub mod particle;
pub mod render;
pub mod script;
pub mod system;
