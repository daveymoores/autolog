use dotenv::dotenv;
use std::env;

fn main() {
    dotenv().ok();

    let autolog_uri = env::var("AUTOLOG_URI").expect("AUTOLOG_URI");
    let expire_time_seconds = env::var("EXPIRE_TIME_SECONDS").expect("EXPIRE_TIME_SECONDS");
    let mongodb_db = env::var("MONGODB_DB").expect("MONGODB_DB");
    let mongodb_collection = env::var("MONGODB_COLLECTION").expect("MONGODB_COLLECTION");
    let test_mode = env::var("TEST_MODE").expect("TEST_MODE");
    let api_endpoint = env::var("API_ENDPOINT").expect("API_ENDPOINT");
    let api_key = env::var("API_ROUTE_BEARER_KEY").expect("API_ROUTE_BEARER_KEY");

    println!("cargo:rustc-env=AUTOLOG_URI={}", autolog_uri);
    println!(
        "cargo:rustc-env=EXPIRE_TIME_SECONDS={}",
        expire_time_seconds
    );
    println!("cargo:rustc-env=MONGODB_DB={}", mongodb_db);
    println!("cargo:rustc-env=MONGODB_COLLECTION={}", mongodb_collection);
    println!("cargo:rustc-env=TEST_MODE={}", test_mode);
    println!("cargo:rustc-env=API_ENDPOINT={}", api_endpoint);
    println!("cargo:rustc-env=API_ROUTE_BEARER_KEY={}", api_key);
}
