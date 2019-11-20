//! FIXME: write short doc here

use flexi_logger::{Duplicate, Logger};
use lsp_server::Connection;

use ra_lsp_server::{show_message, Result, ServerConfig};
use ra_prof;

fn main() -> Result<()> {
    setup_logging()?;
    run_server()?;
    Ok(())
}

fn setup_logging() -> Result<()> {
    std::env::set_var("RUST_BACKTRACE", "short");

    let logger = Logger::with_env_or_str("error").duplicate_to_stderr(Duplicate::All);
    match std::env::var("RA_LOG_DIR") {
        Ok(ref v) if v == "1" => logger.log_to_file().directory("log").start()?,
        _ => logger.start()?,
    };

    ra_prof::set_filter(match std::env::var("RA_PROFILE") {
        Ok(spec) => ra_prof::Filter::from_spec(&spec),
        Err(_) => ra_prof::Filter::disabled(),
    });
    Ok(())
}

fn run_server() -> Result<()> {
    log::info!("lifecycle: server started");

    let (connection, io_threads) = Connection::stdio();
    let server_capabilities = serde_json::to_value(ra_lsp_server::server_capabilities()).unwrap();

    let initialize_params = connection.initialize(server_capabilities)?;
    let initialize_params: lsp_types::InitializeParams = serde_json::from_value(initialize_params)?;

    let cwd = std::env::current_dir()?;
    let root = initialize_params.root_uri.and_then(|it| it.to_file_path().ok()).unwrap_or(cwd);

    let workspace_roots = initialize_params
        .workspace_folders
        .map(|workspaces| {
            workspaces.into_iter().filter_map(|it| it.uri.to_file_path().ok()).collect::<Vec<_>>()
        })
        .filter(|workspaces| !workspaces.is_empty())
        .unwrap_or_else(|| vec![root]);

    let server_config: ServerConfig = initialize_params
        .initialization_options
        .and_then(|v| {
            serde_json::from_value(v)
                .map_err(|e| {
                    log::error!("failed to deserialize config: {}", e);
                    show_message(
                        lsp_types::MessageType::Error,
                        format!("failed to deserialize config: {}", e),
                        &connection.sender,
                    );
                })
                .ok()
        })
        .unwrap_or_default();

    ra_lsp_server::main_loop(
        workspace_roots,
        initialize_params.capabilities,
        server_config,
        connection,
    )?;

    log::info!("shutting down IO...");
    io_threads.join()?;
    log::info!("... IO is down");
    Ok(())
}
