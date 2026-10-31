mod build_support;

use build_support::{MAX_BYTES, SOURCE_URL, snapshot};
use std::{
    env,
    error::Error,
    fs,
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=build_support.rs");
    println!("cargo:rerun-if-changed=model_schema.rs");
    println!("cargo:rerun-if-env-changed=SWARMIE_MODEL_CATALOG_REFRESH");
    let config = ureq::Agent::config_builder()
        .https_only(true)
        .max_redirects(0)
        .timeout_global(Some(Duration::from_secs(30)))
        .build();
    let agent = ureq::Agent::new_with_config(config);
    let mut response = agent
        .get(SOURCE_URL)
        .header("User-Agent", "swarmie-model-catalog")
        .call()?;
    if response.status().as_u16() != 200 {
        return Err("catalog download did not return HTTP 200".into());
    }
    let raw = response
        .body_mut()
        .with_config()
        .limit(MAX_BYTES)
        .read_to_vec()?;
    let now_ms = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
    let bytes = snapshot(&raw, u64::try_from(now_ms)?)?;
    // Cargo serializes this package's build script. Validation completes before
    // the previous build output is replaced; a failure aborts compilation.
    let directory = PathBuf::from(env::var_os("OUT_DIR").ok_or("missing Cargo OUT_DIR")?);
    let temporary = directory.join("models-dev.json.tmp");
    fs::write(&temporary, bytes)?;
    fs::rename(temporary, directory.join("models-dev.json"))?;
    Ok(())
}
