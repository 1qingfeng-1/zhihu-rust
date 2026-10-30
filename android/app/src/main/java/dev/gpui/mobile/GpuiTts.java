package dev.gpui.mobile;

import android.content.Context;
import android.media.AudioAttributes;
import android.media.AudioManager;
import android.os.Bundle;
import android.speech.tts.TextToSpeech;
import android.speech.tts.Voice;

import java.util.Locale;
import java.util.Set;

/** Small TextToSpeech bridge used by the Rust reading player. */
public final class GpuiTts {
    private static TextToSpeech tts;
    private static AudioManager audio;
    private static boolean ready;

    private GpuiTts() {}

    public static synchronized void init(Context context) {
        if (tts != null) return;
        audio = (AudioManager) context.getApplicationContext()
                .getSystemService(Context.AUDIO_SERVICE);
        tts = new TextToSpeech(context.getApplicationContext(), status -> {
            synchronized (GpuiTts.class) {
                ready = status == TextToSpeech.SUCCESS;
                if (ready) {
                    // Route speech through the media stream so it is audible,
                    // follows media volume and appears in the volume panel.
                    tts.setAudioAttributes(new AudioAttributes.Builder()
                            .setUsage(AudioAttributes.USAGE_MEDIA)
                            .setContentType(AudioAttributes.CONTENT_TYPE_SPEECH)
                            .build());
                    int language = tts.setLanguage(Locale.CHINA);
                    if (language == TextToSpeech.LANG_MISSING_DATA
                            || language == TextToSpeech.LANG_NOT_SUPPORTED) {
                        tts.setLanguage(Locale.getDefault());
                    }
                    tts.setSpeechRate(1.0f);
                }
            }
        });
    }

    /** Adjust media volume for the hardware volume keys. */
    public static void adjustVolume(int direction) {
        AudioManager manager = audio;
        if (manager == null) return;
        try {
            manager.adjustStreamVolume(
                    AudioManager.STREAM_MUSIC, direction, AudioManager.FLAG_SHOW_UI);
        } catch (SecurityException ignored) {
            // Volume control can be restricted by device policy; ignore.
        }
    }

    public static synchronized void speak(String text, float rate, float pitch, float volume, String voiceName) {
        if (tts == null || !ready || text == null || text.isEmpty()) return;
        tts.setSpeechRate(clamp(rate, 0.5f, 2.0f));
        tts.setPitch(clamp(pitch, 0.5f, 2.0f));
        if (voiceName != null && !voiceName.isEmpty()) {
            Set<Voice> voices = tts.getVoices();
            if (voices != null) {
                for (Voice voice : voices) {
                    if (voice.getName().equals(voiceName)) {
                        tts.setVoice(voice);
                        break;
                    }
                }
            }
        }
        Bundle params = new Bundle();
        params.putFloat(
                TextToSpeech.Engine.KEY_PARAM_VOLUME, clamp(volume, 0.0f, 1.0f));
        tts.speak(text, TextToSpeech.QUEUE_FLUSH, params, "zhihu-rust");
    }

    public static synchronized boolean isSpeaking() {
        return tts != null && ready && tts.isSpeaking();
    }

    private static float clamp(float value, float min, float max) {
        return Math.max(min, Math.min(max, value));
    }

    public static synchronized void stop() {
        if (tts != null) tts.stop();
    }

    public static synchronized void shutdown() {
        if (tts != null) {
            tts.stop();
            tts.shutdown();
            tts = null;
            ready = false;
        }
    }
}
