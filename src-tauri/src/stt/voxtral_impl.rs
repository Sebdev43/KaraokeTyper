//! Voxtral backend implementation (feature-gated).
//!
//! Ports the orchestration logic from
//! [`voxtral-mini-realtime-rs`](https://github.com/TrevorS/voxtral-mini-realtime-rs)
//! `src/bin/voxtral/transcribe.rs` into a reusable library surface so the STT
//! dispatcher can drive it like any other backend.

use std::path::{Path, PathBuf};

use burn::backend::Wgpu;
use burn::prelude::ElementConversion;
use burn::tensor::{Int, Tensor, TensorData};

use voxtral_mini_realtime::audio::{
    chunk::{chunk_audio, needs_chunking, ChunkConfig},
    mel::{MelConfig, MelSpectrogram},
    pad::{pad_audio, PadConfig},
    AudioBuffer, AudioChunk,
};
use voxtral_mini_realtime::models::time_embedding::TimeEmbedding;
use voxtral_mini_realtime::tokenizer::VoxtralTokenizer;

use super::error::SttError;

type Backend = Wgpu;
type Device = <Backend as burn::tensor::backend::Backend>::Device;

const DEFAULT_DELAY_TOKENS: usize = 6; // 6 × 80ms ≈ 480ms lookahead (sweet spot)
const DEFAULT_MAX_MEL_FRAMES: usize = 1200;
const PREFIX_LEN: usize = 38;
const BOS_TOKEN: i32 = 1;
const STREAMING_PAD: i32 = 32;
const TOKENIZER_FILENAME: &str = "tekken.json";
const BF16_WEIGHTS_FILENAME: &str = "consolidated.safetensors";
const GGUF_CANDIDATES: &[&str] = &["voxtral-q4.gguf", "voxtral.gguf"];

#[allow(clippy::large_enum_variant)]
enum ModelState {
    Bf16(voxtral_mini_realtime::models::voxtral::VoxtralModel<Backend>),
    Q4(voxtral_mini_realtime::gguf::model::Q4VoxtralModel),
}

pub struct VoxtralEngine {
    model: ModelState,
    tokenizer: VoxtralTokenizer,
    mel_extractor: MelSpectrogram,
    pad_config: PadConfig,
    chunk_config: ChunkConfig,
    t_embed: Tensor<Backend, 3>,
    device: Device,
}

impl VoxtralEngine {
    /// Load a Voxtral engine from a model directory.
    ///
    /// Expects `model_dir` to contain `tekken.json` plus either a Q4 GGUF
    /// (`voxtral-q4.gguf` / `voxtral.gguf`) or the BF16 SafeTensors
    /// (`consolidated.safetensors`). Q4 is preferred when present: smaller
    /// (2.5 GB vs 9 GB) and real-time capable on modest GPUs.
    pub fn load(model_dir: &Path) -> Result<Self, SttError> {
        if !model_dir.is_dir() {
            return Err(SttError::ModelNotFound(model_dir.display().to_string()));
        }

        let tokenizer_path = model_dir.join(TOKENIZER_FILENAME);
        if !tokenizer_path.exists() {
            return Err(SttError::LoadError(format!(
                "tokenizer missing at {}",
                tokenizer_path.display()
            )));
        }
        let tokenizer = VoxtralTokenizer::from_file(&tokenizer_path)
            .map_err(|e| SttError::LoadError(format!("tokenizer: {e}")))?;

        let device = Device::default();

        let model = load_model(model_dir, &device)?;

        let mel_extractor = MelSpectrogram::new(MelConfig::voxtral());
        let pad_config = PadConfig::voxtral();
        let chunk_config = ChunkConfig::voxtral().with_max_frames(DEFAULT_MAX_MEL_FRAMES);
        let time_embed = TimeEmbedding::new(3072);
        let t_embed = time_embed.embed::<Backend>(DEFAULT_DELAY_TOKENS as f32, &device);

        Ok(Self {
            model,
            tokenizer,
            mel_extractor,
            pad_config,
            chunk_config,
            t_embed,
            device,
        })
    }

    /// Run transcription on 16 kHz mono PCM samples.
    pub fn transcribe(&mut self, samples: &[f32]) -> Result<String, SttError> {
        if samples.is_empty() {
            return Err(SttError::InvalidAudio("empty audio buffer".into()));
        }

        let mut audio = AudioBuffer::new(samples.to_vec(), 16_000);
        audio.peak_normalize(0.95);

        let chunks = if needs_chunking(audio.samples.len(), &self.chunk_config) {
            chunk_audio(&audio.samples, &self.chunk_config)
        } else {
            vec![AudioChunk {
                samples: audio.samples.clone(),
                start_sample: 0,
                end_sample: audio.samples.len(),
                index: 0,
                is_last: true,
            }]
        };

        let mut texts = Vec::new();
        for chunk in chunks {
            let chunk_audio = AudioBuffer::new(chunk.samples, 16_000);
            let mel_tensor = mel_tensor_from_audio(
                &chunk_audio,
                &self.mel_extractor,
                &self.pad_config,
                &self.device,
            )?;

            let generated = match &self.model {
                ModelState::Q4(model) => {
                    model.transcribe_streaming(mel_tensor, self.t_embed.clone())
                }
                ModelState::Bf16(model) => {
                    decode_bf16(model, mel_tensor, self.t_embed.clone(), &self.device)?
                }
            };

            let text_tokens: Vec<u32> = generated
                .iter()
                .filter(|&&t| t >= 1000)
                .map(|&t| t as u32)
                .collect();
            let text = self
                .tokenizer
                .decode(&text_tokens)
                .map_err(|e| SttError::TranscribeError(format!("decode: {e}")))?;
            if !text.trim().is_empty() {
                texts.push(text.trim().to_string());
            }
        }

        Ok(texts.join(" "))
    }

    /// Release the underlying model from memory.
    pub fn unload(self) {
        // Drop semantics free the model state + device context.
        drop(self.model);
    }
}

fn load_model(model_dir: &Path, device: &Device) -> Result<ModelState, SttError> {
    // Prefer Q4 GGUF when available — smaller and faster on modest GPUs.
    for name in GGUF_CANDIDATES {
        let path = model_dir.join(name);
        if path.exists() {
            return load_q4(&path, device);
        }
    }

    let safetensors = model_dir.join(BF16_WEIGHTS_FILENAME);
    if !safetensors.exists() {
        return Err(SttError::ModelNotFound(format!(
            "no Q4 GGUF or BF16 weights in {}",
            model_dir.display()
        )));
    }
    load_bf16(&safetensors, device)
}

fn load_q4(path: &Path, device: &Device) -> Result<ModelState, SttError> {
    use voxtral_mini_realtime::gguf::loader::Q4ModelLoader;
    let mut loader = Q4ModelLoader::from_file(path)
        .map_err(|e| SttError::LoadError(format!("open gguf: {e}")))?;
    let model = loader
        .load(device)
        .map_err(|e| SttError::LoadError(format!("load q4: {e}")))?;
    Ok(ModelState::Q4(model))
}

fn load_bf16(path: &Path, device: &Device) -> Result<ModelState, SttError> {
    use voxtral_mini_realtime::models::loader::VoxtralModelLoader;
    let loader = VoxtralModelLoader::from_file(path)
        .map_err(|e| SttError::LoadError(format!("open safetensors: {e}")))?;
    let model = loader
        .load(device)
        .map_err(|e| SttError::LoadError(format!("load bf16: {e}")))?;
    Ok(ModelState::Bf16(model))
}

fn mel_tensor_from_audio(
    audio: &AudioBuffer,
    mel_extractor: &MelSpectrogram,
    pad_config: &PadConfig,
    device: &Device,
) -> Result<Tensor<Backend, 3>, SttError> {
    let padded = pad_audio(audio, pad_config);
    let mel = mel_extractor.compute_log(&padded.samples);
    let n_frames = mel.len();
    if n_frames == 0 {
        return Err(SttError::InvalidAudio(
            "audio too short to produce mel frames".into(),
        ));
    }
    let n_mels = mel[0].len();

    let mut mel_transposed = vec![vec![0.0f32; n_frames]; n_mels];
    for (frame_idx, frame) in mel.iter().enumerate() {
        for (mel_idx, &val) in frame.iter().enumerate() {
            mel_transposed[mel_idx][frame_idx] = val;
        }
    }
    let mel_flat: Vec<f32> = mel_transposed.into_iter().flatten().collect();
    Ok(Tensor::from_data(
        TensorData::new(mel_flat, [1, n_mels, n_frames]),
        device,
    ))
}

fn decode_bf16(
    model: &voxtral_mini_realtime::models::voxtral::VoxtralModel<Backend>,
    mel_tensor: Tensor<Backend, 3>,
    t_embed: Tensor<Backend, 3>,
    device: &Device,
) -> Result<Vec<i32>, SttError> {
    let audio_embeds = model.encode_audio(mel_tensor);
    let seq_len = audio_embeds.dims()[1];
    let d_model = audio_embeds.dims()[2];

    if seq_len < PREFIX_LEN {
        return Ok(Vec::new());
    }

    let mut decoder_cache = model.create_decoder_cache_preallocated(seq_len, device);

    let mut prefix: Vec<i32> = vec![BOS_TOKEN];
    prefix.extend(std::iter::repeat_n(STREAMING_PAD, PREFIX_LEN - 1));

    let prefix_tensor = Tensor::<Backend, 2, Int>::from_data(
        TensorData::new(prefix.clone(), [1, PREFIX_LEN]),
        device,
    );
    let prefix_text_embeds = model.decoder().embed_tokens(prefix_tensor);
    let prefix_audio = audio_embeds
        .clone()
        .slice([0..1, 0..PREFIX_LEN, 0..d_model]);
    let prefix_inputs = prefix_audio + prefix_text_embeds;
    let hidden = model.decoder().forward_hidden_with_cache(
        prefix_inputs,
        t_embed.clone(),
        &mut decoder_cache,
    );
    let logits = model.decoder().lm_head(hidden);
    let vocab_size = logits.dims()[2];
    let last_logits = logits.slice([0..1, (PREFIX_LEN - 1)..PREFIX_LEN, 0..vocab_size]);
    let first_token: i32 = last_logits.argmax(2).into_scalar().elem();

    let mut generated = prefix;
    generated.push(first_token);

    for pos in PREFIX_LEN + 1..seq_len {
        let new_token = generated[pos - 1];
        let token_tensor =
            Tensor::<Backend, 2, Int>::from_data(TensorData::new(vec![new_token], [1, 1]), device);
        let text_embed = model.decoder().embed_tokens(token_tensor);
        let audio_pos = audio_embeds
            .clone()
            .slice([0..1, (pos - 1)..pos, 0..d_model]);
        let input = audio_pos + text_embed;
        let hidden =
            model
                .decoder()
                .forward_hidden_with_cache(input, t_embed.clone(), &mut decoder_cache);
        let logits = model.decoder().lm_head(hidden);
        let next_token: i32 = logits.argmax(2).into_scalar().elem();
        generated.push(next_token);
    }

    Ok(generated.into_iter().skip(PREFIX_LEN).collect())
}

/// Resolve the expected layout inside `models_dir` for downloaded Voxtral
/// weights — used by the downloader (V3) and by the engine loader.
pub fn model_dir(models_dir: &Path) -> PathBuf {
    models_dir.join("voxtral-mini-4b")
}
