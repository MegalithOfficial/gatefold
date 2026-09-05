use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use cpal::{
    BufferSize, SampleRate, Stream, StreamConfig, SupportedBufferSize,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use librespot::playback::{
    NUM_CHANNELS, SAMPLE_RATE,
    audio_backend::{Sink, SinkError, SinkResult},
    convert::Converter,
    decoder::AudioPacket,
};
use rtrb::{Consumer, Producer, RingBuffer};
use rubato::{
    Async, FixedAsync, PolynomialDegree, Resampler, audioadapter_buffers::direct::InterleavedSlice,
};

const RING: Duration = Duration::from_millis(500);
const DEVICE_BUFFER: Duration = Duration::from_millis(100);
const RESAMPLE_CHUNK: usize = 1024;
const BACKOFF: Duration = Duration::from_millis(2);

#[derive(Default)]
struct Clock {
    rate: AtomicU32,
    written: AtomicU64,
    consumed: AtomicU64,
    discard_until: AtomicU64,
    latency_ns: AtomicU64,
    callback_ns: AtomicU64,
    period_ns: AtomicU64,
    paused: AtomicBool,
}

#[derive(Clone)]
pub struct SinkHandle {
    clock: Arc<Clock>,
    epoch: Instant,
}

impl Default for SinkHandle {
    fn default() -> Self {
        Self {
            clock: Arc::default(),
            epoch: Instant::now(),
        }
    }
}

impl SinkHandle {
    pub fn flush(&self) {
        let written = self.clock.written.load(Ordering::Acquire);
        self.clock.discard_until.store(written, Ordering::Release);
    }

    pub fn clocked(&self) -> bool {
        self.clock.rate.load(Ordering::Relaxed) != 0
    }

    pub fn written(&self) -> u64 {
        self.clock.written.load(Ordering::Acquire)
    }

    pub fn played_ms(&self, since: u64) -> Option<f64> {
        let clock = &self.clock;
        let rate = clock.rate.load(Ordering::Relaxed);
        if rate == 0 {
            return None;
        }
        let consumed = clock
            .consumed
            .load(Ordering::Acquire)
            .max(clock.discard_until.load(Ordering::Acquire));
        let elapsed = if clock.paused.load(Ordering::Relaxed) {
            0
        } else {
            (self.epoch.elapsed().as_nanos() as u64)
                .saturating_sub(clock.callback_ns.load(Ordering::Relaxed))
                .min(clock.period_ns.load(Ordering::Relaxed))
        };
        let latency = clock.latency_ns.load(Ordering::Relaxed);
        let frames = consumed as f64 - since as f64;

        Some(frames * 1000.0 / rate as f64 + (elapsed as f64 - latency as f64) / 1e6)
    }
}

pub struct Output {
    clock: Arc<Clock>,
    producer: Producer<f32>,
    resampler: Option<(Async<f32>, Vec<f32>)>,
    _stream: Stream,
}

fn refused(error: impl std::fmt::Display) -> SinkError {
    SinkError::ConnectionRefused(error.to_string())
}

fn bounded_buffer(config: &cpal::SupportedStreamConfig) -> BufferSize {
    let SupportedBufferSize::Range { min, max } = config.buffer_size() else {
        return BufferSize::Default;
    };
    let wanted = config.sample_rate().0 * DEVICE_BUFFER.as_millis() as u32 / 1000;
    BufferSize::Fixed(wanted.next_power_of_two().clamp(*min, *max))
}

fn ring(config: &cpal::SupportedStreamConfig) -> (Producer<f32>, Consumer<f32>) {
    let frames = config.sample_rate().0 as usize * RING.as_millis() as usize / 1000;
    RingBuffer::new(frames * NUM_CHANNELS as usize)
}

impl Output {
    pub fn open(handle: &SinkHandle) -> SinkResult<Self> {
        let device = cpal::default_host()
            .default_output_device()
            .ok_or_else(|| refused("no default audio device"))?;
        let default_config = device.default_output_config().map_err(refused)?;
        let preferred = device
            .supported_output_configs()
            .map_err(refused)?
            .find(|config| config.channels() == NUM_CHANNELS as cpal::ChannelCount)
            .and_then(|config| {
                config
                    .try_with_sample_rate(SampleRate(SAMPLE_RATE))
                    .or_else(|| config.try_with_sample_rate(default_config.sample_rate()))
            })
            .unwrap_or_else(|| default_config.clone());

        let clock = handle.clock.clone();
        let attempts = [
            (preferred.clone(), bounded_buffer(&preferred)),
            (preferred, BufferSize::Default),
            (default_config, BufferSize::Default),
        ];
        let mut opened = None;
        for (config, buffer_size) in attempts {
            let (producer, consumer) = ring(&config);
            match Self::stream(
                &device,
                &config,
                buffer_size,
                consumer,
                clock.clone(),
                handle.epoch,
            ) {
                Ok(stream) => {
                    opened = Some((stream, producer, config));
                    break;
                }
                Err(error) => tracing::warn!("audio output: {error}"),
            }
        }
        let (stream, producer, config) =
            opened.ok_or_else(|| refused("no usable audio output configuration"))?;
        stream.play().map_err(refused)?;

        let rate = config.sample_rate().0;
        tracing::info!("audio output: {} ch at {rate} Hz", config.channels());
        clock.rate.store(rate, Ordering::Relaxed);
        let resampler = (rate != SAMPLE_RATE)
            .then(|| {
                Async::<f32>::new_poly(
                    rate as f64 / SAMPLE_RATE as f64,
                    1.0,
                    PolynomialDegree::Cubic,
                    RESAMPLE_CHUNK,
                    NUM_CHANNELS as usize,
                    FixedAsync::Input,
                )
            })
            .transpose()
            .map_err(refused)?
            .map(|resampler| (resampler, Vec::new()));

        Ok(Self {
            clock,
            producer,
            resampler,
            _stream: stream,
        })
    }

    fn stream(
        device: &cpal::Device,
        config: &cpal::SupportedStreamConfig,
        buffer_size: BufferSize,
        mut ring: Consumer<f32>,
        clock: Arc<Clock>,
        epoch: Instant,
    ) -> Result<Stream, cpal::BuildStreamError> {
        let channels = config.channels() as usize;
        let rate = config.sample_rate().0 as u64;
        let stream_config = StreamConfig {
            channels: config.channels(),
            sample_rate: config.sample_rate(),
            buffer_size,
        };
        device.build_output_stream(
            &stream_config,
            move |data: &mut [f32], info: &cpal::OutputCallbackInfo| {
                let stamp = info.timestamp();
                let latency = stamp
                    .playback
                    .duration_since(&stamp.callback)
                    .unwrap_or_default();
                clock
                    .latency_ns
                    .store(latency.as_nanos() as u64, Ordering::Relaxed);
                clock
                    .callback_ns
                    .store(epoch.elapsed().as_nanos() as u64, Ordering::Relaxed);
                let frames = data.len() / channels;
                clock
                    .period_ns
                    .store(frames as u64 * 1_000_000_000 / rate, Ordering::Relaxed);

                let mut consumed = clock.consumed.load(Ordering::Acquire);
                let discard_until = clock.discard_until.load(Ordering::Acquire);
                if consumed < discard_until {
                    let stale = ((discard_until - consumed) as usize * NUM_CHANNELS as usize)
                        .min(ring.slots());
                    if let Ok(chunk) = ring.read_chunk(stale) {
                        chunk.commit_all();
                        consumed += (stale / NUM_CHANNELS as usize) as u64;
                    }
                }

                if clock.paused.load(Ordering::Relaxed) {
                    data.fill(0.0);
                    clock.consumed.store(consumed, Ordering::Release);
                    return;
                }

                let got = if channels == NUM_CHANNELS as usize {
                    let (filled, rest) = ring.pop_partial_slice(data);
                    rest.fill(0.0);
                    filled.len() / channels
                } else {
                    let mut got = 0;
                    for frame in data.chunks_mut(channels) {
                        let (Ok(left), Ok(right)) = (ring.pop(), ring.pop()) else {
                            frame.fill(0.0);
                            continue;
                        };
                        got += 1;
                        if channels == 1 {
                            frame[0] = (left + right) / 2.0;
                        } else {
                            frame.fill(0.0);
                            frame[0] = left;
                            frame[1] = right;
                        }
                    }
                    got
                };
                clock
                    .consumed
                    .store(consumed + got as u64, Ordering::Release);
            },
            |error| tracing::warn!("audio output: {error}"),
            None,
        )
    }

    fn push(&mut self, mut samples: &[f32]) {
        while !samples.is_empty() {
            let (pushed, rest) = self.producer.push_partial_slice(samples);
            self.clock.written.fetch_add(
                (pushed.len() / NUM_CHANNELS as usize) as u64,
                Ordering::Release,
            );
            samples = rest;
            if !samples.is_empty() {
                thread::sleep(BACKOFF);
            }
        }
    }
}

impl Sink for Output {
    fn start(&mut self) -> SinkResult<()> {
        self.clock.paused.store(false, Ordering::Relaxed);

        Ok(())
    }

    fn stop(&mut self) -> SinkResult<()> {
        self.clock.paused.store(true, Ordering::Relaxed);

        Ok(())
    }

    fn write(&mut self, packet: AudioPacket, converter: &mut Converter) -> SinkResult<()> {
        let samples = packet
            .samples()
            .map_err(|error| SinkError::OnWrite(error.to_string()))?;
        let samples = converter.f64_to_f32(samples);

        let Some((resampler, pending)) = &mut self.resampler else {
            self.push(&samples);
            return Ok(());
        };
        pending.extend_from_slice(&samples);
        let chunk = RESAMPLE_CHUNK * NUM_CHANNELS as usize;
        let mut resampled = Vec::new();
        while pending.len() >= chunk {
            let input =
                InterleavedSlice::new(&pending[..chunk], NUM_CHANNELS as usize, RESAMPLE_CHUNK)
                    .map_err(|error| SinkError::OnWrite(error.to_string()))?;
            let output = resampler
                .process(&input, None)
                .map_err(|error| SinkError::OnWrite(error.to_string()))?;
            resampled.extend(output.take_data());
            pending.drain(..chunk);
        }
        self.push(&resampled);

        Ok(())
    }
}
