use assert_cmd::Command;

#[test]
fn runs_binary_with_a_command_that_doesnt_exist() -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd = Command::cargo_bin("autolog")?;
    cmd.arg("foo");
    cmd.assert().failure();

    Ok(())
}

#[test]
fn runs_binary_with_a_command_that_does_exist() -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd = Command::cargo_bin("autolog")?;
    cmd.arg("--help");
    cmd.assert().success();

    Ok(())
}

#[test]
#[ignore]
fn runs_init_for_repo_outside_of_current_directory() {
    let mut cmd = Command::cargo_bin("autolog").unwrap();
    let assert = cmd
        .env("TEST_MODE", "true")
        .arg("init")
        .arg("--path=./tests")
        .assert();

    assert
        .failure()
        .stdout("Initialising new repository.\nWould you like to add it to any of these existing clients?\n");
}

#[test]
#[ignore]
fn runs_init_for_path_that_doesnt_exist() {
    let mut cmd = Command::cargo_bin("autolog").unwrap();
    let assert = cmd
        .env("TEST_MODE", "true")
        .arg("init")
        .arg("--path=/not/a/path")
        .assert();

    assert
        .failure()
        .stdout("No repositories found at path. Please check that the path is valid.\n");
}

#[test]
#[ignore]
fn runs_init_with_args() {
    let mut cmd = Command::cargo_bin("autolog").unwrap();
    let assert = cmd.env("TEST_MODE", "true").arg("init").assert();

    assert.success().stdout(
        "\u{1F916} autolog has already been initialised for this repository.\n\
        Try \'autolog make\' to create your first timesheet \n\
        or \'autolog help\' for more options.\n",
    );
}

#[test]
#[ignore]
fn runs_make_with_success() -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd = Command::cargo_bin("autolog")?;
    let assert = cmd.env("TEST_MODE", "true").arg("make").assert();
    assert.failure().stdout(
        "Finding project data for \'apple\'...\nDoes \'autolog\' require a project/PO number?\n",
    );
    Ok(())
}

#[test]
fn runs_remove_with_failure() {
    let mut cmd = Command::cargo_bin("autolog").unwrap();
    let assert = cmd
        .env("TEST_MODE", "true")
        .arg("remove")
        .arg("--client=doesn't exist")
        .assert();

    assert
        .failure()
        .stderr("The client, or client + namespace combination you passed has not be found.\n");
}

#[test]
fn runs_remove_and_prompts_to_remove_client() -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd = Command::cargo_bin("autolog")?;
    let assert = cmd
        .env("TEST_MODE", "true")
        .arg("remove")
        .arg("--client=apple")
        .assert();

    assert.success();

    Ok(())
}

#[test]
fn runs_remove_and_prompts_to_remove_namespace() -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd = Command::cargo_bin("autolog")?;
    let assert = cmd
        .env("TEST_MODE", "true")
        .arg("remove")
        .arg("--client=apple")
        .arg("--namespace=autolog")
        .assert();

    assert.success();

    Ok(())
}
