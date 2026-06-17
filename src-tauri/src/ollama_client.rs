use serde::Serialize;
use specta::Type;

/// Upper bound on a single Ollama text-correction request. Without it, a
/// reachable-but-stuck server (model loading, wedged inference, half-open
/// socket) would make `generate().await` hang forever and block the whole
/// dictation pipeline — the worst half of the Handy-audit LLM finding.
#[cfg(feature = "ollama")]
const OLLAMA_GENERATE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

#[derive(Clone, Serialize, Type)]
pub struct OllamaStatus {
    pub available: bool,
    pub model: String,
    pub url: String,
}

/// Parse Ollama URL into (host, port) for ollama-rs client
#[cfg(feature = "ollama")]
fn parse_ollama_url(url: &str) -> Result<(String, u16), String> {
    let url = url.trim_end_matches('/');
    // Ensure scheme is present for parsing
    let with_scheme = if url.starts_with("http://") || url.starts_with("https://") {
        url.to_string()
    } else {
        format!("http://{}", url)
    };

    let parsed = reqwest::Url::parse(&with_scheme)
        .map_err(|e| format!("Invalid Ollama URL '{}': {}", url, e))?;

    let host = format!(
        "{}://{}",
        parsed.scheme(),
        parsed.host_str().ok_or("No host in Ollama URL")?
    );
    let port = parsed.port().unwrap_or(11434);

    Ok((host, port))
}

#[cfg(feature = "ollama")]
pub async fn correct_text(
    url: &str,
    model: &str,
    language: &str,
    text: &str,
) -> Result<String, String> {
    use ollama_rs::generation::completion::request::GenerationRequest;
    use ollama_rs::Ollama;

    let (host, port) = parse_ollama_url(url)?;
    let ollama = Ollama::new(host, port);

    let prompt = format!(
        "Corrige uniquement l'orthographe et la grammaire du texte suivant en {}. \
         Ne change pas le sens, ne reformule pas, ne rajoute rien. \
         Réponds UNIQUEMENT avec le texte corrigé, sans explication.\n\n{}",
        language, text
    );

    let request = GenerationRequest::new(model.to_string(), prompt);

    let response = tokio::time::timeout(OLLAMA_GENERATE_TIMEOUT, ollama.generate(request))
        .await
        .map_err(|_| {
            format!(
                "Ollama timed out after {}s — using original text",
                OLLAMA_GENERATE_TIMEOUT.as_secs()
            )
        })?
        .map_err(|e| format!("Ollama error: {}", e))?;

    let corrected = response.response.trim().to_string();
    if corrected.is_empty() {
        Ok(text.to_string())
    } else {
        Ok(corrected)
    }
}

#[cfg(not(feature = "ollama"))]
pub async fn correct_text(
    _url: &str,
    _model: &str,
    _language: &str,
    text: &str,
) -> Result<String, String> {
    Ok(text.to_string())
}

pub async fn check_ollama_available(url: &str) -> bool {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build()
        .unwrap_or_default();

    client
        .get(format!("{}/api/tags", url.trim_end_matches('/')))
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}
