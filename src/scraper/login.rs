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

    // デバッグ: まず簡単なページでブラウザ動作確認
    info!("Navigating to about:blank for browser test...");
    page.goto("about:blank")
        .await
        .map_err(|e| ScraperError::Navigation(format!("about:blank failed: {e}")))?;
    info!("about:blank OK, navigating to login page...");

    page.goto(LOGIN_URL)
        .await
        .map_err(|e| ScraperError::Navigation(format!("Login page navigation failed: {e}")))?;
    info!("Login page loaded");

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

    // 認証情報入力（input/changeイベントも発火させてASP.NET ViewStateに反映）
    let fill_script = format!(
        r#"(function() {{
            function setVal(id, val) {{
                var el = document.querySelector(id);
                if (!el) return false;
                el.value = val;
                el.dispatchEvent(new Event('input', {{ bubbles: true }}));
                el.dispatchEvent(new Event('change', {{ bubbles: true }}));
                return true;
            }}
            var r1 = setVal('#txtID2', '{}');
            var r2 = setVal('#txtID1', '{}');
            var r3 = setVal('#txtPass', '{}');
            return JSON.stringify({{ txtID2: r1, txtID1: r2, txtPass: r3 }});
        }})()"#,
        account.comp_id, account.user_name, account.user_pass
    );

    let fill_result = page.evaluate(fill_script.as_str())
        .await
        .map_err(|e| ScraperError::JavaScript(e.to_string()))?;
    info!("Fill result: {:?}", fill_result.into_value::<String>());

    // ログインボタンの存在確認とクリック
    let click_script = r#"(function() {
        var btn = document.querySelector('#imgLogin');
        if (!btn) {
            var els = document.querySelectorAll('[id*="img"], [id*="btn"], [id*="Login"], input[type="image"], input[type="submit"]');
            var ids = [];
            for (var i = 0; i < els.length; i++) { ids.push(els[i].id + ':' + els[i].tagName + ':' + els[i].type); }
            return JSON.stringify({ error: 'imgLogin not found', ids: ids });
        } else {
            var tag = btn.tagName;
            var type = btn.type || 'none';
            btn.click();
            return JSON.stringify({ clicked: true, tag: tag, type: type });
        }
    })()"#;

    let click_result = page.evaluate(click_script)
        .await
        .map_err(|e| ScraperError::JavaScript(e.to_string()))?;
    info!("Click result: {:?}", click_result.into_value::<String>());

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
        // デバッグ: ページのタイトルとボタン要素を確認
        if let Ok(title) = page.evaluate("document.title").await {
            debug!("Login check attempt {}: title={:?}", i + 1, title.into_value::<String>());
        }
        if let Ok(body_snippet) = page.evaluate("document.body ? document.body.innerHTML.substring(0, 500) : 'no body'").await {
            debug!("Login check attempt {}: body={:?}", i + 1, body_snippet.into_value::<String>());
        }

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
