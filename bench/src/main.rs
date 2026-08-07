use ecat_bench::run_bench;

const DEFAULT_LOGIN_URL: &str = "http://localhost:8080/api/auth/login";
const DEFAULT_HEALTH_URL: &str = "http://localhost:8080/api/health";

async fn health_once(client: &reqwest::Client, url: &str) {
    match client.get(url).send().await {
        Ok(r) if r.status().is_success() => {}
        Ok(r) => eprintln!("health returned {}", r.status()),
        Err(e) => eprintln!("health request failed: {e}"),
    }
}

async fn login_once(client: &reqwest::Client, url: &str) {
    let resp = client
        .post(url)
        .json(&serde_json::json!({"username": "bench", "password": "bench-bench"}))
        .send()
        .await;
    match resp {
        Ok(r) if r.status().is_success() => {}
        Ok(r) => eprintln!("login returned {}", r.status()),
        Err(e) => eprintln!("login request failed: {e}"),
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let target = std::env::var("BENCH_TARGET").unwrap_or_else(|_| "health".into());
    let client = reqwest::Client::new();
    match target.as_str() {
        "login" => {
            let url =
                std::env::var("BENCH_GATEWAY_URL").unwrap_or_else(|_| DEFAULT_LOGIN_URL.into());
            let result = run_bench("login", 10, 500, move || {
                let client = client.clone();
                let url = url.clone();
                async move { login_once(&client, &url).await }
            })
            .await;
            result.print();
        }
        _ => {
            let url =
                std::env::var("BENCH_GATEWAY_URL").unwrap_or_else(|_| DEFAULT_HEALTH_URL.into());
            let result = run_bench("health", 10, 500, move || {
                let client = client.clone();
                let url = url.clone();
                async move { health_once(&client, &url).await }
            })
            .await;
            result.print();
        }
    }
    Ok(())
}
