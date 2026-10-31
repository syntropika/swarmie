pub const GLOBAL_API_BASE_URL: &str = "https://api.minimax.io/v1";
pub const CHINA_API_BASE_URL: &str = "https://api.minimax.cn/v1";
pub const GLOBAL_ANTHROPIC_API_BASE_URL: &str = "https://api.minimax.io/anthropic";
pub const CHINA_ANTHROPIC_API_BASE_URL: &str = "https://api.minimax.cn/anthropic";

pub const MINIMAX_M3: &str = "MiniMax-M3";
pub const MINIMAX_M2_7: &str = "MiniMax-M2.7";
pub const MINIMAX_M2_7_HIGHSPEED: &str = "MiniMax-M2.7-highspeed";
pub const MINIMAX_M2_5: &str = "MiniMax-M2.5";
pub const MINIMAX_M2_5_HIGHSPEED: &str = "MiniMax-M2.5-highspeed";

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Family {
    M31Flash,
    M3,
    Other,
}

pub(crate) fn family(model: &str) -> Family {
    let model = model.to_ascii_lowercase();
    match model.as_str() {
        id if id.starts_with("minimax-m3.1") => Family::M31Flash,
        id if id.starts_with("minimax-m3") => Family::M3,
        _ => Family::Other,
    }
}

pub(crate) fn reads_images(model: &str) -> bool {
    !model.to_ascii_lowercase().starts_with("minimax-m2")
}

pub mod extension;
pub const PROVIDER_NAME: &str = "minimax";
