use crate::helpers::{assert_is_redirect_to, spawn_app};
use std::collections::HashSet;
use reqwest::header::HeaderValue;

#[tokio::test]
async fn an_error_flash_message_is_set_on_failure() {
    // 准备
    let app = spawn_app().await;

    // 执行第1部分：尝试登录
    let login_body = serde_json::json!({
        "username": "random-username",
        "password": "random-password"
    });
    let response = app.post_login(&login_body).await;

    // 断言
    assert_is_redirect_to(&response, "/login");

    let cookies: HashSet<_> = response
        .headers()
        .get_all("Set-Cookie")
        .into_iter()
        .collect();

    assert!(cookies.contains(&HeaderValue::from_str("_flash=Authentication failed").unwrap()));

    let flash_cookie = response.cookies().find(|c| c.name() == "_flash").unwrap();
    assert_eq!(flash_cookie.value(), "Authentication failed");

    // 执行第2部分：跟随重定向
    let html_page = app.get_login_html().await;
    assert!(html_page.contains(r#"<p><i>Authentication failed</i></p>"#));

    // 执行第3部分：重新加载登录页面
    let html_page = app.get_login_html().await;
    assert!(!html_page.contains(r#"<p><i>Authentication failed</i></p>"#));
}
