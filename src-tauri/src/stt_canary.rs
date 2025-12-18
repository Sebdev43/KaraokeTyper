use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Arc, Mutex};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CanaryError {
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

    #[error("NeMo not installed. Run: pip install nemo_toolkit[asr]")]
    NeMoNotInstalled,
}

#[derive(Serialize, Deserialize, Debug)]
struct CanaryRequest {
    command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    model_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    audio_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    audio_data: Option<Vec<f32>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sample_rate: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_lang: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    target_lang: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
struct CanaryResponse {
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
}

struct ServiceProcess {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

pub struct CanarySttEngine {
    service: Arc<Mutex<Option<ServiceProcess>>>,
    model_name: String,
    language: String,
}

impl CanarySttEngine {
    pub fn new(model_name: &str, language: &str) -> Self {
        Self {
            service: Arc::new(Mutex::new(None)),
            model_name: model_name.to_string(),
            language: language.to_string(),
        }
    }

    /// Start Python service subprocess
    fn start_service(&self) -> Result<ServiceProcess, CanaryError> {
        // Find Python service script
        let python_script = if cfg!(debug_assertions) {
            // Development: try multiple possible paths
            let cwd = std::env::current_dir().unwrap();

            // Path 1: CWD is project root (karaoketyper/), need src-tauri/python/
            let path1 = cwd
                .join("src-tauri")
                .join("python")
                .join("canary_service.py");

            // Path 2: CWD is src-tauri/, need python/
            let path2 = cwd.join("python").join("canary_service.py");

            // Path 3: CWD is somewhere else, try relative to Cargo.toml location
            let path3 = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("python")
                .join("canary_service.py");

            if path1.exists() {
                path1
            } else if path2.exists() {
                path2
            } else {
                path3
            }
        } else {
            // Production: bundled with app
            std::env::current_exe()
                .unwrap()
                .parent()
                .unwrap()
                .join("python")
                .join("canary_service.py")
        };

        if !python_script.exists() {
            return Err(CanaryError::ServiceStartError(format!(
                "Python service not found at: {}",
                python_script.display()
            )));
        }

        println!(
            "[CANARY] Starting Python service: {}",
            python_script.display()
        );

        // Get venv Python path from lib.rs
        let venv_python = crate::get_venv_python();

        // Try venv Python first (has NeMo installed), then system Python
        let python_cmd: std::ffi::OsString = if venv_python.exists() {
            println!("[CANARY] Using venv Python: {}", venv_python.display());
            venv_python.into_os_string()
        } else if Command::new("python3").arg("--version").output().is_ok() {
            "python3".into()
        } else if Command::new("python").arg("--version").output().is_ok() {
            "python".into()
        } else {
            return Err(CanaryError::PythonNotFound);
        };

        // Start subprocess
        let mut child = Command::new(python_cmd)
            .arg(python_script)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| CanaryError::ServiceStartError(e.to_string()))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| CanaryError::ServiceStartError("Failed to get stdin".to_string()))?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| CanaryError::ServiceStartError("Failed to get stdout".to_string()))?;

        let mut service = ServiceProcess {
            child,
            stdin,
            stdout: BufReader::new(stdout),
        };

        // Wait for ready signal
        let mut ready_line = String::new();
        service.stdout.read_line(&mut ready_line).map_err(|e| {
            CanaryError::ServiceStartError(format!("Failed to read ready signal: {}", e))
        })?;

        let ready_response: CanaryResponse = serde_json::from_str(&ready_line)
            .map_err(|e| CanaryError::ServiceStartError(format!("Invalid ready signal: {}", e)))?;

        if !ready_response.ready.unwrap_or(false) {
            return Err(CanaryError::ServiceStartError(
                ready_response
                    .error
                    .unwrap_or_else(|| "Unknown error".to_string()),
            ));
        }

        println!("[CANARY] ✓ Python service ready");
        Ok(service)
    }

    /// Send command to service and get response
    fn send_command(&self, request: CanaryRequest) -> Result<CanaryResponse, CanaryError> {
        let mut service_lock = self.service.lock().unwrap();

        // Start service if not running
        if service_lock.is_none() {
            println!("[CANARY] Service not running, starting...");
            *service_lock = Some(self.start_service()?);
        }

        let service = service_lock
            .as_mut()
            .ok_or(CanaryError::ServiceNotRunning)?;

        // Send request as JSON line
        let request_json = serde_json::to_string(&request)?;
        writeln!(service.stdin, "{}", request_json)?;
        service.stdin.flush()?;

        // Read response
        let mut response_line = String::new();
        service.stdout.read_line(&mut response_line)?;

        let response: CanaryResponse = serde_json::from_str(response_line.trim())?;

        Ok(response)
    }

    /// Initialize and load model
    pub fn initialize(&self) -> Result<(), CanaryError> {
        println!("[CANARY] Loading model: {}", self.model_name);

        let request = CanaryRequest {
            command: "load_model".to_string(),
            model_name: Some(self.model_name.clone()),
            language: Some(self.language.clone()),
            audio_path: None,
            audio_data: None,
            sample_rate: None,
            source_lang: None,
            target_lang: None,
        };

        let response = self.send_command(request)?;

        if !response.success.unwrap_or(false) {
            let error_msg = response
                .error
                .unwrap_or_else(|| "Unknown error".to_string());

            // Check if it's a dependency error
            if error_msg.contains("Missing dependency") || error_msg.contains("ModuleNotFoundError")
            {
                return Err(CanaryError::NeMoNotInstalled);
            }

            return Err(CanaryError::ModelLoadError(error_msg));
        }

        println!(
            "[CANARY] ✓ Model loaded: {}",
            response.message.unwrap_or_default()
        );
        Ok(())
    }

    /// Transcribe audio data (f32 samples at 16kHz)
    pub fn transcribe(&self, audio_data: &[f32]) -> Result<String, CanaryError> {
        let request = CanaryRequest {
            command: "transcribe_audio_data".to_string(),
            model_name: None,
            language: None,
            audio_path: None,
            audio_data: Some(audio_data.to_vec()),
            sample_rate: Some(16000),
            source_lang: Some(self.language.clone()),
            target_lang: Some(self.language.clone()),
        };

        let response = self.send_command(request)?;

        if !response.success.unwrap_or(false) {
            let error_msg = response
                .error
                .unwrap_or_else(|| "Unknown error".to_string());
            return Err(CanaryError::TranscriptionError(error_msg));
        }

        Ok(response.text.unwrap_or_else(String::new))
    }

    /// Shutdown service gracefully
    pub fn shutdown(&self) -> Result<(), CanaryError> {
        let mut service_lock = self.service.lock().unwrap();

        if let Some(mut service) = service_lock.take() {
            println!("[CANARY] Shutting down service...");

            let request = CanaryRequest {
                command: "shutdown".to_string(),
                model_name: None,
                language: None,
                audio_path: None,
                audio_data: None,
                sample_rate: None,
                source_lang: None,
                target_lang: None,
            };

            // Try to send shutdown command
            if let Ok(request_json) = serde_json::to_string(&request) {
                let _ = writeln!(service.stdin, "{}", request_json);
                let _ = service.stdin.flush();
            }

            // Wait for process to exit (with timeout)
            let _ = service.child.wait();

            println!("[CANARY] ✓ Service shutdown complete");
        }

        Ok(())
    }
}

impl Drop for CanarySttEngine {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}
