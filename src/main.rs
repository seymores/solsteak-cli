use std::io::{self, IsTerminal, Write};
use std::process::ExitCode;

use ssteak::cli::{self, Launch, LaunchError, Options};
use ssteak::{
    app,
    helius::{Helius, RequestContext},
    storage::{Store, default_path},
};
use std::sync::{Arc, atomic::AtomicBool};
use std::time::Duration;

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let json = cli::requests_json(&args);
    let options = match cli::parse(&args) {
        Ok(Launch::Text(text)) => {
            return if io::stdout().lock().write_all(text.as_bytes()).is_ok() {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(5)
            };
        }
        Ok(Launch::Inspect(options)) => options,
        Err(error) => return fail(error, json, None),
    };
    let key = std::env::var_os("HELIUS_API_KEY");
    let term = std::env::var("TERM").unwrap_or_default();
    let usable_terminal = io::stdin().is_terminal()
        && io::stdout().is_terminal()
        && !term.is_empty()
        && term != "dumb";
    if let Err(error) = options.validate_environment(key.as_deref(), usable_terminal) {
        return fail(error, options.json, Some(&options));
    }

    if options.json {
        return json_mode(&options, key.as_deref());
    }
    ExitCode::from(ssteak::tui::run(
        options,
        key.and_then(|key| key.into_string().ok()),
    ))
}

fn json_mode(options: &Options, key: Option<&std::ffi::OsStr>) -> ExitCode {
    let cancelled = Arc::new(AtomicBool::new(false));
    let signal =
        signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&cancelled)).ok();
    let report = match default_path().and_then(Store::open) {
        Ok(store) => {
            let client = if options.offline {
                None
            } else {
                match key
                    .and_then(|key| key.to_str())
                    .and_then(|key| Helius::new(key).ok())
                {
                    Some(client) => Some(client),
                    None => {
                        return fail(
                            LaunchError {
                                code: "INTERNAL",
                                message: "Unable to initialize the provider transport.",
                                exit_code: 5,
                            },
                            true,
                            Some(options),
                        );
                    }
                }
            };
            app::inspect(
                options,
                &store,
                client.as_ref(),
                &RequestContext::new(
                    if options.epochs == 1 {
                        Duration::from_secs(30)
                    } else {
                        Duration::from_secs(120)
                    },
                    Arc::clone(&cancelled),
                ),
                options.refresh,
                |_| {},
            )
        }
        Err(error) => app::failure(
            options,
            error.code(),
            match error.code() {
                "UNSUPPORTED_SCHEMA" => {
                    "This database needs a newer ssteak version; it was not modified."
                }
                _ => {
                    "Local storage failed. Check the database location, free space and permissions; preserve the file before recovery."
                }
            },
        ),
    };
    if let Some(signal) = signal {
        signal_hook::low_level::unregister(signal);
    }
    let code = app::exit_code(&report);
    let write = serde_json::to_writer(io::stdout().lock(), &report)
        .map_err(io::Error::other)
        .and_then(|()| writeln!(io::stdout().lock()));
    ExitCode::from(if write.is_ok() { code } else { 5 })
}

fn fail(error: LaunchError, json: bool, options: Option<&Options>) -> ExitCode {
    let result = if json {
        let mut stdout = io::stdout().lock();
        serde_json::to_writer(&mut stdout, &error.report(options))
            .map_err(io::Error::other)
            .and_then(|()| writeln!(stdout))
    } else {
        writeln!(io::stderr().lock(), "{}: {}", error.code, error.message)
    };
    ExitCode::from(if result.is_ok() { error.exit_code } else { 5 })
}
