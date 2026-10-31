pub const GENERAL_API_BASE_URL: &str = "https://api.z.ai/api/paas/v4";
pub const CODING_API_BASE_URL: &str = "https://api.z.ai/api/coding/paas/v4";
pub const ANTHROPIC_API_BASE_URL: &str = "https://api.z.ai/api/anthropic";

pub const GLM_4_6: &str = "glm-4.6";
pub const GLM_4_6_AIR: &str = "glm-4.6-air";
pub const GLM_4_6_X: &str = "glm-4.6-x";
pub const GLM_4_5: &str = "glm-4.5";
pub const GLM_4_5_AIR: &str = "glm-4.5-air";
pub const GLM_4_5V: &str = "glm-4.5v";
pub const GLM_4_5_AIRX: &str = "glm-4.5-airx";

pub(crate) fn reads_images(model: &str) -> bool {
    model.strip_prefix("glm-").is_some_and(|rest| {
        rest.trim_start_matches(|c: char| c.is_ascii_digit() || c == '.')
            .starts_with('v')
    })
}

pub mod extension;

pub const PROVIDER_NAME: &str = "zai";
