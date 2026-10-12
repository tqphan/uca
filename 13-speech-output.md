# 13 — Speech Output (text-to-speech)

**Purpose (decided):** speech output is the core of UCA's AAC function (see `16`). The user builds a message from words, symbols, phrases or letters, and UCA speaks it aloud with a text-to-speech voice.

**No voice chat (decided).** UCA does not route speech into voice chat, games or call apps. It has no virtual-microphone output and no push-to-talk. Implementers must not add these.

UI names: "Speak", "Message", "Quick phrases".

## Where speech goes

UCA does not install audio drivers. It synthesises speech to an audio buffer and plays it to **output devices the user picks**:

| Output | Purpose | Default |
| --- | --- | --- |
| `speaker_device` | **Speaking aloud** to people in the room. The main output. | system default output, on |
| `cue_device` | Private auditory-scanning cues (see `16`), e.g. headphones, so only the user hears them. | none (cues off until set) |

Playback uses `cpal` to the chosen device(s) at their native sample rate, resampling as needed. If a device disappears, UCA falls back to the system default output for `speaker_device` (speech must never silently fail) and shows a one-line notice.

## Speech engines

Engines implement one trait in a new crate `uca-voice`:

```rust
pub trait TtsEngine: Send {
    fn voices(&self) -> Result<Vec<VoiceInfo>>;            // id, name, language, gender?, engine
    fn synthesize(&mut self, text: &str, v: &VoiceParams)  // voice id, rate, pitch, volume
        -> Result<PcmBuffer>;                               // f32 mono + sample rate
}
```

- Synthesis must return audio samples, never "speak to the default device", so UCA can route to speaker and cue devices separately.
- Synthesis runs on a voice worker thread, never on the engine or UI thread.
- Long text is split at sentence boundaries; the first sentence starts playing while the rest synthesise.

| Platform | Built-in engine (v1) | Notes |
| --- | --- | --- |
| Windows | `Windows.Media.SpeechSynthesis` via the `windows` crate (`SynthesizeTextToStreamAsync` → WAV stream → PCM) | uses the voices installed in Windows |
| macOS (M7) | `AVSpeechSynthesizer` writing to buffers | system voices |
| Linux (M5) | **Piper** (decided), see below | offline neural voices; eSpeak NG as fallback |

### Piper (Linux engine; optional on Windows and macOS)

**Decision:** Linux uses **Piper**, a fast, local neural TTS engine.
- **Version.** Development moved in October 2025 from `rhasspy/piper` (archived) to `OHF-Voice/piper1-gpl`. Use the C/C++ library `libpiper`, pinned to a tested commit or release.
- **Licence.** `piper1-gpl` is GPL-3.0 and embeds eSpeak NG (GPL-3.0) for phonemization. It also uses ONNX Runtime. All of this is compatible with UCA's GPL-3.0-or-later (`18`).
- **Maintenance risk.** The Open Home Foundation says it is looking for maintainers. UCA pins the version, builds it in CI, and keeps the fallback below, so speech never depends on upstream activity.

**How it runs.**
- Piper runs in a helper process, `uca-tts-piper`, shipped with UCA (GPL-3.0). It speaks the `tts-provider` contract (`06`). A crash in the neural runtime can't take down UCA, and the host restarts it under the normal supervision rules.
- It synthesises to audio samples, never straight to a device. Voices report their own sample rate (often 22,050 Hz), and UCA resamples for output.
- **Latency target:** first audio of a short sentence within 200 ms on reference hardware. Long text is streamed sentence by sentence, as above.
- **Fallback:** if the Piper helper fails, can't load a voice, or has no voice for the language, UCA uses **eSpeak NG** directly. It is robotic but tiny, covers many languages, and is already a dependency. UCA shows a one-line notice. Speech never silently stops (`16`).

**Voices.**
- A Piper voice is a model file plus a config file. Each published voice has a model card naming its training dataset and licence, and **licences differ between voices**. Some datasets restrict commercial use or redistribution.
- **Bundled:** UCA bundles at most one default voice per supported language pack, and only voices whose model and dataset licences allow redistribution (e.g. public domain, CC0, CC BY, CC BY-SA). Record the licence and source in the third-party notices and the language pack (`17`).
- **More voices:** Settings → Voices lists Piper voices for the profile's language. It shows each voice's licence and size before downloading. Downloads happen only when the user asks (no background network use), are verified by SHA256, and are stored in `<config>/voices/` for all profiles.

**Other platforms.** Windows and macOS keep their OS voices as the default. Piper is an optional engine there once its builds pass CI, for users who want the same offline neural voice on every computer.

More engines, such as other offline neural voices or cloud voices, arrive as plugins through a new `tts-provider` contract (`06`):
- H→P `Synthesize {text, voice, rate, pitch}` → P→H `Audio {sample_rate, samples_f32 (chunked)}`
- H→P `ListVoices` → P→H `Voices`

Cloud engines must state in their manifest that text leaves the computer, and the voice setup screen shows that before they are enabled.

## Composing and saying

- **Compose buffer** = the AAC message bar (`16`). UCA keeps its own text buffer; typing into it does not send keystrokes to other apps.
    - A keyboard layer whose items use `speech.buffer.*` actions types into the buffer.
    - The ordinary keyboard board can be switched to "voice compose" mode with `speech.compose.on` / `.off`. Its `key.tap` / `text.type` then go to the buffer instead of the OS.
- **Quick phrases.** Items with `speech.say: "..."` speak immediately without touching the buffer. Default vocabulary and phrases are specified in `16`.
- **History.** The last 50 utterances, each re-sayable with one selection; `speech.repeat` repeats the last one.
- **Queue.** Utterances play one at a time in order. `speech.stop` stops the current one and clears the queue.
- **Word prediction** uses the compose buffer as context (built in, see `16`; plugins can add more via `item-provider`, `06`).

## Actions (added to `04`)

| Action | Args | Effect |
| --- | --- | --- |
| `speech.say` | text | speak this text now (queued) |
| `speech.speak_buffer` | — | speak the compose buffer, then clear it |
| `speech.buffer.append` | text | add text to the buffer |
| `speech.buffer.backspace` / `.clear` / `.word_back` | — | edit the buffer |
| `speech.compose.on` / `.off` / `.toggle` | — | route keyboard board output to the buffer |
| `speech.repeat` | — | say the last utterance again |
| `speech.stop` | — | stop speaking, clear queue |
| `speech.voice` | voice id | switch voice |

## Profile settings

```yaml
voice:
  enabled: true
  engine: windows            # windows | macos | plugin:<id>
  voice_id: "<engine voice id>"
  rate: 1.0                  # 0.5–2.0
  pitch: 1.0                 # 0.5–2.0
  volume: 1.0
  speaker_device: default    # default | <device name>
  cue_device: null           # headphones for auditory-scanning cues; null = cues off
  cue_voice_id: null         # optional different voice for cues
  max_chars: 1000            # longer text is refused with a notice
```

## Privacy and logging

- Text in the compose buffer, history and spoken utterances is **typed text**. It is wrapped in `Sensitive(...)` and redacted in logs, recordings and crash files unless sensitive logging is on (`11`, SAFETY-11).
- Synthesised audio is never written to disk or logged. History is kept in memory and cleared on exit unless `voice.keep_history: true` (default false). When kept, it is stored in the profile folder, never sent anywhere.

## Safety

- An utterance in progress is **not** cut off when presence is lost (the user may look away while speaking). Escape, the Safe Board and `speech.stop` stop it.
- UCA never speaks a message on its own initiative. Every message comes from a user action; only auditory-scanning cues (`16`) are automatic, and they go to `cue_device` only.
- Speech output is not an emergency system, and the help text says it must not be relied on as the only way to get help (consistent with `09`).

## Acceptance tests

- `FakeTts` returns a known PCM buffer; routing plays it to speaker and cue devices (fake `cpal` sinks) as configured; a missing speaker device falls back to the system default.
- `speech.stop` mid-utterance stops audio at once and clears the queue.
- Compose mode: `key.tap q` goes to the buffer, not to `Platform::inject_key`; turning compose off restores normal output.
- Long text splitting starts playback before full synthesis completes.
- Redaction: spoken text never appears in logs at default levels.
- Windows integration (manual): speak a sentence aloud through the speakers; unplug the chosen speaker device and check the fallback to the system default with a notice.
