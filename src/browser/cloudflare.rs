use anyhow::Result;
use crate::browser::cdp::Browser;

pub async fn patch_cdp_detection(browser: &Browser) -> Result<()> {
    let scripts = vec![
        "Object.defineProperty(navigator, 'webdriver', {get: () => undefined})",
        "window.chrome = { runtime: {} }",
        "Object.defineProperty(navigator, 'plugins', {get: () => [1, 2, 3, 4, 5]})",
        "Object.defineProperty(navigator, 'languages', {get: () => ['en-US', 'en']})",
    ];

    for script in scripts {
        browser.eval_js(script).await?;
    }

    Ok(())
}
