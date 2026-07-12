use std::sync::{Arc, Mutex, MutexGuard};

use wgpu::{Backends, PresentMode};

use redixel::prelude::*;
use redixel_platform::window::WindowConfig;
use redixel_renderer::RendererConfig;

const WARMUP_FRAMES: u32 = 60;
const MEASURE_FRAMES: u32 = 300;

const TIERS: [(usize, &str); 3] = [
    (500, "Render FPS (500 quads)"),
    (2000, "Render FPS (2000 quads)"),
    (8000, "Render FPS (8000 quads)"),
];

struct TierResult {
    name: &'static str,
    avg_fps: f64,
}

struct FpsBenchmark {
    tier_index: usize,
    frame: u32,
    samples: Vec<f64>,
    results: Arc<Mutex<Vec<TierResult>>>,
}

impl FpsBenchmark {
    fn quad_count(&self) -> usize {
        TIERS[self.tier_index].0
    }
}

impl Game for FpsBenchmark {
    type Action = ();

    fn on_start(&mut self, _ctx: &mut dyn GameContext<Self::Action>) {}

    fn on_update(&mut self, ctx: &mut dyn GameContext<Self::Action>) {
        self.frame += 1;

        if self.frame > WARMUP_FRAMES {
            self.samples.push(ctx.delta_time());
        }

        if self.frame >= WARMUP_FRAMES + MEASURE_FRAMES {
            let (_, name) = TIERS[self.tier_index];
            let total_delta: f64 = self.samples.iter().sum();
            let avg_fps: f64 = self.samples.len() as f64 / total_delta;
            self.results.lock().unwrap().push(TierResult { name, avg_fps });

            self.tier_index += 1;
            self.frame = 0;
            self.samples.clear();

            if self.tier_index >= TIERS.len() {
                ctx.exit();
            }
        }
    }

    fn on_render(&mut self, ctx: &mut dyn GameContext<Self::Action>) {
        ctx.clear_color(Color::rgb(0.05, 0.05, 0.08));

        const COLS: usize = 80;
        const CELL: f32 = 16.0;

        for i in 0..self.quad_count() {
            let col: f32 = (i % COLS) as f32;
            let row: f32 = (i / COLS) as f32;
            let position: Vec2 = Vec2::new(col * CELL, row * CELL);
            ctx.draw_rect(position, Vec2::new(CELL - 2.0, CELL - 2.0), Color::rgb(0.9, 0.4, 0.2));
        }
    }
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let results: Arc<Mutex<Vec<TierResult>>> = Arc::new(Mutex::new(Vec::with_capacity(TIERS.len())));
    let game: FpsBenchmark = FpsBenchmark {
        tier_index: 0,
        frame: 0,
        samples: Vec::with_capacity(MEASURE_FRAMES as usize),
        results: results.clone(),
    };

    let config: RuntimeConfig = RuntimeConfig::windowed(
        WindowConfig {
            title: String::from("redixel-fps-bench"),
            width: 1280,
            height: 720,
            fullscreen: false,
        },
        RendererConfig {
            backends: Backends::all(),
            present_mode: PresentMode::Immediate,
        },
        0.0,
        60.0,
    );

    redixel::run_desktop_with(game, config).expect("fps-bench: engine run failed");

    let data: MutexGuard<'_, Vec<TierResult>> = results.lock().unwrap();
    let json_results: Vec<serde_json::Value> = data
        .iter()
        .map(|r: &TierResult| {
            log::info!("{}: {:.2} fps", r.name, r.avg_fps);
            serde_json::json!({
                "name": r.name,
                "unit": "fps",
                "value": (r.avg_fps * 100.0).round() / 100.0,
            })
        })
        .collect();

    let out_path: String = std::env::args()
        .nth(1)
        .unwrap_or_else(|| String::from("benchmark_result.json"));
    let json: String = serde_json::to_string_pretty(&json_results).expect("fps-bench: failed to serialize results");
    std::fs::write(&out_path, json).expect("fps-bench: failed to write output file");
}
