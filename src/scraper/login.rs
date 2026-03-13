use std::time::Duration;

use chromiumoxide::Page;
use tokio::time::sleep;
use tracing::{debug, info};

use crate::config::Account;
use crate::error::ScraperError;

const LOGIN_URL: &str = "https://theearth-np.com/F-OES1010[Login].aspx?mode=timeout";

pub async fn login(page: &Page, account: &Account) -> Result<(), ScraperError> {
    info!(
        "Logging in: comp_id={}, user={}",
        account.comp_id, account.user_name
    );

    page.goto(LOGIN_URL)
        .await
        .map_err(|e| ScraperError::Navigation(e.to_string()))?;

    sleep(Duration::from_secs(3)).await;

    // ログインフォーム存在確認
    let has_form = page
        .evaluate("document.querySelector('#txtPass') !== null")
        .await
        .map_err(|e| ScraperError::JavaScript(e.to_string()))?;

    if !has_form.into_value::<bool>().unwrap_or(false) {
        return Err(ScraperError::Login("Login form not found".into()));
    }

    // ポップアップ処理
    let _ = page
        .evaluate(
            r#"
            const popup = document.querySelector('#popup_1');
            if (popup && popup.style.display !== 'none') { popup.click(); }
        "#,
        )
        .await;

    sleep(Duration::from_secs(1)).await;

    // 認証情報入力
    let fill_script = format!(
        r#"
        document.querySelector('#txtID2').value = '{}';
        document.querySelector('#txtID1').value = '{}';
        document.querySelector('#txtPass').value = '{}';
    "#,
        account.comp_id, account.user_name, account.user_pass
    );

    page.evaluate(fill_script.as_str())
        .await
        .map_err(|e| ScraperError::JavaScript(e.to_string()))?;

    // ログインボタンクリック
    page.evaluate("document.querySelector('#imgLogin').click()")
        .await
        .map_err(|e| ScraperError::JavaScript(e.to_string()))?;

    sleep(Duration::from_secs(5)).await;

    // ログイン後のポップアップ処理
    let _ = page
        .evaluate(
            r#"
            const popup = document.querySelector('#popup_1');
            if (popup && popup.style.display !== 'none') { popup.click(); }
        "#,
        )
        .await;

    sleep(Duration::from_secs(1)).await;

    // ログイン成功確認
    let mut success = false;
    for i in 0..10 {
        match page
            .evaluate("document.querySelector('#Button1st_2') !== null || document.querySelector('#Button1st_7') !== null")
            .await
        {
            Ok(result) => {
                success = result.into_value::<bool>().unwrap_or(false);
                if success {
                    break;
                }
            }
            Err(e) => {
                debug!("Login check attempt {} failed: {}", i + 1, e);
            }
        }
        sleep(Duration::from_secs(1)).await;
    }

    if !success {
        return Err(ScraperError::Login("Login verification failed".into()));
    }

    info!("Login successful");
    Ok(())
}
