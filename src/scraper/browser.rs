use std::path::PathBuf;
use std::time::Duration;

use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::cdp::browser_protocol::browser::{
    SetDownloadBehaviorBehavior, SetDownloadBehaviorParams,
};
use chromiumoxide::cdp::browser_protocol::page::{
    EventJavascriptDialogOpening, HandleJavaScriptDialogParams,
};
use chromiumoxide::Page;
use futures::StreamExt;
use tracing::{debug, info, warn};

use crate::error::ScraperError;

pub struct BrowserSession {
    pub browser: Browser,
    pub page: Page,
    download_dir: PathBuf,
}

impl BrowserSession {
    pub async fn new(download_dir: &str) -> Result<Self, ScraperError> {
        let download_path = PathBuf::from(download_dir);
        std::fs::create_dir_all(&download_path)?;
        let download_path = download_path
            .canonicalize()
            .unwrap_or_else(|_| PathBuf::from(download_dir));

        let chrome_path = std::env::var("CHROME_PATH")
            .or_else(|_| std::env::var("CHROMIUM_PATH"))
            .unwrap_or_else(|_| "chromium".to_string());

        let builder = BrowserConfig::builder()
            .chrome_executable(chrome_path)
            .no_sandbox()
            .request_timeout(Duration::from_secs(60))
            .window_size(1280, 800)
            .arg("--disable-blink-features=AutomationControlled")
            .arg("--disable-dev-shm-usage")
            .arg("--disable-gpu")
            .arg("--disable-web-security")
            .arg("--allow-running-insecure-content");

        let config = builder
            .build()
            .map_err(|e| ScraperError::BrowserInit(e.to_string()))?;

        let (browser, mut handler) = Browser::launch(config)
            .await
            .map_err(|e| ScraperError::BrowserInit(e.to_string()))?;

        tokio::spawn(async move {
            while let Some(event) = handler.next().await {
                debug!("Browser event: {:?}", event);
            }
        });

        let page = browser
            .new_page("about:blank")
            .await
            .map_err(|e| ScraperError::BrowserInit(e.to_string()))?;

        // ダウンロード先を設定
        let download_path_str = download_path.to_string_lossy().to_string();
        page.execute(
            SetDownloadBehaviorParams::builder()
                .behavior(SetDownloadBehaviorBehavior::AllowAndName)
                .download_path(&download_path_str)
                .events_enabled(true)
                .build()
                .map_err(|e| ScraperError::BrowserInit(format!("Download behavior: {e}")))?,
        )
        .await
        .map_err(|e| ScraperError::BrowserInit(format!("Set download behavior: {e}")))?;

        // JS ダイアログ自動応答
        let mut dialog_events = page
            .event_listener::<EventJavascriptDialogOpening>()
            .await
            .map_err(|e| ScraperError::BrowserInit(format!("Dialog listener: {e}")))?;

        let page_for_dialog = page.clone();
        tokio::spawn(async move {
            while let Some(event) = dialog_events.next().await {
                info!("Dialog: type={:?}, msg={}", event.r#type, event.message);
                let params = HandleJavaScriptDialogParams::builder()
                    .accept(true)
                    .build()
                    .expect("HandleJavaScriptDialogParams build");
                if let Err(e) = page_for_dialog.execute(params).await {
                    warn!("Dialog response error: {}", e);
                }
            }
        });

        info!("Browser initialized, download_dir={}", download_path_str);

        Ok(Self {
            browser,
            page,
            download_dir: download_path,
        })
    }

    pub fn download_dir(&self) -> &PathBuf {
        &self.download_dir
    }
}
