mod lease;
mod server;
mod startup;

fn main() -> std::process::ExitCode {
    // Read and clear the launch secret before creating any runtime threads.
    let result = startup::Config::from_env().and_then(|config| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| "could not start engine runtime")?;
        runtime.block_on(server::run(config))
    });
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(message) => {
            // Only static diagnostics reach stderr; decoder errors can contain input.
            eprintln!("autodj-live-engine: {message}");
            std::process::ExitCode::FAILURE
        }
    }
}
