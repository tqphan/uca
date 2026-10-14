// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! WASAPI shared-mode playback with automatic format conversion, so Windows
//! resamples the voice to the device rate (13, decided).

use std::time::Duration;

use windows::core::{Interface, PCWSTR};
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Media::Audio::{
    eConsole, eRender, IAudioClient, IAudioRenderClient, IMMDevice, IMMDeviceEnumerator,
    MMDeviceEnumerator, AUDCLNT_E_DEVICE_INVALIDATED, AUDCLNT_SHAREMODE_SHARED,
    AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM, AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
    DEVICE_STATE_ACTIVE, WAVEFORMATEX,
};
use windows::Win32::Media::Multimedia::WAVE_FORMAT_IEEE_FLOAT;
use windows::Win32::System::Com::StructuredStorage::PropVariantClear;
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL, STGM_READ};
use windows::Win32::System::Variant::VT_LPWSTR;

use uca_config::profile::AudioDevice;

use crate::audio::{AudioError, AudioOutput, AudioSink};
use crate::tts::PcmBuffer;

/// The shared-mode buffer asked for, in 100 ns units (100 ms).
const BUFFER_100NS: i64 = 1_000_000;
/// How often playback checks for a stop and refills the buffer.
const POLL: Duration = Duration::from_millis(10);

fn failed(e: windows::core::Error) -> AudioError {
    if e.code() == AUDCLNT_E_DEVICE_INVALIDATED {
        AudioError::Gone
    } else {
        AudioError::Failed(e.message().to_string())
    }
}

pub struct WasapiOutput {
    enumerator: IMMDeviceEnumerator,
}

impl WasapiOutput {
    /// Creates the device enumerator on the calling thread (the player thread).
    pub fn new() -> Result<Self, AudioError> {
        super::com_init();
        // SAFETY: COM is initialised on this thread.
        let enumerator =
            unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }.map_err(failed)?;
        Ok(Self { enumerator })
    }

    /// Active output devices with their friendly names.
    fn active(&self) -> Vec<(String, IMMDevice)> {
        // SAFETY: plain COM calls on objects owned by this thread.
        let list = match unsafe {
            self.enumerator
                .EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)
        } {
            Ok(l) => l,
            Err(e) => {
                tracing::warn!(target: "uca::voice", error = %e.message(), "cannot list audio devices");
                return Vec::new();
            }
        };
        let count = unsafe { list.GetCount() }.unwrap_or(0);
        (0..count)
            .filter_map(|i| {
                let device = unsafe { list.Item(i) }.ok()?;
                Some((friendly_name(&device)?, device))
            })
            .collect()
    }

    fn find(&self, device: &AudioDevice) -> Result<IMMDevice, AudioError> {
        match device {
            // SAFETY: a plain COM call.
            AudioDevice::Default => {
                unsafe { self.enumerator.GetDefaultAudioEndpoint(eRender, eConsole) }
                    .map_err(|_| AudioError::NotFound)
            }
            AudioDevice::Named(name) => self
                .active()
                .into_iter()
                .find(|(n, _)| n == name)
                .map(|(_, d)| d)
                .ok_or(AudioError::NotFound),
        }
    }
}

fn friendly_name(device: &IMMDevice) -> Option<String> {
    // SAFETY: reading a string property; the string is copied before the
    // PROPVARIANT is cleared.
    unsafe {
        let store = device.OpenPropertyStore(STGM_READ).ok()?;
        let mut value = store.GetValue(&PKEY_Device_FriendlyName).ok()?;
        let name = if value.Anonymous.Anonymous.vt == VT_LPWSTR {
            PCWSTR(value.Anonymous.Anonymous.Anonymous.pwszVal.0)
                .to_string()
                .ok()
        } else {
            None
        };
        let _ = PropVariantClear(&mut value);
        name
    }
}

impl AudioOutput for WasapiOutput {
    fn devices(&self) -> Vec<String> {
        self.active().into_iter().map(|(n, _)| n).collect()
    }

    fn open(&mut self, device: &AudioDevice) -> Result<Box<dyn AudioSink>, AudioError> {
        // Checked now, so a missing speaker falls back before the first sentence.
        self.find(device)?;
        Ok(Box::new(WasapiSink {
            enumerator: self.enumerator.clone(),
            device: device.clone(),
        }))
    }
}

/// Finds its device again for each sentence, so "default" follows the
/// system default and an unplugged device is noticed (13).
struct WasapiSink {
    enumerator: IMMDeviceEnumerator,
    device: AudioDevice,
}

// SAFETY: the sink is created and used only on the player thread, which is
// in the multithreaded apartment; MTA COM objects may cross threads.
unsafe impl Send for WasapiSink {}
// SAFETY: as above.
unsafe impl Send for WasapiOutput {}

impl AudioSink for WasapiSink {
    fn play(&mut self, pcm: &PcmBuffer, stopped: &dyn Fn() -> bool) -> Result<(), AudioError> {
        if pcm.samples.is_empty() {
            return Ok(());
        }
        let output = WasapiOutput {
            enumerator: self.enumerator.clone(),
        };
        let device = output.find(&self.device).map_err(|_| AudioError::Gone)?;
        play(&device, pcm, stopped)
    }
}

fn play(device: &IMMDevice, pcm: &PcmBuffer, stopped: &dyn Fn() -> bool) -> Result<(), AudioError> {
    let format = WAVEFORMATEX {
        wFormatTag: WAVE_FORMAT_IEEE_FLOAT as u16,
        nChannels: 1,
        nSamplesPerSec: pcm.sample_rate,
        nAvgBytesPerSec: pcm.sample_rate * 4,
        nBlockAlign: 4,
        wBitsPerSample: 32,
        cbSize: 0,
    };
    // SAFETY: COM calls on this thread's objects. Buffers from GetBuffer are
    // written within the frame count asked for and released straight after.
    unsafe {
        let client: IAudioClient = device.Activate(CLSCTX_ALL, None).map_err(failed)?;
        client
            .Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
                BUFFER_100NS,
                0,
                &format,
                None,
            )
            .map_err(failed)?;
        let size = client.GetBufferSize().map_err(failed)? as usize;
        let render: IAudioRenderClient = client.GetService().map_err(failed)?;
        let mut written = 0usize;
        let mut started = false;
        loop {
            if stopped() {
                let _ = client.Stop();
                tracing::debug!(target: "uca::voice", "playback stopped");
                return Ok(());
            }
            let padding = client.GetCurrentPadding().map_err(failed)? as usize;
            let room = size.saturating_sub(padding);
            let left = pcm.samples.len() - written;
            if left == 0 && padding == 0 {
                break;
            }
            let n = room.min(left);
            if n > 0 {
                let buf = render.GetBuffer(n as u32).map_err(failed)?;
                std::ptr::copy_nonoverlapping(
                    pcm.samples[written..].as_ptr(),
                    buf.cast::<f32>(),
                    n,
                );
                render.ReleaseBuffer(n as u32, 0).map_err(failed)?;
                written += n;
            }
            if !started {
                client.Start().map_err(failed)?;
                started = true;
            }
            std::thread::sleep(POLL);
        }
        let _ = client.Stop();
        // Keep the interface alive until here.
        let _ = render.as_raw();
    }
    Ok(())
}
