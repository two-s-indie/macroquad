//! Cross-platform mouse, keyboard (and gamepads soon) module.

use std::collections::HashSet;

use crate::DroppedFile;
use crate::Vec2;
pub use miniquad::{KeyCode, MouseButton};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TouchPhase {
    Started,
    Stationary,
    Moved,
    Ended,
    Cancelled,
}

impl From<miniquad::TouchPhase> for TouchPhase {
    fn from(miniquad_phase: miniquad::TouchPhase) -> TouchPhase {
        match miniquad_phase {
            miniquad::TouchPhase::Started => TouchPhase::Started,
            miniquad::TouchPhase::Moved => TouchPhase::Moved,
            miniquad::TouchPhase::Ended => TouchPhase::Ended,
            miniquad::TouchPhase::Cancelled => TouchPhase::Cancelled,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Touch {
    pub id: u64,
    pub phase: TouchPhase,
    pub position: Vec2,
}

/// Constrain mouse to window
pub fn set_cursor_grab(grab: bool) {
    crate::with_context(|context| {
        context.cursor_grabbed = grab;
        miniquad::window::set_cursor_grab(grab);
    })
}

/// Set mouse cursor visibility
pub fn show_mouse(shown: bool) {
    miniquad::window::show_mouse(shown);
}

/// Return the mouse position in logical client units, using the current DPI scale.
/// These units match `screen_width()` and `screen_height()`. While grabbed, the
/// position is virtual: raw relative motion is accumulated instead of tracking
/// the operating system cursor.
pub fn mouse_position() -> (f32, f32) {
    crate::with_context(|context| {
        (
            context.mouse_position.x / miniquad::window::dpi_scale(),
            context.mouse_position.y / miniquad::window::dpi_scale(),
        )
    })
}

/// Return mouse position in range [-1; 1].
pub fn mouse_position_local() -> Vec2 {
    crate::with_context(|context| context.mouse_position_local())
}

/// Returns the difference between the current mouse position and the mouse position on the previous frame.
pub fn mouse_delta_position() -> Vec2 {
    crate::with_context(|context| {
        let current_position = context.mouse_position_local();
        let last_position = context.last_mouse_position.unwrap_or(current_position);

        // Calculate the delta
        last_position - current_position
    })
}

/// This is set to true by default, meaning touches will raise mouse events in addition to raising touch events.
/// If set to false, touches won't affect mouse events.
pub fn is_simulating_mouse_with_touch() -> bool {
    crate::with_context(|context| context.simulate_mouse_with_touch)
}

/// This is set to true by default, meaning touches will raise mouse events in addition to raising touch events.
/// If set to false, touches won't affect mouse events.
pub fn simulate_mouse_with_touch(option: bool) {
    crate::with_context(|context| context.simulate_mouse_with_touch = option);
}

/// Return touches with positions in pixels.
pub fn touches() -> Vec<Touch> {
    crate::with_context(|context| context.touches.values().cloned().collect())
}

/// Return touches with positions in range [-1; 1].
pub fn touches_local() -> Vec<Touch> {
    crate::with_context(|context| {
        let size =
            Vec2::new(context.screen_width, context.screen_height) / miniquad::window::dpi_scale();
        context
            .touches
            .values()
            .map(|touch| {
                let mut touch = touch.clone();
                touch.position = convert_to_local(touch.position, size);
                touch
            })
            .collect()
    })
}

/// Returns the accumulated horizontal and vertical scroll delta for this frame.
/// The deltas are reset at the end of each frame.
pub fn mouse_wheel() -> (f32, f32) {
    crate::with_context(|context| (context.mouse_wheel.x, context.mouse_wheel.y))
}

/// Detect if the key has been pressed once
pub fn is_key_pressed(key_code: KeyCode) -> bool {
    crate::with_context(|context| context.keys_pressed.contains(&key_code))
}

/// Detect if the key is being pressed
pub fn is_key_down(key_code: KeyCode) -> bool {
    crate::with_context(|context| context.keys_down.contains(&key_code))
}

/// Detect if the key has been released this frame
pub fn is_key_released(key_code: KeyCode) -> bool {
    crate::with_context(|context| context.keys_released.contains(&key_code))
}

/// Detect if any key is being pressed
pub fn is_any_key_down() -> bool {
    crate::with_context(|context| context.keys_down.len() > 0)
}

/// Return the last pressed char.
/// Each "get_char_pressed" call will consume a character from the input queue.
pub fn get_char_pressed() -> Option<char> {
    crate::with_context(|context| context.chars_pressed_queue.pop_front())
}

/// Return the last pressed key.
pub fn get_last_key_pressed() -> Option<KeyCode> {
    crate::with_context(|context| {
        // TODO: this will return a random key from keys_pressed HashMap instead of the last one, fix me later
        context.keys_pressed.iter().next().cloned()
    })
}

pub fn get_keys_pressed() -> HashSet<KeyCode> {
    crate::with_context(|context| context.keys_pressed.clone())
}

pub fn get_keys_down() -> HashSet<KeyCode> {
    crate::with_context(|context| context.keys_down.clone())
}

pub fn get_keys_released() -> HashSet<KeyCode> {
    crate::with_context(|context| context.keys_released.clone())
}

/// Clears input queue
pub fn clear_input_queue() {
    crate::with_context(|context| {
        context.chars_pressed_queue.clear();
        context.chars_pressed_ui_queue.clear();
        context.mouse_pressed.clear();
        context.keys_pressed.clear();
    })
}

/// Detect if the button is being pressed
pub fn is_mouse_button_down(btn: MouseButton) -> bool {
    crate::with_context(|context| context.mouse_down.contains(&btn))
}

/// Detect if the button has been pressed once
pub fn is_mouse_button_pressed(btn: MouseButton) -> bool {
    crate::with_context(|context| context.mouse_pressed.contains(&btn))
}

/// Detect if the button has been released this frame
pub fn is_mouse_button_released(btn: MouseButton) -> bool {
    crate::with_context(|context| context.mouse_released.contains(&btn))
}

/// Convert a position in pixels to a position in the range [-1; 1].
pub(crate) fn convert_to_local(pixel_pos: Vec2, screen_size: Vec2) -> Vec2 {
    (pixel_pos / screen_size) * 2.0 - Vec2::new(1.0, 1.0)
}

/// Prevents quit
pub fn prevent_quit() {
    crate::with_context(|context| context.prevent_quit_event = true);
}

/// Detect if quit has been requested
pub fn is_quit_requested() -> bool {
    crate::with_context(|context| context.quit_requested)
}

/// Gets the files which have been dropped on the window.
pub fn get_dropped_files() -> Vec<DroppedFile> {
    crate::with_context(|context| context.dropped_files())
}

/// Functions for advanced input processing.
///
/// Functions in this module should be used by external tools that uses miniquad system, like different UI libraries. User shouldn't use this function.
pub mod utils {

    /// Register input subscriber. Returns subscriber identifier that must be used in `repeat_all_miniquad_input`.
    pub fn register_input_subscriber() -> usize {
        crate::with_context(|context| {
            context.input_events.push(vec![]);

            context.input_events.len() - 1
        })
    }

    /// Repeats all events that came since last call of this function with current value of `subscriber`. This function must be called at each frame.
    /// Callbacks run outside the context borrow. Events added during replay are
    /// left for a later call. If a callback panics, undelivered events remain
    /// queued; the event whose callback panicked is not repeated.
    pub fn repeat_all_miniquad_input<T: miniquad::EventHandler>(t: &mut T, subscriber: usize) {
        struct Replay {
            subscriber: usize,
            events: std::vec::IntoIter<crate::MiniquadInputEvent>,
        }
        impl Drop for Replay {
            fn drop(&mut self) {
                if self.events.len() == 0 {
                    return;
                }
                // Keep undelivered events ahead of any events appended by a
                // callback. Never replay the event whose callback panicked.
                let mut remaining: Vec<_> = self.events.by_ref().collect();
                crate::with_context(|context| {
                    remaining.append(&mut context.input_events[self.subscriber]);
                    context.input_events[self.subscriber] = remaining;
                })
            }
        }
        let events =
            crate::with_context(|context| std::mem::take(&mut context.input_events[subscriber]));
        let mut replay = Replay {
            subscriber,
            events: events.into_iter(),
        };
        while let Some(event) = replay.events.next() {
            event.repeat(t);
        }
    }
}
