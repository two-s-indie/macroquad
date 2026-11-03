//! Loading and playing sounds.

#![allow(dead_code)]

use crate::{
    file::load_file,
    resource_disposal::{DisposalQueue, DisposalSender},
    Error,
};
use std::sync::Arc;

#[cfg(feature = "audio")]
use quad_snd::{AudioContext as QuadSndContext, Sound as QuadSndSound};

#[cfg(feature = "audio")]
pub use quad_snd::PlaySoundParams;

#[cfg(not(feature = "audio"))]
mod dummy_audio {
    use crate::audio::PlaySoundParams;

    pub struct AudioContext {}

    impl AudioContext {
        pub fn new() -> AudioContext {
            AudioContext {}
        }

        #[cfg(target_os = "android")]
        pub fn pause(&mut self) {}

        #[cfg(target_os = "android")]
        pub fn resume(&mut self) {}
    }

    pub struct Sound {}

    impl Sound {
        pub fn load(_ctx: &mut AudioContext, _data: &[u8]) -> Sound {
            Sound {}
        }

        pub fn play(&self, _ctx: &mut AudioContext, _params: PlaySoundParams) {
            eprintln!("warn: macroquad's \"audio\" feature disabled.");
        }

        pub fn stop(&self, _ctx: &mut AudioContext) {}

        pub fn set_volume(&self, _ctx: &mut AudioContext, _volume: f32) {}

        #[allow(dead_code)]
        pub fn is_loaded(&self) -> bool {
            true
        }

        pub fn delete(&self, _ctx: &AudioContext) {}
    }
}

#[cfg(not(feature = "audio"))]
use dummy_audio::{AudioContext as QuadSndContext, Sound as QuadSndSound};

#[cfg(not(feature = "audio"))]
pub struct PlaySoundParams {
    pub looped: bool,
    pub volume: f32,
}

pub struct AudioContext {
    native_ctx: QuadSndContext,
    disposal: DisposalQueue<QuadSndSound>,
}

impl AudioContext {
    pub fn new() -> AudioContext {
        AudioContext {
            native_ctx: QuadSndContext::new(),
            disposal: DisposalQueue::new(),
        }
    }

    pub(crate) fn garbage_collect(&self) {
        for sound in self.disposal.take() {
            sound.delete(&self.native_ctx);
        }
    }

    pub(crate) fn close_disposal(&self) {
        self.disposal.close();
    }

    fn validate(&self, sound: &Sound) {
        assert!(
            self.disposal.owns(&sound.0.disposal),
            "sound belongs to another engine session"
        );
    }

    #[cfg(target_os = "android")]
    pub fn pause(&mut self) {
        self.native_ctx.pause()
    }

    #[cfg(target_os = "android")]
    pub fn resume(&mut self) {
        self.native_ctx.resume()
    }
}

struct QuadSndSoundGuarded {
    sound: Option<QuadSndSound>,
    disposal: DisposalSender<QuadSndSound>,
}

impl Drop for QuadSndSoundGuarded {
    fn drop(&mut self) {
        self.disposal.send(self.sound.take().unwrap());
    }
}

#[derive(Clone)]
pub struct Sound(Arc<QuadSndSoundGuarded>);

impl std::fmt::Debug for Sound {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sound").finish()
    }
}

/// Load audio file.
///
/// Attempts to automatically detect the format of the source of data.

pub async fn load_sound(path: &str) -> Result<Sound, Error> {
    let data = load_file(path).await?;

    load_sound_from_bytes(&data).await
}

/// Load audio data.
///
/// Attempts to automatically detect the format of the source of data.
pub async fn load_sound_from_bytes(data: &[u8]) -> Result<Sound, Error> {
    let sound = crate::with_context(|context| {
        let ctx = &mut context.audio_context;
        Sound(Arc::new(QuadSndSoundGuarded {
            sound: Some(QuadSndSound::load(&mut ctx.native_ctx, data)),
            disposal: ctx.disposal.sender(),
        }))
    });

    // only on wasm the sound is not ready right away
    #[cfg(target_arch = "wasm32")]
    while sound.0.sound.as_ref().unwrap().is_loaded() == false {
        crate::window::next_frame().await;
    }

    Ok(sound)
}

pub fn play_sound_once(sound: &Sound) {
    play_sound(
        sound,
        PlaySoundParams {
            looped: false,
            volume: 1.0,
        },
    );
}

pub fn play_sound(sound: &Sound, params: PlaySoundParams) {
    crate::with_context(|context| {
        let ctx = &mut context.audio_context;
        ctx.validate(sound);
        sound
            .0
            .sound
            .as_ref()
            .unwrap()
            .play(&mut ctx.native_ctx, params);
    });
}

pub fn stop_sound(sound: &Sound) {
    crate::with_context(|context| {
        let ctx = &mut context.audio_context;
        ctx.validate(sound);
        sound.0.sound.as_ref().unwrap().stop(&mut ctx.native_ctx);
    });
}

pub fn set_sound_volume(sound: &Sound, volume: f32) {
    crate::with_context(|context| {
        let ctx = &mut context.audio_context;
        ctx.validate(sound);
        sound
            .0
            .sound
            .as_ref()
            .unwrap()
            .set_volume(&mut ctx.native_ctx, volume);
    });
}
