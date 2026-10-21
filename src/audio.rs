//! Optional procedural tunnel audio. The callback never locks or allocates.
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};

pub struct TunnelAudio {
    _stream: cpal::Stream,
    gain: Arc<AtomicU32>,
    buffet: Arc<AtomicU32>,
}
impl TunnelAudio {
    pub fn start() -> Result<Self, String> {
        let device = cpal::default_host()
            .default_output_device()
            .ok_or("Нет звукового устройства")?;
        let supported = device.default_output_config().map_err(|e| e.to_string())?;
        let config: cpal::StreamConfig = supported.clone().into();
        let gain = Arc::new(AtomicU32::new(0.0_f32.to_bits()));
        let buffet = Arc::new(AtomicU32::new(0.0_f32.to_bits()));
        let stream = match supported.sample_format() {
            cpal::SampleFormat::F32 => build::<f32>(&device, &config, gain.clone(), buffet.clone()),
            cpal::SampleFormat::I16 => build::<i16>(&device, &config, gain.clone(), buffet.clone()),
            cpal::SampleFormat::U16 => build::<u16>(&device, &config, gain.clone(), buffet.clone()),
            _ => return Err("Неподдерживаемый формат звукового устройства".into()),
        }?;
        stream.play().map_err(|e| e.to_string())?;
        Ok(Self {
            _stream: stream,
            gain,
            buffet,
        })
    }
    pub fn update(&self, velocity: f64, stall: f64, volume: f32) {
        self.gain.store(
            (volume * (velocity / 200.0).clamp(0.0, 1.0) as f32 * 0.2).to_bits(),
            Ordering::Relaxed,
        );
        self.buffet
            .store((stall as f32).to_bits(), Ordering::Relaxed);
    }
}
fn build<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    gain: Arc<AtomicU32>,
    buffet: Arc<AtomicU32>,
) -> Result<cpal::Stream, String>
where
    T: cpal::SizedSample + cpal::FromSample<f32>,
{
    let channels = config.channels as usize;
    let sample_rate = config.sample_rate.0 as f32;
    let mut random = 0x5a17c9e3_u32;
    let mut filtered = 0.0;
    let mut phase = 0.0_f32;
    let mut smooth_gain = 0.0;
    device
        .build_output_stream(
            config,
            move |output: &mut [T], _| {
                let target = f32::from_bits(gain.load(Ordering::Relaxed));
                let stall = f32::from_bits(buffet.load(Ordering::Relaxed));
                for frame in output.chunks_mut(channels) {
                    random ^= random << 13;
                    random ^= random >> 17;
                    random ^= random << 5;
                    let noise = random as f32 / u32::MAX as f32 * 2.0 - 1.0;
                    filtered = 0.93 * filtered + 0.07 * noise;
                    smooth_gain += 0.001 * (target - smooth_gain);
                    phase = (phase + std::f32::consts::TAU * 38.0 / sample_rate)
                        % std::f32::consts::TAU;
                    let value = (filtered * 3.0 + stall * phase.sin() * 0.3) * smooth_gain;
                    for sample in frame {
                        *sample = T::from_sample(value);
                    }
                }
            },
            |error| eprintln!("Audio stream: {error}"),
            None,
        )
        .map_err(|e| e.to_string())
}
