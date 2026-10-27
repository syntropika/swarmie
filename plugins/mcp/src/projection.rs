use crate::error::projection;
use rmcp::model::{CallToolResult, ContentBlock, ResourceContents};
use swarmie_tools::ToolResult;

/// Strip only protocol metadata, preserving fields in structured business data.
/// The unmodified, typed response remains in the original operation receipt.
pub(crate) fn model_result(response: &CallToolResult) -> tembra::Result<ToolResult> {
    let mut response = response.clone();
    response.meta = None;
    for content in &mut response.content {
        match content {
            ContentBlock::Text(content) => content.meta = None,
            ContentBlock::Image(content) => content.meta = None,
            ContentBlock::Audio(content) => content.meta = None,
            ContentBlock::ResourceLink(resource) => resource.meta = None,
            ContentBlock::Resource(content) => {
                content.meta = None;
                match &mut content.resource {
                    ResourceContents::TextResourceContents { meta, .. }
                    | ResourceContents::BlobResourceContents { meta, .. } => *meta = None,
                    _ => return Err(projection("unsupported resource content")),
                }
            }
            _ => return Err(projection("unsupported protocol content")),
        }
    }
    let failed = response.is_error == Some(true);
    let value = serde_json::to_value(response).map_err(projection)?;
    Ok(if failed {
        ToolResult::Failed(value.to_string())
    } else {
        ToolResult::Success(value)
    })
}
