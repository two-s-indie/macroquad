//! Frame profiling and rendering diagnostics local to the current thread.

use crate::{get_context, get_quad_context, time::get_time};

use std::{cell::RefCell, collections::HashMap};

thread_local! {
    static PROFILER: RefCell<Profiler> = RefCell::new(Profiler::new());
}

#[derive(Debug, Clone)]
pub struct Zone {
    pub name: String,
    pub start_time: f64,
    pub duration: f64,
    pub children: Vec<Zone>,

    parent: *mut Zone,
}

impl Zone {
    fn clone(&self, parent: *mut Zone) -> Zone {
        Zone {
            name: self.name.clone(),
            start_time: self.start_time,
            duration: self.duration,
            children: self
                .children
                .iter()
                .map(|zone| zone.clone(self as *const _ as *mut _))
                .collect(),
            parent,
        }
    }
}

pub struct ZoneGuard {
    _marker: (),
}

impl ZoneGuard {
    pub fn new(name: &str) -> ZoneGuard {
        begin_zone(name);
        ZoneGuard { _marker: () }
    }
}

impl Drop for ZoneGuard {
    fn drop(&mut self) {
        end_zone();
    }
}

pub fn enable() {
    PROFILER.with_borrow_mut(|profiler| profiler.enable_request = Some(true));
}

pub fn disable() {
    PROFILER.with_borrow_mut(|profiler| profiler.enable_request = Some(false));
}

pub fn is_enabled() -> bool {
    PROFILER.with_borrow(|profiler| profiler.enabled)
}

pub fn begin_zone(name: &str) {
    if is_enabled() {
        let now = get_time();
        PROFILER.with_borrow_mut(|profiler| profiler.begin_zone(name, now));
    }
}

pub fn end_zone() {
    if is_enabled() {
        let now = get_time();
        PROFILER.with_borrow_mut(|profiler| profiler.end_zone(now));
    }
}

pub fn begin_gpu_query(name: &str) {
    PROFILER.with_borrow_mut(|profiler| profiler.begin_gpu_query(name));
}

pub fn end_gpu_query() {
    PROFILER.with_borrow_mut(Profiler::end_gpu_query);
}

/// Workaround to stop gl capture on debug rendering
#[doc(hidden)]
pub fn pause_gl_capture() {
    if PROFILER.with_borrow(|profiler| profiler.capture) {
        crate::get_context().gl.capture(false);
    }
}

/// Workaround to stop gl capture on debug rendering
pub fn resume_gl_capture() {
    if PROFILER.with_borrow(|profiler| profiler.capture) {
        crate::get_context().gl.capture(false);
    }
}

pub(crate) fn reset() {
    let frame_time = crate::time::get_frame_time();
    let capture = PROFILER.with_borrow_mut(|profiler| profiler.reset(frame_time));
    // Apply rendering state only after releasing the profiler borrow.
    if let Some(capture) = capture {
        get_context().gl.capture(capture);
    }
}

#[derive(Debug, Clone)]
pub struct Frame {
    pub full_frame_time: f32,
    pub zones: Vec<Zone>,
    active_zone: *mut Zone,
}

impl Frame {
    const fn new() -> Frame {
        Frame {
            full_frame_time: 0.0,
            zones: vec![],
            active_zone: std::ptr::null_mut(),
        }
    }

    pub fn try_clone(&self) -> Option<Frame> {
        if self.active_zone.is_null() == false {
            return None;
        }

        Some(Frame {
            full_frame_time: self.full_frame_time,
            zones: self
                .zones
                .iter()
                .map(|zone| zone.clone(std::ptr::null_mut()))
                .collect(),
            active_zone: std::ptr::null_mut(),
        })
    }
}

pub fn frame() -> Frame {
    PROFILER.with_borrow(|profiler| profiler.prev_frame.clone())
}

pub fn gpu_queries() -> Vec<(String, u64)> {
    PROFILER.with_borrow(|profiler| {
        profiler
            .queries
            .iter()
            .map(|(name, query)| (name.to_owned(), query.value))
            .collect()
    })
}

pub fn sample_gpu_queries() {
    PROFILER.with_borrow_mut(|profiler| {
        for query in profiler.queries.values_mut() {
            query.force_resume = true;
        }
    });
}

struct Profiler {
    frame: Frame,
    prev_frame: Frame,
    queries: HashMap<String, GpuQuery>,
    active_query: Option<String>,
    enabled: bool,
    capture_request: bool,
    capture: bool,
    enable_request: Option<bool>,
    drawcalls: Vec<DrawCallTelemetry>,
    strings: Vec<String>,
}

impl Profiler {
    fn new() -> Self {
        Self {
            frame: Frame::new(),
            queries: HashMap::new(),
            active_query: None,
            prev_frame: Frame::new(),
            enabled: false,
            enable_request: None,
            capture_request: false,
            capture: false,
            drawcalls: vec![],
            strings: vec![],
        }
    }

    // Return a requested rendering-state change without borrowing the renderer.
    fn reset(&mut self, frame_time: f32) -> Option<bool> {
        assert!(
            self.frame.active_zone.is_null(),
            "New frame started with unpaired begin/end zones."
        );

        self.frame.full_frame_time = frame_time;
        std::mem::swap(&mut self.prev_frame, &mut self.frame);
        self.frame = Frame::new();

        if let Some(enable) = self.enable_request.take() {
            self.enabled = enable;
        }

        let mut capture = None;
        if self.capture {
            self.capture = false;
            capture = Some(false);
        }
        if self.capture_request {
            self.drawcalls.clear();
            self.capture = true;
            capture = Some(true);
            self.capture_request = false;
        }
        capture
    }

    fn begin_gpu_query(&mut self, name: &str) {
        assert!(
            self.active_query.is_none(),
            "Only one active query is allowed by OpenGL"
        );

        let name = name.to_string();
        let query = self
            .queries
            .entry(name.clone())
            .or_insert_with(|| GpuQuery {
                query: miniquad::graphics::ElapsedQuery::new(),
                in_progress: false,
                value: 0,
                force_resume: false,
            });
        self.active_query = Some(name);
        if query.force_resume {
            query.in_progress = true;
            query.query.begin_query();
        }
    }

    fn end_gpu_query(&mut self) {
        let name = self
            .active_query
            .take()
            .expect("End query without begin query");
        let query = self.queries.get_mut(&name).unwrap();
        if query.in_progress {
            query.force_resume = false;
            query.in_progress = false;
            query.query.end_query();
        }
        if query.query.is_available() {
            query.value = query.query.get_result();
        }
    }

    fn begin_zone(&mut self, name: &str, now: f64) {
        let zones = if self.frame.active_zone.is_null() {
            &mut self.frame.zones
        } else {
            unsafe { &mut (&mut *self.frame.active_zone).children }
        };

        zones.push(Zone {
            name: name.to_string(),
            start_time: now,
            duration: 0.0,
            parent: self.frame.active_zone,
            children: vec![],
        });
        self.frame.active_zone = zones.last_mut().unwrap() as _;
    }

    fn end_zone(&mut self, now: f64) {
        assert!(
            self.frame.active_zone.is_null() == false,
            "end_zone called without begin_zone"
        );

        let start_time = unsafe { (&mut *self.frame.active_zone).start_time };
        let duration = now - start_time;

        unsafe { (&mut *self.frame.active_zone).duration = duration };
        self.frame.active_zone = unsafe { (&mut *self.frame.active_zone).parent };
    }
}

pub struct GpuQuery {
    pub query: miniquad::graphics::ElapsedQuery,
    pub in_progress: bool,
    pub value: u64,
    pub force_resume: bool,
}

pub fn scene_allocated_memory() -> usize {
    use crate::experimental::scene;

    scene::allocated_memory()
}

/// ```skip
/// {
///    let _t = telemetry::LogTimeGuard::new("Atlas build time");
///     mq::texture::build_textures_atlas();
/// }
/// ```
/// Will add "Time query: Atlas build time, 0.5s" string to
/// `telemetry::strings()`
pub struct LogTimeGuard<'a> {
    name: &'a str,
    start_time: f64,
}

impl<'a> LogTimeGuard<'a> {
    pub fn new(name: &'a str) -> LogTimeGuard<'a> {
        LogTimeGuard {
            name,
            start_time: get_time(),
        }
    }
}

impl<'a> Drop for LogTimeGuard<'a> {
    fn drop(&mut self) {
        log_string(&format!(
            "Time query: {}, {:.1}s",
            self.name,
            get_time() - self.start_time
        ));
    }
}

pub fn log_string(string: &str) {
    PROFILER.with_borrow_mut(|profiler| profiler.strings.push(string.to_owned()));
}

pub fn drawcalls() -> Vec<DrawCallTelemetry> {
    PROFILER.with_borrow(|profiler| profiler.drawcalls.clone())
}

pub fn strings() -> Vec<String> {
    PROFILER.with_borrow(|profiler| profiler.strings.clone())
}

/// Note that coroutines pre-allocate 56Kb, so 56000 as a result doesnt mean a leak
pub fn coroutines_allocated_memory() -> usize {
    get_context().coroutines_context.allocated_memory()
}

pub fn active_coroutines_count() -> usize {
    get_context().coroutines_context.active_coroutines_count()
}

pub fn capture_frame() {
    PROFILER.with_borrow_mut(|profiler| profiler.capture_request = true);
}

#[derive(Clone, Debug)]
pub struct DrawCallTelemetry {
    pub indices_count: usize,
    pub texture: miniquad::TextureId,
}

pub(crate) fn track_drawcall(
    pipeline: &miniquad::Pipeline,
    bindings: &miniquad::Bindings,
    indices_count: usize,
) {
    let texture = get_quad_context().new_render_texture(miniquad::TextureParams {
        width: 128,
        height: 128,
        ..Default::default()
    });

    let pass = Some(get_quad_context().new_render_pass(texture, None));
    get_quad_context().begin_pass(pass, miniquad::PassAction::clear_color(0.4, 0.8, 0.4, 1.));
    get_quad_context().apply_pipeline(pipeline);
    get_quad_context().apply_bindings(bindings);
    get_quad_context().draw(0, indices_count as _, 1);
    get_quad_context().end_render_pass();

    PROFILER.with_borrow_mut(|profiler| {
        profiler.drawcalls.push(DrawCallTelemetry {
            indices_count,
            texture,
        });
    });
}

pub fn textures_count() -> usize {
    get_context().textures.len()
}

#[cfg(test)]
mod tests;
