//! The particle system templates and particle materials of an install: every `.pu` script in
//! data/particles/scripts (Ogre's script patterns "*.pua" then "*.pu", files in directory order:
//! NTFS's, case-insensitive), a name already taken giving "CopyOf<name>"
//! (createParticleSystemTemplate); the materials of data/particles/materials.

use super::script::{Node, parse};
use super::system::System;
use std::collections::HashMap;
use std::path::Path;

/// scene_blend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Blend {
    /// add: one, one.
    Add,
    /// alpha_blend: src_alpha, one_minus_src_alpha.
    Alpha,
    /// colour_blend: src_colour, one_minus_src_colour.
    Colour,
    /// modulate: dest_colour, zero.
    Modulate,
    /// replace (no blending).
    Replace,
}

/// A particle material's pass as Kenshi writes them: a texture, blending, depth test and write,
/// depth bias, the programs (forward/basic.program, particles.program).
#[derive(Debug, Clone, PartialEq)]
pub struct ParticleMaterial {
    pub name: String,
    pub texture: Option<String>,
    pub blend: Blend,
    pub depth_check: bool,
    pub depth_write: bool,
    /// depth_bias constant (slope scale).
    pub depth_bias: (f32, f32),
    /// Basic_Coloured_Ambient_VP: the colour dimmed by the sun's height (AMBIENT).
    pub ambient: bool,
    /// Particle_Blend_Depth: alpha faded near what is behind (soft particles).
    pub depth_blend: bool,
    /// The texture unit's tex_address_mode clamp.
    pub clamp: bool,
    /// filtering none.
    pub point_filtering: bool,
    /// The colour from the vertices (the vertex programs with COLOURED).
    pub coloured: bool,
}

pub struct ParticleLibrary {
    pub templates: HashMap<String, System>,
    pub materials: HashMap<String, ParticleMaterial>,
}

impl ParticleLibrary {
    pub fn load(install: &Path) -> ParticleLibrary {
        let mut templates = HashMap::new();
        let dir = install.join("data/particles/scripts");
        for pattern in ["pua", "pu"] {
            for path in sorted_files(&dir, pattern) {
                let Ok(src) = std::fs::read(&path) else { continue };
                let src = String::from_utf8_lossy(&src);
                for n in parse(&src) {
                    if n.class != "system" {
                        continue;
                    }
                    let mut s = System::from_node(&n);
                    if templates.contains_key(&s.name) {
                        s.name = format!("CopyOf{}", s.name);
                    }
                    templates.insert(s.name.clone(), s);
                }
            }
        }
        let mut materials = HashMap::new();
        for path in sorted_files(&install.join("data/particles/materials"), "material") {
            let Ok(src) = std::fs::read(&path) else { continue };
            for n in parse(&String::from_utf8_lossy(&src)) {
                if n.class == "material" {
                    let m = material(&n);
                    materials.insert(m.name.clone(), m);
                }
            }
        }
        ParticleLibrary { templates, materials }
    }
}

/// The files of a directory with an extension, in NTFS order (case-insensitive by name).
fn sorted_files(dir: &Path, ext: &str) -> Vec<std::path::PathBuf> {
    let mut v: Vec<_> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case(ext)))
        .collect();
    v.sort_by_key(|p| p.file_name().map(|f| f.to_string_lossy().to_uppercase()));
    v
}

fn material(n: &Node) -> ParticleMaterial {
    let mut m = ParticleMaterial {
        name: n.name.clone(),
        texture: None,
        blend: Blend::Replace,
        depth_check: true,
        depth_write: true,
        depth_bias: (0.0, 0.0),
        ambient: false,
        depth_blend: false,
        clamp: false,
        point_filtering: false,
        coloured: false,
    };
    // the first technique's first pass
    let Some(pass) = n
        .children
        .iter()
        .find(|c| c.class == "technique")
        .and_then(|t| t.children.iter().find(|c| c.class == "pass"))
    else {
        return m;
    };
    for p in &pass.props {
        match p.name.as_str() {
            "scene_blend" => {
                m.blend = match p.string() {
                    Some("add") => Blend::Add,
                    Some("alpha_blend") => Blend::Alpha,
                    Some("colour_blend") => Blend::Colour,
                    Some("modulate") => Blend::Modulate,
                    _ => Blend::Replace,
                }
            }
            "depth_check" => m.depth_check = p.boolean().unwrap_or(true),
            "depth_write" => m.depth_write = p.boolean().unwrap_or(true),
            "depth_bias" => m.depth_bias = (p.f(0).unwrap_or(0.0), p.f(1).unwrap_or(0.0)),
            _ => {}
        }
    }
    for c in &pass.children {
        match c.class.as_str() {
            "vertex_program_ref" => {
                m.ambient = c.name == "Basic_Coloured_Ambient_VP";
                m.coloured = matches!(
                    c.name.as_str(),
                    "Basic_Coloured_Ambient_VP" | "Basic_Coloured_Texture_VP" | "Basic_Colour_VP"
                );
            }
            "fragment_program_ref" => m.depth_blend = c.name == "Particle_Blend_Depth",
            "texture_unit" => {
                // the first unit with a texture (the interior mask unit has none)
                if m.texture.is_some() || c.props.iter().any(|p| p.name == "content_type") {
                    continue;
                }
                if let Some(t) = c.props.iter().find(|p| p.name == "texture") {
                    m.texture = t.string().map(|s| s.to_string());
                }
                for p in &c.props {
                    match p.name.as_str() {
                        "tex_address_mode" => m.clamp = p.string() == Some("clamp"),
                        "filtering" => m.point_filtering = p.string() == Some("none"),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    m
}
