//! Window and associated to window rendering context related functions.

use crate::color::Color;

// miniquad is re-exported for use with `with_internal_gl`.
pub use miniquad;

pub use miniquad::conf::Conf;

/// Block execution until the next frame.
#[must_use = "use `next_frame().await` to advance to the next frame"]
pub fn next_frame() -> crate::exec::FrameFuture {
    crate::thread_assert::same_thread();
    crate::exec::FrameFuture::default()
}

/// Fill window background with solid color.
/// Note: even when "clear_background" is not called explicitly,
/// the screen will be cleared at the beginning of the frame.
pub fn clear_background(color: Color) {
    crate::with_context(|context| {
        context.gl.clear(&mut *context.quad_context, color);
    })
}

#[doc(hidden)]
pub fn gl_set_drawcall_buffer_capacity(max_vertices: usize, max_indices: usize) {
    crate::with_context(|context| {
        context
            .gl
            .update_drawcall_capacity(&mut *context.quad_context, max_vertices, max_indices);
    })
}

/// Scoped renderer/backend access supplied by [`with_internal_gl`].
/// Use these fields and methods inside the closure; engine globals are unavailable.
pub struct InternalGlContext<'a> {
    pub quad_context: &'a mut dyn miniquad::RenderingBackend,
    pub quad_gl: &'a mut crate::quad_gl::QuadGl,
    textures: &'a crate::texture::TexturesContext,
    projection: glam::Mat4,
    screen_size: (f32, f32),
}

impl<'a> InternalGlContext<'a> {
    /// Submit batched Macroquad geometry using this scope's projection and
    /// texture registry. Call before drawing with raw Miniquad/OpenGL commands.
    pub fn flush(&mut self) {
        let textures = self.textures;
        let white_texture = self.quad_gl.white_texture;
        self.quad_gl
            .draw(self.quad_context, self.projection, |texture| {
                textures.resolve(&texture.texture, white_texture)
            });
    }

    /// Resolve a texture using this session's registry, without global access.
    /// The ID does not keep the texture alive; retain its `Texture2D` owner.
    pub fn texture_id(&self, texture: &crate::texture::Texture2D) -> miniquad::TextureId {
        self.textures
            .resolve(&texture.texture, self.quad_gl.white_texture)
    }

    /// Select a Macroquad texture for subsequent batched geometry.
    /// Keep its owner alive until queued drawing finishes.
    pub fn texture(&mut self, texture: Option<&crate::texture::Texture2D>) {
        let texture = texture.map(|texture| self.texture_id(texture));
        self.quad_gl.texture(texture);
    }

    /// Camera projection captured when entering the scope.
    pub fn get_projection_matrix(&self) -> glam::Mat4 {
        self.projection
    }

    /// Current renderer viewport, defaulting to the captured logical screen size.
    pub fn get_viewport(&self) -> (i32, i32, i32, i32) {
        self.quad_gl.get_viewport(self.screen_size)
    }

    /// Create a separate UI backed by this engine session.
    pub fn new_ui(&mut self, width: f32, height: f32) -> crate::ui::Ui {
        crate::ui::Ui::new_with_disposal(self.quad_context, width, height, self.textures.disposal())
    }
}

/// Access the renderer and backend for the duration of a synchronous closure.
/// Replaces the unsafe, unconstrained references returned by `get_internal_gl`.
///
/// Read timing/input and create engine resources before entering. Calls that
/// access the engine context (including nested `with_internal_gl`, drawing,
/// camera changes, and `Texture2D` methods) panic inside the closure. Use the
/// supplied texture/projection/viewport helpers and backend instead. Resource
/// drops enqueue disposal and are allowed. The boundary is released on unwind.
/// Call `gl.flush()` before raw rendering to submit pending Macroquad geometry.
///
/// ```no_run
/// use macroquad::prelude::*;
/// with_internal_gl(|mut gl| {
///     gl.flush();
///     gl.quad_context.begin_default_pass(miniquad::PassAction::Nothing);
///     // Raw backend drawing goes here.
///     gl.quad_context.end_render_pass();
/// });
/// ```
///
/// References cannot escape, including into a future:
/// ```compile_fail
/// use macroquad::prelude::*;
/// let backend = with_internal_gl(|gl| gl.quad_context);
/// ```
/// ```compile_fail
/// use macroquad::prelude::*;
/// let future = with_internal_gl(|mut gl| async move { gl.flush() });
/// ```
pub fn with_internal_gl<R>(f: impl FnOnce(InternalGlContext<'_>) -> R) -> R {
    crate::with_context(|context| {
        let projection = context.projection_matrix();
        let dpi = miniquad::window::dpi_scale();
        let screen_size = (context.screen_width / dpi, context.screen_height / dpi);
        f(InternalGlContext {
            quad_context: &mut *context.quad_context,
            quad_gl: &mut context.gl,
            textures: &context.textures,
            projection,
            screen_size,
        })
    })
}

/// Logical client width: framebuffer pixels divided by the current rendering DPI scale.
pub fn screen_width() -> f32 {
    crate::with_context(|context| context.screen_width / miniquad::window::dpi_scale())
}

/// Logical client height: framebuffer pixels divided by the current rendering DPI scale.
pub fn screen_height() -> f32 {
    crate::with_context(|context| context.screen_height / miniquad::window::dpi_scale())
}

/// Current rendering scale supplied by Miniquad. It can change while the window is open.
pub fn screen_dpi_scale() -> f32 {
    miniquad::window::dpi_scale()
}

/// Request a client size in logical units. This takes the current DPI into account.
/// On Windows, round once to the nearest framebuffer pixel before passing the
/// request to Miniquad. OS constraints can still change the actual client size.
///
/// Note that the OS might decide to give a different size. Additionally, the size in macroquad won't be updated until the next `next_frame().await`.
pub fn request_new_screen_size(width: f32, height: f32) {
    let dpi = miniquad::window::dpi_scale();
    let (width, height) = (width * dpi, height * dpi);
    // Match this fork's Windows startup/DPI-transition rounding. Other backends
    // retain their existing size-request conversion and native sizing contracts.
    #[cfg(target_os = "windows")]
    let (width, height) = (width.round(), height.round());
    miniquad::window::set_window_size(width as u32, height as u32);
    // We do not set the context.screen_width and context.screen_height here.
    // After `set_window_size` is called, EventHandlerFree::resize will be invoked, setting the size correctly.
    // Because the OS might decide to give a different screen dimension, setting the context.screen_* here would be confusing.
}

/// Toggle whether the window is fullscreen.
pub fn set_fullscreen(fullscreen: bool) {
    miniquad::window::set_fullscreen(fullscreen);
}

/// With `set_panic_handler` set to a handler code, macroquad will use
/// `std::panic::catch_unwind` on user code to catch some panics.
///
/// Sometimes it is nice to let player send a bug report with a screenshot of an
/// error. It is way easier to ask for a screenshot than ask to connect to a phone
/// with adb and post a log.
///
/// For this very case "set_panic_handler" exists.
/// ```ignore
/// set_panic_handler(|msg, backtrace| async move {
///     loop {
///         clear_background(RED);
///         ui::root_ui().label(None, &msg);
///         for line in backtrace.split('\n') {
///             root_ui().label(None, line);
///         }
///         next_frame().await;
///      }
/// });
/// ```
///
/// `set_panic_handler` acts as a second app entry-point, that will be used
/// after a panic in user code will happen. Macroquad will also try to catch some OS
/// panics, but not all of them - some compatibility bugs may end up crashing the app.
///
/// Without `set_panic_handler` macroquad will not use `catch_unwind` at all,
/// therefore `panic_handler` is completely optional.
/// The supplied factory is called after unwinding, outside engine borrows.
/// NOTE: only with "backtrace" macroquad feature `backtrace` string will contain an
/// actual backtrace. Otherwise only panic location and message will be available.
/// NOTE: on android, even with "backtrace" nice backtrace is available only if the game is compiled with sdk >= 21.
/// To use sdk >= 21 add "min_sdk_version = 21" to Cargo.toml
pub fn set_panic_handler<T, F>(future: F)
where
    T: std::future::Future<Output = ()> + 'static,
    F: Fn(String, String) -> T + Send + Sync + 'static,
{
    let handler: RecoveryHandler =
        std::sync::Arc::new(move |message, backtrace| Box::pin(future(message, backtrace)));
    let previous = {
        crate::with_context(|context| {
            context.unwind = true;
            context.recovery_handler.replace(handler)
        })
    };
    drop(previous);
    PANIC_INFO.with(|info| *info.borrow_mut() = None);
    std::panic::set_hook(Box::new(move |info| {
        let message = format!("{info:?}");
        #[cfg(feature = "backtrace")]
        let backtrace_string = format!("{:?}", backtrace::Backtrace::new());
        #[cfg(not(feature = "backtrace"))]
        let backtrace_string = format!("Macroquad compiled without \"backtrace\" feature");
        crate::logging::error!("{}", message);
        crate::logging::error!("{}", backtrace_string);

        // The hook can run while a context/scheduler borrow is unwinding.
        // Record plain data only; invoke the user's factory after catch_unwind.
        let _ = PANIC_INFO.try_with(|info| {
            if let Ok(mut info) = info.try_borrow_mut() {
                *info = Some((message, backtrace_string));
            }
        });
    }));
}

pub(crate) type RecoveryHandler = std::sync::Arc<
    dyn Fn(String, String) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()>>>
        + Send
        + Sync,
>;

thread_local! {
    static PANIC_INFO: std::cell::RefCell<Option<(String, String)>> = const { std::cell::RefCell::new(None) };
}

pub(crate) fn take_recovery_future(
) -> Option<std::pin::Pin<Box<dyn std::future::Future<Output = ()>>>> {
    let handler = crate::with_context(|context| context.recovery_handler.clone())?;
    let (message, backtrace) = PANIC_INFO.with(|info| info.borrow_mut().take())?;
    Some(handler(message, backtrace))
}
