#[tokio::main]
async fn main() -> std::process::ExitCode {
    ac_man::cli::run().await
}
