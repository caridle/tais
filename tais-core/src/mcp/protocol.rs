// MCP protocol handler — JSON-RPC request/response lifecycle

use crate::rpc;
use super::{Gateway, GatewayError};

/// Process a raw MCP JSON-RPC request through the gateway
pub async fn handle_request(
    gateway: &Gateway,
    raw: &str,
) -> Result<String, GatewayError> {
    let request: rpc::Request = serde_json::from_str(raw)
        .map_err(|e| GatewayError::ProtocolError(format!("invalid json-rpc: {e}")))?;

    let response = match request.method.as_str() {
        "tools/list" => {
            let tools = gateway.list_tools().await;
            serde_json::to_value(tools)
            .map(|v| rpc::Response::success(request.id, v))
            .unwrap_or_else(|e| rpc::Response::error(request.id, -32603, &format!("serialization: {e}")))
        }
        "tools/call" => {
            let params = request.params.ok_or_else(|| {
                GatewayError::ProtocolError("tools/call requires params".into())
            })?;
            let tool_name = params["name"]
                .as_str()
                .ok_or_else(|| GatewayError::ProtocolError("missing tool name".into()))?;
            let args = params.get("arguments").cloned().unwrap_or(serde_json::json!({}));

            match gateway.call_tool(tool_name, args).await {
                Ok(result) => rpc::Response::success(request.id, result),
                Err(e) => rpc::Response::error(request.id, -32000, &e.to_string()),
            }
        }
        "initialize" => {
            // MCP handshake
            rpc::Response::success(
                request.id,
                serde_json::json!({
                    "protocolVersion": "2024-11-05",
                    "serverInfo": {
                        "name": "tais-core",
                        "version": "0.1.0"
                    },
                    "capabilities": {
                        "tools": {}
                    }
                }),
            )
        }
        _ => rpc::Response::error(request.id, -32601, &format!("unknown method: {}", request.method)),
    };

    serde_json::to_string(&response)
        .map_err(|e| GatewayError::ProtocolError(format!("serialize error: {e}")))
}
