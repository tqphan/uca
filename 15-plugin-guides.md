# 15 — Plugin Guides (Tobii and MediaPipe)

**Decision:** UCA does **not** include Tobii or MediaPipe support. The repository, installers and releases contain no code, bindings, libraries, models or headers for either. Instead, UCA publishes **detailed step-by-step guides** so anyone can build these as plugins, using the public plugin contracts (`06`).

Implementers must not add Tobii- or MediaPipe-specific code to UCA's core, first-party plugins, templates or tests. The guides contain code **snippets** as documentation; they are not compiled into UCA.

## What UCA ships instead

| Item | Location | Purpose |
| --- | --- | --- |
| Guide: building an eye-tracker plugin for Tobii Eye Tracker 5 | `docs/plugin-guides/tobii-stream-engine.md` | step-by-step, contents below |
| Guide: building a facial-expression plugin with MediaPipe | `docs/plugin-guides/mediapipe-face.md` | step-by-step, contents below |
| Guide: plugin basics | `docs/plugin-guides/getting-started.md` | manifest, handshake, contracts, testing with the mock host, packaging, installing into UCA |
| Template: point provider | `examples/plugins/point-provider-template/` (Rust) | compiles and runs with UCA; emits a scripted gaze path; the Tobii guide starts from it |
| Template: analog trigger provider | `examples/plugins/analog-trigger-template/` (Rust) | compiles and runs; emits scripted blendshape-style scores; the MediaPipe guide starts from it |
| Guide: virtual gamepad plugins | `docs/plugin-guides/virtual-gamepad.md` | Linux `uinput`, Windows input-injection gamepad, vJoy and ViGEmBus: set-up, pros and cons, safety rules (contents below) |
| Template: gamepad output | `examples/plugins/gamepad-output-template/` (Rust) | compiles and runs; logs pad commands and enforces the 300 ms neutral rule; starting point for gamepad plugins |
| Template: predictor | `examples/plugins/predictor-template/` (Rust) | compiles and runs; answers `Predict` from a small word list; starting point for prediction plugins (`06`, `16`) |
| Mock host | `uca-plugin-sdk` (`06`) | lets authors test a plugin without running UCA |

The templates are vendor-neutral: no Tobii or MediaPipe names, symbols or files. CI builds and tests them on every change, so the guides' starting point always works.

## Conventions the guides rely on (part of UCA's public API)

- **Facial expression ids.** A trigger-provider whose manifest declares `trigger_namespace: face` has its analog ids exposed as `face:<id>`.
    - Ids should use the 52 ARKit-style blendshape names that MediaPipe also uses (`eyeBlinkLeft`, `jawOpen`, `browInnerUp`, …).
    - UCA's expression UI, default thresholds and the assessment's expression test (`08`) recognise these names. Any plugin using them (MediaPipe or another face tracker) works the same way.
- **Head pose.** New contract `head-provider` (`06`): P→H `Head {t_ns, yaw, pitch, roll (radians), x?, y?, z? (metres), confidence}`. Used for mouse-look (`12`). A face plugin can provide both `trigger-provider` and `head-provider`.
- **Presence.** Any plugin can send `Status{state: user_lost | user_found}`. UCA pauses dwell and scanning on `user_lost` (SAFETY-03).
- **Privacy rule for camera plugins.** The plugin protocol has no message type for images, so camera frames cannot reach UCA. Guides require authors not to store, log or send frames anywhere, and to declare `permissions: [camera]` in the manifest.

## Guide requirements (both guides)

Each guide must:
- **Audience:** be written for a developer who knows Rust basics but not UCA.
- **Structure:** follow numbered steps, each with what to do, a code snippet, and how to check it worked.
- **Starting point:** start from the matching template, then replace the scripted data with the real device or library.
- **Testing:** cover testing with the mock host, then installing into UCA (`<config>/plugins/<id>/`) and checking it in Settings → Plugins and the debug overlay (`11`).
- **Third-party terms:** tell the author to read and follow the vendor's terms (Tobii's API terms; Apache-2.0 notices for libmediapipe and Google's model). State that redistributing vendor files is the author's responsibility, and that UCA neither checks nor grants any rights.
- **Licence:** say that a plugin may use any licence. The plugin SDK and templates are `MIT OR Apache-2.0`, and UCA's GPL does not extend to plugins that only use the plugin protocol (`18`).
- **Pinning:** name the exact library or API versions the guide was last verified with, and the date.
- **Troubleshooting:** end with a troubleshooting section and a "publishing your plugin" section (zip layout, checksums, README).

**Keeping guides accurate.** Before each UCA release that changes the plugin protocol, and at least every 6 months, a maintainer follows each guide on a clean machine and records the result (date, UCA version, vendor versions) at the top of the guide. A guide that fails is marked "needs update" until fixed.

## Contents: Tobii guide (`tobii-stream-engine.md`)

Steps the guide walks through (facts below come from UCA's earlier design work; verify against current Tobii docs when writing):

1. **Prerequisites.**
    - Tobii Eye Tracker 5 with Tobii's own software installed and calibrated.
    - Windows. Tobii does not support the ET5 on macOS.
2. **Start from `point-provider-template`.** Set `id`, `platforms: [windows]`, `provides: [point-provider]`, plus `head-provider` if adding head pose.
3. **Don't bundle Tobii files.** Recommend loading the Stream Engine library the user already has installed, at runtime with `libloading`, and writing FFI declarations only for the public functions used.
4. **Find the library**, first hit wins:
    1. `library_path` setting
    2. an environment variable chosen by the author
    3. default OS search for `tobii_stream_engine.dll`
    4. Tobii's install folders, searched recursively

   For each candidate:
    - skip it if the architecture doesn't match (64-bit plugin needs a 64-bit DLL)
    - resolve every symbol and check the API version
    - log the path and version

   The guide tells authors to confirm the install folders on their own machine; they change between Tobii software versions.
5. **Use the public API.** Names come from Tobii's public Stream Engine headers; confirm them against the installed version:
    - `tobii_get_api_version`, `tobii_api_create` / `_destroy`
    - `tobii_enumerate_local_device_urls` (device choice setting), `tobii_device_create` / `_destroy`
    - `tobii_gaze_point_subscribe` → `PointSample` with `space: normalized` over the tracker's display
    - `tobii_user_presence_subscribe` → `presence`
    - `tobii_wait_for_callbacks` + `tobii_device_process_callbacks` on a dedicated thread
    - `tobii_device_reconnect` on connection loss
    - head pose, if the installed API version provides it
6. **Map to UCA.** Valid gaze → confidence 1.0, invalid → drop the sample. Calibration is `delegated` (Tobii's own software). Coverage is the display the tracker is set up on.
7. **Handle "not found".**
    - The plugin still starts, sends `Status{state: error}` with a plain-language message and a Retry action, and never crashes.
    - Example message: "Tobii software not found. Install it for your Eye Tracker 5, then press Retry."
8. **Testing.**
    - A mock library (a small test `cdylib` exporting the same symbols) drives gaze, presence and reconnect.
    - Discovery tests use a fake filesystem.
9. **Packaging.** Plugin binary + `plugin.yaml` + README; no Tobii files.

## Contents: MediaPipe face guide (`mediapipe-face.md`)

1. **Prerequisites.**
    - A webcam.
    - The MediaPipe Tasks C API library. The guide shows prebuilt binaries from [tqphan/libmediapipe](https://github.com/tqphan/libmediapipe) as one option. Those builds come from MediaPipe's public source without the usage-telemetry logger found in the PyPI wheels.
    - Platforms with prebuilt zips: Windows x86_64 (Windows 10+), Linux x86_64 (glibc 2.39+), macOS arm64 (macOS 12+). None for Windows ARM64, Linux ARM or Intel Macs.
2. **Start from `analog-trigger-template`.** Set `provides: [trigger-provider, head-provider]`, `trigger_namespace: face`, `permissions: [camera]`.
3. **Pin dependencies.**
    - Record the libmediapipe release tag, the per-zip SHA256 and the MediaPipe commit from `SOURCE.txt`. Verify downloads against the checksums.
    - Load the library only from the plugin's own folder, by absolute path, never from `PATH`.
4. **Get the model.**
    - Google's `face_landmarker.task` (float16): BlazeFace short-range detector, FaceMesh V2 (478 landmarks), Blendshape V2 (52 scores in [0, 1]).
    - Pin a numbered URL plus SHA256, not the `latest` URL.
    - The FaceMesh V2 and Blendshape V2 model cards state Apache-2.0. The guide tells authors to check all three model cards, including BlazeFace, and include the notices.
5. **FFI.**
    - Generate bindings with `bindgen` from the MediaPipe Tasks C API headers at the commit in `SOURCE.txt`.
    - Required symbols: `MpFaceLandmarkerCreate`, `MpFaceLandmarkerDetectForVideo`, `MpFaceLandmarkerCloseResult`, `MpFaceLandmarkerClose`, `MpImageCreateFromUint8Data`, `MpImageFree`, `MpErrorFree`.
    - A missing symbol → error status naming it.
6. **Options.**
    - running mode VIDEO
    - `num_faces: 1` (MediaPipe's smoothing only works with one face)
    - `output_face_blendshapes: true` (MediaPipe's default is off)
    - confidences 0.5
    - `output_facial_transformation_matrixes: true` for head pose
7. **Per-frame loop** (capture thread):
    1. Grab a frame (e.g. with `nokhwa`, 640×480, 30 fps) and convert it to RGB8.
    2. `MpImageCreateFromUint8Data`, then `MpFaceLandmarkerDetectForVideo` with increasing ms timestamps.
    3. With a face: send one `Analog` with the 52 scores, plus `Head` from the transformation matrix.
    4. No face for 500 ms: send `user_lost`.
    5. Always close results and free images and errors, with RAII guards. Drop frames rather than queueing them.
8. **Privacy.** Frames stay in the plugin process; never stored, logged or sent.
9. **Known model limits** to put in the plugin's README:
    - designed for front-facing phone cameras and AR entertainment
    - face within about 80° of the camera and at least half visible
    - jitter in poor light or with motion
    - fairness testing did not cover facial differences such as palsy, weakness or masks, so per-user thresholds from UCA's assessment matter
    - `cheekPuff` and `tongueOut` come from FaceMesh, not the blendshape model
10. **Testing.**
    - Mock host plus a short recorded clip of the author's own face.
    - A leak check over 10,000 frames.
    - The no-face clip → `user_lost` within 600 ms.
11. **Packaging.** Plugin binary, library files, model, licence notices, `plugin.yaml`, README.

## Contents: virtual gamepad guide (`virtual-gamepad.md`)

1. **Start from `gamepad-output-template`.** Implement `gamepad-output` (`06`) and keep its safety rules: return to neutral on stop, disconnect or 300 ms of silence.
2. **Linux `uinput`.** No driver. Create a virtual device with gamepad capabilities (buttons, `ABS_X/Y/RX/RY/Z/RZ`, hat). Explain the `/dev/uinput` permission (udev rule or group). Games and Steam see it through evdev / SDL.
3. **Windows input-injection gamepad.** `InputInjector.InitializeGamepadInjection` / `InjectGamepadInput` are built into Windows 10 1709+. They need the `inputInjectionBrokered` restricted capability, so the plugin must be an MSIX-packaged app.
    - Verify and record which games actually receive this input (Windows.Gaming.Input / GameInput titles vs XInput titles) before recommending it.
4. **vJoy.** A DirectInput joystick driver the user installs. The common fork's owner says it gets limited maintenance, so check for a signed release. XInput-only games need Steam Input or a translator.
5. **ViGEmBus.** XInput / DS4 emulation. Retired in 2023: existing installs keep working, but it's an unmaintained kernel driver. The guide states this plainly and leaves the choice to the user.
6. **Testing.** With the mock host, check button and axis mapping, the 300 ms neutral rule, and reconnect.
7. **Packaging.** UCA does not distribute drivers. The plugin's README tells users what to install and where from.

## Acceptance (milestone M2)

- Both templates build, pass their tests with the mock host, and load into UCA in CI.
- A team member who did not write the guide follows each guide on a clean machine and gets a working plugin. Record the outcome in the guide.
- The repository, installers and release assets contain no Tobii or MediaPipe code, bindings, libraries, headers or models. CI fails the build if any file name or content matches `tobii` or `mediapipe` / `libmediapipe` / `face_landmarker`, except under `docs/plugin-guides/` and the design spec documents.
