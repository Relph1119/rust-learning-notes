use argon2::password_hash::SaltString;
use argon2::{Algorithm, Argon2, Params, PasswordHasher, Version};
use dotenv::dotenv;
use mailnewsletter::configuration::{get_configuration, DatabaseSettings};
use mailnewsletter::email_client::EmailClient;
use mailnewsletter::issue_delivery_worker::{try_execute_task, ExecutionOutcome};
use mailnewsletter::startup::{get_connection_pool, Application};
use mailnewsletter::telemetry::{get_subscriber, init_subscriber};
use once_cell::sync::Lazy;
use sqlx::{Connection, Executor, PgConnection, PgPool};
use tokio::time::{timeout, Duration};
use uuid::Uuid;
use wiremock::MockServer;
/*
 * 使用once_cell，确保在测试期间最多只被初始化一次
 */
static TRACING: Lazy<()> = Lazy::new(|| {
    dotenv().ok();
    let default_filter_level = "info".to_string();
    let subscriber_name = "test".to_string();
    // 通过条件语句，将sink和stdout分开
    if std::env::var("TEST_LOG").is_ok() {
        let subscriber = get_subscriber(default_filter_level, subscriber_name, std::io::stdout);
        init_subscriber(subscriber);
    } else {
        let subscriber = get_subscriber(default_filter_level, subscriber_name, std::io::sink);
        init_subscriber(subscriber);
    }
});

pub struct TestApp {
    pub address: String,
    pub db_pool: PgPool,
    pub email_server: MockServer,
    // 添加端口
    pub port: u16,
    pub test_user: TestUser,
    pub api_client: reqwest::Client,
    pub email_client: EmailClient,
}

// 在发送给邮件API的请求中所包含的确认链接
pub struct ConfirmationLinks {
    pub html: reqwest::Url,
    pub plain_text: reqwest::Url,
}

impl TestApp {
    pub async fn dispatch_all_pending_emails(&self) {
        // 消费所有队列任务
        loop {
            if let ExecutionOutcome::EmptyQueue =
                try_execute_task(&self.db_pool, &self.email_client)
                    .await
                    .unwrap()
            {
                break;
            }
        }
    }

    pub async fn post_subscriptions(&self, body: String) -> reqwest::Response {
        self.api_client
            .post(&format!("{}/subscriptions", &self.address))
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(body)
            .send()
            .await
            .expect("Failed to execute request.")
    }

    pub async fn post_login<Body>(&self, body: &Body) -> reqwest::Response
    where
        Body: serde::Serialize,
    {
        self.api_client
            .post(&format!("{}/login", &self.address))
            // 这个reqwest方法确保请求体为URL编码，并相应地设置Content-Type请求头
            .form(body)
            .send()
            .await
            .expect("Failed to execute request.")
    }

    pub async fn get_login_html(&self) -> String {
        // 只需要观察HTML页面
        self.api_client
            .get(&format!("{}/login", &self.address))
            .send()
            .await
            .expect("Failed to execute request.")
            .text()
            .await
            .unwrap()
    }

    pub async fn get_admin_dashboard(&self) -> reqwest::Response {
        self.api_client
            .get(&format!("{}/admin/dashboard", &self.address))
            .send()
            .await
            .expect("Failed to execute request.")
    }

    pub async fn get_admin_dashboard_html(&self) -> String {
        self.get_admin_dashboard().await.text().await.unwrap()
    }

    pub async fn get_change_password(&self) -> reqwest::Response {
        self.api_client
            .get(&format!("{}/admin/password", &self.address))
            .send()
            .await
            .expect("Failed to execute request.")
    }

    pub async fn get_change_password_html(&self) -> String {
        self.get_change_password().await.text().await.unwrap()
    }

    pub async fn post_logout(&self) -> reqwest::Response {
        self.api_client
            .post(&format!("{}/admin/logout", &self.address))
            .send()
            .await
            .expect("Failed to execute request.")
    }

    pub async fn post_change_password<Body>(&self, body: &Body) -> reqwest::Response
    where
        Body: serde::Serialize,
    {
        self.api_client
            .post(&format!("{}/admin/password", &self.address))
            .form(body)
            .send()
            .await
            .expect("Failed to execute request.")
    }

    pub async fn get_publish_newsletter(&self) -> reqwest::Response {
        self.api_client
            .get(&format!("{}/admin/newsletters", &self.address))
            .send()
            .await
            .expect("Failed to execute request.")
    }

    pub async fn get_publish_newsletter_html(&self) -> String {
        self.get_publish_newsletter().await.text().await.unwrap()
    }

    pub async fn post_publish_newsletter<Body>(&self, body: &Body) -> reqwest::Response
    where
        Body: serde::Serialize,
    {
        self.api_client
            .post(&format!("{}/admin/newsletters", &self.address))
            .form(body)
            .send()
            .await
            .expect("Failed to execute request.")
    }

    // 从发送给邮件API的请求中提取确认链接
    pub fn get_confirmation_links(&self, email_request: &wiremock::Request) -> ConfirmationLinks {
        let body: serde_json::Value = serde_json::from_slice(&email_request.body).unwrap();
        // 从指定的字段中提取链接
        let get_link = |s: &str| {
            let links: Vec<_> = linkify::LinkFinder::new()
                .links(s)
                .filter(|l| *l.kind() == linkify::LinkKind::Url)
                .collect();
            assert_eq!(links.len(), 1);
            let raw_link = links[0].as_str().to_owned();
            // 解析确认的链接
            let mut confirmation_link = reqwest::Url::parse(&raw_link).unwrap();
            // 确保调用的API是本地的
            assert_eq!(confirmation_link.host_str().unwrap(), "127.0.0.1");
            // 设置URL中的端口
            confirmation_link.set_port(Some(self.port)).unwrap();
            confirmation_link
        };

        let html = get_link(&body["HtmlBody"].as_str().unwrap());
        let plain_text = get_link(&body["TextBody"].as_str().unwrap());
        ConfirmationLinks { html, plain_text }
    }

    // 删除数据库
    pub async fn cleanup(&self) {
        let connection_options = self.db_pool.connect_options();
        // 获取数据库名称，关闭数据库
        let database_name = connection_options.get_database().unwrap();
        // 关闭数据库连接池
        self.db_pool.close().await;

        // 等待所有连接关闭，设置超时时间为5秒
        let close_future = self.db_pool.close_event();
        match timeout(Duration::from_secs(5), close_future).await {
            Ok(_) => {
                if self.db_pool.is_closed() {
                    // 删除数据库
                    let configuration = get_configuration().expect("Failed to read configuration.");
                    let connection = PgPool::connect_with(configuration.database.without_db())
                        .await
                        .expect("Failed to connect to Postgres.");
                    connection
                        .execute(format!(r#"DROP DATABASE "{}";"#, database_name).as_str())
                        .await
                        .expect("Failed to create database.");
                }
            }
            Err(_) => tracing::error!("Timeout waiting for all connections to close."),
        }
    }
}

pub async fn spawn_app() -> TestApp {
    // 只在第一次运行测试的时候调用
    Lazy::force(&TRACING);
    // 模拟一个服务器
    let email_server = MockServer::start().await;

    let configuration = {
        // 连接数据库
        let mut c = get_configuration().expect("Failed to read configuration.");
        // 创建随机名称的数据库
        c.database.database_name = Uuid::new_v4().to_string();
        c.application.port = 0;
        // 使用模拟服务器作为邮件API
        c.email_client.base_url = email_server.uri();
        c
    };

    configuration_database(&configuration.database).await;

    // 启动应用程序作为后台服务
    let application = Application::build(configuration.clone())
        .await
        .expect("Failed to build application.");
    // 在启动应用程序之前获取端口
    let application_port = application.port();
    let _ = tokio::spawn(application.run_until_stopped());

    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .cookie_store(true)
        .build()
        .unwrap();

    // 将应用程序地址返回给调用者
    let test_app = TestApp {
        address: format!("http://localhost:{}", application_port),
        port: application_port,
        db_pool: get_connection_pool(&configuration.database),
        email_server,
        test_user: TestUser::generate(),
        api_client: client,
        email_client: configuration.email_client.client(),
    };

    // 添加一个随机的用户名和密码
    test_app.test_user.store(&test_app.db_pool).await;

    test_app
}

async fn configuration_database(config: &DatabaseSettings) -> PgPool {
    // 创建数据库
    let mut connection = PgConnection::connect_with(&config.without_db())
        .await
        .expect("Failed to connect to Postgres.");
    connection
        .execute(format!(r#"CREATE DATABASE "{}";"#, config.database_name).as_str())
        .await
        .expect("Failed to create database.");

    // 迁移数据库
    let connection_pool = PgPool::connect_with(config.with_db())
        .await
        .expect("Failed to connect to Postgres.");
    sqlx::migrate!("./migrations")
        .run(&connection_pool)
        .await
        .expect("Failed to migrate the database");

    connection_pool
}

pub struct TestUser {
    pub user_id: Uuid,
    pub username: String,
    pub password: String,
}

impl TestUser {
    pub fn generate() -> Self {
        Self {
            user_id: Uuid::new_v4(),
            username: Uuid::new_v4().to_string(),
            password: Uuid::new_v4().to_string(),
        }
    }

    pub async fn login(&self, app: &TestApp) {
        app.post_login(&serde_json::json!({
            "username": &self.username,
            "password": &self.password
        }))
        .await;
    }

    async fn store(&self, pool: &PgPool) {
        let salt = SaltString::generate(&mut rand::thread_rng());
        let password_hash = Argon2::new(
            Algorithm::Argon2id,
            Version::V0x13,
            Params::new(15000, 2, 1, None).unwrap(),
        )
        .hash_password(self.password.as_bytes(), &salt)
        .unwrap()
        .to_string();
        sqlx::query!(
            r#"INSERT INTO users (user_id, username, password_hash)
        VALUES ($1, $2, $3)"#,
            self.user_id,
            self.username,
            password_hash,
        )
        .execute(pool)
        .await
        .expect("Failed to store test user.");
    }
}

pub fn assert_is_redirect_to(response: &reqwest::Response, location: &str) {
    assert_eq!(response.status().as_u16(), 303);
    assert_eq!(response.headers().get("Location").unwrap(), location);
}
