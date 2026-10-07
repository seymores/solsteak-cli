use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

const ADDRESS: &str = "11111111111111111111111111111111";
const SECRET: &str = "test-secret-that-must-not-be-printed";
static NEXT_HOME: AtomicU64 = AtomicU64::new(0);

fn run(args: &[&str], key: Option<&str>) -> Output {
    let home = std::env::temp_dir().join(format!(
        "ssteak-cli-{}-{}",
        std::process::id(),
        NEXT_HOME.fetch_add(1, Ordering::Relaxed)
    ));
    let mut command = Command::new(env!("CARGO_BIN_EXE_ssteak"));
    command
        .args(args)
        .env("HOME", home)
        .env_remove("HELIUS_API_KEY")
        .env("TERM", "xterm-256color");
    if let Some(key) = key {
        command.env("HELIUS_API_KEY", key);
    }
    command.output().expect("run CLI")
}

fn json_error(output: &Output, exit: i32, code: &str) -> serde_json::Value {
    assert_eq!(output.status.code(), Some(exit), "{output:?}");
    assert!(
        output.stderr.is_empty(),
        "JSON diagnostics are unnecessary here: {output:?}"
    );
    assert!(!output.stdout.contains(&0x1b));
    let report: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("exactly one JSON value");
    assert_eq!(report["schema_version"], 2);
    assert_eq!(report["status"], "error");
    assert_eq!(report["errors"][0]["code"], code);
    assert!(report["data"].is_null());
    assert!(report["network"]["genesis_hash"].is_null());
    assert!(!String::from_utf8_lossy(&output.stdout).contains(SECRET));
    report
}

#[test]
fn help_and_version_work_without_credentials_or_terminal_even_with_json() {
    for flag in ["--help", "-h", "--version", "-V"] {
        let output = run(&["--json", flag], None);
        assert!(output.status.success(), "{output:?}");
        assert!(output.stderr.is_empty());
        assert!(!output.stdout.contains(&0x1b));
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.contains("ssteak"));
        if flag.contains("help") || flag == "-h" {
            for hint in [
                "--address",
                "--offline",
                "--refresh",
                "--no-color",
                "--json",
                "HELIUS_API_KEY",
                "latest 30 completed epochs",
                "annualized account return estimate",
                "historical validator attribution is unverified",
            ] {
                assert!(text.contains(hint), "missing {hint}: {text}");
            }
        } else {
            assert!(text.contains(env!("CARGO_PKG_VERSION")));
        }
    }
}

#[test]
fn missing_address_is_an_actionable_plain_error() {
    let output = run(&[], None);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("--help"));
    assert!(!stderr.contains('\x1b'));
}

#[test]
fn malformed_json_arguments_are_one_safe_object() {
    for args in [
        vec!["--json"],
        vec!["--unknown", SECRET, "--json"],
        vec!["--address", "--json"],
        vec!["--json=true", "--json"],
        vec!["-a", ADDRESS, "-a", ADDRESS, "--json"],
        vec!["-a", ADDRESS, "--offline", "--refresh", "--json"],
    ] {
        let report = json_error(&run(&args, Some(SECRET)), 2, "INVALID_ARGUMENTS");
        assert!(report["input"].is_null());
    }
}

#[test]
fn removed_epochs_flag_is_rejected_before_side_effects() {
    for args in [
        vec!["--epochs", "1"],
        vec!["--epochs", "15"],
        vec!["--epochs=15"],
        vec!["--epochs"],
    ] {
        let mut full = vec!["-a", ADDRESS, "--json"];
        full.extend(args);
        let report = json_error(&run(&full, None), 2, "INVALID_ARGUMENTS");
        assert!(report["input"].is_null());
    }
    let output = run(&["--help"], None);
    assert!(
        !String::from_utf8(output.stdout)
            .unwrap()
            .contains("--epochs")
    );
}

#[test]
fn invalid_addresses_are_rejected_without_echoing_them() {
    let long = "1".repeat(1000);
    for address in [
        "",
        "0OIl",
        "abc",
        "1111111111111111111111111111111",
        "111111111111111111111111111111111",
        &long,
        SECRET,
        "\x1b[31m",
    ] {
        let report = json_error(
            &run(&["--address", address, "--json"], Some(SECRET)),
            2,
            "INVALID_ADDRESS",
        );
        assert!(report["input"].is_null());
    }
}

#[test]
fn online_requires_a_valid_key_but_offline_ignores_it() {
    json_error(&run(&["-a", ADDRESS, "--json"], None), 2, "MISSING_API_KEY");
    for key in ["", " ", "bad key", "bad\nkey"] {
        json_error(
            &run(&["-a", ADDRESS, "--json"], Some(key)),
            2,
            "INVALID_API_KEY",
        );
        json_error(
            &run(&["-a", ADDRESS, "--json", "--offline"], Some(key)),
            4,
            "OFFLINE_MISS",
        );
    }
}

#[test]
fn piped_interactive_mode_explains_explicit_json_mode() {
    for (args, key) in [
        (vec!["-a", ADDRESS], Some(SECRET)),
        (vec!["-a", ADDRESS, "--offline"], None),
    ] {
        let output = run(&args, key);
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("--json"));
        assert!(!output.stderr.contains(&0x1b));
    }
}

#[test]
fn valid_online_options_parse_without_contacting_a_provider() {
    // Arbitrary 32-byte keys must be accepted without an on-curve restriction.
    let arbitrary = bs58::encode([255u8; 32]).into_string();
    for address in [ADDRESS, arbitrary.as_str()] {
        let args = ["-a", address, "--json", "--no-color"]
            .into_iter()
            .map(std::ffi::OsString::from)
            .collect::<Vec<_>>();
        let ssteak::cli::Launch::Inspect(options) = ssteak::cli::parse(&args).unwrap() else {
            panic!("expected options")
        };
        assert_eq!(options.address, address);
        assert!(options.json);
    }
    let report = json_error(
        &run(&["-a", ADDRESS, "--json", "--offline"], None),
        4,
        "OFFLINE_MISS",
    );
    assert!(report["input"].get("epochs").is_none());
    assert_eq!(report["input"]["offline"], true);
}

#[test]
fn json_after_end_of_options_does_not_select_output_mode() {
    let output = run(&["--", "--json"], None);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
}

#[cfg(unix)]
#[test]
fn non_unicode_arguments_and_credentials_do_not_panic_or_leak() {
    use std::os::unix::ffi::OsStringExt;
    let bad = std::ffi::OsString::from_vec(vec![0xff]);
    let output = Command::new(env!("CARGO_BIN_EXE_ssteak"))
        .args(["--json", "-a"])
        .arg(&bad)
        .env_remove("HELIUS_API_KEY")
        .output()
        .unwrap();
    json_error(&output, 2, "INVALID_ARGUMENTS");
    let output = Command::new(env!("CARGO_BIN_EXE_ssteak"))
        .args(["--json", "-a", ADDRESS])
        .env("HELIUS_API_KEY", bad)
        .output()
        .unwrap();
    json_error(&output, 2, "INVALID_API_KEY");
}
