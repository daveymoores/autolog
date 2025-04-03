pub mod date;
pub mod db;
pub mod file;
pub mod link;

use dialoguer::Confirm;
use dotenv::dotenv;
use std::env;
use std::error::Error;
use std::process::Output;

pub fn confirm() -> Result<bool, Box<dyn Error>> {
    if is_test_mode() {
        return Ok(true);
    }

    Ok(Confirm::new().default(true).interact()?)
}

pub fn is_test_mode() -> bool {
    // Load environment variables from .env file if available
    let _ = dotenv();

    // Check for TEST_MODE environment variable first
    match env::var("TEST_MODE") {
        Ok(value) => {
            // Try to parse as boolean; any of "1", "true", "True", "TRUE" will work
            value.parse::<bool>().unwrap_or_else(|_| {
                // If not a valid boolean, check for "1"
                value == "1"
            })
        }
        // If TEST_MODE isn't set in environment, check if we're in a test context
        Err(_) => cfg!(test),
    }
}

pub fn exit_process() {
    if !is_test_mode() {
        std::process::exit(exitcode::OK);
    }
}

pub fn trim_output_from_utf8(output: Output) -> Result<String, Box<dyn std::error::Error>> {
    let x = String::from_utf8(output.stdout)?.trim().parse().unwrap();
    Ok(x)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::os::unix::process::ExitStatusExt;
    use std::process::{ExitStatus, Output};

    use envtestkit::lock::lock_test;
    use envtestkit::set_env;

    use super::*;

    #[test]
    fn it_returns_test_mode_is_true() {
        let _lock = lock_test();
        let _test = set_env(OsString::from("TEST_MODE"), "true");

        assert_eq!(is_test_mode(), true);
    }

    #[test]
    fn it_returns_test_mode_is_false() {
        let _lock = lock_test();
        let _test = set_env(OsString::from("TEST_MODE"), "false");

        assert_eq!(is_test_mode(), false);
    }

    #[test]
    fn it_trims_output_from_utf8() {
        let output_path = Output {
            status: ExitStatus::from_raw(0),
            stdout: vec![68, 97, 118, 101, 121, 32, 77, 111, 111, 114, 101, 115, 10],
            stderr: vec![],
        };

        assert_eq!(trim_output_from_utf8(output_path).unwrap(), "Davey Moores");
    }
}
