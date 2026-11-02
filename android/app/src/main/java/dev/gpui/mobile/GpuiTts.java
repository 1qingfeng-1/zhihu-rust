package dev.gpui.mobile;

import android.content.Context;
import android.media.AudioAttributes;
import android.media.AudioManager;
import android.os.Bundle;
import android.speech.tts.TextToSpeech;
import android.speech.tts.Voice;
import android.util.Log;

import java.util.Locale;
import java.util.Set;

/** Small TextToSpeech bridge used by the Rust reading player. */
public final class GpuiTts {
    private static final String TAG = "GpuiTts";
    private static TextToSpeech tts;
    private static AudioManager audio;
    private static boolean ready;
    private static boolean languageSupported;

    private GpuiTts() {}

    public static synchronized void init(Context context) {
        if (tts != null) return;
        audio = (AudioManager) context.getApplicationContext()
                .getSystemService(Context.AUDIO_SERVICE);
        tts = new TextToSpeech(context.getApplicationContext(), status -> {
            synchronized (GpuiTts.class) {
                ready = status == TextToSpeech.SUCCESS;
                Log.i(TAG, "init status=" + status + " ready=" + ready);
                if (ready) {
                    // Route speech through the media stream so it is audible,
                    // follows media volume and appears in the volume panel.
                    tts.setAudioAttributes(new AudioAttributes.Builder()
                            .setUsage(AudioAttributes.USAGE_MEDIA)
                            .setContentType(AudioAttributes.CONTENT_TYPE_SPEECH)
                            .build());
                    int language = tts.setLanguage(Locale.CHINA);
                    languageSupported = language != TextToSpeech.LANG_MISSING_DATA
                            && language != TextToSpeech.LANG_NOT_SUPPORTED;
                    if (!languageSupported) {
                        // Some engines (Talkify among them) advertise
                        // language-only locales and have no default voice
                        // until one is picked, so zh_CN is rejected. Try the
                        // bare language, then select a Chinese voice.
                        int bare = tts.setLanguage(new Locale("zh"));
                        languageSupported = bare != TextToSpeech.LANG_MISSING_DATA
                                && bare != TextToSpeech.LANG_NOT_SUPPORTED;
                        if (!languageSupported) {
                            Voice chinese = firstChineseVoice();
                            if (chinese != null) {
                                languageSupported =
                                        tts.setVoice(chinese) == TextToSpeech.SUCCESS;
                                Log.i(TAG, "selected voice=" + chinese.getName()
                                        + " locale=" + chinese.getLocale()
                                        + " supported=" + languageSupported);
                            }
                        }
                    }
                    if (!languageSupported) {
                        int fallback = tts.setLanguage(Locale.getDefault());
                        languageSupported = fallback != TextToSpeech.LANG_MISSING_DATA
                                && fallback != TextToSpeech.LANG_NOT_SUPPORTED;
                        Log.i(TAG, "setLanguage fallback=" + fallback
                                + " supported=" + languageSupported);
                    }
                    tts.setSpeechRate(1.0f);
                    Log.i(TAG, "engine voice=" + tts.getVoice()
                            + " languages=" + tts.getAvailableLanguages());
                }
            }
        });
    }

    /** First voice whose locale is Chinese, preferring offline voices. */
    private static Voice firstChineseVoice() {
        Set<Voice> voices = tts.getVoices();
        if (voices == null) {
            Log.i(TAG, "getVoices returned null");
            return null;
        }
        Voice candidate = null;
        for (Voice voice : voices) {
            Log.i(TAG, "voice name=" + voice.getName() + " locale=" + voice.getLocale()
                    + " network=" + voice.isNetworkConnectionRequired());
            if (!"zh".equals(voice.getLocale().getLanguage())) {
                continue;
            }
            if (candidate == null || (candidate.isNetworkConnectionRequired()
                    && !voice.isNetworkConnectionRequired())) {
                candidate = voice;
            }
        }
        return candidate;
    }

    /** Adjust the media stream for the hardware volume keys. */
    public static void adjustVolume(int direction) {
        AudioManager manager = audio;
        if (manager == null) {
            Log.w(TAG, "adjustVolume: no AudioManager");
            return;
        }
        int before = manager.getStreamVolume(AudioManager.STREAM_MUSIC);
        try {
            manager.adjustStreamVolume(
                    AudioManager.STREAM_MUSIC, direction, AudioManager.FLAG_SHOW_UI);
            Log.d(TAG, "adjustVolume dir=" + direction + " music " + before + "->"
                    + manager.getStreamVolume(AudioManager.STREAM_MUSIC));
        } catch (SecurityException e) {
            Log.w(TAG, "adjustStreamVolume denied", e);
        }
    }

    public static synchronized boolean speak(String text, float rate, float pitch, float volume, String voiceName) {
        if (tts == null) {
            Log.w(TAG, "speak: engine not created");
            return false;
        }
        if (!ready) {
            Log.w(TAG, "speak: engine not ready");
            return false;
        }
        if (text == null || text.isEmpty()) return false;
        tts.setSpeechRate(clamp(rate, 0.5f, 2.0f));
        tts.setPitch(clamp(pitch, 0.5f, 2.0f));
        boolean voiceSelected = false;
        if (voiceName != null && !voiceName.isEmpty()) {
            Set<Voice> voices = tts.getVoices();
            if (voices != null) {
                for (Voice voice : voices) {
                    if (voice.getName().equals(voiceName)) {
                        tts.setVoice(voice);
                        voiceSelected = true;
                        break;
                    }
                }
            }
        }
        // No Chinese (or device-language) voice means the engine would accept
        // the utterance and stay silent; report it so the reader can say why.
        if (!languageSupported && !voiceSelected) {
            Log.w(TAG, "speak: no usable language, voice=" + tts.getVoice());
            return false;
        }
        Bundle params = new Bundle();
        params.putFloat(
                TextToSpeech.Engine.KEY_PARAM_VOLUME, clamp(volume, 0.0f, 1.0f));
        int result = tts.speak(text, TextToSpeech.QUEUE_FLUSH, params, "zhihu-rust");
        if (result != TextToSpeech.SUCCESS) {
            Log.w(TAG, "speak: queue rejected result=" + result);
        }
        return result == TextToSpeech.SUCCESS;
    }

    /** Whether the engine finished initializing and has a usable language. */
    public static synchronized boolean isReady() {
        return tts != null && ready && languageSupported;
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
