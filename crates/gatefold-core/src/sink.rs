use std::{
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use librespot::playback::{
    NUM_CHANNELS, SAMPLE_RATE,
    audio_backend::{Sink, SinkError, SinkResult},
    convert::Converter,
    decoder::AudioPacket,
};
use rodio::cpal::{
    self, SampleRate,
    traits::{DeviceTrait, HostTrait},
};

const QUEUE_DEPTH: usize = 26;

#[derive(Clone, Default)]
pub struct SinkHandle(Arc<Mutex<Option<Arc<rodio::Sink>>>>);

impl SinkHandle {
    pub fn flush(&self) {
        if let Some(sink) = self.0.lock().unwrap().as_ref() {
            sink.clear();
            sink.play();
        }
    }
}

pub struct Rodio {
    sink: Arc<rodio::Sink>,
    _stream: rodio::OutputStream,
}

impl Rodio {
    pub fn open(handle: &SinkHandle) -> SinkResult<Self> {
        let device = cpal::default_host()
            .default_output_device()
            .ok_or_else(|| SinkError::ConnectionRefused("no default audio device".into()))?;

        let default_config = device
            .default_output_config()
            .map_err(|error| SinkError::ConnectionRefused(error.to_string()))?;
        let config = device
            .supported_output_configs()
            .map_err(|error| SinkError::ConnectionRefused(error.to_string()))?
            .find(|config| config.channels() == NUM_CHANNELS as cpal::ChannelCount)
            .and_then(|config| {
                config
                    .try_with_sample_rate(SampleRate(SAMPLE_RATE))
                    .or_else(|| config.try_with_sample_rate(default_config.sample_rate()))
            })
            .unwrap_or(default_config);
        tracing::info!(
            "audio output: {} ch at {} Hz",
            config.channels(),
            config.sample_rate().0
        );

        let mut stream = rodio::OutputStreamBuilder::default()
            .with_device(device.clone())
            .with_config(&config.config())
            .with_sample_format(cpal::SampleFormat::F32)
            .open_stream()
            .or_else(|error| {
                tracing::warn!("audio output fallback: {error}");
                rodio::OutputStreamBuilder::from_device(device)
                    .map_err(|error| SinkError::ConnectionRefused(error.to_string()))?
                    .open_stream_or_fallback()
                    .map_err(|error| SinkError::ConnectionRefused(error.to_string()))
            })?;
        stream.log_on_drop(false);

        let sink = Arc::new(rodio::Sink::connect_new(stream.mixer()));
        *handle.0.lock().unwrap() = Some(sink.clone());

        Ok(Self {
            sink,
            _stream: stream,
        })
    }
}

impl Sink for Rodio {
    fn start(&mut self) -> SinkResult<()> {
        self.sink.play();

        Ok(())
    }

    fn stop(&mut self) -> SinkResult<()> {
        self.sink.pause();

        Ok(())
    }

    fn write(&mut self, packet: AudioPacket, converter: &mut Converter) -> SinkResult<()> {
        let samples = packet
            .samples()
            .map_err(|error| SinkError::OnWrite(error.to_string()))?;
        let samples: &[f32] = &converter.f64_to_f32(samples);

        self.sink.append(rodio::buffer::SamplesBuffer::new(
            NUM_CHANNELS as u16,
            SAMPLE_RATE,
            samples,
        ));

        while self.sink.len() > QUEUE_DEPTH {
            thread::sleep(Duration::from_millis(10));
        }

        Ok(())
    }
}
