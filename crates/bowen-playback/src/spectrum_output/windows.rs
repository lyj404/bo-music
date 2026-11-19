use crate::spectrum::{SAMPLE_COUNT, SpectrumAnalyzer, SpectrumLevels};
use std::{
    num::{NonZeroU32, NonZeroUsize},
    ptr,
};
use windows::Win32::{
    Foundation::E_INVALIDARG,
    Media::Audio::{
        AUDCLNT_BUFFERFLAGS_DATA_DISCONTINUITY, AUDCLNT_BUFFERFLAGS_SILENT,
        AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK, IAudioCaptureClient, IAudioClient,
        IMMDeviceEnumerator, MMDeviceEnumerator, WAVEFORMATEX, WAVEFORMATEXTENSIBLE, eMultimedia,
        eRender,
    },
    System::Com::{
        CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
        CoUninitialize,
    },
};
use windows::core::{Error, GUID};

const FLOAT_SUBFORMAT: GUID = GUID::from_u128(0x00000003_0000_0010_8000_00aa00389b71);
const PCM_SUBFORMAT: GUID = GUID::from_u128(0x00000001_0000_0010_8000_00aa00389b71);

struct Apartment;

impl Apartment {
    fn open() -> windows::core::Result<Self> {
        // All COM interfaces below are created and released on this worker.
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok()? };
        Ok(Self)
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

pub(super) struct Capture {
    packets: IAudioCaptureClient,
    audio: IAudioClient,
    analyzer: SpectrumAnalyzer,
    samples: Vec<f32>,
    channels: NonZeroUsize,
    width: usize,
    floating_point: bool,
    _apartment: Apartment,
}

struct MixFormat(*mut WAVEFORMATEX);

impl Drop for MixFormat {
    fn drop(&mut self) {
        // GetMixFormat allocates this buffer with CoTaskMemAlloc.
        unsafe { CoTaskMemFree(Some(self.0.cast())) };
    }
}

fn pcm_sample(sample: &[u8], floating_point: bool) -> f32 {
    if floating_point {
        return f32::from_le_bytes(sample.try_into().unwrap());
    }
    // The bytes are left-aligned in a signed 32-bit word, including packed 24-bit PCM.
    let mut word = [0; 4];
    word[4 - sample.len()..].copy_from_slice(sample);
    i32::from_le_bytes(word) as f32 / 2_147_483_648.0
}

impl Capture {
    pub fn open() -> windows::core::Result<Self> {
        let apartment = Apartment::open()?;
        // The lifetime of each interface is confined to the initialized COM apartment.
        unsafe {
            let devices: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
            let endpoint = devices.GetDefaultAudioEndpoint(eRender, eMultimedia)?;
            let audio: IAudioClient = endpoint.Activate(CLSCTX_ALL, None)?;
            let format = MixFormat(audio.GetMixFormat()?);
            let invalid = || Error::from_hresult(E_INVALIDARG);
            let data = format.0.as_ref().ok_or_else(invalid)?;
            let subformat = match data.wFormatTag {
                1 => PCM_SUBFORMAT,
                3 => FLOAT_SUBFORMAT,
                0xfffe if data.cbSize >= 22 => (*format.0.cast::<WAVEFORMATEXTENSIBLE>()).SubFormat,
                _ => return Err(invalid()),
            };
            let (width, floating_point) = match (subformat, data.wBitsPerSample) {
                (FLOAT_SUBFORMAT, 32) => (4, true),
                (PCM_SUBFORMAT, bits @ (16 | 24 | 32)) => (usize::from(bits / 8), false),
                _ => return Err(invalid()),
            };
            let rate = NonZeroU32::new(data.nSamplesPerSec).ok_or_else(invalid)?;
            let channels = NonZeroUsize::new(usize::from(data.nChannels))
                .filter(|channels| channels.get() * width == usize::from(data.nBlockAlign))
                .ok_or_else(invalid)?;
            audio.Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                AUDCLNT_STREAMFLAGS_LOOPBACK,
                0,
                0,
                format.0,
                None,
            )?;
            let packets = audio.GetService()?;
            audio.Start()?;
            Ok(Self {
                audio,
                packets,
                analyzer: SpectrumAnalyzer::new(rate),
                samples: Vec::with_capacity(SAMPLE_COUNT * channels.get()),
                channels,
                width,
                floating_point,
                _apartment: apartment,
            })
        }
    }

    pub fn poll(&mut self) -> windows::core::Result<Option<SpectrumLevels>> {
        let mut received = false;
        // Cap work per wake-up so that a busy endpoint cannot starve stop requests.
        for _ in 0..16 {
            let mut data = ptr::null_mut();
            let mut frames = 0;
            let mut flags = 0;
            // GetBuffer and ReleaseBuffer are paired on this thread. The packet is copied before release.
            unsafe {
                if self.packets.GetNextPacketSize()? == 0 {
                    break;
                }
                self.packets
                    .GetBuffer(&mut data, &mut frames, &mut flags, None, None)?;
                if frames == 0 {
                    break;
                }
                if flags & AUDCLNT_BUFFERFLAGS_DATA_DISCONTINUITY.0 as u32 != 0 {
                    self.analyzer.reset();
                }
                let frames_to_copy = (frames as usize).min(SAMPLE_COUNT);
                let samples = frames_to_copy * self.channels.get();
                self.samples.clear();
                if data.is_null() || flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 {
                    self.samples.resize(samples, 0.0);
                } else {
                    let offset =
                        (frames as usize - frames_to_copy) * self.channels.get() * self.width;
                    let bytes = std::slice::from_raw_parts(data.add(offset), samples * self.width);
                    self.samples.extend(
                        bytes
                            .chunks_exact(self.width)
                            .map(|word| pcm_sample(word, self.floating_point)),
                    );
                }
                self.packets.ReleaseBuffer(frames)?;
            }
            self.analyzer.append(&self.samples, self.channels);
            received = true;
        }
        Ok(received.then(|| *self.analyzer.levels()))
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        // Stop before releasing the packet and audio client interfaces.
        let _ = unsafe { self.audio.Stop() };
    }
}
