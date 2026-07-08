use super::config_processor::map_error as map_config_error;
use crate::config_manager::ConfigManager;
use codex_app_server_protocol::ConfigValueWriteParams;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_app_server_protocol::MergeStrategy;
use codex_model_provider::AMAZON_BEDROCK_PROVIDER_ID;

pub(super) async fn set_user_model_provider_to_bedrock(
    config_manager: &ConfigManager,
) -> Result<(), JSONRPCErrorError> {
    config_manager
        .write_value(ConfigValueWriteParams {
            key_path: "model_provider".to_string(),
            value: serde_json::json!(AMAZON_BEDROCK_PROVIDER_ID),
            merge_strategy: MergeStrategy::Replace,
            file_path: None,
            expected_version: None,
        })
        .await
        .map(|_| ())
        .map_err(map_config_error)
}

pub(super) async fn clear_user_model_provider_if_bedrock(
    config_manager: &ConfigManager,
) -> Result<(), JSONRPCErrorError> {
    config_manager
        .clear_user_value_if_matches(
            "model_provider",
            serde_json::json!(AMAZON_BEDROCK_PROVIDER_ID),
        )
        .await
        .map_err(map_config_error)
}
