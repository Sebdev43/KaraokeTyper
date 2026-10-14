//! The model registry, as versioned data rather than Rust code.
//!
//! Every model this app can install — its URL, its exact size, its SHA-256, its
//! languages — used to be a `ModelInfo` literal in `model_downloader.rs`, with the
//! hashes typed in by hand from HuggingFace's LFS metadata. That worked for five
//! models and does not scale: a hash is 64 characters that nobody can eyeball, and a
//! wrong one is only discovered when a download mysteriously refuses to install.
//!
//! So the registry lives in `catalog.json`, generated and checked against upstream by
//! `scripts/catalog.py`. It is `include_str!`'d, which matters twice: listing models
//! never touches the network, and a malformed catalogue is a compile-time asset that
//! the test suite can reject before anyone ships it.
//!
//! The catalogue deliberately says nothing about *runtimes*. It describes models; which
//! engine loads them is `engine_type`, and swapping engines later does not invalidate a
//! single hash. That separation is the reason this was worth doing on its own — see
//! `.claude/output/apex-rust/16-bloc-4-evaluation-transcribe-cpp/`.

use serde::Deserialize;
use std::collections::HashMap;
use std::sync::OnceLock;

use crate::model_downloader::{ModelFile, ModelInfo};
use crate::stt::EngineType;

/// The catalogue, compiled into the binary.
const CATALOG_JSON: &str = include_str!("catalog.json");

/// Schema version this build understands. A catalogue written for a newer schema is
/// refused rather than half-read.
const SUPPORTED_CATALOG_VERSION: u32 = 1;

#[derive(Deserialize)]
struct Catalog {
    catalog_version: u32,
    #[allow(dead_code)]
    generated_at: String,
    /// Alternative hosts. **Parsed and validated, but not yet consumed by the
    /// downloader** — `download_model` still builds its source list from `url` alone.
    /// Populating this does nothing until that is wired up; the test below asserts it
    /// stays empty so nobody adds one believing they have enabled a fallback.
    ///
    /// It is safe to wire later precisely because the per-file SHA-256 is the trust
    /// anchor: a mirror serving the wrong bytes fails exactly like a corrupt transfer.
    #[serde(default)]
    mirrors: Vec<String>,
    /// Language lists, named once and referenced by models, so the 13-entry Whisper
    /// list is not repeated five times.
    #[serde(default)]
    language_sets: HashMap<String, Vec<String>>,
    models: Vec<CatalogModel>,
}

#[derive(Deserialize)]
struct CatalogModel {
    name: String,
    display_name: String,
    /// French and English blurbs. Both are required: the UI shows one or the
    /// other with no fallback, so a missing one would render as an empty line
    /// rather than as something a reader can act on.
    description: String,
    description_en: String,
    engine_type: EngineType,
    is_directory: bool,
    size_mb: u32,
    /// Key into `language_sets`; `None` means "universal / auto".
    languages: Option<String>,
    is_recommended: bool,
    accuracy_score: f32,
    speed_score: f32,
    coming_soon: bool,
    /// Single-file models only.
    #[serde(default)]
    url: String,
    #[serde(default)]
    sha256: String,
    #[serde(default)]
    size_bytes: u64,
    /// Directory-based models only; each entry carries its own hash.
    #[serde(default)]
    files: Vec<ModelFile>,
}

fn is_sha256(s: &str) -> bool {
    s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit())
}

/// Parse and validate the catalogue.
///
/// Validation is strict on purpose. The failure this guards against is not a crash but
/// a *silence*: a catalogue that half-parses leaves the user staring at an empty model
/// list with nothing to act on, and a model whose hash is malformed downloads happily
/// and installs whatever the network returned.
fn parse(json: &str) -> Result<Vec<ModelInfo>, String> {
    let catalog: Catalog =
        serde_json::from_str(json).map_err(|e| format!("catalog.json is malformed: {e}"))?;

    if catalog.catalog_version != SUPPORTED_CATALOG_VERSION {
        return Err(format!(
            "catalog.json is version {}, this build understands {}",
            catalog.catalog_version, SUPPORTED_CATALOG_VERSION
        ));
    }
    if catalog.models.is_empty() {
        return Err("catalog.json lists no models".to_string());
    }

    for mirror in &catalog.mirrors {
        if !mirror.starts_with("https://") {
            return Err(format!("mirror '{mirror}' is not https"));
        }
    }

    let mut seen = std::collections::HashSet::new();
    let mut models = Vec::with_capacity(catalog.models.len());

    for m in catalog.models {
        // `name` is the only input to every path built under the models directory —
        // `get_model_file_path`, the bundle directory, the staging directory. Nothing
        // else sanitises it, because `download_model` trusts whatever the catalogue
        // matched. An empty name in particular makes `models_dir.join("")` resolve to
        // the models directory itself, which then reads as permanently installed.
        if m.name.is_empty()
            || m.name.contains("..")
            || m.name.contains(['/', '\\', '\0'])
            || std::path::Path::new(&m.name).is_absolute()
        {
            return Err(format!(
                "model name '{}' is not a safe path segment",
                m.name
            ));
        }
        if !seen.insert(m.name.clone()) {
            return Err(format!("duplicate model name '{}'", m.name));
        }
        if m.size_mb == 0 {
            return Err(format!("model '{}': size_mb is 0", m.name));
        }
        for (label, score) in [
            ("accuracy_score", m.accuracy_score),
            ("speed_score", m.speed_score),
        ] {
            if !(0.0..=1.0).contains(&score) {
                return Err(format!(
                    "model '{}': {label} {score} is out of 0.0..=1.0",
                    m.name
                ));
            }
        }
        // The two payload shapes are exclusive. Mixing them is silent, not loud: a
        // single-file entry carrying `files` gets its hashes ignored by the downloader
        // *and* skipped by `scripts/catalog.py`, which then still reports success.
        if m.is_directory && !m.url.is_empty() {
            return Err(format!(
                "model '{}' is directory-based but also has a url",
                m.name
            ));
        }
        if !m.is_directory && !m.files.is_empty() {
            return Err(format!(
                "model '{}' is single-file but also lists files — their hashes would never be checked",
                m.name
            ));
        }
        // A url is validated whenever present, `coming_soon` or not: it is serialized to
        // the frontend either way.
        if !m.url.is_empty() && !m.url.starts_with("https://") {
            return Err(format!("model '{}': url is not https", m.name));
        }

        let supported_languages = match &m.languages {
            Some(key) => catalog.language_sets.get(key).cloned().ok_or_else(|| {
                format!("model '{}' references unknown language set '{key}'", m.name)
            })?,
            None => Vec::new(),
        };

        if m.is_directory {
            if m.files.is_empty() && !m.coming_soon {
                return Err(format!(
                    "model '{}' is directory-based but lists no files",
                    m.name
                ));
            }
            let mut filenames = std::collections::HashSet::new();
            for f in &m.files {
                if !filenames.insert(f.filename.as_str()) {
                    return Err(format!(
                        "model '{}': duplicate file '{}' — one would silently overwrite the other",
                        m.name, f.filename
                    ));
                }
                if f.filename.contains("..") || std::path::Path::new(&f.filename).is_absolute() {
                    return Err(format!(
                        "model '{}': file '{}' escapes its directory",
                        m.name, f.filename
                    ));
                }
                if !is_sha256(&f.sha256) {
                    return Err(format!(
                        "model '{}', file '{}': sha256 is not 64 hex characters",
                        m.name, f.filename
                    ));
                }
                if !f.url.starts_with("https://") {
                    return Err(format!(
                        "model '{}', file '{}': url is not https",
                        m.name, f.filename
                    ));
                }
            }
        } else if !m.coming_soon {
            // A single-file model with no URL cannot be fetched, and one with no hash
            // installs whatever arrives — both are catalogue bugs, not runtime states.
            if !m.url.starts_with("https://") {
                return Err(format!("model '{}': url is not https", m.name));
            }
            if !is_sha256(&m.sha256) {
                return Err(format!(
                    "model '{}': sha256 is not 64 hex characters",
                    m.name
                ));
            }
            if m.size_bytes == 0 {
                return Err(format!("model '{}': size_bytes is missing", m.name));
            }
        }

        models.push(ModelInfo {
            name: m.name,
            display_name: m.display_name,
            size_mb: m.size_mb,
            url: m.url,
            installed: false,
            description: m.description,
            description_en: m.description_en,
            engine_type: m.engine_type,
            is_directory: m.is_directory,
            supported_languages,
            is_recommended: m.is_recommended,
            accuracy_score: m.accuracy_score,
            speed_score: m.speed_score,
            coming_soon: m.coming_soon,
            files: m.files,
            sha256: m.sha256,
            size_bytes: m.size_bytes,
        });
    }

    Ok(models)
}

/// The catalogue's models, parsed once.
///
/// On a validation failure this returns an empty list *and says so on stderr* — but the
/// real gate is `the_shipped_catalog_is_valid` below, which fails the build's test run.
/// The catalogue is compiled in, so a broken one is a developer error that must never
/// reach a user, and a test is where that gets caught.
pub fn models() -> &'static [ModelInfo] {
    static MODELS: OnceLock<Vec<ModelInfo>> = OnceLock::new();
    MODELS.get_or_init(|| match parse(CATALOG_JSON) {
        Ok(models) => models,
        Err(e) => {
            eprintln!("[CATALOG] FATAL: {e}");
            eprintln!("[CATALOG] No models can be listed or installed.");
            Vec::new()
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The gate. The catalogue ships inside the binary, so this test is what stands
    /// between a typo in a hash and a user who cannot install anything.
    #[test]
    fn the_shipped_catalog_is_valid() {
        let models = parse(CATALOG_JSON).expect("the shipped catalog.json must be valid");
        assert_eq!(models.len(), 9, "model count changed unexpectedly");
    }

    /// Every Whisper download verifies against these. They come from the published
    /// catalogue of the GGUF mirror, never typed by hand; a wrong value here means
    /// the download is rejected after the user waited for it.
    #[test]
    fn the_pinned_whisper_hashes_are_unchanged() {
        let expected = [
            (
                "whisper-tiny",
                "325b9c7997cd1eff81ef709d55766565e71be696130cc3a3d444713798706834",
                45_981_088u64,
            ),
            (
                "whisper-base",
                "81c069428bc8a24551a8169cf31cf09bcfd9d4cf50389ae281323c9aa9648c81",
                84_962_880,
            ),
            (
                "whisper-small",
                "9b9c8811bbcc82a7766f0fb0925614bdacb0923b2cc630daeac17108b655b860",
                269_751_136,
            ),
            (
                "whisper-medium",
                "09e6a65e7de377aa5b10bae24608bc6f8ca2ed04b3993ef10d4a02bcd9a82adf",
                831_538_144,
            ),
            (
                "whisper-large-v3",
                "6053d0fd69a0fd48b8fea5ea7a52b9e0cde389343566fa30e453a1b2b258dc38",
                1_161_143_008,
            ),
            (
                "whisper-large-v3-turbo",
                "b2e30cc286bc9f3aba4db9099fc7403543497c05ce7100d0d83091ddfd25a183",
                886_381_760,
            ),
        ];
        let models = parse(CATALOG_JSON).unwrap();
        for (name, sha, size) in expected {
            let m = models
                .iter()
                .find(|m| m.name == name)
                .unwrap_or_else(|| panic!("{name} vanished from the catalogue"));
            assert_eq!(m.sha256, sha, "{name} hash changed");
            assert_eq!(m.size_bytes, size, "{name} size changed");
        }
    }

    /// The Whisper engine reads GGUF and refuses anything else, so an entry still
    /// pointing at a GGML `.bin` would download fine and fail at load time.
    #[test]
    fn every_whisper_entry_ships_a_gguf() {
        let models = parse(CATALOG_JSON).unwrap();
        let whisper: Vec<_> = models
            .iter()
            .filter(|m| m.engine_type == EngineType::Whisper && !m.coming_soon)
            .collect();
        assert!(!whisper.is_empty(), "the catalogue lost its Whisper models");
        for m in whisper {
            assert!(
                m.url.ends_with(".gguf"),
                "{} is not a GGUF: {}",
                m.name,
                m.url
            );
            assert!(!m.is_directory, "{} must stay a single file", m.name);
            assert_eq!(m.sha256.len(), 64, "{} has no usable hash", m.name);
            assert!(m.size_bytes > 0, "{} has no size", m.name);
        }
    }

    fn tampered(find: &str, replace: &str) -> String {
        let json = CATALOG_JSON.replacen(find, replace, 1);
        assert_ne!(json, CATALOG_JSON, "the tamper pattern did not match");
        json
    }

    #[test]
    fn a_malformed_catalog_is_refused() {
        let err = parse("{ not json").unwrap_err();
        assert!(err.contains("malformed"), "{err}");
    }

    #[test]
    fn a_future_schema_version_is_refused() {
        let err = parse(&tampered(
            "\"catalog_version\": 1",
            "\"catalog_version\": 99",
        ))
        .unwrap_err();
        assert!(err.contains("version 99"), "{err}");
    }

    #[test]
    fn a_truncated_hash_is_refused() {
        let err = parse(&tampered(
            "325b9c7997cd1eff81ef709d55766565e71be696130cc3a3d444713798706834",
            "325b9c79",
        ))
        .unwrap_err();
        assert!(err.contains("64 hex"), "{err}");
    }

    #[test]
    fn a_non_hex_hash_is_refused() {
        let err = parse(&tampered(
            "325b9c7997cd1eff81ef709d55766565e71be696130cc3a3d444713798706834",
            "zzzzzzzz97cd1eff81ef709d55766565e71be696130cc3a3d444713798706834",
        ))
        .unwrap_err();
        assert!(err.contains("64 hex"), "{err}");
    }

    #[test]
    fn a_non_https_url_is_refused() {
        let err = parse(&tampered("https://huggingface.co", "http://huggingface.co")).unwrap_err();
        assert!(err.contains("not https"), "{err}");
    }

    #[test]
    fn an_unknown_language_set_is_refused() {
        let err = parse(&tampered(
            "\"languages\": \"whisper-multilingue\"",
            "\"languages\": \"does-not-exist\"",
        ))
        .unwrap_err();
        assert!(err.contains("unknown language set"), "{err}");
    }

    #[test]
    fn an_empty_catalog_is_refused() {
        let err = parse(r#"{"catalog_version":1,"generated_at":"x","models":[]}"#).unwrap_err();
        assert!(err.contains("no models"), "{err}");
    }

    /// Every model carries both descriptions and its scores, because the UI sorts
    /// and labels from them — an extraction that dropped them would look fine in tests
    /// that only check hashes.
    #[test]
    fn every_model_keeps_its_ui_metadata() {
        for m in parse(CATALOG_JSON).unwrap() {
            assert!(!m.display_name.is_empty(), "{} has no display name", m.name);
            assert!(!m.description.is_empty(), "{} has no description", m.name);
            assert!(
                !m.description_en.is_empty(),
                "{} has no English description",
                m.name
            );
            assert!(m.size_mb > 0, "{} has no size", m.name);
        }
    }

    /// The tamper tests above all hit the first model, which is single-file. These
    /// cover the branches that would otherwise survive being deleted outright.
    fn minimal(models: &str) -> String {
        format!(
            r#"{{"catalog_version":1,"generated_at":"x","language_sets":{{}},"models":[{models}]}}"#
        )
    }

    const OK_SHA: &str = "325b9c7997cd1eff81ef709d55766565e71be696130cc3a3d444713798706834";

    fn single(extra: &str) -> String {
        minimal(&format!(
            r#"{{"name":"m","display_name":"M","description":"d","description_en":"d en","engine_type":"whisper",
                 "is_directory":false,"size_mb":1,"languages":null,"is_recommended":false,
                 "accuracy_score":0.5,"speed_score":0.5,"coming_soon":false,
                 "url":"https://huggingface.co/a/b/resolve/c/f.bin",
                 "sha256":"{OK_SHA}","size_bytes":1{extra}}}"#
        ))
    }

    fn bundle(files: &str) -> String {
        minimal(&format!(
            r#"{{"name":"m","display_name":"M","description":"d","description_en":"d en","engine_type":"parakeet",
                 "is_directory":true,"size_mb":1,"languages":null,"is_recommended":false,
                 "accuracy_score":0.5,"speed_score":0.5,"coming_soon":false,"files":[{files}]}}"#
        ))
    }

    fn file_entry(name: &str, sha: &str, url: &str) -> String {
        format!(r#"{{"filename":"{name}","url":"{url}","sha256":"{sha}"}}"#)
    }

    #[test]
    fn a_bundle_file_with_a_bad_hash_is_refused() {
        let f = file_entry("a.onnx", "deadbeef", "https://huggingface.co/x");
        let err = parse(&bundle(&f)).unwrap_err();
        assert!(err.contains("64 hex"), "{err}");
    }

    #[test]
    fn a_bundle_file_over_plain_http_is_refused() {
        let f = file_entry("a.onnx", OK_SHA, "http://huggingface.co/x");
        let err = parse(&bundle(&f)).unwrap_err();
        assert!(err.contains("not https"), "{err}");
    }

    #[test]
    fn a_duplicated_bundle_file_is_refused() {
        let f = file_entry("a.onnx", OK_SHA, "https://huggingface.co/x");
        let err = parse(&bundle(&format!("{f},{f}"))).unwrap_err();
        assert!(err.contains("duplicate file"), "{err}");
    }

    #[test]
    fn a_bundle_file_escaping_its_directory_is_refused() {
        let f = file_entry("../evil.onnx", OK_SHA, "https://huggingface.co/x");
        let err = parse(&bundle(&f)).unwrap_err();
        assert!(err.contains("escapes"), "{err}");
    }

    #[test]
    fn a_model_name_that_is_not_a_path_segment_is_refused() {
        for name in ["", "..", "a/b", "/abs"] {
            let json = single("").replacen(r#""name":"m""#, &format!(r#""name":"{name}""#), 1);
            let err = parse(&json).unwrap_err();
            assert!(err.contains("safe path segment"), "{name}: {err}");
        }
    }

    #[test]
    fn a_single_file_model_that_also_lists_files_is_refused() {
        let f = file_entry("a.onnx", OK_SHA, "https://huggingface.co/x");
        let err = parse(&single(&format!(r#","files":[{f}]"#))).unwrap_err();
        assert!(err.contains("would never be checked"), "{err}");
    }

    #[test]
    fn a_missing_size_is_refused() {
        let json = single("").replacen(r#""size_bytes":1"#, r#""size_bytes":0"#, 1);
        assert!(parse(&json).unwrap_err().contains("size_bytes"));
        let json = single("").replacen(r#""size_mb":1"#, r#""size_mb":0"#, 1);
        assert!(parse(&json).unwrap_err().contains("size_mb"));
    }

    #[test]
    fn an_out_of_range_score_is_refused() {
        let json = single("").replacen(r#""accuracy_score":0.5"#, r#""accuracy_score":1.5"#, 1);
        assert!(parse(&json).unwrap_err().contains("out of"));
    }

    #[test]
    fn a_plain_http_mirror_is_refused() {
        let json = single("").replacen(
            r#""language_sets":{}"#,
            r#""language_sets":{},"mirrors":["http://evil"]"#,
            1,
        );
        assert!(parse(&json).unwrap_err().contains("not https"));
    }

    /// `mirrors` is validated but the downloader does not read it yet. Keep it empty
    /// so nobody adds one and believes a fallback is active.
    #[test]
    fn the_shipped_catalog_declares_no_mirrors_yet() {
        let c: Catalog = serde_json::from_str(CATALOG_JSON).unwrap();
        assert!(
            c.mirrors.is_empty(),
            "mirrors are parsed but never used by download_model — wire them up first"
        );
    }

    /// A mutable `resolve/main` URL means an upstream re-upload silently invalidates
    /// every pinned hash at once, for every client already shipped.
    #[test]
    fn every_url_pins_an_immutable_revision() {
        for m in parse(CATALOG_JSON).unwrap() {
            let urls: Vec<&str> = if m.is_directory {
                m.files.iter().map(|f| f.url.as_str()).collect()
            } else {
                vec![m.url.as_str()]
            };
            for u in urls {
                assert!(
                    !u.contains("/resolve/main/"),
                    "{}: {u} points at a mutable ref",
                    m.name
                );
            }
        }
    }
}
