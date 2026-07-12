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

struct FpsBenchmark {
    quad_count: usize,
    frame: u32,
    samples: Arc<Mutex<Vec<f64>>>,
}

impl Game for FpsBenchmark {
    type Action = ();

    fn on_start(&mut self, _ctx: &mut dyn GameContext<Self::Action>) {}

    fn on_update(&mut self, ctx: &mut dyn GameContext<Self::Action>) {
        self.frame += 1;

        if self.frame > WARMUP_FRAMES {
            self.samples.lock().unwrap().push(ctx.delta_time());
        }

        if self.frame >= WARMUP_FRAMES + MEASURE_FRAMES {
            ctx.exit();
        }
    }

    fn on_render(&mut self, ctx: &mut dyn GameContext<Self::Action>) {
        ctx.clear_color(Color::rgb(0.05, 0.05, 0.08));

        const COLS: usize = 80;
        const CELL: f32 = 16.0;

        for i in 0..self.quad_count {
            let col: f32 = (i % COLS) as f32;
            let row: f32 = (i / COLS) as f32;
            let position: Vec2 = Vec2::new(col * CELL, row * CELL);
            ctx.draw_rect(position, Vec2::new(CELL - 2.0, CELL - 2.0), Color::rgb(0.9, 0.4, 0.2));
        }
    }
}

fn run_tier(quad_count: usize) -> f64 {
    let samples: Arc<Mutex<Vec<f64>>> = Arc::new(Mutex::new(Vec::with_capacity(MEASURE_FRAMES as usize)));

    let game: FpsBenchmark = FpsBenchmark {
        quad_count,
        frame: 0,
        samples: samples.clone(),
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
    let data: MutexGuard<'_, Vec<f64>> = samples.lock().unwrap();
    let total_delta: f64 = data.iter().sum();
    data.len() as f64 / total_delta
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let mut results: Vec<serde_json::Value> = Vec::with_capacity(TIERS.len());

    for (quad_count, name) in TIERS {
        let avg_fps: f64 = run_tier(quad_count);
        log::info!("{name}: {avg_fps:.2} fps");

        results.push(serde_json::json!({
            "name": name,
            "unit": "fps",
            "value": (avg_fps * 100.0).round() / 100.0,
        }));
    }

    let out_path: String = std::env::args()
        .nth(1)
        .unwrap_or_else(|| String::from("benchmark_result.json"));
    let json: String = serde_json::to_string_pretty(&results).expect("fps-bench: failed to serialize results");
    std::fs::write(&out_path, json).expect("fps-bench: failed to write output file");
}
