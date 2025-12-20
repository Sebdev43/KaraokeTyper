use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Arc, Mutex};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ParakeetError {
    #[error("Failed to start Python service: {0}")]
    ServiceStartError(String),

    #[error("Service not running")]
    ServiceNotRunning,

    #[error("JSON serialization error: {0}")]
    JsonError(#[from] serde_json::Error),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Transcription failed: {0}")]
    TranscriptionError(String),

    #[error("Model load failed: {0}")]
    ModelLoadError(String),

    #[error("Python not found. Please install Python 3.8+")]
    PythonNotFound,

    #[error("sherpa-onnx not installed. Install from Settings.")]
    SherpaOnnxNotInstalled,
}

#[derive(Serialize, Deserialize, Debug)]
struct ParakeetRequest {
    command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    model_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    model_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    num_threads: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    audio_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    audio_data: Option<Vec<f32>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sample_rate: Option<u32>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ProviderInfo {
    pub success: bool,
    #[serde(default)]
    pub providers: Vec<String>,
    #[serde(default)]
    pub has_cuda: bool,
    #[serde(default)]
    pub has_rocm: bool,
    #[serde(default)]
    pub has_coreml: bool,
}

#[derive(Serialize, Deserialize, Debug)]
struct ParakeetResponse {
    success: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ready: Option<bool>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    error_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    providers: Option<ProviderInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    model_loaded: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_dir: Option<String>,
}

struct ServiceProcess {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

pub struct ParakeetSttEngine {
    service: Arc<Mutex<Option<ServiceProcess>>>,
    model_path: Option<String>,
    language: String,
    provider: Option<String>,
}

/// Map model names to HuggingFace repo IDs
fn get_repo_id_for_model(model_name: &str) -> Option<&'static str> {
    match model_name {
        "sherpa-onnx-whisper-tiny" => Some("csukuangfj/sherpa-onnx-whisper-tiny.en"),
        "sherpa-onnx-whisper-base" => Some("csukuangfj/sherpa-onnx-whisper-base"),
        "sherpa-onnx-whisper-small" => Some("csukuangfj/sherpa-onnx-whisper-small"),
        "sherpa-onnx-whisper-medium" => Some("csukuangfj/sherpa-onnx-whisper-medium"),
        "sherpa-onnx-sense-voice" => Some("csukuangfj/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17"),
        "sherpa-onnx-paraformer" => Some("csukuangfj/sherpa-onnx-paraformer-zh-2023-03-28"),
        _ => None,
    }
}

/// Resolve model path from model name or repo ID by looking in HuggingFace cache
pub fn resolve_model_path(model_name: &str) -> Option<String> {
    // If it's already a full path, return it
    if model_name.starts_with('/') || model_name.contains("snapshots") {
        return Some(model_name.to_string());
    }

    // Determine repo_id: either from model name mapping or direct repo ID (org/repo format)
    let repo_id = if model_name.contains('/') {
        // It's already a repo ID like "csukuangfj/sherpa-onnx-whisper-base"
        model_name.to_string()
    } else {
        // It's a model name, look up the repo ID
        get_repo_id_for_model(model_name)?.to_string()
    };

    // Build cache path: ~/.cache/huggingface/hub/models--org--repo/snapshots/*/
    let cache_dir = dirs::cache_dir()?;
    let repo_folder = format!("models--{}", repo_id.replace('/', "--"));
    let snapshots_dir = cache_dir
        .join("huggingface")
        .join("hub")
        .join(&repo_folder)
        .join("snapshots");

    println!("[PARAKEET] Looking for model in: {}", snapshots_dir.display());

    if !snapshots_dir.exists() {
        println!("[PARAKEET] Snapshots dir not found: {}", snapshots_dir.display());
        return None;
    }

    // Find the first (and usually only) snapshot directory
    if let Ok(mut entries) = std::fs::read_dir(&snapshots_dir) {
        if let Some(Ok(entry)) = entries.next() {
            let path = entry.path();
            if path.is_dir() {
                println!("[PARAKEET] Resolved model path: {}", path.display());
                return Some(path.to_string_lossy().to_string());
            }
        }
    }

    None
}

impl ParakeetSttEngine {
    pub fn new(model_path: Option<&str>, language: &str, provider: Option<&str>) -> Self {
        // Try to resolve model path if it looks like a model name
        let resolved_path = model_path.and_then(|p| {
            if p.starts_with('/') || p.contains("snapshots") {
                Some(p.to_string())
            } else {
                // It's a model name or repo ID, try to resolve it
                resolve_model_path(p).or_else(|| {
                    // Try treating it as a model name
                    let model_name = p.split('/').last().unwrap_or(p);
                    resolve_model_path(model_name)
                })
            }
        });

        Self {
            service: Arc::new(Mutex::new(None)),
            model_path: resolved_path,
            language: language.to_string(),
            provider: provider.map(|s| s.to_string()),
        }
    }

    /// Start Python service subprocess
    fn start_service(&self) -> Result<ServiceProcess, ParakeetError> {
        // Find Python service script - try multiple locations
        let cwd = std::env::current_dir().unwrap_or_default();

        // All possible paths to check
        let possible_paths = [
            // Path 1: CWD is project root (karaoketyper/), need src-tauri/python/
            cwd.join("src-tauri").join("python").join("parakeet_service.py"),
            // Path 2: CWD is src-tauri/, need python/
            cwd.join("python").join("parakeet_service.py"),
            // Path 3: Relative to Cargo.toml location (works in dev and when running from source)
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("python")
                .join("parakeet_service.py"),
            // Path 4: Production - bundled next to executable
            std::env::current_exe()
                .unwrap_or_default()
                .parent()
                .unwrap_or(std::path::Path::new("."))
                .join("python")
                .join("parakeet_service.py"),
            // Path 5: Production - in resources directory
            std::env::current_exe()
                .unwrap_or_default()
                .parent()
                .unwrap_or(std::path::Path::new("."))
                .join("resources")
                .join("python")
                .join("parakeet_service.py"),
        ];

        let python_script = possible_paths
            .iter()
            .find(|p| p.exists())
            .cloned()
            .unwrap_or_else(|| possible_paths[2].clone()); // Default to CARGO_MANIFEST_DIR

        if !python_script.exists() {
            return Err(ParakeetError::ServiceStartError(format!(
                "Python service not found at: {}",
                python_script.display()
            )));
        }

        println!(
            "[PARAKEET] Starting Python service: {}",
            python_script.display()
        );

        // Get venv Python path from lib.rs
        let venv_python = crate::get_venv_python();

        // Try venv Python first (has sherpa-onnx installed), then system Python
        let python_cmd: std::ffi::OsString = if venv_python.exists() {
            println!("[PARAKEET] Using venv Python: {}", venv_python.display());
            venv_python.into_os_string()
        } else if Command::new("python3").arg("--version").output().is_ok() {
            "python3".into()
        } else if Command::new("python").arg("--version").output().is_ok() {
            "python".into()
        } else {
            return Err(ParakeetError::PythonNotFound);
        };

        // Start subprocess
        let mut child = Command::new(python_cmd)
            .arg(&python_script)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| ParakeetError::ServiceStartError(e.to_string()))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| ParakeetError::ServiceStartError("Failed to get stdin".to_string()))?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| ParakeetError::ServiceStartError("Failed to get stdout".to_string()))?;

        let mut service = ServiceProcess {
            child,
            stdin,
            stdout: BufReader::new(stdout),
        };

        // Wait for ready signal
        let mut ready_line = String::new();
        service.stdout.read_line(&mut ready_line).map_err(|e| {
            ParakeetError::ServiceStartError(format!("Failed to read ready signal: {}", e))
        })?;

        let ready_response: ParakeetResponse = serde_json::from_str(&ready_line).map_err(|e| {
            ParakeetError::ServiceStartError(format!("Invalid ready signal: {} - raw: {}", e, ready_line))
        })?;

        if !ready_response.ready.unwrap_or(false) {
            return Err(ParakeetError::ServiceStartError(
                ready_response
                    .error
                    .unwrap_or_else(|| "Unknown error".to_string()),
            ));
        }

        println!("[PARAKEET] ✓ Python service ready");

        // Log available providers
        if let Some(providers) = &ready_response.providers {
            println!(
                "[PARAKEET] Available providers: {:?}",
                providers.providers
            );
            if providers.has_cuda {
                println!("[PARAKEET] ✓ CUDA available (NVIDIA GPU)");
            }
            if providers.has_rocm {
                println!("[PARAKEET] ✓ ROCm available (AMD GPU)");
            }
            if providers.has_coreml {
                println!("[PARAKEET] ✓ CoreML available (Apple Silicon)");
            }
        }

        Ok(service)
    }

    /// Send command to service and get response
    fn send_command(&self, request: ParakeetRequest) -> Result<ParakeetResponse, ParakeetError> {
        let mut service_lock = self.service.lock().unwrap();

        // Start service if not running
        if service_lock.is_none() {
            println!("[PARAKEET] Service not running, starting...");
            *service_lock = Some(self.start_service()?);
        }

        let service = service_lock
            .as_mut()
            .ok_or(ParakeetError::ServiceNotRunning)?;

        // Send request as JSON line
        let request_json = serde_json::to_string(&request)?;
        writeln!(service.stdin, "{}", request_json)?;
        service.stdin.flush()?;

        // Read response
        let mut response_line = String::new();
        service.stdout.read_line(&mut response_line)?;

        let response: ParakeetResponse = serde_json::from_str(response_line.trim())?;

        Ok(response)
    }

    /// Get available ONNX Runtime execution providers
    #[allow(dead_code)]
    pub fn get_providers(&self) -> Result<ProviderInfo, ParakeetError> {
        let request = ParakeetRequest {
            command: "detect_providers".to_string(),
            model_path: None,
            model_name: None,
            provider: None,
            num_threads: None,
            language: None,
            audio_path: None,
            audio_data: None,
            sample_rate: None,
        };

        let response = self.send_command(request)?;

        if !response.success.unwrap_or(false) {
            let error_msg = response
                .error
                .unwrap_or_else(|| "Unknown error".to_string());
            return Err(ParakeetError::ServiceStartError(error_msg));
        }

        Ok(response.providers.unwrap_or(ProviderInfo {
            success: false,
            providers: vec![],
            has_cuda: false,
            has_rocm: false,
            has_coreml: false,
        }))
    }

    /// Download model from HuggingFace
    #[allow(dead_code)]
    pub fn download_model(&self, model_name: &str) -> Result<String, ParakeetError> {
        println!("[PARAKEET] Downloading model: {}", model_name);

        let request = ParakeetRequest {
            command: "download_model".to_string(),
            model_path: None,
            model_name: Some(model_name.to_string()),
            provider: None,
            num_threads: None,
            language: None,
            audio_path: None,
            audio_data: None,
            sample_rate: None,
        };

        let response = self.send_command(request)?;

        if !response.success.unwrap_or(false) {
            let error_msg = response
                .error
                .unwrap_or_else(|| "Unknown error".to_string());
            return Err(ParakeetError::ModelLoadError(error_msg));
        }

        let cache_dir = response.cache_dir.unwrap_or_default();
        println!("[PARAKEET] ✓ Model downloaded to: {}", cache_dir);
        Ok(cache_dir)
    }

    /// Initialize and load model
    pub fn initialize(&self) -> Result<(), ParakeetError> {
        let model_path = self.model_path.clone().ok_or_else(|| {
            ParakeetError::ModelLoadError("No model path specified".to_string())
        })?;

        println!("[PARAKEET] Loading model from: {}", model_path);

        let request = ParakeetRequest {
            command: "load_model".to_string(),
            model_path: Some(model_path),
            model_name: None,
            provider: self.provider.clone(),
            num_threads: Some(4),
            language: Some(self.language.clone()),
            audio_path: None,
            audio_data: None,
            sample_rate: None,
        };

        let response = self.send_command(request)?;

        if !response.success.unwrap_or(false) {
            let error_msg = response
                .error
                .unwrap_or_else(|| "Unknown error".to_string());

            // Check if it's a dependency error
            if error_msg.contains("Missing dependency")
                || error_msg.contains("ModuleNotFoundError")
                || error_msg.contains("sherpa_onnx")
            {
                return Err(ParakeetError::SherpaOnnxNotInstalled);
            }

            return Err(ParakeetError::ModelLoadError(error_msg));
        }

        println!(
            "[PARAKEET] ✓ Model loaded with provider: {}",
            response.provider.unwrap_or_else(|| "unknown".to_string())
        );
        Ok(())
    }

    /// Transcribe audio data (f32 samples at 16kHz)
    pub fn transcribe(&self, audio_data: &[f32]) -> Result<String, ParakeetError> {
        let t0 = std::time::Instant::now();

        let request = ParakeetRequest {
            command: "transcribe_audio_data".to_string(),
            model_path: None,
            model_name: None,
            provider: None,
            num_threads: None,
            language: Some(self.language.clone()),
            audio_path: None,
            audio_data: Some(audio_data.to_vec()),
            sample_rate: Some(16000),
        };

        println!(
            "[STT_TIMING] Request built: {}ms",
            t0.elapsed().as_millis()
        );

        let response = self.send_command_timed(request)?;

        if !response.success.unwrap_or(false) {
            let error_msg = response
                .error
                .unwrap_or_else(|| "Unknown error".to_string());
            return Err(ParakeetError::TranscriptionError(error_msg));
        }

        println!(
            "[STT_TIMING] Total transcribe(): {}ms",
            t0.elapsed().as_millis()
        );

        Ok(response.text.unwrap_or_default())
    }

    /// Send command with detailed timing
    fn send_command_timed(&self, request: ParakeetRequest) -> Result<ParakeetResponse, ParakeetError> {
        let t0 = std::time::Instant::now();
        let mut service_lock = self.service.lock().unwrap();

        if service_lock.is_none() {
            println!("[PARAKEET] Service not running, starting...");
            *service_lock = Some(self.start_service()?);
        }

        let service = service_lock
            .as_mut()
            .ok_or(ParakeetError::ServiceNotRunning)?;

        println!(
            "[STT_TIMING] Lock acquired: {}ms",
            t0.elapsed().as_millis()
        );

        // Serialize
        let t1 = std::time::Instant::now();
        let request_json = serde_json::to_string(&request)?;
        println!(
            "[STT_TIMING] JSON serialize ({}KB): {}ms",
            request_json.len() / 1024,
            t1.elapsed().as_millis()
        );

        // Send to Python
        let t2 = std::time::Instant::now();
        writeln!(service.stdin, "{}", request_json)?;
        service.stdin.flush()?;
        println!(
            "[STT_TIMING] Send to Python: {}ms",
            t2.elapsed().as_millis()
        );

        // Wait for Python response (this is where sherpa-onnx runs)
        let t3 = std::time::Instant::now();
        let mut response_line = String::new();
        service.stdout.read_line(&mut response_line)?;
        println!(
            "[STT_TIMING] Python processing (sherpa-onnx): {}ms",
            t3.elapsed().as_millis()
        );

        // Parse response
        let t4 = std::time::Instant::now();
        let response: ParakeetResponse = serde_json::from_str(response_line.trim())?;
        println!(
            "[STT_TIMING] JSON parse response: {}ms",
            t4.elapsed().as_millis()
        );

        Ok(response)
    }

    /// Get service status
    #[allow(dead_code)]
    pub fn status(&self) -> Result<serde_json::Value, ParakeetError> {
        let request = ParakeetRequest {
            command: "status".to_string(),
            model_path: None,
            model_name: None,
            provider: None,
            num_threads: None,
            language: None,
            audio_path: None,
            audio_data: None,
            sample_rate: None,
        };

        let response = self.send_command(request)?;
        Ok(serde_json::json!({
            "model_loaded": response.model_loaded.unwrap_or(false),
            "model": response.model,
            "provider": response.provider
        }))
    }

    /// Shutdown service gracefully
    pub fn shutdown(&self) -> Result<(), ParakeetError> {
        let mut service_lock = self.service.lock().unwrap();

        if let Some(mut service) = service_lock.take() {
            println!("[PARAKEET] Shutting down service...");

            let request = ParakeetRequest {
                command: "shutdown".to_string(),
                model_path: None,
                model_name: None,
                provider: None,
                num_threads: None,
                language: None,
                audio_path: None,
                audio_data: None,
                sample_rate: None,
            };

            // Try to send shutdown command
            if let Ok(request_json) = serde_json::to_string(&request) {
                let _ = writeln!(service.stdin, "{}", request_json);
                let _ = service.stdin.flush();
            }

            // Wait for process to exit (with timeout)
            let _ = service.child.wait();

            println!("[PARAKEET] ✓ Service shutdown complete");
        }

        Ok(())
    }
}

impl Drop for ParakeetSttEngine {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}
