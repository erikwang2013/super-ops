use ecat_bench::run_bench;

const DEFAULT_URL: &str = "http://localhost:8080/api/auth/login";

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
    let url = std::env::var("BENCH_GATEWAY_URL").unwrap_or_else(|_| DEFAULT_URL.into());
    let client = reqwest::Client::new();
    let result = run_bench("login", 10, 500, move || {
        let client = client.clone();
        let url = url.clone();
        async move { login_once(&client, &url).await }
    })
    .await;
    result.print();
    Ok(())
}
