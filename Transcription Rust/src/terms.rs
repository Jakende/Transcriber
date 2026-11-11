//! Local German/English named-entity recognition. No Python or remote text processing.
use anyhow::{Context, Result, ensure};
use candle_core::{DType, Module, Tensor};
use candle_nn::{Linear, VarBuilder};
use candle_transformers::models::distilbert::{Config, DistilBertModel};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::PathBuf,
    sync::atomic::AtomicBool,
};
use tokenizers::{
    Tokenizer, models::wordpiece::WordPiece, normalizers::bert::BertNormalizer,
    pre_tokenizers::bert::BertPreTokenizer,
};
const REPO: &str = "Davlan/distilbert-base-multilingual-cased-ner-hrl";
const REVISION: &str = "d421f57d5b1d36b375408588669e9340f9b11a89";
const WEIGHTS_SHA: &str = "c702d2d069b66236f57552155c90b50b0b89a7762190b423ded3910ef953ac29";
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Candidate {
    pub term: String,
    pub kind: String,
    pub count: usize,
}
fn assets(cancel: &AtomicBool, progress: &mut dyn FnMut(f32, String)) -> Result<PathBuf> {
    let dir = crate::models::model_root().join("terms-multilingual");
    fs::create_dir_all(&dir)?;
    let http = crate::models::client()?;
    for name in ["config.json", "vocab.txt", "model.safetensors"] {
        crate::audio::check_cancel(cancel)?;
        let path = dir.join(name);
        if !path.is_file() {
            crate::models::download(
                &http,
                &format!("https://huggingface.co/{REPO}/resolve/{REVISION}/{name}"),
                &path,
                cancel,
                &mut |p| progress(p, format!("Sprachmodell: {name} · {:.0}%", p * 100.)),
            )?;
        }
    }
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut file = fs::File::open(dir.join("model.safetensors"))?;
    let mut hash = Sha256::new();
    let mut buffer = vec![0; 1024 * 1024];
    loop {
        crate::audio::check_cancel(cancel)?;
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    ensure!(
        format!("{:x}", hash.finalize()) == WEIGHTS_SHA,
        "Prüfsumme des Sprachmodells stimmt nicht. Modell unter {} entfernen und erneut laden.",
        dir.display()
    );
    Ok(dir)
}
pub fn analyze(
    text: &str,
    cpu: bool,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(f32, String),
) -> Result<Vec<Candidate>> {
    if text.trim().is_empty() {
        return Ok(vec![]);
    }
    let dir = assets(cancel, progress)?;
    let config: Config = serde_json::from_slice(&fs::read(dir.join("config.json"))?)?;
    let device = crate::engine::select_device(cpu)?;
    let vb = VarBuilder::from_buffered_safetensors(
        fs::read(dir.join("model.safetensors"))?,
        DType::F32,
        &device,
    )?;
    let model = DistilBertModel::load(vb.clone(), &config)?;
    let classifier: Linear = candle_nn::linear(config.dim, 9, vb.pp("classifier"))?;
    let vocab = dir.join("vocab.txt");
    let mut tokenizer = Tokenizer::new(
        WordPiece::from_file(vocab.to_str().context("Ungültiger Modellpfad")?)
            .build()
            .map_err(anyhow::Error::msg)?,
    );
    tokenizer.with_normalizer(Some(BertNormalizer::new(true, true, Some(false), false)));
    tokenizer.with_pre_tokenizer(Some(BertPreTokenizer));
    let encoding = tokenizer.encode(text, false).map_err(anyhow::Error::msg)?;
    let ids = encoding.get_ids();
    let offsets = encoding.get_offsets();
    let mut entities = BTreeSet::new();
    let mut at = 0;
    while at < ids.len() {
        crate::audio::check_cancel(cancel)?;
        let end = (at + 256).min(ids.len());
        let input = std::iter::once(101)
            .chain(ids[at..end].iter().copied())
            .chain(std::iter::once(102))
            .collect::<Vec<u32>>();
        let input = Tensor::new(input.as_slice(), &device)?.unsqueeze(0)?;
        // Candle uses true for masked positions; this unpadded batch has none.
        let mask = Tensor::zeros(input.shape(), DType::U8, &device)?;
        let labels = classifier
            .forward(&model.forward(&input, &mask)?)?
            .argmax(2)?
            .squeeze(0)?
            .to_vec1::<u32>()?;
        let mut active: Option<(usize, usize, u32)> = None;
        for (i, &label) in labels[1..labels.len() - 1].iter().enumerate() {
            let (start, until) = offsets[at + i];
            let kind = match label {
                3 | 4 => 3,
                5 | 6 => 5,
                7 | 8 => 7,
                _ => 0,
            };
            let continues = label % 2 == 0
                && kind > 0
                && active.is_some_and(|(_, last, k)| {
                    k == kind
                        && text
                            .get(last..start)
                            .is_some_and(|gap| gap.trim().is_empty())
                });
            if continues {
                if let Some((_, last, _)) = &mut active {
                    *last = until;
                }
            } else {
                if let Some(entity) = active.take() {
                    entities.insert(entity);
                }
                if kind > 0 {
                    active = Some((start, until, kind));
                }
            }
        }
        if let Some(entity) = active {
            entities.insert(entity);
        }
        progress(
            end as f32 / ids.len() as f32,
            format!("Begriffe analysieren · {end}/{} Textbausteine", ids.len()),
        );
        if end == ids.len() {
            break;
        }
        at = end - 32;
    }
    // Overlapping token windows must not count the same occurrence twice.
    let mut filtered = vec![];
    for &(start, end, kind) in &entities {
        if entities
            .iter()
            .any(|&(a, b, k)| k == kind && a <= start && b >= end && (a < start || b > end))
        {
            continue;
        }
        if let Some(term) = text.get(start..end).filter(|t| t.chars().count() >= 3) {
            filtered.push((term, kind));
        }
    }
    let mut grouped = BTreeMap::<String, Candidate>::new();
    for (term, kind) in filtered {
        let entry = grouped
            .entry(term.to_lowercase())
            .or_insert_with(|| Candidate {
                term: term.into(),
                kind: match kind {
                    3 => "Person",
                    5 => "Organisation",
                    _ => "Ort",
                }
                .into(),
                count: 0,
            });
        entry.count += 1;
    }
    let mut result = grouped.into_values().collect::<Vec<_>>();
    result.sort_by(|a, b| b.count.cmp(&a.count).then(a.term.cmp(&b.term)));
    result.truncate(100);
    Ok(result)
}
