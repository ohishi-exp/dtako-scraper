#[derive(Debug, thiserror::Error)]
pub enum ScraperError {
    #[error("Browser init failed: {0}")]
    BrowserInit(String),

    #[error("Navigation failed: {0}")]
    Navigation(String),

    #[error("Login failed: {0}")]
    Login(String),

    #[error("JavaScript error: {0}")]
    JavaScript(String),

    #[error("Download failed: {0}")]
    Download(String),

    #[error("Timeout: {0}")]
    Timeout(String),

    #[error("Upload failed: {0}")]
    Upload(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}
