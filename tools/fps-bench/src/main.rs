use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use wgpu::{Backends, PresentMode};

use redixel::prelude::*;
use redixel_platform::window::WindowConfig;
use redixel_renderer::RendererConfig;

const WARMUP_FRAMES: u32 = 60;
const MEASURE_FRAMES: u32 = 300;

const TIERS: [(usize, &str); 5] = [
    (100, "100 quads"),
    (500, "500 quads"),
    (2000, "2000 quads"),
    (5000, "5000 quads"),
    (8000, "8000 quads"),
];

static ALLOC_COUNT: AtomicU64 = AtomicU64::new(0);
static ALLOC_BYTES: AtomicU64 = AtomicU64::new(0);

struct CountingAllocator;

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
        ALLOC_BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

fn alloc_snapshot() -> (u64, u64) {
    (ALLOC_COUNT.load(Ordering::Relaxed), ALLOC_BYTES.load(Ordering::Relaxed))
}

struct TierResult {
    quad_count: usize,
    scene: &'static str,
    avg_fps: f64,
    avg_frame_ms: f64,
    p95_frame_ms: f64,
    allocations: u64,
    allocated_bytes: u64,
}

struct FpsBenchmark {
    tier_index: usize,
    frame: u32,
    samples: Vec<f64>,
    alloc_start: (u64, u64),
    results: Arc<Mutex<Vec<TierResult>>>,
}

impl FpsBenchmark {
    fn quad_count(&self) -> usize {
        TIERS[self.tier_index].0
    }
}

fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

fn percentile_ms(samples: &[f64], p: f64) -> f64 {
    let mut sorted: Vec<f64> = samples.to_vec();
    sorted.sort_by(|a: &f64, b: &f64| a.partial_cmp(b).unwrap());
    let idx: usize = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted[idx] * 1000.0
}

impl Game for FpsBenchmark {
    type Action = ();

    fn on_start(&mut self, _ctx: &mut dyn GameContext<Self::Action>) {}

    fn on_update(&mut self, ctx: &mut dyn GameContext<Self::Action>) {
        self.frame += 1;

        if self.frame == WARMUP_FRAMES + 1 {
            self.alloc_start = alloc_snapshot();
        }

        if self.frame > WARMUP_FRAMES {
            self.samples.push(ctx.delta_time());
        }

        if self.frame >= WARMUP_FRAMES + MEASURE_FRAMES {
            let (quad_count, scene): (usize, &str) = TIERS[self.tier_index];
            let total_delta: f64 = self.samples.iter().sum();
            let avg_fps: f64 = self.samples.len() as f64 / total_delta;
            let avg_frame_ms: f64 = (total_delta / self.samples.len() as f64) * 1000.0;
            let p95_frame_ms: f64 = percentile_ms(&self.samples, 0.95);
            let (alloc_end_count, alloc_end_bytes): (u64, u64) = alloc_snapshot();

            self.results.lock().unwrap().push(TierResult {
                quad_count,
                scene,
                avg_fps: round2(avg_fps),
                avg_frame_ms: round2(avg_frame_ms),
                p95_frame_ms: round2(p95_frame_ms),
                allocations: alloc_end_count - self.alloc_start.0,
                allocated_bytes: alloc_end_bytes - self.alloc_start.1,
            });

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

fn marginal_ms_per_1000_quads(data: &[TierResult]) -> f64 {
    let first: &TierResult = data.first().expect("fps-bench: no tiers recorded");
    let last: &TierResult = data.last().expect("fps-bench: no tiers recorded");
    let quad_delta: f64 = (last.quad_count - first.quad_count) as f64;
    (last.avg_frame_ms - first.avg_frame_ms) / (quad_delta / 1000.0)
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let results: Arc<Mutex<Vec<TierResult>>> = Arc::new(Mutex::new(Vec::with_capacity(TIERS.len())));
    let game: FpsBenchmark = FpsBenchmark {
        tier_index: 0,
        frame: 0,
        samples: Vec::with_capacity(MEASURE_FRAMES as usize),
        alloc_start: (0, 0),
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
    let marginal_ms: f64 = round2(marginal_ms_per_1000_quads(&data));

    let tiers: Vec<serde_json::Value> = data
        .iter()
        .map(|r: &TierResult| {
            log::info!(
                "{}: {:.2} fps (avg {:.2} ms, p95 {:.2} ms, {} allocs / {} bytes)",
                r.scene,
                r.avg_fps,
                r.avg_frame_ms,
                r.p95_frame_ms,
                r.allocations,
                r.allocated_bytes
            );

            serde_json::json!({
                "scene": r.scene,
                "avg_fps": format!("{:.2}", r.avg_fps),
                "avg_frame_ms": format!("{:.2}", r.avg_frame_ms),
                "p95_frame_ms": format!("{:.2}", r.p95_frame_ms),
                "allocations": r.allocations,
                "allocated_bytes": r.allocated_bytes,
            })
        })
        .collect();

    log::info!("Marginal cost: {marginal_ms:.2} ms per 1000 additional quads");

    let output: serde_json::Value = serde_json::json!({
        "tiers": tiers,
        "marginal_ms_per_1000_quads": format!("{:.2}", marginal_ms),
    });

    let out_path: String = std::env::args()
        .nth(1)
        .unwrap_or_else(|| String::from("benchmark_result.json"));
    let json: String = serde_json::to_string_pretty(&output).expect("fps-bench: failed to serialize results");
    std::fs::write(&out_path, json).expect("fps-bench: failed to write output file");
}
