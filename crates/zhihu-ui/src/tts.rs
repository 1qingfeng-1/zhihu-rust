//! Text-to-speech bridge.
//!
//! Android uses the native `TextToSpeech` engine via the host's `GpuiTts`
//! helper. Desktop shells out to the platform speech synthesizer; iOS is a
//! no-op until an AVSpeechSynthesizer bridge lands.
//!
//! Engine note: Android uses the system TTS engine selected in system
//! settings; Windows uses SAPI5 through `System.Speech` (installed voices such
//! as "Microsoft Huihui/Kangkang/Yaoyao Desktop"); macOS uses `say`; Linux uses
//! `spd-say`. No audio leaves the device.

#[derive(Clone, Debug)]
pub struct TtsOptions {
    /// 0.5..=2.0, 1.0 is the engine default.
    pub rate: f32,
    /// 0.5..=2.0, 1.0 is the engine default. Ignored by System.Speech.
    pub pitch: f32,
    /// 0.0..=1.0.
    pub volume: f32,
    /// Platform voice name; empty means the engine default.
    pub voice: String,
}

impl Default for TtsOptions {
    fn default() -> Self {
        Self {
            rate: 1.0,
            pitch: 1.0,
            volume: 1.0,
            voice: String::new(),
        }
    }
}

#[cfg(target_os = "android")]
pub fn speak(text: &str, options: &TtsOptions) {
    use jni::objects::JValue;
    let _ = gpui_mobile::android::jni::with_env(|env| {
        let class = env
            .find_class(jni::jni_str!("dev/gpui/mobile/GpuiTts"))
            .map_err(|error| error.to_string())?;
        let text = env.new_string(text).map_err(|error| error.to_string())?;
        let voice = env
            .new_string(&options.voice)
            .map_err(|error| error.to_string())?;
        env.call_static_method(
            &class,
            jni::jni_str!("speak"),
            jni::jni_sig!("(Ljava/lang/String;FFFLjava/lang/String;)V"),
            &[
                JValue::Object(&text),
                JValue::Float(options.rate),
                JValue::Float(options.pitch),
                JValue::Float(options.volume),
                JValue::Object(&voice),
            ],
        )
        .map_err(|error| error.to_string())?;
        Ok(())
    });
}

#[cfg(target_os = "android")]
pub fn stop_speaking() {
    let _ = gpui_mobile::android::jni::with_env(|env| {
        let class = env
            .find_class(jni::jni_str!("dev/gpui/mobile/GpuiTts"))
            .map_err(|error| error.to_string())?;
        env.call_static_method(&class, jni::jni_str!("stop"), jni::jni_sig!("()V"), &[])
            .map_err(|error| error.to_string())?;
        Ok(())
    });
}

#[cfg(target_os = "android")]
pub fn is_speaking() -> bool {
    gpui_mobile::android::jni::with_env(|env| {
        let class = env
            .find_class(jni::jni_str!("dev/gpui/mobile/GpuiTts"))
            .map_err(|error| error.to_string())?;
        let value = env
            .call_static_method(
                &class,
                jni::jni_str!("isSpeaking"),
                jni::jni_sig!("()Z"),
                &[],
            )
            .map_err(|error| error.to_string())?;
        Ok(value.z().unwrap_or(false))
    })
    .unwrap_or(false)
}

#[cfg(target_os = "ios")]
pub fn speak(_text: &str, _options: &TtsOptions) {}

#[cfg(target_os = "ios")]
pub fn is_speaking() -> bool {
    false
}

#[cfg(target_os = "ios")]
pub fn stop_speaking() {}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
mod desktop {
    use std::io::Write;
    use std::process::{Child, Command, Stdio};
    use std::sync::Mutex;

    use super::TtsOptions;

    static SPEECH: Mutex<Option<Child>> = Mutex::new(None);

    /// Speak `text` with the system voice. A previous utterance is stopped
    /// first so the reader never overlaps itself.
    pub fn speak(text: &str, options: &TtsOptions) {
        stop_speaking();
        if text.trim().is_empty() {
            return;
        }

        let mut command = speech_command(options);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        match command.spawn() {
            Ok(mut child) => {
                if let Some(mut stdin) = child.stdin.take() {
                    let _ = stdin.write_all(text.as_bytes());
                }
                if let Ok(mut slot) = SPEECH.lock() {
                    *slot = Some(child);
                }
            }
            Err(error) => eprintln!("failed to start desktop TTS: {error}"),
        }
    }

    pub fn is_speaking() -> bool {
        if let Ok(mut slot) = SPEECH.lock() {
            if let Some(child) = slot.as_mut() {
                return child.try_wait().ok().flatten().is_none();
            }
        }
        false
    }

    pub fn stop_speaking() {
        if let Ok(mut slot) = SPEECH.lock() {
            if let Some(mut child) = slot.take() {
                let _ = child.kill();
                // Reap off the UI thread; `kill` alone leaves a zombie on Unix.
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
            }
        }
    }

    fn escape_single_quotes(value: &str) -> String {
        value.replace('\'', "''")
    }

    #[cfg(target_os = "windows")]
    fn speech_command(options: &TtsOptions) -> Command {
        // System.Speech rate range is -10..10; volume is 0..100.
        let rate = (((options.rate.clamp(0.5, 2.0) - 1.0) * 5.0).round() as i32).clamp(-10, 10);
        let volume = (options.volume.clamp(0.0, 1.0) * 100.0).round() as i32;
        let voice = escape_single_quotes(options.voice.trim());
        let select_voice = if voice.is_empty() {
            String::new()
        } else {
            format!("try {{ $s.SelectVoice('{voice}') }} catch {{}}; ")
        };
        let script = format!(
            "Add-Type -AssemblyName System.Speech; \
             $s = New-Object System.Speech.Synthesis.SpeechSynthesizer; \
             $s.Rate = {rate}; $s.Volume = {volume}; {select_voice}\
             $s.Speak([Console]::In.ReadToEnd())"
        );
        let mut command = Command::new("powershell");
        command.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
        command
    }

    #[cfg(target_os = "macos")]
    fn speech_command(options: &TtsOptions) -> Command {
        // `say` does not read stdin, so use a tiny shell wrapper that does.
        let rate = (180.0 * options.rate.clamp(0.5, 2.0)).round() as i32;
        let voice = options.voice.trim();
        let voice_arg = if voice.is_empty() {
            String::new()
        } else {
            format!(" -v '{}'", voice.replace('\'', "'\\''"))
        };
        let mut command = Command::new("/bin/sh");
        command.args(["-c", &format!("say -r {rate}{voice_arg} \"$(cat)\"")]);
        command
    }

    #[cfg(target_os = "linux")]
    fn speech_command(options: &TtsOptions) -> Command {
        let rate = (options.rate.clamp(0.5, 2.0) * 100.0).round() as i32;
        let mut command = Command::new("/bin/sh");
        command.args(["-c", &format!("spd-say --wait -r {rate} \"$(cat)\"")]);
        command
    }
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub use desktop::{is_speaking, speak, stop_speaking};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_options_are_neutral() {
        let options = TtsOptions::default();
        assert_eq!(options.rate, 1.0);
        assert_eq!(options.pitch, 1.0);
        assert_eq!(options.volume, 1.0);
        assert!(options.voice.is_empty());
    }
}
