//! Launch validation. No database, HTTP, or terminal-mode side effects.

use std::ffi::{OsStr, OsString};
use std::time::{SystemTime, UNIX_EPOCH};

use clap::{Arg, ArgAction, Command, error::ErrorKind};
use serde_json::{Value, json};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    pub address: String,
    pub json: bool,
    pub offline: bool,
    pub refresh: bool,
    pub no_color: bool,
}

#[derive(Debug)]
pub enum Launch {
    Text(String),
    Inspect(Options),
}

#[derive(Debug)]
pub struct LaunchError {
    pub code: &'static str,
    pub message: &'static str,
    pub exit_code: u8,
}

impl LaunchError {
    fn input(code: &'static str, message: &'static str) -> Self {
        Self {
            code,
            message,
            exit_code: 2,
        }
    }

    pub fn report(&self, options: Option<&Options>) -> Value {
        let generated_at = SystemTime::now().duration_since(UNIX_EPOCH).ok();
        json!({
            "schema_version": 2,
            "input": options.map(|options| json!({
                "address": options.address,
                "offline": options.offline,
            })),
            "network": {"cluster": "mainnet", "genesis_hash": null},
            "generated_at": generated_at.map(|time| time.as_secs()),
            "status": "error",
            "data": null,
            "coverage": null,
            "sources": [],
            "warnings": [],
            "errors": [{"code": self.code, "message": self.message, "address": null}],
        })
    }
}

/// Also selects structured errors when the rest of the command line is invalid.
pub fn requests_json(args: &[OsString]) -> bool {
    args.iter()
        .take_while(|arg| *arg != "--")
        .any(|arg| arg == "--json")
}

pub fn parse(args: &[OsString]) -> Result<Launch, LaunchError> {
    let matches = Command::new("ssteak")
        .bin_name("ssteak")
        .no_binary_name(true)
        .version(env!("CARGO_PKG_VERSION"))
        .about("Inspect native Solana stake on mainnet")
        .after_help(
            "Online use requires HELIUS_API_KEY in the environment. Offline needs no key.\n\
             Public addresses are sent to Helius; no signing, wallet connection or telemetry.\n\
             Rewards always cover the latest 30 completed epochs (the recent 15 compared with the previous 15)\n\
             of the account's current validator; historical validator attribution is unverified. The annualized account return estimate\n\
             uses total pre-reward account balance and a nominal two-day epoch; it is not validator\n\
             or staking APY.\n\
             JSON emits one report object; interactive mode launches the dashboard.",
        )
        .arg(
            Arg::new("address")
                .short('a')
                .long("address")
                .value_name("ADDRESS")
                .required(true)
                .help("Wallet/authority or native stake-account address"),
        )
        .arg(flag("json", "Print one JSON result without a TUI"))
        .arg(
            flag("offline", "Read saved observations without network calls")
                .conflicts_with("refresh"),
        )
        .arg(flag(
            "refresh",
            "Revalidate observations and the 30-epoch reward window",
        ))
        .arg(flag("no-color", "Use monochrome terminal output"))
        .try_get_matches_from(args);

    let matches = match matches {
        Ok(matches) => matches,
        Err(error)
            if matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) =>
        {
            return Ok(Launch::Text(error.to_string()));
        }
        // Clap's detailed error can echo arbitrary arguments, including a misplaced secret.
        Err(_) => {
            return Err(LaunchError::input(
                "INVALID_ARGUMENTS",
                "Invalid arguments. Supply -a ADDRESS, do not combine --offline with --refresh, and note the reward window is fixed (no --epochs). Run ssteak --help for usage.",
            ));
        }
    };
    let address = matches
        .get_one::<String>("address")
        .expect("required by clap");
    let mut decoded = [0u8; 32];
    if !(32..=44).contains(&address.len()) || bs58::decode(address).onto(&mut decoded) != Ok(32) {
        return Err(LaunchError::input(
            "INVALID_ADDRESS",
            "Address must be Base58 encoding exactly 32 bytes. Run ssteak --help for usage.",
        ));
    }
    Ok(Launch::Inspect(Options {
        address: address.clone(),
        json: matches.get_flag("json"),
        offline: matches.get_flag("offline"),
        refresh: matches.get_flag("refresh"),
        no_color: matches.get_flag("no-color"),
    }))
}

fn flag(name: &'static str, help: &'static str) -> Arg {
    Arg::new(name)
        .long(name)
        .action(ArgAction::SetTrue)
        .help(help)
}

impl Options {
    pub fn validate_environment(
        &self,
        api_key: Option<&OsStr>,
        usable_terminal: bool,
    ) -> Result<(), LaunchError> {
        if !self.offline {
            let key = api_key.ok_or_else(|| {
                LaunchError::input(
                    "MISSING_API_KEY",
                    "Set HELIUS_API_KEY in your environment, or use --offline for saved data.",
                )
            })?;
            if key.to_str().is_none_or(|value| {
                value.is_empty() || value.chars().any(|c| c.is_whitespace() || c.is_control())
            }) {
                return Err(LaunchError::input(
                    "INVALID_API_KEY",
                    "HELIUS_API_KEY must be nonempty text without whitespace or control characters.",
                ));
            }
        }
        if !self.json && !usable_terminal {
            return Err(LaunchError::input(
                "TERMINAL_REQUIRED",
                "Interactive mode needs terminal stdin/stdout and a usable TERM. Use --json for noninteractive output.",
            ));
        }
        Ok(())
    }
}
