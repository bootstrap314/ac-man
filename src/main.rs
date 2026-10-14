#[tokio::main]
async fn main() -> std::process::ExitCode {
    ac_control::cli::run().await
}
