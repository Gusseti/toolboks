use cpal::traits::{DeviceTrait, StreamTrait};
use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc, Mutex,
    },
};

// Independent bounded buffers let headphones and the virtual cable have different clocks.
pub struct Bridge {
    frames: VecDeque<[f32; 2]>,
    phase: f64,
    ratio: f64,
    target: usize,
    limit: usize,
    ready: bool,
}

impl Bridge {
    pub fn new(input_rate: u32, output_rate: u32) -> Self {
        Self {
            frames: VecDeque::with_capacity(input_rate as usize / 5 + 2),
            phase: 0.0,
            ratio: input_rate as f64 / output_rate as f64,
            target: (input_rate as usize / 50).max(2),
            limit: (input_rate as usize / 10).max(4),
            ready: false,
        }
    }

    pub fn push(&mut self, frames: impl Iterator<Item = [f32; 2]>) {
        for frame in frames {
            if self.frames.len() >= self.limit {
                self.frames.pop_front();
                self.phase = 0.0;
            }
            self.frames.push_back(frame);
        }
    }

    fn next(&mut self) -> [f32; 2] {
        if !self.ready {
            if self.frames.len() < self.target {
                return [0.0; 2];
            }
            self.ready = true;
        }
        if self.frames.len() < 2 {
            self.ready = false;
            self.phase = 0.0;
            return [0.0; 2];
        }
        let a = self.frames[0];
        let b = self.frames[1];
        let p = self.phase as f32;
        let result = [a[0] + (b[0] - a[0]) * p, a[1] + (b[1] - a[1]) * p];
        // Small correction prevents growing latency when independent device clocks drift.
        let correction = ((self.frames.len() as f64 - self.target as f64) / self.target as f64
            * 0.001)
            .clamp(-0.005, 0.005);
        self.phase += self.ratio * (1.0 + correction);
        while self.phase >= 1.0 {
            if self.frames.pop_front().is_none() {
                self.phase = 0.0;
                self.ready = false;
                break;
            }
            self.phase -= 1.0;
        }
        result
    }
}

pub type Routes = Mutex<[Option<Arc<Mutex<Bridge>>>; 2]>;

pub fn forward<T: cpal::Sample>(data: &[T], channels: usize, gain: f32, routes: &Routes)
where
    f32: cpal::FromSample<T>,
{
    if let Ok(routes) = routes.try_lock() {
        for route in routes.iter().flatten() {
            if let Ok(mut bridge) = route.try_lock() {
                bridge.push(data.chunks_exact(channels).map(|f| {
                    let left = (f[0].to_sample::<f32>() * gain).clamp(-1.0, 1.0);
                    let right = if channels == 1 {
                        left
                    } else {
                        (f[1].to_sample::<f32>() * gain).clamp(-1.0, 1.0)
                    };
                    [left, right]
                }));
            }
        }
    }
}

fn render<T: cpal::Sample + cpal::FromSample<f32>>(
    data: &mut [T],
    channels: usize,
    bridge: &Mutex<Bridge>,
    level: &AtomicU32,
) {
    let g = f32::from_bits(level.load(Ordering::Relaxed));
    if let Ok(mut bridge) = bridge.try_lock() {
        for frame in data.chunks_mut(channels) {
            let sample = bridge.next();
            for (c, out) in frame.iter_mut().enumerate() {
                let value = if channels == 1 {
                    (sample[0] + sample[1]) * 0.5
                } else if c < 2 {
                    sample[c]
                } else {
                    0.0
                };
                *out = T::from_sample((value * g).clamp(-1.0, 1.0));
            }
        }
    } else {
        data.fill(T::from_sample(0.0));
    }
}

pub fn open(
    device: &cpal::Device,
    input_rate: u32,
    level: Arc<AtomicU32>,
    error: Arc<Mutex<Option<String>>>,
) -> Result<(cpal::Stream, Arc<Mutex<Bridge>>), String> {
    let supported = device.default_output_config().map_err(|e| e.to_string())?;
    let format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();
    let channels = config.channels as usize;
    if channels == 0 || config.sample_rate.0 == 0 {
        return Err("The output reported an invalid audio format.".into());
    }
    let bridge = Arc::new(Mutex::new(Bridge::new(input_rate, config.sample_rate.0)));
    macro_rules! build {
        ($t:ty) => {{
            let bridge = bridge.clone();
            let level = level.clone();
            let error = error.clone();
            device.build_output_stream(
                &config,
                move |data: &mut [$t], _| render(data, channels, &bridge, &level),
                move |e| {
                    *error.lock().unwrap() = Some(format!(
                        "Output disconnected: {e}. Enable this route again."
                    ));
                },
                None,
            )
        }};
    }
    let stream = match format {
        cpal::SampleFormat::F32 => build!(f32),
        cpal::SampleFormat::I16 => build!(i16),
        cpal::SampleFormat::U16 => build!(u16),
        cpal::SampleFormat::I32 => build!(i32),
        cpal::SampleFormat::F64 => build!(f64),
        cpal::SampleFormat::I8 => build!(i8),
        cpal::SampleFormat::U8 => build!(u8),
        cpal::SampleFormat::U32 => build!(u32),
        cpal::SampleFormat::I64 => build!(i64),
        cpal::SampleFormat::U64 => build!(u64),
        _ => return Err("Unsupported output sample format.".into()),
    }
    .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    Ok((stream, bridge))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resamples_and_bounds_latency() {
        let mut bridge = Bridge::new(48000, 44100);
        bridge.push(std::iter::repeat_n([0.25, -0.5], 48000));
        assert_eq!(bridge.frames.len(), 4800);
        for _ in 0..2000 {
            let f = bridge.next();
            assert!((f[0] - 0.25).abs() < 0.001);
            assert!((f[1] + 0.5).abs() < 0.001);
        }
        for _ in 0..10000 {
            bridge.next();
        }
        assert_eq!(bridge.next(), [0.0, 0.0]);
    }
    #[test]
    fn mono_routing_and_monitor_volume() {
        let bridge = Arc::new(Mutex::new(Bridge::new(100, 100)));
        let routes = Mutex::new([Some(bridge.clone()), None]);
        forward(&[0.25_f32, 0.5, 0.75], 1, 2.0, &routes);
        let mut output = [0.0_f32; 4];
        render(&mut output, 2, &bridge, &AtomicU32::new(0.5_f32.to_bits()));
        assert_eq!(&output[..2], &[0.25, 0.25]);
        assert!(output.iter().all(|v| v.is_finite() && v.abs() <= 0.5));
    }
    #[test]
    fn unsigned_silence_is_midpoint() {
        let bridge = Mutex::new(Bridge::new(48000, 48000));
        let mut data = [0_u16; 4];
        render(&mut data, 2, &bridge, &AtomicU32::new(1.0_f32.to_bits()));
        assert_eq!(data, [32768; 4]);
    }

    #[test]
    fn headphone_volume_does_not_change_the_cable_signal() {
        let headphones = Arc::new(Mutex::new(Bridge::new(100, 100)));
        let cable = Arc::new(Mutex::new(Bridge::new(100, 100)));
        let routes = Mutex::new([Some(headphones.clone()), Some(cable.clone())]);
        forward(&[0.25_f32; 8], 1, 2.0, &routes);
        let (mut heard, mut sent) = ([0.0_f32; 4], [0.0_f32; 4]);
        render(
            &mut heard,
            2,
            &headphones,
            &AtomicU32::new(0.5_f32.to_bits()),
        );
        render(&mut sent, 2, &cable, &AtomicU32::new(1.0_f32.to_bits()));
        assert_eq!(heard, [0.25; 4]);
        assert_eq!(sent, [0.5; 4]);
    }
}
