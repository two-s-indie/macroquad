//! Instrumentation for an isolated copy of the sibling game's scene.
//! Counters cover Rust allocations, not native driver or GPU allocations.
use macroquad::prelude::*;
use std::{
    alloc::{GlobalAlloc, Layout, System},
    hint::black_box,
    io::Write,
    sync::atomic::{AtomicU64, Ordering::Relaxed},
    time::Instant,
};

struct CountingAllocator;
static CALLS: AtomicU64 = AtomicU64::new(0);
static ALLOCATED: AtomicU64 = AtomicU64::new(0);
static FREED: AtomicU64 = AtomicU64::new(0);

// Delegate the allocation contract unchanged to System. Atomics do not allocate.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            CALLS.fetch_add(1, Relaxed);
            ALLOCATED.fetch_add(layout.size() as u64, Relaxed);
        }
        pointer
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if !pointer.is_null() {
            CALLS.fetch_add(1, Relaxed);
            ALLOCATED.fetch_add(layout.size() as u64, Relaxed);
        }
        pointer
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        FREED.fetch_add(layout.size() as u64, Relaxed);
        unsafe { System.dealloc(pointer, layout) };
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let result = unsafe { System.realloc(pointer, layout, size) };
        if !result.is_null() {
            CALLS.fetch_add(1, Relaxed);
            ALLOCATED.fetch_add(size as u64, Relaxed);
            FREED.fetch_add(layout.size() as u64, Relaxed);
        }
        result
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

struct Sample {
    work_us: f64,
    frame_us: f64,
    calls: u64,
    allocated: u64,
    freed: u64,
}

pub struct Probe {
    frames: usize,
    warmup: usize,
    count: usize,
    stress: bool,
    prefix: String,
    start: Instant,
    counters: (u64, u64, u64),
    work_us: f64,
    samples: Vec<Sample>,
}

impl Probe {
    pub fn new() -> Self {
        let number = |name, default| {
            std::env::var(name)
                .map(|value| value.parse().unwrap())
                .unwrap_or(default)
        };
        let count = number("MQ_BENCH_SAMPLES", 600);
        Self {
            frames: 0,
            warmup: number("MQ_BENCH_WARMUP", 120),
            count,
            stress: std::env::var("MQ_BENCH_MODE").unwrap() == "stress",
            prefix: std::env::var("MQ_BENCH_OUTPUT").unwrap(),
            start: Instant::now(),
            counters: (0, 0, 0),
            work_us: 0.,
            samples: Vec::with_capacity(count),
        }
    }

    pub fn begin_frame(&mut self) {
        self.counters = (
            CALLS.load(Relaxed),
            ALLOCATED.load(Relaxed),
            FREED.load(Relaxed),
        );
        self.start = Instant::now();
    }

    pub fn extra_work(&self) {
        if self.stress {
            for i in 0..2000 {
                black_box(is_key_down(KeyCode::A));
                black_box(is_key_pressed(KeyCode::Space));
                black_box(mouse_position());
                black_box(get_time());
                black_box(screen_width());
                draw_rectangle(
                    (i % 100) as f32 * 8.,
                    (i / 100) as f32 * 8. + 150.,
                    6.,
                    6.,
                    BLUE,
                );
            }
        }
    }

    // One extra frame is redrawn for capture after all timed samples. Screenshot,
    // hashing, CSV writing and termination are excluded from measurements.
    pub fn finish_work(&mut self) -> bool {
        self.work_us = self.start.elapsed().as_secs_f64() * 1e6;
        if self.frames < self.warmup + self.count {
            return false;
        }
        let image = get_screen_data();
        image.export_png(&format!("{}.png", self.prefix));
        let hash = image
            .bytes
            .iter()
            .fold(0xcbf29ce484222325u64, |hash, byte| {
                (hash ^ *byte as u64).wrapping_mul(0x100000001b3)
            });
        let mut output = std::fs::File::create(format!("{}.csv", self.prefix)).unwrap();
        writeln!(
            output,
            "work_us,frame_us,alloc_calls,allocated_bytes,freed_bytes"
        )
        .unwrap();
        for sample in &self.samples {
            writeln!(
                output,
                "{:.3},{:.3},{},{},{}",
                sample.work_us, sample.frame_us, sample.calls, sample.allocated, sample.freed
            )
            .unwrap();
        }
        std::fs::write(
            format!("{}.image.txt", self.prefix),
            format!("{}x{} rgba-fnv1a={hash:016x}\n", image.width, image.height),
        )
        .unwrap();
        true
    }

    pub fn finish_frame(&mut self) {
        let frame_us = self.start.elapsed().as_secs_f64() * 1e6;
        let counters = (
            CALLS.load(Relaxed),
            ALLOCATED.load(Relaxed),
            FREED.load(Relaxed),
        );
        if self.frames >= self.warmup {
            self.samples.push(Sample {
                work_us: self.work_us,
                frame_us,
                calls: counters.0 - self.counters.0,
                allocated: counters.1 - self.counters.1,
                freed: counters.2 - self.counters.2,
            });
        }
        self.frames += 1;
    }
}
