//! Decoding to interleaved `f32` samples with Symphonia.

use std::io::Cursor;

use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::{MediaSource, MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::MetadataOptions;

use crate::Error;

/// Decoded audio at its own rate and channel count.
pub(crate) struct Pcm {
    pub interleaved: Vec<f32>,
    pub channels: usize,
    pub rate: u32,
}

pub(crate) fn decode_bytes(bytes: Vec<u8>) -> Result<Pcm, Error> {
    decode(Box::new(Cursor::new(bytes)), None)
}

pub(crate) fn decode(source: Box<dyn MediaSource>, extension: Option<&str>) -> Result<Pcm, Error> {
    let err = |m: String| Error::Decode(m);
    let mss = MediaSourceStream::new(source, MediaSourceStreamOptions::default());
    let mut hint = Hint::new();
    if let Some(ext) = extension {
        hint.with_extension(ext);
    }
    let mut format = symphonia::default::get_probe()
        .probe(&hint, mss, FormatOptions::default(), MetadataOptions::default())
        .map_err(|e| err(format!("probe: {e}")))?;
    let track = format.default_track(TrackType::Audio).ok_or_else(|| err("no audio track".into()))?;
    let track_id = track.id;
    let params =
        track.codec_params.as_ref().and_then(|p| p.audio()).ok_or_else(|| err("no audio codec params".into()))?.clone();
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(&params, &AudioDecoderOptions::default())
        .map_err(|e| err(format!("decoder: {e}")))?;

    let mut out = Vec::new();
    let mut chunk: Vec<f32> = Vec::new();
    let (mut channels, mut rate) = (0usize, 0u32);
    loop {
        let packet = match format.next_packet() {
            Ok(Some(p)) => p,
            Ok(None) => break,
            // A truncated tail is common in cut previews; keep what decoded.
            Err(SymError::IoError(_)) => break,
            Err(e) => return Err(err(format!("demux: {e}"))),
        };
        if packet.track_id != track_id {
            continue;
        }
        let buf = match decoder.decode(&packet) {
            Ok(b) => b,
            Err(SymError::DecodeError(_)) => continue,
            Err(e) => return Err(err(format!("decode: {e}"))),
        };
        // Header rate/channels can lie; trust the first decoded frame.
        if channels == 0 {
            channels = buf.spec().channels().count();
            rate = buf.spec().rate();
        }
        buf.copy_to_vec_interleaved(&mut chunk);
        out.extend_from_slice(&chunk);
    }
    if channels == 0 || out.is_empty() {
        return Err(err("no audio decoded".into()));
    }
    Ok(Pcm { interleaved: out, channels, rate })
}
