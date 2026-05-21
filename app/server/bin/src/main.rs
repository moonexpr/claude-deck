#[tokio::main]
async fn main() -> anyhow::Result<()> {
    server_core::run().await
}
