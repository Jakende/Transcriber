# Third-party components

The MIT license in this directory covers the new Rust application only. It does
not relicense the other platform implementations or third-party components.
The exact Rust dependency graph is recorded in `Cargo.lock`.

- [Candle](https://github.com/huggingface/candle), [egui/eframe](https://github.com/emilk/egui),
  [rusty_ytdl](https://github.com/Mithronn/rusty-ytdl), [Boa](https://github.com/boa-dev/boa),
  [Tokenizers](https://github.com/huggingface/tokenizers), [Rubato](https://github.com/HEnquist/rubato),
  RustFFT, Rodio, Reqwest, Serde, and their dependencies: see each locked crate's license files.
- [Symphonia](https://github.com/pdeljanov/Symphonia): MPL-2.0. No Symphonia source
  files were modified. Corresponding source: the exact version 0.5.5 and individual
  component versions identified in Cargo.lock, available from crates.io and its upstream repository.
- [Whisper code and weights](https://github.com/openai/whisper): MIT.
  Whisper model repositories: `openai/whisper-tiny`, `openai/whisper-small`,
  `openai/whisper-medium`, `openai/whisper-large-v3-turbo` on Hugging Face.
  Revisions and SHA-256 checksums are pinned in `src/models.rs`.
  The mel-filter test fixture samples OpenAI's `whisper/assets/mel_filters.npz`.
- [SpeechBrain ECAPA-TDNN](https://huggingface.co/speechbrain/spkrec-ecapa-voxceleb):
  Apache-2.0. The ECAPA-TDNN architecture is implemented using Candle; the original
  pretrained embedding weights are loaded by Candle's Rust pickle reader.
  Checkpoint revision `0f99f2d0ebe89ac095bcc5903c4dd8f72b367286`.
- Apple frameworks and system libraries are supplied by macOS. The application
  uses their Rust bindings for windows, file dialogs, graphics, Metal, and audio.

The compressed inventory `THIRD-PARTY-LICENSES.html.gz` is expanded into the
app bundle as `THIRD-PARTY-LICENSES.html`. It inventories the locked macOS dependency
graph and includes packaged and upstream license texts. Distributable bundles
include both files. Regenerate the inventory when updating Cargo.lock.

Model downloads contact Hugging Face; RSS, PodcastIndex, and YouTube requests
contact the corresponding services only when invoked. No transcription API,
telemetry, or external language model service is used.
