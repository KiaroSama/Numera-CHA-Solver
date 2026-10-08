#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() {
    let code = if std::env::args().nth(1).as_deref() == Some("__ocr-worker") {
        match numera_cha_solver::config::Settings::from_env() {
            Ok(settings) => numera_cha_solver::engines::ocr_worker::run(settings),
            Err(_) => 2,
        }
    } else {
        numera_cha_solver::cli::run().await
    };
    std::process::exit(code);
}
