//! Kenshi's HDR chain (port_map `render`): the scene is drawn in raw HDR, its luminance measured
//! and adapted over time, and the scene composited at EXPOSURE_KEY over the adapted luminance.
//! See kenshi_hdr.wgsl.
//!
//! Spec: data/materials/post/hdr.material (HDRSettings: EXPOSURE_KEY 0.55, AUTOEXP_ADAPTATION_RATE
//! 0.5, BLOOM_MAGNITUDE 0), compositors/main.compositor (rt_lum4 128 x 128 by
//! HDR/Downsample2x2Luminance, rt_lum3 64, rt_lum2 16, rt_lum1 4, rt_lum0 1 by HDR/Downsample3x3,
//! HDR/AdaptLuminance into the adapted luminance, the composite), F040805 (MIN_LUMINANCE each
//! frame), F040804 (MAX_LUMINANCE = "exposure max"); then the post effects on the 8-bit composite
//! (post.compositor's PostProcessing PF_A8R8G8B8 buffers) in compositors.cfg's order, F045904
//! chaining them ping-pong and the last to the window: FXAA (post/FXAA.hlsl, the FXAA option; see
//! kenshi_fxaa.wgsl), then HeatHaze (post/heathaze.hlsl, the HeatHaze option) with the weather's
//! heatHaze (F051194; see kenshi_haze.wgsl).

use bevy::core_pipeline::{Core3d, Core3dSystems, FullscreenShader};
use bevy::prelude::*;
use bevy::render::extract_resource::{ExtractResource, ExtractResourcePlugin};
use bevy::render::render_resource::binding_types::{
    sampler, texture_2d, uniform_buffer,
};
use bevy::render::render_resource::{
    BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, BufferInitDescriptor,
    BufferUsages, CachedRenderPipelineId, ColorTargetState, ColorWrites, Extent3d, FragmentState,
    LoadOp, Operations, PipelineCache, RenderPassColorAttachment, RenderPassDescriptor,
    RenderPipelineDescriptor, Sampler, SamplerBindingType, SamplerDescriptor, ShaderStages,
    ShaderType, StoreOp, TextureDescriptor, TextureDimension, TextureFormat, TextureSampleType,
    TextureUsages, TextureView,
};
use bevy::render::renderer::{RenderContext, RenderDevice, ViewQuery};
use bevy::render::view::ViewTarget;
use bevy::render::{RenderApp, RenderStartup};
use bevy::shader::Shader;

/// The HDR settings Kenshi feeds the chain (HDRSettings, F040805).
#[derive(Resource, ExtractResource, Clone, Debug)]
pub struct KenshiHdr {
    pub exposure_key: f32,
    pub adaptation_rate: f32,
    pub min_luminance: f32,
    pub max_luminance: f32,
    /// The last frame's time (s).
    pub frame_time: f32,
    /// The weather's heatHaze, gameTime (hours) and the camera's clip distances; the option.
    pub heat_haze: f32,
    pub game_time: f32,
    pub near: f32,
    pub far: f32,
    /// tan of half the field of view across and up.
    pub tan_half_fov: Vec2,
    pub haze_enabled: bool,
    /// The FXAA option.
    pub fxaa: bool,
    pub flow: Handle<Image>,
    pub perturber: Handle<Image>,
}

impl Default for KenshiHdr {
    fn default() -> Self {
        KenshiHdr {
            exposure_key: 0.55,
            adaptation_rate: 0.5,
            min_luminance: 0.8,
            max_luminance: 0.96,
            frame_time: 0.0,
            heat_haze: 0.0,
            game_time: 0.0,
            near: 0.1,
            far: 60000.0,
            tan_half_fov: Vec2::ONE,
            haze_enabled: false,
            fxaa: false,
            flow: Handle::default(),
            perturber: Handle::default(),
        }
    }
}

#[derive(ShaderType, Clone, Copy, Default)]
struct HdrParams {
    a: Vec4,
    b: Vec4,
}

#[derive(ShaderType, Clone, Copy, Default)]
struct HazeParams {
    a: Vec4,
    b: Vec4,
}

#[derive(Resource)]
struct HdrPipelines {
    layout: BindGroupLayoutDescriptor,
    haze_layout: BindGroupLayoutDescriptor,
    haze: CachedRenderPipelineId,
    fxaa_layout: BindGroupLayoutDescriptor,
    fxaa: CachedRenderPipelineId,
    present: CachedRenderPipelineId,
    wrap: Sampler,
    sampler: Sampler,
    down2: CachedRenderPipelineId,
    down3: CachedRenderPipelineId,
    adapt: CachedRenderPipelineId,
    composite: CachedRenderPipelineId,
}

/// The luminance targets: 128, 64, 16, 4, 1, and the adapted luminance's two 1 x 1 targets.
#[derive(Resource)]
struct HdrTargets {
    lum: Vec<TextureView>,
    adapted: [TextureView; 2],
    current: usize,
    /// The post effects' two 8-bit buffers, at the view's size.
    ldr: Option<(UVec2, [TextureView; 2])>,
}

const LUM_SIZES: [u32; 5] = [128, 64, 16, 4, 1];

/// The post effects' buffers (PF_A8R8G8B8, read as they are).
const LDR: TextureFormat = TextureFormat::Rgba8Unorm;

fn init(
    mut commands: Commands,
    device: Res<RenderDevice>,
    assets: Res<AssetServer>,
    fullscreen: Res<FullscreenShader>,
    pipeline_cache: Res<PipelineCache>,
) {
    let layout = BindGroupLayoutDescriptor::new(
        "kenshi_hdr_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                texture_2d(TextureSampleType::Float { filterable: false }),
                uniform_buffer::<HdrParams>(false),
            ),
        ),
    );
    let haze_layout = BindGroupLayoutDescriptor::new(
        "kenshi_haze_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                texture_2d(TextureSampleType::Float { filterable: true }),
                texture_2d(TextureSampleType::Float { filterable: true }),
                bevy::render::render_resource::binding_types::texture_depth_2d(),
                uniform_buffer::<HazeParams>(false),
                sampler(SamplerBindingType::Filtering),
            ),
        ),
    );
    let fxaa_layout = BindGroupLayoutDescriptor::new(
        "kenshi_fxaa_layout",
        &BindGroupLayoutEntries::single(
            ShaderStages::FRAGMENT,
            texture_2d(TextureSampleType::Float { filterable: false }),
        ),
    );
    let fxaa_shader: Handle<Shader> =
        assets.load("embedded://kenshi_render/render/kenshi_fxaa.wgsl");
    let fxaa = pipeline_cache.queue_render_pipeline(RenderPipelineDescriptor {
        label: Some("kenshi_fxaa".into()),
        layout: vec![fxaa_layout.clone()],
        vertex: fullscreen.to_vertex_state(),
        fragment: Some(FragmentState {
            shader: fxaa_shader,
            entry_point: Some("fxaa".into()),
            targets: vec![Some(ColorTargetState {
                format: LDR,
                blend: None,
                write_mask: ColorWrites::ALL,
            })],
            ..default()
        }),
        ..default()
    });
    let haze_shader: Handle<Shader> =
        assets.load("embedded://kenshi_render/render/kenshi_haze.wgsl");
    let haze = pipeline_cache.queue_render_pipeline(RenderPipelineDescriptor {
        label: Some("kenshi_haze".into()),
        layout: vec![haze_layout.clone()],
        vertex: fullscreen.to_vertex_state(),
        fragment: Some(FragmentState {
            shader: haze_shader,
            entry_point: Some("haze".into()),
            targets: vec![Some(ColorTargetState {
                format: LDR,
                blend: None,
                write_mask: ColorWrites::ALL,
            })],
            ..default()
        }),
        ..default()
    });
    let shader: Handle<Shader> = assets.load("embedded://kenshi_render/render/kenshi_hdr.wgsl");
    let pipeline = |entry: &'static str, format: TextureFormat| {
        pipeline_cache.queue_render_pipeline(RenderPipelineDescriptor {
            label: Some(entry.into()),
            layout: vec![layout.clone()],
            vertex: fullscreen.to_vertex_state(),
            fragment: Some(FragmentState {
                shader: shader.clone(),
                entry_point: Some(entry.into()),
                targets: vec![Some(ColorTargetState {
                    format,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
                ..default()
            }),
            ..default()
        })
    };
    let pipes = HdrPipelines {
        down2: pipeline("downsample_2x2_luminance", TextureFormat::R16Float),
        down3: pipeline("downsample_3x3", TextureFormat::R16Float),
        adapt: pipeline("adapt_luminance", TextureFormat::R32Float),
        composite: pipeline("composite", LDR),
        present: pipeline("present", TextureFormat::Rgba16Float),
        layout,
        fxaa_layout,
        fxaa,
        haze_layout,
        haze,
        // the material's default texture units: wrapping, Kenshi's default filtering
        // (anisotropic x16, F045916)
        wrap: device.create_sampler(&SamplerDescriptor {
            address_mode_u: bevy::render::render_resource::AddressMode::Repeat,
            address_mode_v: bevy::render::render_resource::AddressMode::Repeat,
            mag_filter: bevy::render::render_resource::FilterMode::Linear,
            min_filter: bevy::render::render_resource::FilterMode::Linear,
            mipmap_filter: bevy::render::render_resource::MipmapFilterMode::Linear,
            anisotropy_clamp: 16,
            ..default()
        }),
        sampler: device.create_sampler(&SamplerDescriptor {
            mag_filter: bevy::render::render_resource::FilterMode::Linear,
            min_filter: bevy::render::render_resource::FilterMode::Linear,
            ..default()
        }),
    };
    let target = |size: u32, format: TextureFormat| {
        device
            .create_texture(&TextureDescriptor {
                label: Some("kenshi_hdr_lum"),
                size: Extent3d {
                    width: size,
                    height: size,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format,
                usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&default())
    };
    commands.insert_resource(HdrTargets {
        lum: LUM_SIZES
            .iter()
            .map(|&s| target(s, TextureFormat::R16Float))
            .collect(),
        adapted: [target(1, TextureFormat::R32Float), target(1, TextureFormat::R32Float)],
        current: 0,
        ldr: None,
    });
    commands.insert_resource(pipes);
}

#[allow(clippy::too_many_arguments)]
fn kenshi_hdr(
    view: ViewQuery<(
        &ViewTarget,
        Option<&bevy::core_pipeline::prepass::ViewPrepassTextures>,
    )>,
    images: Res<bevy::render::render_asset::RenderAssets<bevy::render::texture::GpuImage>>,
    pipeline_cache: Res<PipelineCache>,
    pipes: Option<Res<HdrPipelines>>,
    targets: Option<ResMut<HdrTargets>>,
    settings: Option<Res<KenshiHdr>>,
    mut ctx: RenderContext,
) {
    let (Some(pipes), Some(mut targets), Some(settings)) = (pipes, targets, settings) else {
        return;
    };
    let (target, prepass) = view.into_inner();
    // the main camera's (the water reflection's target is not exposed)
    if target.main_texture_format() != bevy::render::render_resource::TextureFormat::Rgba16Float {
        return;
    }
    let (Some(down2), Some(down3), Some(adapt), Some(composite)) = (
        pipeline_cache.get_render_pipeline(pipes.down2),
        pipeline_cache.get_render_pipeline(pipes.down3),
        pipeline_cache.get_render_pipeline(pipes.adapt),
        pipeline_cache.get_render_pipeline(pipes.composite),
    ) else {
        return;
    };
    let device = ctx.render_device().clone();
    let params = HdrParams {
        a: Vec4::new(
            settings.frame_time,
            settings.min_luminance,
            settings.max_luminance,
            settings.adaptation_rate,
        ),
        b: Vec4::new(settings.exposure_key, 0.0, 0.0, 0.0),
    };
    let mut bytes = bevy::render::render_resource::encase::UniformBuffer::new(Vec::new());
    bytes.write(&params).expect("hdr params");
    let buffer = device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("kenshi_hdr_params"),
        contents: bytes.as_ref(),
        usage: BufferUsages::UNIFORM,
    });
    let layout = pipeline_cache.get_bind_group_layout(&pipes.layout);
    let (last, next) = (targets.current, 1 - targets.current);
    let bind = |input: &TextureView, lum: &TextureView| {
        device.create_bind_group(
            "kenshi_hdr_bind_group",
            &layout,
            &BindGroupEntries::sequential((input, &pipes.sampler, lum, buffer.as_entire_binding())),
        )
    };
    let post = target.post_process_write();
    let mut pass = |pipeline: &bevy::render::render_resource::RenderPipeline,
                    out: &TextureView,
                    group: &bevy::render::render_resource::BindGroup| {
        let mut rp = ctx.command_encoder().begin_render_pass(&RenderPassDescriptor {
            label: Some("kenshi_hdr"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: out,
                depth_slice: None,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Clear(Default::default()),
                    store: StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        rp.set_pipeline(pipeline);
        rp.set_bind_group(0, group, &[]);
        rp.draw(0..3, 0..1);
    };
    // the scene's luminance, downsampled to one pixel
    let last_adapted = &targets.adapted[last];
    pass(down2, &targets.lum[0], &bind(post.source, last_adapted));
    for k in 1..LUM_SIZES.len() {
        pass(down3, &targets.lum[k], &bind(&targets.lum[k - 1], last_adapted));
    }
    // adapt toward it
    pass(adapt, &targets.adapted[next], &bind(&targets.lum[4], last_adapted));
    // and expose the scene by it, into the first 8-bit buffer
    let size = target.main_texture().size();
    let size = UVec2::new(size.width, size.height);
    if targets.ldr.as_ref().is_none_or(|(s, _)| *s != size) {
        let make = || {
            device
                .create_texture(&TextureDescriptor {
                    label: Some("kenshi_post_buffer"),
                    size: Extent3d {
                        width: size.x,
                        height: size.y,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: TextureDimension::D2,
                    format: LDR,
                    usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&default())
        };
        targets.ldr = Some((size, [make(), make()]));
    }
    let Some((_, ldr)) = targets.ldr.as_ref() else {
        return;
    };
    pass(composite, &ldr[0], &bind(post.source, &targets.adapted[next]));
    let mut cur = 0;
    // FXAA
    if settings.fxaa
        && let Some(fxaa) = pipeline_cache.get_render_pipeline(pipes.fxaa)
    {
        let group = device.create_bind_group(
            "kenshi_fxaa_bind_group",
            &pipeline_cache.get_bind_group_layout(&pipes.fxaa_layout),
            &BindGroupEntries::single(&ldr[cur]),
        );
        pass(fxaa, &ldr[1 - cur], &group);
        cur = 1 - cur;
    }
    // the heat haze
    let depth = prepass.and_then(|p| p.depth_view());
    let flow = images.get(&settings.flow);
    let perturber = images.get(&settings.perturber);
    // (testing) KENSHI_HAZE_DEBUG: what the haze pass has
    if std::env::var("KENSHI_HAZE_DEBUG").is_ok() {
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        if N.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % 120 == 0 {
            eprintln!(
                "haze: enabled {} amount {:.3} pipeline {} depth {} flow {} perturber {}",
                settings.haze_enabled,
                settings.heat_haze,
                pipeline_cache.get_render_pipeline(pipes.haze).is_some(),
                depth.is_some(),
                flow.is_some(),
                perturber.is_some()
            );
        }
    }
    if settings.haze_enabled
        && settings.heat_haze > 0.0
        && let (Some(haze), Some(depth), Some(flow), Some(perturber)) = (
            pipeline_cache.get_render_pipeline(pipes.haze),
            depth,
            flow,
            perturber,
        )
    {
        let hp = HazeParams {
            a: Vec4::new(settings.game_time, settings.heat_haze, settings.near, settings.far),
            b: settings.tan_half_fov.extend(0.0).extend(0.0),
        };
        let mut bytes = bevy::render::render_resource::encase::UniformBuffer::new(Vec::new());
        bytes.write(&hp).expect("haze params");
        let hbuf = device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("kenshi_haze_params"),
            contents: bytes.as_ref(),
            usage: BufferUsages::UNIFORM,
        });
        let group = device.create_bind_group(
            "kenshi_haze_bind_group",
            &pipeline_cache.get_bind_group_layout(&pipes.haze_layout),
            &BindGroupEntries::sequential((
                &ldr[cur],
                &pipes.sampler,
                &flow.texture_view,
                &perturber.texture_view,
                depth,
                hbuf.as_entire_binding(),
                &pipes.wrap,
            )),
        );
        pass(haze, &ldr[1 - cur], &group);
        cur = 1 - cur;
    }
    // to the window
    if let Some(present) = pipeline_cache.get_render_pipeline(pipes.present) {
        pass(present, post.destination, &bind(&ldr[cur], &targets.adapted[next]));
    }
    targets.current = next;
}

fn frame_time(time: Res<Time>, mut hdr: ResMut<KenshiHdr>) {
    hdr.frame_time = time.delta_secs();
}

/// Kenshi's HDR chain on cameras with `Hdr`.
pub struct KenshiHdrPlugin;

impl Plugin for KenshiHdrPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "kenshi_hdr.wgsl");
        bevy::asset::embedded_asset!(app, "kenshi_haze.wgsl");
        bevy::asset::embedded_asset!(app, "kenshi_fxaa.wgsl");
        app.init_resource::<KenshiHdr>()
            .add_plugins(ExtractResourcePlugin::<KenshiHdr>::default())
            .add_systems(First, frame_time);
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app.add_systems(RenderStartup, init).add_systems(
            Core3d,
            kenshi_hdr
                .in_set(Core3dSystems::PostProcess)
                .before(bevy::core_pipeline::tonemapping::tonemapping),
        );
    }
}
