pub const GLOBAL_API_BASE_URL: &str = "https://api.moonshot.ai/v1";
pub const CHINA_API_BASE_URL: &str = "https://api.moonshot.cn/v1";
pub const ANTHROPIC_API_BASE_URL: &str = "https://api.moonshot.ai/anthropic";
pub const CHINA_ANTHROPIC_API_BASE_URL: &str = "https://api.moonshot.cn/anthropic";

pub const KIMI_K3: &str = "kimi-k3";

pub const KIMI_K2_7_CODE: &str = "kimi-k2.7-code";

pub const KIMI_K2_7_CODE_HIGHSPEED: &str = "kimi-k2.7-code-highspeed";

pub const KIMI_K2_6: &str = "kimi-k2.6";

pub(crate) fn reads_images(model: &str) -> bool {
    !(model == "kimi-k2"
        || model.starts_with("kimi-k2-")
        || (model.starts_with("moonshot-v1") && !model.contains("vision")))
}

pub mod extension;

pub const PROVIDER_NAME: &str = "moonshot";
