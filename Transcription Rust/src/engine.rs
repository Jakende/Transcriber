use crate::{
    audio::{self, check_cancel},
    document::{Document, Segment},
    models,
    storage::Settings,
};
use anyhow::{Context, Result};
use candle_core::{DType, Device, IndexOp, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::whisper::{self as whisper, Config, model::Whisper};
use std::{fs, path::Path, sync::atomic::AtomicBool};
use tokenizers::Tokenizer;

pub struct Engine {
    model: Whisper,
    tokenizer: Tokenizer,
    device: Device,
    filters: Vec<f32>,
}
impl Engine {
    pub fn load(
        settings: &Settings,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(f32, String),
    ) -> Result<Self> {
        let assets = models::resolve_model(settings.model, cancel, progress)?;
        progress(0., "Whisper-Modell laden …".into());
        let device = select_device(settings.cpu)?;
        let config: Config =
            serde_json::from_slice(&fs::read(assets.metadata.join("config.json"))?)?;
        let tokenizer = Tokenizer::from_file(assets.metadata.join("tokenizer.json"))
            .map_err(anyhow::Error::msg)?;
        let vb = match assets.format {
            crate::discovery::WeightFormat::Safetensors => VarBuilder::from_buffered_safetensors(
                fs::read(&assets.weights)?,
                DType::F32,
                &device,
            )?,
            crate::discovery::WeightFormat::Ggml => {
                let weights =
                    crate::ggml::read_tensors(&assets.weights, &config, cancel, progress)?;
                let weights = weights
                    .into_iter()
                    .map(|(name, tensor)| {
                        Ok((name, tensor.to_dtype(DType::F32)?.to_device(&device)?))
                    })
                    .collect::<Result<std::collections::HashMap<_, _>>>()?;
                VarBuilder::from_tensors(weights, DType::F32, &device)
            }
            crate::discovery::WeightFormat::OpenAiPytorch => {
                let weights = crate::ggml::read_openai_pytorch(&assets.weights, cancel)?;
                let weights = weights
                    .into_iter()
                    .map(|(name, tensor)| {
                        Ok((name, tensor.to_dtype(DType::F32)?.to_device(&device)?))
                    })
                    .collect::<Result<std::collections::HashMap<_, _>>>()?;
                VarBuilder::from_tensors(weights, DType::F32, &device)
            }
        };
        let model = Whisper::load(&vb, config.clone())?;
        Ok(Self {
            model,
            tokenizer,
            device,
            filters: mel_filters(config.num_mel_bins),
        })
    }
    fn token(&self, name: &str) -> Result<u32> {
        self.tokenizer
            .token_to_id(name)
            .with_context(|| format!("Tokenizer kennt {name} nicht."))
    }
    pub fn transcribe(
        &mut self,
        path: &Path,
        settings: &Settings,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(f32, String),
    ) -> Result<Document> {
        let pcm = audio::decode(path, cancel)?;
        let device = if self.device.is_metal() {
            "metal"
        } else {
            "cpu"
        };
        let mut doc = Document::new(path, settings.model.name(), &settings.language, device);
        doc.timecodes = settings.timecodes;
        doc.fps_timecode = settings.fps;
        let ranges = if settings.language == "auto" {
            audio::language_chunks(&pcm, cancel)?
        } else {
            audio::chunks(&pcm)
        };
        let count = ranges.len();
        for (i, (start, end)) in ranges.into_iter().enumerate() {
            check_cancel(cancel)?;
            progress(
                i as f32 / count as f32,
                format!(
                    "Abschnitt {}/{} · {:.0} s",
                    i + 1,
                    count,
                    start as f64 / audio::RATE as f64
                ),
            );
            let chunk = &pcm[start..end];
            let rms = (chunk.iter().map(|v| v * v).sum::<f32>() / chunk.len() as f32).sqrt();
            if rms < 0.0001 {
                continue;
            }
            let mut padded = chunk.to_vec();
            padded.resize(whisper::N_SAMPLES, 0.);
            let mel = whisper::audio::pcm_to_mel(&self.model.config, &padded, &self.filters);
            let frames = mel.len() / self.model.config.num_mel_bins;
            let mel = Tensor::from_vec(
                mel,
                (1, self.model.config.num_mel_bins, frames),
                &self.device,
            )?
            .narrow(2, 0, whisper::N_FRAMES)?;
            let features = self.model.encoder.forward(&mel, true)?;
            let language = if settings.language == "auto" {
                self.detect_language(&features)?
            } else {
                settings.language.clone()
            };
            let tokens = self.decode(
                &features,
                &language,
                &settings.glossary,
                chunk.len() as f64 / audio::RATE as f64,
                cancel,
            )?;
            let segments = self.segments(
                &tokens,
                start as f64 / audio::RATE as f64,
                end as f64 / audio::RATE as f64,
                &language,
            )?;
            let mut segments = segments;
            if settings.language == "auto" {
                for segment in &mut segments {
                    if let Some(language) = crate::glossary::language(&segment.text) {
                        segment.language = Some(language);
                    }
                }
            }
            doc.segments.extend(segments);
        }
        if settings.diarization && !doc.segments.is_empty() {
            progress(0.95, "Sprecher erkennen …".into());
            crate::speakers::assign(&pcm, &mut doc, settings, cancel, &mut |p, message| {
                progress(0.95 + p * 0.05, message)
            })?;
        }
        doc.validate()?;
        progress(1., format!("{} Segmente erstellt", doc.segments.len()));
        Ok(doc)
    }
    fn detect_language(&mut self, features: &Tensor) -> Result<String> {
        // The original app's mixed setting is specifically German + English.
        let sot = self.token(whisper::SOT_TOKEN)?;
        let input = Tensor::new(&[sot], &self.device)?.unsqueeze(0)?;
        let decoded = self.model.decoder.forward(&input, features, true)?;
        let logits = self
            .model
            .decoder
            .final_linear(&decoded)?
            .i((0, 0))?
            .to_vec1::<f32>()?;
        let de = self.token("<|de|>")? as usize;
        let en = self.token("<|en|>")? as usize;
        Ok(if logits[de] > logits[en] { "de" } else { "en" }.into())
    }
    fn decode(
        &mut self,
        features: &Tensor,
        language: &str,
        glossary: &[String],
        duration: f64,
        cancel: &AtomicBool,
    ) -> Result<Vec<u32>> {
        let sot = self.token(whisper::SOT_TOKEN)?;
        let eot = self.token(whisper::EOT_TOKEN)?;
        let no_ts = self.token(whisper::NO_TIMESTAMPS_TOKEN)?;
        let ts_begin = no_ts + 1;
        let lang = self.token(&format!("<|{language}|>"))?;
        let mut tokens = vec![];
        if !glossary.is_empty() {
            let prompt = glossary.join(", ");
            let encoding = self
                .tokenizer
                .encode(prompt, false)
                .map_err(anyhow::Error::msg)?;
            tokens.push(self.token("<|startofprev|>")?);
            tokens.extend(encoding.get_ids().iter().copied().take(96));
        }
        let sot_index = tokens.len();
        tokens.extend([sot, lang, self.token(whisper::TRANSCRIBE_TOKEN)?]);
        let begin = tokens.len();
        let limit = self.model.config.max_target_positions;
        let no_speech = whisper::NO_SPEECH_TOKENS
            .iter()
            .find_map(|s| self.tokenizer.token_to_id(s));
        let mut sum_logprob = 0f32;
        let mut no_speech_prob = 0f32;
        while tokens.len() < limit - 1 {
            check_cancel(cancel)?;
            let input = Tensor::new(tokens.as_slice(), &self.device)?.unsqueeze(0)?;
            let hidden = self
                .model
                .decoder
                .forward(&input, features, tokens.len() == begin)?;
            if tokens.len() == begin
                && let Some(id) = no_speech
            {
                let first = self
                    .model
                    .decoder
                    .final_linear(&hidden.i((.., sot_index..sot_index + 1))?)?
                    .i((0, 0))?;
                no_speech_prob = candle_nn::ops::softmax(&first, 0)?.to_vec1::<f32>()?[id as usize];
            }
            let len = hidden.dim(1)?;
            let logits = self
                .model
                .decoder
                .final_linear(&hidden.i((.., len - 1..len))?)?
                .i((0, 0))?;
            let mut scores = logits.to_vec1::<f32>()?;
            for &id in &self.model.config.suppress_tokens {
                if let Some(v) = scores.get_mut(id as usize) {
                    *v = f32::NEG_INFINITY;
                }
            }
            scores[no_ts as usize] = f32::NEG_INFINITY;
            if tokens.len() == begin {
                // Initial timestamp must lie within the first second.
                for (id, v) in scores.iter_mut().enumerate() {
                    if id < (ts_begin as usize) || id > ts_begin as usize + 50 {
                        *v = f32::NEG_INFINITY;
                    }
                }
            } else {
                let generated = &tokens[begin..];
                let last = generated[generated.len() - 1];
                let last_ts = last >= ts_begin;
                let previous_ts = generated.len() < 2 || generated[generated.len() - 2] >= ts_begin;
                if last_ts && previous_ts {
                    scores[ts_begin as usize..].fill(f32::NEG_INFINITY);
                } else if last_ts {
                    scores[..eot as usize].fill(f32::NEG_INFINITY);
                }
                if let Some(&last_stamp) = generated.iter().rev().find(|&&t| t >= ts_begin) {
                    let minimum = if last_ts && !previous_ts {
                        last_stamp
                    } else {
                        last_stamp + 1
                    };
                    let end = (minimum as usize).min(scores.len());
                    scores[ts_begin as usize..end].fill(f32::NEG_INFINITY);
                }
            }
            let max_stamp = ts_begin as usize + (duration / 0.02).ceil() as usize;
            if max_stamp + 1 < scores.len() {
                scores[max_stamp + 1..].fill(f32::NEG_INFINITY);
            }
            // Prefer a timestamp if its combined probability exceeds all text tokens.
            let max = scores.iter().copied().fold(f32::NEG_INFINITY, f32::max);
            let ts_mass = scores[ts_begin as usize..]
                .iter()
                .map(|s| (*s - max).exp())
                .sum::<f32>();
            let text_max = scores[..ts_begin as usize]
                .iter()
                .copied()
                .fold(f32::NEG_INFINITY, f32::max);
            if ts_mass > 0. && ts_mass.ln() + max > text_max {
                scores[..ts_begin as usize].fill(f32::NEG_INFINITY);
            }
            let (next, score) = scores
                .iter()
                .enumerate()
                .filter(|(_, s)| s.is_finite())
                .max_by(|a, b| a.1.total_cmp(b.1))
                .context("Kein gültiges Whisper-Token.")?;
            let max = scores.iter().copied().fold(f32::NEG_INFINITY, f32::max);
            let logsum = scores.iter().map(|s| (*s - max).exp()).sum::<f32>().ln() + max;
            sum_logprob += *score - logsum;
            if next as u32 == eot {
                break;
            }
            tokens.push(next as u32);
        }
        let generated = tokens[begin..].to_vec();
        if no_speech_prob > 0.6 && sum_logprob / (generated.len().max(1) as f32) < -1. {
            return Ok(vec![]);
        }
        Ok(generated)
    }
    fn segments(
        &self,
        tokens: &[u32],
        offset: f64,
        end: f64,
        language: &str,
    ) -> Result<Vec<Segment>> {
        let ts = self.token(whisper::NO_TIMESTAMPS_TOKEN)? + 1;
        let mut result = vec![];
        let mut text = vec![];
        let mut start = offset;
        for &token in tokens {
            if token >= ts {
                let at = (offset + (token - ts) as f64 * 0.02).min(end);
                if !text.is_empty() {
                    let decoded = self
                        .tokenizer
                        .decode(&text, true)
                        .map_err(anyhow::Error::msg)?;
                    if !decoded.trim().is_empty() {
                        let mut s = Segment::new(start, at.max(start), decoded.trim().into());
                        s.language = Some(language.into());
                        result.push(s);
                    }
                    text.clear();
                }
                start = at;
            } else {
                text.push(token);
            }
        }
        if !text.is_empty() {
            let decoded = self
                .tokenizer
                .decode(&text, true)
                .map_err(anyhow::Error::msg)?;
            if !decoded.trim().is_empty() {
                let mut s = Segment::new(start, end.max(start), decoded.trim().into());
                s.language = Some(language.into());
                result.push(s);
            }
        }
        Ok(result)
    }
}
pub fn select_device(cpu: bool) -> Result<Device> {
    if cpu {
        return Ok(Device::Cpu);
    }
    #[cfg(feature = "metal")]
    if let Ok(device) = Device::new_metal(0) {
        return Ok(device);
    }
    Ok(Device::Cpu)
}
/// Slaney-normalized triangular filters, equivalent to Whisper's librosa filter bank.
pub fn mel_filters(bins: usize) -> Vec<f32> {
    fn hz_to_mel(hz: f64) -> f64 {
        if hz < 1000. {
            hz / (200. / 3.)
        } else {
            15. + (hz / 1000.).ln() / (6.4f64.ln() / 27.)
        }
    }
    fn mel_to_hz(mel: f64) -> f64 {
        if mel < 15. {
            mel * 200. / 3.
        } else {
            1000. * ((mel - 15.) * (6.4f64.ln() / 27.)).exp()
        }
    }
    let maximum = hz_to_mel(8000.);
    let edges: Vec<_> = (0..bins + 2)
        .map(|i| mel_to_hz(maximum * i as f64 / (bins + 1) as f64))
        .collect();
    let mut filters = vec![0.; bins * 201];
    for b in 0..bins {
        for k in 0..201 {
            let hz = k as f64 * 16000. / 400.;
            let value = ((hz - edges[b]) / (edges[b + 1] - edges[b]))
                .min((edges[b + 2] - hz) / (edges[b + 2] - edges[b + 1]))
                .max(0.);
            filters[b * 201 + k] = (value * 2. / (edges[b + 2] - edges[b])) as f32;
        }
    }
    filters
}
pub fn term_candidates(doc: &Document, known: &[String]) -> Vec<(String, usize)> {
    let mut counts = std::collections::BTreeMap::new();
    for s in &doc.segments {
        for term in s.text.split(|c: char| !c.is_alphabetic() && c != '-') {
            if term.chars().count() >= 5
                && term.chars().next().is_some_and(char::is_uppercase)
                && !known.iter().any(|k| k.eq_ignore_ascii_case(term))
            {
                *counts.entry(term.to_string()).or_insert(0) += 1;
            }
        }
    }
    let mut terms: Vec<_> = counts.into_iter().collect();
    terms.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    terms.truncate(100);
    terms
}
