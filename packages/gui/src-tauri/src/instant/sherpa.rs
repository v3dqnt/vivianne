//! sherpa-onnx's offline transducer, behind one interface on every platform.
//!
//! On Windows and Linux it is the `sherpa-rs` crate, linked at build time.
//!
//! On macOS it can't be. sherpa-onnx's prebuilt macOS library (and the ONNX
//! Runtime it carries) is built for a newer macOS than Vivianne supports, and
//! a library linked at build time that can't load stops the *whole app* from
//! launching — on a 10.15 Mac, Vivianne would not open at all for the sake of
//! one speech engine. So on macOS the library is downloaded on first use and
//! opened with `dlopen`. If that fails (the OS is too old for it), the failure
//! is remembered and speech-to-text uses whisper instead (`instant::stt`).
//!
//! The C structs below mirror sherpa-onnx v1.12.9's `c-api.h` exactly, as
//! `sherpa-rs-sys` 0.6.8 generates them; the version is pinned in
//! [`RUNTIME_TAG`] for that reason.

/// What a recognizer needs: the model files and how to decode.
#[derive(Debug, Clone, Default)]
pub struct Config {
    pub encoder: String,
    pub decoder: String,
    pub joiner: String,
    pub tokens: String,
    pub model_type: String,
    pub decoding_method: String,
    pub num_threads: i32,
    pub sample_rate: i32,
    pub feature_dim: i32,
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use super::Config;

    pub struct Recognizer(sherpa_rs::transducer::TransducerRecognizer);

    impl Recognizer {
        pub fn new(config: Config, _runtime: Option<&std::path::Path>) -> Result<Self, String> {
            sherpa_rs::transducer::TransducerRecognizer::new(sherpa_rs::transducer::TransducerConfig {
                encoder: config.encoder,
                decoder: config.decoder,
                joiner: config.joiner,
                tokens: config.tokens,
                model_type: config.model_type,
                decoding_method: config.decoding_method,
                num_threads: config.num_threads,
                sample_rate: config.sample_rate,
                feature_dim: config.feature_dim,
                ..Default::default()
            })
            .map(Self)
            .map_err(|e| e.to_string())
        }

        pub fn transcribe(&mut self, sample_rate: u32, samples: &[f32]) -> String {
            self.0.transcribe(sample_rate, samples)
        }
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::Config;
    use std::ffi::{CStr, CString};
    use std::os::raw::c_char;
    use std::ptr::null;

    type P = *const c_char;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct FeatureConfig {
        sample_rate: i32,
        feature_dim: i32,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct TransducerModel {
        encoder: P,
        decoder: P,
        joiner: P,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct One {
        model: P,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct WhisperModel {
        encoder: P,
        decoder: P,
        language: P,
        task: P,
        tail_paddings: i32,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct SenseVoiceModel {
        model: P,
        language: P,
        use_itn: i32,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct MoonshineModel {
        preprocessor: P,
        encoder: P,
        uncached_decoder: P,
        cached_decoder: P,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Two {
        encoder: P,
        decoder: P,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CanaryModel {
        encoder: P,
        decoder: P,
        src_lang: P,
        tgt_lang: P,
        use_pnc: i32,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct ModelConfig {
        transducer: TransducerModel,
        paraformer: One,
        nemo_ctc: One,
        whisper: WhisperModel,
        tdnn: One,
        tokens: P,
        num_threads: i32,
        debug: i32,
        provider: P,
        model_type: P,
        modeling_unit: P,
        bpe_vocab: P,
        telespeech_ctc: P,
        sense_voice: SenseVoiceModel,
        moonshine: MoonshineModel,
        fire_red_asr: Two,
        dolphin: One,
        zipformer_ctc: One,
        canary: CanaryModel,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct LmConfig {
        model: P,
        scale: f32,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct HomophoneReplacer {
        dict_dir: P,
        lexicon: P,
        rule_fsts: P,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct RecognizerConfig {
        feat_config: FeatureConfig,
        model_config: ModelConfig,
        lm_config: LmConfig,
        decoding_method: P,
        max_active_paths: i32,
        hotwords_file: P,
        hotwords_score: f32,
        rule_fsts: P,
        rule_fars: P,
        blank_penalty: f32,
        hr: HomophoneReplacer,
    }
    #[repr(C)]
    struct RecognizerResult {
        text: P,
        // The rest of the struct is never read here.
    }

    type Opaque = std::ffi::c_void;

    struct Api {
        _lib: libloading::Library,
        create: unsafe extern "C" fn(*const RecognizerConfig) -> *const Opaque,
        destroy: unsafe extern "C" fn(*const Opaque),
        create_stream: unsafe extern "C" fn(*const Opaque) -> *const Opaque,
        accept: unsafe extern "C" fn(*const Opaque, i32, *const f32, i32),
        decode: unsafe extern "C" fn(*const Opaque, *const Opaque),
        result: unsafe extern "C" fn(*const Opaque) -> *const RecognizerResult,
        destroy_result: unsafe extern "C" fn(*const RecognizerResult),
        destroy_stream: unsafe extern "C" fn(*const Opaque),
    }

    impl Api {
        fn open(dir: &std::path::Path) -> Result<Self, String> {
            // ONNX Runtime first, so the C API finds it already loaded rather
            // than relying on the dylib's @rpath being resolvable from here.
            if let Some(ort) = std::fs::read_dir(dir)
                .ok()
                .and_then(|d| d.filter_map(Result::ok).map(|e| e.path()).find(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("libonnxruntime"))))
            {
                unsafe { libloading::Library::new(&ort) }.map(std::mem::forget).map_err(|e| format!("ONNX Runtime won't load on this Mac: {e}"))?;
            }
            let lib = unsafe { libloading::Library::new(dir.join("libsherpa-onnx-c-api.dylib")) }.map_err(|e| format!("sherpa-onnx won't load on this Mac: {e}"))?;
            unsafe {
                macro_rules! f {
                    ($n:literal) => {
                        *lib.get($n).map_err(|e| format!("sherpa-onnx is missing {}: {e}", String::from_utf8_lossy($n)))?
                    };
                }
                Ok(Self {
                    create: f!(b"SherpaOnnxCreateOfflineRecognizer"),
                    destroy: f!(b"SherpaOnnxDestroyOfflineRecognizer"),
                    create_stream: f!(b"SherpaOnnxCreateOfflineStream"),
                    accept: f!(b"SherpaOnnxAcceptWaveformOffline"),
                    decode: f!(b"SherpaOnnxDecodeOfflineStream"),
                    result: f!(b"SherpaOnnxGetOfflineStreamResult"),
                    destroy_result: f!(b"SherpaOnnxDestroyOfflineRecognizerResult"),
                    destroy_stream: f!(b"SherpaOnnxDestroyOfflineStream"),
                    _lib: lib,
                })
            }
        }
    }

    pub struct Recognizer {
        api: Api,
        handle: *const Opaque,
    }

    unsafe impl Send for Recognizer {}

    impl Recognizer {
        pub fn new(config: Config, runtime: Option<&std::path::Path>) -> Result<Self, String> {
            let dir = runtime.ok_or("the speech runtime isn't downloaded")?;
            let api = Api::open(dir)?;
            let c = |s: &str| CString::new(s).unwrap_or_default();
            let (encoder, decoder, joiner, tokens) = (c(&config.encoder), c(&config.decoder), c(&config.joiner), c(&config.tokens));
            let (model_type, method, provider, empty) = (c(&config.model_type), c(&config.decoding_method), c("cpu"), c(""));
            // Zeroed like sherpa-rs does: every unused model's paths are NULL.
            let mut model: ModelConfig = unsafe { std::mem::zeroed() };
            model.transducer = TransducerModel { encoder: encoder.as_ptr(), decoder: decoder.as_ptr(), joiner: joiner.as_ptr() };
            model.tokens = tokens.as_ptr();
            model.num_threads = config.num_threads;
            model.provider = provider.as_ptr();
            model.model_type = model_type.as_ptr();
            model.modeling_unit = empty.as_ptr();
            model.bpe_vocab = empty.as_ptr();
            let mut rc: RecognizerConfig = unsafe { std::mem::zeroed() };
            rc.feat_config = FeatureConfig { sample_rate: config.sample_rate, feature_dim: config.feature_dim };
            rc.model_config = model;
            rc.decoding_method = method.as_ptr();
            rc.hotwords_file = empty.as_ptr();
            rc.rule_fsts = null();
            rc.rule_fars = null();
            let handle = unsafe { (api.create)(&rc) };
            if handle.is_null() {
                return Err("sherpa-onnx couldn't create the recognizer".into());
            }
            Ok(Self { api, handle })
        }

        pub fn transcribe(&mut self, sample_rate: u32, samples: &[f32]) -> String {
            unsafe {
                let stream = (self.api.create_stream)(self.handle);
                (self.api.accept)(stream, sample_rate as i32, samples.as_ptr(), samples.len() as i32);
                (self.api.decode)(self.handle, stream);
                let r = (self.api.result)(stream);
                let text = if r.is_null() || (*r).text.is_null() { String::new() } else { CStr::from_ptr((*r).text).to_string_lossy().into_owned() };
                (self.api.destroy_result)(r);
                (self.api.destroy_stream)(stream);
                text
            }
        }
    }

    impl Drop for Recognizer {
        fn drop(&mut self) {
            unsafe { (self.api.destroy)(self.handle) }
        }
    }
}

pub use imp::Recognizer;

/// The sherpa-onnx release the macOS runtime is fetched from. Must match the
/// struct layouts above.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub const RUNTIME_TAG: &str = "v1.12.9";

/// Set once sherpa-onnx has failed to load on this Mac, so later calls go
/// straight to whisper instead of trying (and failing) again.
static UNAVAILABLE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn mark_unavailable() {
    UNAVAILABLE.store(true, std::sync::atomic::Ordering::SeqCst);
}

/// Whether Parakeet can run here. Always on Windows and Linux; on macOS until
/// the library has been tried and failed to load.
pub fn available() -> bool {
    !UNAVAILABLE.load(std::sync::atomic::Ordering::SeqCst)
}

/// Download sherpa-onnx's macOS runtime (universal2 shared build) once, and
/// return the folder its dylibs are in. Not needed elsewhere.
#[cfg(target_os = "macos")]
pub async fn ensure_runtime(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    use tauri::Manager;
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?.join(format!("sherpa-onnx-{RUNTIME_TAG}"));
    if dir.join("libsherpa-onnx-c-api.dylib").exists() {
        return Ok(dir);
    }
    let url = format!("https://github.com/k2-fsa/sherpa-onnx/releases/download/{RUNTIME_TAG}/sherpa-onnx-{RUNTIME_TAG}-osx-universal2-shared.tar.bz2");
    log::info!("[sherpa] downloading the macOS runtime");
    let bytes = reqwest::get(&url).await.map_err(|e| format!("download speech runtime: {e}"))?.bytes().await.map_err(|e| e.to_string())?;
    let target = dir.clone();
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;
        let mut archive = tar::Archive::new(bzip2::read::BzDecoder::new(std::io::Cursor::new(bytes)));
        for entry in archive.entries().map_err(|e| e.to_string())? {
            let mut entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path().map_err(|e| e.to_string())?.into_owned();
            let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            // Only the libraries; the archive also carries headers and tools.
            if name.ends_with(".dylib") {
                entry.unpack(target.join(&name)).map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())??;
    if !dir.join("libsherpa-onnx-c-api.dylib").exists() {
        return Err("the speech runtime download didn't contain the expected library".into());
    }
    Ok(dir)
}
