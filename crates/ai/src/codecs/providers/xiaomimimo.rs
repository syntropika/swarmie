pub const API_BASE_URL: &str = "https://api.xiaomimimo.com/v1";
pub const ANTHROPIC_API_BASE_URL: &str = "https://api.xiaomimimo.com/anthropic";

pub const MIMO_V2_FLASH: &str = "mimo-v2-flash";
pub const MIMO_V2_OMNI: &str = "mimo-v2-omni";
pub const MIMO_V2_PRO: &str = "mimo-v2-pro";
pub const MIMO_V2_5: &str = "mimo-v2.5";
pub const MIMO_V2_5_PRO: &str = "mimo-v2.5-pro";

pub(crate) fn reads_images(model: &str) -> bool {
    !((model.starts_with("mimo-v2-") && !model.contains("omni"))
        || model.starts_with("mimo-v2.5-pro"))
}

pub mod extension;

pub const PROVIDER_NAME: &str = "xiaomimimo";
