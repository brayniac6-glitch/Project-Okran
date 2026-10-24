//! (testing) KENSHI_SPAN_PROF with the `span-prof` feature (bevy's `trace`): the time spent in
//! each of Bevy's spans (its systems, schedules and render passes), summed in memory by a
//! tracing layer and printed every five seconds as milliseconds per frame (a frame: one run of
//! the Main schedule), with the longest single run of each. No trace file is written, so the
//! frames run close to their untraced speed.
use bevy::log::tracing_subscriber::{self, Layer, layer::Context, registry::LookupSpan};
use bevy::log::tracing::{Subscriber, field::{Field, Visit}, span};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

struct Label(String);
struct Entered(Instant);

#[derive(Default)]
struct Totals {
    since: Option<Instant>,
    frames: u32,
    spent: HashMap<String, (f64, f64, u32)>,
}

static TOTALS: Mutex<Option<Totals>> = Mutex::new(None);

struct NameField(Option<String>);

impl Visit for NameField {
    fn record_str(&mut self, field: &Field, value: &str) {
        if matches!(field.name(), "name" | "system" | "camera") {
            self.0 = Some(value.to_string());
        }
    }
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if matches!(field.name(), "name" | "system" | "camera") && self.0.is_none() {
            self.0 = Some(format!("{value:?}").trim_matches('"').to_string());
        }
    }
}

pub struct SpanTimes;

impl<S: Subscriber + for<'a> LookupSpan<'a>> Layer<S> for SpanTimes {
    fn on_new_span(&self, attrs: &span::Attributes<'_>, id: &span::Id, ctx: Context<'_, S>) {
        let mut v = NameField(None);
        attrs.record(&mut v);
        let base = attrs.metadata().name();
        let label = match v.0 {
            Some(n) => format!("{base} {n}"),
            None => base.to_string(),
        };
        if let Some(s) = ctx.span(id) {
            s.extensions_mut().insert(Label(label));
        }
    }
    fn on_enter(&self, id: &span::Id, ctx: Context<'_, S>) {
        if let Some(s) = ctx.span(id) {
            s.extensions_mut().replace(Entered(Instant::now()));
        }
    }
    fn on_exit(&self, id: &span::Id, ctx: Context<'_, S>) {
        let Some(s) = ctx.span(id) else { return };
        let ext = s.extensions();
        let (Some(l), Some(e)) = (ext.get::<Label>(), ext.get::<Entered>()) else { return };
        let ms = e.0.elapsed().as_secs_f64() * 1000.0;
        let frame = l.0 == "schedule Main";
        // a single run over 8 ms of a system or a pass (not of a schedule holding them)
        let outer = l.0.starts_with("schedule") || l.0.starts_with("sub app") || l.0 == "update" || l.0 == "main app" || l.0.starts_with("main_render") || l.0.starts_with("camera_schedule") || l.0 == "system bevy_render::run_render_schedule" || l.0 == "system bevy_render::renderer::render_system" || l.0 == "system bevy_core_pipeline::schedule::camera_driver" || l.0 == "system bevy_time::fixed::run_fixed_main_schedule";
        if ms > 8.0 && !outer {
            static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
            let t0 = *START.get_or_init(Instant::now);
            eprintln!("slow span {ms:6.1} ms at {:7.2} s: {}", t0.elapsed().as_secs_f64(), l.0);
        }
        let mut g = TOTALS.lock().unwrap();
        let t = g.get_or_insert_with(Totals::default);
        let x = t.spent.entry(l.0.clone()).or_default();
        x.0 += ms;
        x.1 = x.1.max(ms);
        x.2 += 1;
        if frame {
            t.frames += 1;
            let since = *t.since.get_or_insert_with(Instant::now);
            if since.elapsed().as_secs_f64() >= 5.0 {
                let frames = t.frames.max(1) as f64;
                let mut v: Vec<_> = t.spent.iter().collect();
                v.sort_by(|a, b| b.1.0.total_cmp(&a.1.0));
                eprintln!("spans: {} frames, {:.2} ms/frame (wall)", t.frames, since.elapsed().as_secs_f64() * 1000.0 / frames);
                for (k, (sum, max, n)) in v.into_iter().take(200) {
                    eprintln!("  {:7.3} ms/frame  max {:7.2}  x{:5.1}  {k}", sum / frames, max, *n as f64 / frames);
                }
                t.spent.clear();
                t.frames = 0;
                t.since = Some(Instant::now());
            }
        }
    }
}

/// The layer for LogPlugin's custom_layer, when KENSHI_SPAN_PROF is set.
pub fn layer(_app: &mut bevy::app::App) -> Option<bevy::log::BoxedLayer> {
    if std::env::var("KENSHI_SPAN_PROF").is_err() {
        return None;
    }
    let _ = tracing_subscriber::registry::Registry::default;
    Some(Box::new(SpanTimes))
}
