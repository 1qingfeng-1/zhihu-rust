package dev.gpui.mobile;

import android.content.Context;
import android.speech.tts.TextToSpeech;

import java.util.Locale;

/** Small TextToSpeech bridge used by the Rust reading player. */
public final class GpuiTts {
    private static TextToSpeech tts;
    private static boolean ready;

    private GpuiTts() {}

    public static synchronized void init(Context context) {
        if (tts != null) return;
        tts = new TextToSpeech(context.getApplicationContext(), status -> {
            synchronized (GpuiTts.class) {
                ready = status == TextToSpeech.SUCCESS;
                if (ready) {
                    tts.setLanguage(Locale.CHINA);
                    tts.setSpeechRate(1.0f);
                }
            }
        });
    }

    public static synchronized void speak(String text, float rate, float pitch, float volume, String voiceName) {
        if (tts == null || !ready || text == null || text.isEmpty()) return;
        tts.setSpeechRate(clamp(rate, 0.5f, 2.0f));
        tts.setPitch(clamp(pitch, 0.5f, 2.0f));
        tts.setVolume(clamp(volume, 0.0f, 1.0f));
        if (voiceName != null && !voiceName.isEmpty()) {
            for (Voice voice : tts.getVoices()) {
                if (voice.getName().equals(voiceName)) {
                    tts.setVoice(voice);
                    break;
                }
            }
        }
        tts.speak(text, TextToSpeech.QUEUE_FLUSH, null, "zhihu-rust");
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
