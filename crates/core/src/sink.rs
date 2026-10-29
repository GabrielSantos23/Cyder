//! Saída de áudio com pouco buffer. A do librespot (rodio) guarda ~0,5 s e, ao pausar ou trocar de faixa,
//! espera esse buffer tocar inteiro antes de parar: o "atraso de 1 segundo" nos botões.
use std::{thread, time::Duration};

use librespot::playback::{
    NUM_CHANNELS, SAMPLE_RATE,
    audio_backend::{Sink, SinkError, SinkResult},
    convert::Converter,
    decoder::AudioPacket,
};

/// Pedaços na fila do rodio. Cada um tem ~20 ms; 10 ≈ 0,2 s, folga para o decodificador sem atrasar os botões.
const MAX_CHUNKS: usize = 10;

pub struct LowLatencySink {
    fx: crate::fx::Processor,
    sink: rodio::Sink,
    _stream: rodio::OutputStream,
}

pub fn open() -> Result<LowLatencySink, String> {
    let mut stream = rodio::OutputStreamBuilder::open_default_stream().map_err(|e| e.to_string())?;
    stream.log_on_drop(false);
    let sink = rodio::Sink::connect_new(stream.mixer());
    Ok(LowLatencySink { fx: Default::default(), sink, _stream: stream })
}

impl Sink for LowLatencySink {
    fn start(&mut self) -> SinkResult<()> {
        self.sink.play();
        Ok(())
    }

    /// Pausa na hora. O que sobrou no buffer (no máximo ~0,2 s) toca ao continuar, sem pular áudio.
    fn stop(&mut self) -> SinkResult<()> {
        self.sink.pause();
        Ok(())
    }

    fn write(&mut self, packet: AudioPacket, converter: &mut Converter) -> SinkResult<()> {
        let samples = packet.samples().map_err(|e| SinkError::OnWrite(e.to_string()))?;
        // Equalizador e fade entre músicas (`fx.rs`).
        let mut samples = samples.to_vec();
        self.fx.process(&mut samples, NUM_CHANNELS as usize);
        let samples_f32: &[f32] = &converter.f64_to_f32(&samples);
        self.sink.append(rodio::buffer::SamplesBuffer::new(NUM_CHANNELS as u16, SAMPLE_RATE, samples_f32));
        while self.sink.len() > MAX_CHUNKS {
            thread::sleep(Duration::from_millis(5));
        }
        Ok(())
    }
}
