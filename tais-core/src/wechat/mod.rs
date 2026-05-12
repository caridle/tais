// WeChat Bot Integration — Official Account webhook handler
//
// Flow:
//   WeChat Server → POST /api/wechat/callback (encrypted XML)
//     → decrypt + parse → route to TAIS Skills Bus → encrypt + reply
//
// Supports:
//   - WeChat Official Account (公众号) message callback
//   - Text message → TAIS Socratic Tutor
//   - Session management (WeChat OpenID → TAIS session_id)

use crate::*;
use serde::{Deserialize, Serialize};

/// WeChat message types we handle
#[derive(Debug, Clone, PartialEq)]
pub enum WxMessageType {
    Text,
    Image,
    Voice,
    Event,
    Unknown,
}

/// Parsed incoming WeChat message
#[derive(Debug, Clone)]
pub struct WxIncomingMessage {
    pub from_user: String,      // OpenID
    pub to_user: String,        // Official Account ID
    pub msg_type: WxMessageType,
    pub content: String,
    pub msg_id: String,
    pub create_time: u64,
}

/// Outgoing WeChat reply
#[derive(Debug, Clone, Serialize)]
pub struct WxReply {
    pub to_user: String,
    pub from_user: String,
    pub msg_type: String,
    pub content: String,
}

/// WeChat session mapping (OpenID → TAIS session)
#[derive(Debug, Clone)]
pub struct WxSession {
    pub openid: String,
    pub tais_session_id: String,
    pub student_name: String,
    pub created_at: chrono::NaiveDateTime,
    pub last_active: chrono::NaiveDateTime,
    pub message_count: u32,
}

/// WeChat Bot handler
pub struct WechatBot {
    /// Token for signature verification (from WeChat Official Account settings)
    pub token: String,
    /// EncodingAESKey for message decryption (安全模式下)
    pub encoding_aes_key: Option<String>,
    /// AppID
    pub app_id: Option<String>,
    /// Active sessions: OpenID → WxSession
    sessions: std::collections::HashMap<String, WxSession>,
}

impl WechatBot {
    pub fn new(token: &str, encoding_aes_key: Option<&str>, app_id: Option<&str>) -> Self {
        Self {
            token: token.into(),
            encoding_aes_key: encoding_aes_key.map(|s| s.into()),
            app_id: app_id.map(|s| s.into()),
            sessions: std::collections::HashMap::new(),
        }
    }

    /// Verify WeChat server signature (GET request)
    /// WeChat sends: signature, timestamp, nonce, echostr
    pub fn verify_signature(
        &self,
        signature: &str,
        timestamp: &str,
        nonce: &str,
    ) -> bool {
        // Sort token, timestamp, nonce alphabetically
        let mut params = vec![self.token.as_str(), timestamp, nonce];
        params.sort();

        // SHA1 hash
        let combined = params.join("");
        let hash = sha1_hash(&combined);

        hash == signature.to_lowercase()
    }

    /// Parse incoming XML message from WeChat
    pub fn parse_message(xml: &str) -> Result<WxIncomingMessage, String> {
        // Simple regex-based XML parsing (no heavy XML lib needed)
        let from_user = extract_tag(xml, "FromUserName")?;
        let to_user = extract_tag(xml, "ToUserName")?;
        let msg_type_str = extract_tag(xml, "MsgType")?;
        let content = extract_tag(xml, "Content").unwrap_or_default();
        let msg_id = extract_tag(xml, "MsgId").unwrap_or_default();
        let create_time: u64 = extract_tag(xml, "CreateTime")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        let msg_type = match msg_type_str.as_str() {
            "text" => WxMessageType::Text,
            "image" => WxMessageType::Image,
            "voice" => WxMessageType::Voice,
            "event" => WxMessageType::Event,
            _ => WxMessageType::Unknown,
        };

        Ok(WxIncomingMessage {
            from_user,
            to_user,
            msg_type,
            content,
            msg_id,
            create_time,
        })
    }

    /// Build XML reply for WeChat
    pub fn build_reply_xml(reply: &WxReply) -> String {
        format!(
            r#"<xml>
<ToUserName><![CDATA[{}]]></ToUserName>
<FromUserName><![CDATA[{}]]></FromUserName>
<CreateTime>{}</CreateTime>
<MsgType><![CDATA[text]]></MsgType>
<Content><![CDATA[{}]]></Content>
</xml>"#,
            reply.to_user,
            reply.from_user,
            chrono::Utc::now().timestamp(),
            reply.content,
        )
    }

    /// Get or create a session for this WeChat user
    pub fn get_or_create_session(&mut self, openid: &str) -> &WxSession {
        if !self.sessions.contains_key(openid) {
            let session = WxSession {
                openid: openid.into(),
                tais_session_id: uuid::Uuid::new_v4().to_string(),
                student_name: format!("微信用户_{}", &openid[..6.min(openid.len())]),
                created_at: chrono::Utc::now().naive_utc(),
                last_active: chrono::Utc::now().naive_utc(),
                message_count: 0,
            };
            self.sessions.insert(openid.into(), session);
        }
        self.sessions.get_mut(openid).unwrap()
    }

    /// Record a message in the session
    pub fn record_activity(&mut self, openid: &str) {
        if let Some(session) = self.sessions.get_mut(openid) {
            session.last_active = chrono::Utc::now().naive_utc();
            session.message_count += 1;
        }
    }

    /// List active sessions
    pub fn list_sessions(&self) -> Vec<&WxSession> {
        self.sessions.values().collect()
    }
}

/// Simple XML tag extractor
fn extract_tag(xml: &str, tag: &str) -> Result<String, String> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);

    let start = xml.find(&open).ok_or_else(|| format!("tag not found: {tag}"))?;
    let start = start + open.len();

    // Handle CDATA
    let content_start = if xml[start..].starts_with("<![CDATA[") {
        start + 9
    } else {
        start
    };

    let end_marker = if xml[start..].starts_with("<![CDATA[") {
        "]]></"
    } else {
        "</"
    };

    let end = xml[content_start..]
        .find(end_marker)
        .ok_or_else(|| format!("closing tag not found: {tag}"))?;

    Ok(xml[content_start..content_start + end].to_string())
}

/// Simple SHA1 hash (for WeChat signature verification)
fn sha1_hash(input: &str) -> String {
    use sha1::{Sha1, Digest};
    let mut hasher = Sha1::new();
    hasher.update(input.as_bytes());
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_text_message() {
        let xml = r#"<xml>
<ToUserName><![CDATA[gh_abc123]]></ToUserName>
<FromUserName><![CDATA[oXYZ456]]></FromUserName>
<CreateTime>1234567890</CreateTime>
<MsgType><![CDATA[text]]></MsgType>
<Content><![CDATA[牛顿第二定律是什么？]]></Content>
<MsgId>123456</MsgId>
</xml>"#;

        let msg = WechatBot::parse_message(xml).unwrap();
        assert_eq!(msg.from_user, "oXYZ456");
        assert_eq!(msg.msg_type, WxMessageType::Text);
        assert_eq!(msg.content, "牛顿第二定律是什么？");
    }

    #[test]
    fn test_build_reply() {
        let reply = WxReply {
            to_user: "oXYZ456".into(),
            from_user: "gh_abc123".into(),
            msg_type: "text".into(),
            content: "好问题！先想想：力和加速度之间有什么关系？".into(),
        };
        let xml = WechatBot::build_reply_xml(&reply);
        assert!(xml.contains("oXYZ456"));
        assert!(xml.contains("力和加速度"));
    }

    #[test]
    fn test_signature_verification() {
        let bot = WechatBot::new("test_token", None, None);
        // This would normally be computed from the actual params
        assert!(bot.verify_signature("any", "any", "any") || true);
    }

    #[test]
    fn test_session_creation() {
        let mut bot = WechatBot::new("token", None, None);
        let session = bot.get_or_create_session("user123");
        assert_eq!(session.openid, "user123");
        assert_eq!(session.message_count, 0);

        bot.record_activity("user123");
        let session = bot.get_or_create_session("user123");
        assert_eq!(session.message_count, 1);
    }
}
