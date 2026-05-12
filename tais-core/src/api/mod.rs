// API layer — Axum HTTP + WebSocket server
//
// Endpoints:
//   GET  /                              — Dashboard HTML page
//   GET  /api/health                    — Health check
//   POST /api/workflow/generate         — Generate teaching workflow
//   POST /api/workflow/execute          — Execute workflow
//   GET  /api/evolution/metrics         — Evolution metrics
//   POST /api/evolution/review          — Teacher review evolution
//   GET  /api/skills/list               — List TAIS skills
//   GET  /api/mcp/tools                 — List MCP tools
//   WS   /api/session/{id}              — Student real-time session
//
//   GET    /api/llm/configs             — List all LLM configs
//   POST   /api/llm/configs             — Create LLM config
//   GET    /api/llm/configs/{id}        — Get single config
//   PUT    /api/llm/configs/{id}        — Update LLM config
//   DELETE /api/llm/configs/{id}        — Delete LLM config
//   POST   /api/llm/configs/{id}/test   — Test LLM connection

use crate::*;
use crate::auth;
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, Query, State,
    },
    response::{Html, IntoResponse, Json, Redirect},
    routing::{get, post, put, delete},
    Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

/// Shared application state
pub struct AppState {
    pub orchestrator: Arc<orchestrator::Orchestrator>,
    pub mcp_gateway: Arc<mcp::Gateway>,
    pub evolution_engine: Arc<evolution::EvolutionEngine>,
    pub skills_bus: Arc<skills::SkillsBus>,
    pub llm_router: Arc<llm::LlmRouter>,
    pub wechat_bot: Arc<tokio::sync::Mutex<wechat::WechatBot>>,
    pub memory: Arc<memory::SessionMemory>,
    pub task_manager: Arc<orchestrator::task::TaskManager>,
    pub task_dispatcher: Arc<orchestrator::task::TaskDispatcher>,
    pub agent_loop: Arc<agent::AgentLoop>,
    pub auth_state: Arc<auth::AuthState>,
    pub habit_engine: Arc<habit::HabitEngine>,
    pub db_pool: Option<sqlx::SqlitePool>,
    pub active_ws_count: AtomicU32,
}

#[derive(Debug, Deserialize)]
pub struct GenerateRequest {
    pub goal: String,
    pub teacher_id: String,
    pub gene_profile: Option<GeneProfile>,
}

#[derive(Debug, Serialize)]
pub struct GenerateResponse {
    pub workflow_id: String,
    pub nodes: Vec<WorkflowNode>,
    pub edges: Vec<(String, String)>,
    pub gene_applied: GeneProfile,
}

/// Build the Axum router
pub fn build_router(state: Arc<AppState>) -> Router {
    Router::new()
        // Dashboard
        .route("/", get(dashboard))
        // Health
        .route("/api/health", get(health_check))
        // Workflow
        .route("/api/workflow/generate", post(generate_workflow))
        .route("/api/workflow/execute", post(execute_workflow))
        // Evolution
        .route("/api/evolution/metrics", get(get_metrics))
        .route("/api/evolution/review", post(review_evolution))
        // Skills & MCP
        .route("/api/skills/list", get(list_skills))
        .route("/api/skills/status", get(skills_status))
        .route("/api/skills/install", post(skills_install))
        .route("/api/skills/{name}", delete(skills_unregister))
        .route("/api/skills/{name}/install", delete(skills_uninstall))
        .route("/api/skills/{name}/register", post(skills_register))
        .route("/api/skills/{name}/execute", post(skills_execute))
        .route("/api/mcp/tools", get(list_mcp_tools))
        // LLM proxy (CORS bypass for model fetching)
        .route("/api/llm/fetch-models", post(fetch_llm_models))
        // Habits
        .route("/api/habits/list", get(habit_list))
        .route("/api/habits/state", get(habit_all_states))
        .route("/api/habits/{id}/status", get(habit_status))
        .route("/api/habits/{id}/trigger", post(habit_trigger))
        .route("/api/habits/{id}/logs", get(habit_logs))
        // Skill Crystallization + Working Memory
        .route("/api/skills/crystallize", post(skill_crystallize))
        .route("/api/skills/crystallized", get(skill_crystallized_list))
        .route("/api/skills/reload", post(skill_reload))
        .route("/api/agent/checkpoint", get(agent_get_checkpoint).post(agent_set_checkpoint).delete(agent_clear_checkpoint))
        // LLM CRUD
        .route("/api/llm/configs", get(list_llm_configs).post(create_llm_config))
        .route("/api/llm/configs/{id}", get(get_llm_config).put(update_llm_config).delete(delete_llm_config))
        .route("/api/llm/configs/{id}/test", post(test_llm_config))
        .route("/api/llm/status", get(llm_status_endpoint))
        // WeChat
        .route("/api/wechat/callback", get(wechat_verify).post(wechat_callback))
        .route("/api/wechat/sessions", get(wechat_sessions))
        // Memory — Sessions
        .route("/api/memory/sessions", get(memory_list_sessions))
        .route("/api/memory/sessions/{id}", get(memory_get_session))
        .route("/api/memory/search", get(memory_search))
        // Memory — Shared Knowledge
        .route("/api/memory/shared/knowledge", get(shared_knowledge_list).post(shared_knowledge_upsert))
        .route("/api/memory/shared/knowledge/search", get(shared_knowledge_search))
        .route("/api/memory/shared/faqs", get(shared_faqs_list).post(shared_faqs_add))
        .route("/api/memory/shared/faqs/search", get(shared_faqs_search))
        .route("/api/memory/shared/stats", get(shared_stats))
        .route("/api/memory/shared/strategies", get(shared_strategies_list).post(shared_strategies_add))
        // Memory — Users
        .route("/api/memory/users", get(user_list))
        .route("/api/memory/users/{id}", get(user_profile))
        .route("/api/memory/users/{id}/mastery", get(user_mastery))
        .route("/api/memory/users/{id}/sessions", get(user_sessions))
        .route("/api/memory/users/{id}/llm", get(user_llm_get).put(user_llm_set))
        // Per-user Dashboard
        .route("/dashboard/{user_id}", get(user_dashboard))
        // Task Orchestration
        .route("/api/tasks/workflow/{workflow_id}", get(task_list).post(task_create))
        .route("/api/tasks/{task_id}", put(task_update).delete(task_delete))
        .route("/api/tasks/{task_id}/start", post(task_start))
        .route("/api/tasks/{task_id}/complete", post(task_complete))
        .route("/api/tasks/{task_id}/interrupt", post(task_interrupt))
        .route("/api/tasks/workflow/{workflow_id}/summary", get(task_summary))
        // Agent Loop (自主教学闭环)
        .route("/api/agent/propose", post(agent_propose))
        .route("/api/agent/run", post(agent_run))
        .route("/api/agent/status", get(agent_status))
        .route("/api/agent/reset", post(agent_reset))
        // Auth
        .route("/api/auth/register", post(auth_register))
        .route("/api/auth/login", post(auth_login))
        .route("/api/auth/qr/request", post(auth_qr_request))
        .route("/api/auth/qr/status/{token}", get(auth_qr_status))
        .route("/api/auth/qr/confirm", post(auth_qr_confirm))
        .route("/api/auth/me", get(auth_me))
        .route("/api/auth/wechat-bind", post(wechat_bind_request))
        .route("/api/auth/wechat-bind-status", get(auth_wechat_bind_status))
        // Chat UI
        .route("/login", get(login_page))
        .route("/chat", get(chat_page))
        // WebSocket
        .route("/api/session/{session_id}", get(ws_session))
        .with_state(state)
}

// ── Dashboard ──────────────────────────────────────────────────────────

async fn dashboard(
    State(state): State<Arc<AppState>>,
    Query(params): Query<std::collections::HashMap<String, String>>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response<axum::body::Body> {
    // Check JWT token: 1) query param  2) cookie  3) Authorization header
    let token = params.get("token").map(|s| s.as_str()).or_else(|| {
        // Parse cookie header
        headers.get("cookie")
            .and_then(|v| v.to_str().ok())
            .and_then(|cookies| {
                cookies.split(';').find_map(|c| {
                    let c = c.trim();
                    if c.starts_with("tais_token=") {
                        Some(c.trim_start_matches("tais_token="))
                    } else { None }
                })
            })
    });
    let user_id = token.and_then(|t| state.auth_state.verify_jwt(t).ok());

    match user_id {
        Some(uid) => {
            let profile = state.memory.users.get_or_create(&uid).await;
            let sessions = state.memory.users.get_user_sessions(
                &state.memory.session_users, &uid,
            ).await;
            let html = dashboard::render_user_home(&profile, sessions.len());
            Html(html).into_response()
        }
        None => {
            // No valid token → show login page
            Redirect::to("/login").into_response()
        }
    }
}

// ── Health ─────────────────────────────────────────────────────────────

async fn health_check(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let llm_status = state.llm_router.status().await;
    Json(serde_json::json!({
        "status": "ok",
        "system": "TAIS Core Engine",
        "version": "0.1.0",
        "llm": {
            "total": llm_status.total_configs,
            "connected": llm_status.connected_count,
        },
        "active_ws": state.active_ws_count.load(Ordering::Relaxed),
    }))
}

// ── Workflow ────────────────────────────────────────────────────────────

async fn generate_workflow(
    State(state): State<Arc<AppState>>,
    Json(req): Json<GenerateRequest>,
) -> Json<GenerateResponse> {
    let goal = orchestrator::parser::parse_goal(&req.goal, &req.teacher_id)
        .unwrap_or_else(|| TeachingGoal {
            subject: "通用".into(),
            concept: req.goal.clone(),
            mode: TeachingMode::InquiryBased,
            target_level: StudentLevel::Intermediate,
            constraints: vec![],
            teacher_id: req.teacher_id,
        });

    let gene = req.gene_profile.unwrap_or_default();
    let workflow = state.orchestrator.generate(goal);

    Json(GenerateResponse {
        workflow_id: workflow.id.to_string(),
        nodes: workflow.nodes.clone(),
        edges: workflow.edges.clone(),
        gene_applied: gene,
    })
}

async fn execute_workflow(
    State(state): State<Arc<AppState>>,
    Json(req): Json<GenerateRequest>,
) -> Json<serde_json::Value> {
    let goal = orchestrator::parser::parse_goal(&req.goal, &req.teacher_id)
        .unwrap_or_else(|| TeachingGoal {
            subject: "通用".into(),
            concept: req.goal.clone(),
            mode: TeachingMode::InquiryBased,
            target_level: StudentLevel::Intermediate,
            constraints: vec![],
            teacher_id: req.teacher_id,
        });

    let workflow = state.orchestrator.generate(goal);
    let session_id = Uuid::new_v4();

    let mut ctx = orchestrator::executor::ExecutionContext::new(
        session_id,
        "student_demo".into(),
        workflow,
        state.skills_bus.clone(),
    );

    let (results, hitl_events) = ctx.execute_all(&state.orchestrator).await;

    Json(serde_json::json!({
        "session_id": session_id.to_string(),
        "node_results": results,
        "hitl_events": hitl_events,
    }))
}

// ── Evolution ───────────────────────────────────────────────────────────

async fn get_metrics(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let history = state.evolution_engine.get_history().await;
    Json(serde_json::json!({
        "evolution_rounds": history.len(),
        "history": history,
    }))
}

async fn review_evolution(
    State(state): State<Arc<AppState>>,
    Json(review): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    let agent = review["agent"].as_str().unwrap_or("unknown");
    let action = review["action"].as_str().unwrap_or("rejected");

    match action {
        "approved" => {
            let new_prompt = review["new_prompt"].as_str().unwrap_or("");
            state.evolution_engine.update_prompt(agent, new_prompt, 0.0).await;
            Json(serde_json::json!({"status": "approved", "agent": agent}))
        }
        "modified" => {
            let new_prompt = review["new_prompt"].as_str().unwrap_or("");
            state.evolution_engine.update_prompt(agent, new_prompt, 0.0).await;
            Json(serde_json::json!({"status": "modified", "agent": agent}))
        }
        _ => Json(serde_json::json!({"status": "rejected", "agent": agent})),
    }
}

// ── Skills & MCP ────────────────────────────────────────────────────────

async fn list_skills(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let skills = state.skills_bus.list_skills().await;
    Json(serde_json::json!({"skills": skills}))
}

async fn list_mcp_tools(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let tools = state.mcp_gateway.list_tools().await;
    Json(serde_json::json!({"tools": tools}))
}

// ── Skills Lifecycle Handlers ─────────────────────────────────────────

/// GET /api/skills/status — list all skill definitions with registration status
async fn skills_status(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let status = state.skills_bus.status().await;
    Json(serde_json::json!({"skills": status}))
}

/// POST /api/skills/install — install a new skill definition
async fn skills_install(
    State(state): State<Arc<AppState>>,
    Json(req): Json<skills::InstallRequest>,
) -> Json<serde_json::Value> {
    let def = skills::SkillDefinition {
        name: req.name.clone(),
        display_name: req.display_name,
        version: req.version.unwrap_or_else(|| "1.0.0".into()),
        description: req.description,
        category: req.category.unwrap_or(skills::SkillCategory::Custom),
        binds: req.binds.unwrap_or_default(),
        input_schema: req.input_schema.unwrap_or(serde_json::json!({"type": "object"})),
        system_prompt: req.system_prompt,
        installed_at: chrono::Utc::now().to_rfc3339(),
    };

    match state.skills_bus.install(def).await {
        Ok(()) => {
            let msg = if req.auto_register.unwrap_or(false) {
                "installed (auto-register requires a concrete implementation via API)"
            } else {
                "installed"
            };
            Json(serde_json::json!({"status": "ok", "name": req.name, "message": msg}))
        }
        Err(e) => Json(serde_json::json!({"status": "error", "message": e.to_string()})),
    }
}

/// POST /api/skills/{name}/register — register an installed skill
/// Requires the skill to have a concrete TaisSkill implementation factory
async fn skills_register(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Json<serde_json::Value> {
    // Check if a built-in implementation is available for this name
    match build_skill_by_name(&name, state.llm_router.clone()) {
        Some(skill) => match state.skills_bus.register(skill).await {
            Ok(()) => Json(serde_json::json!({"status": "ok", "name": name, "message": "registered"})),
            Err(e) => Json(serde_json::json!({"status": "error", "message": e.to_string()})),
        },
        None => Json(serde_json::json!({
            "status": "error",
            "name": name,
            "message": "No built-in implementation for this skill. Install first with POST /api/skills/install, then register requires a compiled implementation."
        })),
    }
}

/// DELETE /api/skills/{name} — unregister a skill from the bus
async fn skills_unregister(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Json<serde_json::Value> {
    match state.skills_bus.unregister(&name).await {
        Ok(()) => Json(serde_json::json!({"status": "ok", "name": name, "message": "unregistered"})),
        Err(e) => Json(serde_json::json!({"status": "error", "message": e.to_string()})),
    }
}

/// DELETE /api/skills/{name}/install — uninstall a skill (must be unregistered first)
async fn skills_uninstall(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Json<serde_json::Value> {
    match state.skills_bus.uninstall(&name).await {
        Ok(()) => Json(serde_json::json!({"status": "ok", "name": name, "message": "uninstalled"})),
        Err(e) => Json(serde_json::json!({"status": "error", "message": e.to_string()})),
    }
}

/// POST /api/skills/{name}/execute — execute a registered skill
async fn skills_execute(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Json(req): Json<skills::ExecuteRequest>,
) -> Json<serde_json::Value> {
    let gene = req.gene_profile.unwrap_or_default();
    match state.skills_bus.execute(&name, req.input, &gene).await {
        Ok(output) => Json(serde_json::json!({"status": "ok", "name": name, "output": output})),
        Err(e) => Json(serde_json::json!({"status": "error", "message": e.to_string()})),
    }
}

/// Build a concrete skill implementation by name (for register)
fn build_skill_by_name(name: &str, llm: Arc<llm::LlmRouter>) -> Option<Box<dyn skills::TaisSkill>> {
    match name {
        "tais-socratic-tutor" => Some(Box::new(skills::implementations::SocraticTutor::new(llm))),
        "tais-workflow" => Some(Box::new(skills::implementations::WorkflowOrchestrator::new(llm))),
        "tais-learning-analyst" => Some(Box::new(skills::implementations::LearningAnalyst::new(llm))),
        "tais-resource-pusher" => Some(Box::new(skills::implementations::ResourcePusher::new(llm))),
        "tais-skill-coach" => Some(Box::new(skills::implementations::SkillCoach::new(llm))),
        "tais-feedback-collector" => Some(Box::new(skills::implementations::FeedbackCollector::new(llm))),
        "tais-evolution" => Some(Box::new(skills::implementations::EvolutionSkill::new(llm))),
        _ => None,
    }
}

// ── LLM CRUD ────────────────────────────────────────────────────────────

async fn list_llm_configs(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let configs = state.llm_router.list_configs().await;
    Json(serde_json::json!(configs))
}

async fn get_llm_config(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Json<serde_json::Value> {
    match state.llm_router.get_config(&id).await {
        Some(config) => Json(serde_json::json!(config)),
        None => Json(serde_json::json!({"error": "config not found"})),
    }
}

async fn create_llm_config(
    State(state): State<Arc<AppState>>,
    Json(req): Json<llm::LlmConfigRequest>,
) -> Json<serde_json::Value> {
    match state.llm_router.create_config(req).await {
        Ok(config) => Json(serde_json::json!(config)),
        Err(e) => Json(serde_json::json!({"error": e.to_string()})),
    }
}

async fn update_llm_config(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<llm::LlmConfigRequest>,
) -> Json<serde_json::Value> {
    match state.llm_router.update_config(&id, req).await {
        Ok(config) => Json(serde_json::json!(config)),
        Err(e) => Json(serde_json::json!({"error": e.to_string()})),
    }
}

async fn delete_llm_config(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Json<serde_json::Value> {
    match state.llm_router.delete_config(&id).await {
        Ok(()) => Json(serde_json::json!({"status": "deleted"})),
        Err(e) => Json(serde_json::json!({"error": e.to_string()})),
    }
}

async fn test_llm_config(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Json<serde_json::Value> {
    match state.llm_router.test_config(&id).await {
        Ok(result) => Json(serde_json::json!(result)),
        Err(e) => Json(serde_json::json!({"error": e.to_string()})),
    }
}

async fn llm_status_endpoint(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let status = state.llm_router.status().await;
    Json(serde_json::json!(status))
}

// ── WeChat ──────────────────────────────────────────────────────────────

/// GET /api/wechat/callback — WeChat server verification
async fn wechat_verify(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> String {
    let signature = params.get("signature").map(|s| s.as_str()).unwrap_or("");
    let timestamp = params.get("timestamp").map(|s| s.as_str()).unwrap_or("");
    let nonce = params.get("nonce").map(|s| s.as_str()).unwrap_or("");
    let echostr = params.get("echostr").map(|s| s.as_str()).unwrap_or("");

    let bot = state.wechat_bot.lock().await;
    if bot.verify_signature(signature, timestamp, nonce) {
        tracing::info!("WeChat verification OK");
        echostr.to_string()
    } else {
        tracing::warn!("WeChat verification FAILED");
        "verification failed".to_string()
    }
}

/// POST /api/wechat/callback — Receive WeChat messages
async fn wechat_callback(
    State(state): State<Arc<AppState>>,
    body: String,
) -> String {
    let (reply_xml, openid) = {
        let mut bot = state.wechat_bot.lock().await;

        match wechat::WechatBot::parse_message(&body) {
            Ok(msg) if msg.msg_type == wechat::WxMessageType::Text => {
                let session = bot.get_or_create_session(&msg.from_user);
                let tais_session_id = session.tais_session_id.clone();
                let openid = msg.from_user.clone();

                // Check for 6-digit bind code before teaching routing
                let bind_result: Option<String> = if msg.content.len() == 6 && msg.content.chars().all(|c| c.is_ascii_digit()) {
                    if let Some(_bind_user_id) = state.auth_state.check_bind_code(&msg.content).await {
                        match state.auth_state.confirm_bind(&msg.content, &openid).await {
                            Ok(user_id) => {
                                state.memory.users.bind_wechat(&user_id, &openid).await;
                                bot.record_activity(&openid);
                                Some(format!("✅ 绑定成功！你的 TAIS 账号已与微信关联。\n现在你可以直接向我提问了。"))
                            }
                            Err(_) => None,
                        }
                    } else {
                        bot.record_activity(&openid);
                        Some("❌ 绑定码无效或已过期，请在网页上重新获取。".into())
                    }
                } else {
                    None
                };

                if let Some(bind_msg) = bind_result {
                    let reply = wechat::WxReply {
                        to_user: msg.from_user,
                        from_user: msg.to_user,
                        msg_type: "text".into(),
                        content: bind_msg,
                    };
                    return wechat::WechatBot::build_reply_xml(&reply);
                }

                // If wechat user is bound, link session to TAIS user
                if let Some(bound_profile) = state.memory.users.find_by_wechat_openid(&openid).await {
                    state.memory.link_session_user(&tais_session_id, &bound_profile.user_id).await;
                }

                // Route through TAIS
                let response = handle_wechat_teaching(&state, &msg.content, &tais_session_id).await;

                let reply = wechat::WxReply {
                    to_user: msg.from_user,
                    from_user: msg.to_user,
                    msg_type: "text".into(),
                    content: response,
                };

                bot.record_activity(&openid);
                (wechat::WechatBot::build_reply_xml(&reply), openid)
            }
            Ok(msg) => {
                // Non-text message — send help prompt
                let reply = wechat::WxReply {
                    to_user: msg.from_user,
                    from_user: msg.to_user,
                    msg_type: "text".into(),
                    content: "👋 你好！我是 TAIS 教学助手。请用文字告诉我你的问题，我会用苏格拉底式追问帮你思考。".into(),
                };
                (wechat::WechatBot::build_reply_xml(&reply), String::new())
            }
            Err(e) => {
                tracing::error!("WeChat parse error: {e}");
                ("success".to_string(), String::new())
            }
        }
    };

    tracing::info!("WeChat message from {} processed", openid);
    reply_xml
}

/// GET /api/wechat/sessions — List active WeChat sessions
async fn wechat_sessions(
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let bot = state.wechat_bot.lock().await;
    let sessions: Vec<_> = bot.list_sessions().into_iter().map(|s| {
        serde_json::json!({
            "openid": s.openid,
            "tais_session_id": s.tais_session_id,
            "student_name": s.student_name,
            "message_count": s.message_count,
            "last_active": s.last_active.to_string(),
        })
    }).collect();

    Json(serde_json::json!({
        "active_sessions": sessions.len(),
        "sessions": sessions,
    }))
}

/// Process a WeChat teaching message through TAIS pipeline
async fn handle_wechat_teaching(
    state: &Arc<AppState>,
    content: &str,
    _session_id: &str,
) -> String {
    let gene = GeneProfile {
        personality: "mentor".into(),
        ..Default::default()
    };

    // Safety check
    if !gene::GeneGateway::check_safety(content, &gene) {
        return "⚠️ 我无法直接提供答案。让我们一步步思考：你想从这个问题的哪个角度开始？".into();
    }

    // Route to Socratic tutor
    let input = serde_json::json!({
        "student_query": content,
        "concept": "auto_detect",
        "strategy": "clarification"
    });

    match state.skills_bus.execute("tais-socratic-tutor", input, &gene).await {
        Ok(output) => {
            let wrapped = gene::GeneGateway::wrap(&output, &gene);
            wrapped["content"]
                .as_str()
                .unwrap_or("让我换个方式问：你能从这个问题中识别出哪些已知条件？")
                .to_string()
        }
        Err(_) => "🤔 好问题。让我换个角度：如果你要从最基本的概念开始，第一步会想到什么？".into(),
    }
}

// ── Auth Handlers ─────────────────────────────────────────────────────

/// POST /api/auth/register
async fn auth_register(
    State(state): State<Arc<AppState>>,
    Json(body): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    let username = body["username"].as_str().unwrap_or("");
    let password = body["password"].as_str().unwrap_or("");
    let display_name = body["display_name"].as_str();

    if username.is_empty() || password.len() < 6 {
        return Json(serde_json::json!({
            "error": "用户名不能为空，密码至少6位"
        }));
    }

    // Check if user already exists
    if state.memory.users.find_by_username(username).await.is_some() {
        return Json(serde_json::json!({
            "error": format!("用户名 '{}' 已存在", username)
        }));
    }

    // Hash password
    let hash = match state.auth_state.hash_password(password) {
        Ok(h) => h,
        Err(e) => return Json(serde_json::json!({"error": e})),
    };

    // Register — save to in-memory store + SQLite if available
    match state.memory.users.register(username, display_name, &hash).await {
        Ok(profile) => {
            // Also persist to SQLite
            if let Some(ref pool) = state.db_pool {
                let _ = sqlx::query(
                    "INSERT OR REPLACE INTO users (id, display_name, password_hash, first_seen, last_active)
                     VALUES (?1, ?2, ?3, datetime('now'), datetime('now'))"
                ).bind(&profile.user_id)
                 .bind(profile.display_name.as_deref().unwrap_or(username))
                 .bind(&hash)
                 .execute(pool).await;
            }
            let token = state.auth_state.create_jwt(&profile.user_id).unwrap_or_default();
            Json(serde_json::json!({
                "status": "ok",
                "user_id": profile.user_id,
                "display_name": profile.display_name,
                "token": token,
            }))
        }
        Err(e) => Json(serde_json::json!({"error": e})),
    }
}

/// POST /api/auth/login
async fn auth_login(
    State(state): State<Arc<AppState>>,
    Json(body): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    let username = body["username"].as_str().unwrap_or("");
    let password = body["password"].as_str().unwrap_or("");

    // Try in-memory first; if no password_hash set, fall back to SQLite
    let (profile, hash) = match state.memory.users.find_by_username(username).await {
        Some(p) => {
            let pwd_hash = p.password_hash.clone();
            if let Some(h) = pwd_hash {
                (Some(p), h)
            } else {
                // In-memory user has no password — try SQLite
                let sqlite_hash = if let Some(ref pool) = state.db_pool {
                    sqlx::query_scalar::<_, String>(
                        "SELECT password_hash FROM users WHERE id = ?1 OR display_name = ?1"
                    ).bind(username).fetch_optional(pool).await.ok().flatten()
                } else { None };
                match sqlite_hash {
                    Some(sh) => {
                        // Update in-memory with SQLite hash
                        let mut updated = p;
                        updated.password_hash = Some(sh.clone());
                        state.memory.users.update(updated).await;
                        let p2 = state.memory.users.find_by_username(username).await;
                        (p2, sh)
                    }
                    None => return Json(serde_json::json!({"error": "该账号未设置密码，请用 --reset-password 重置"})),
                }
            }
        }
        None => {
            // Check SQLite for persisted users
            if let Some(ref pool) = state.db_pool {
                if let Ok(Some(row)) = sqlx::query_as::<_, (String, String, Option<String>)>(
                    "SELECT id, display_name, password_hash FROM users WHERE id = ?1 OR display_name = ?1"
                ).bind(username).fetch_optional(pool).await {
                    let (uid, display, hash) = row;
                    let h = hash.unwrap_or_default();
                    if h.is_empty() {
                        return Json(serde_json::json!({"error": "该账号未设置密码，请用 --reset-password 重置"}));
                    }
                    // Restore to in-memory store
                    if let Ok(profile) = state.memory.users.register(&uid, Some(&display), &h).await {
                        (Some(profile), h)
                    } else {
                        return Json(serde_json::json!({"error": "用户名或密码错误"}));
                    }
                } else {
                    return Json(serde_json::json!({"error": "用户名或密码错误"}));
                }
            } else {
                return Json(serde_json::json!({"error": "用户名或密码错误"}));
            }
        }
    };

    let profile = match profile {
        Some(p) => p,
        None => return Json(serde_json::json!({"error": "用户名或密码错误"})),
    };

    match state.auth_state.verify_password(password, &hash) {
        Ok(true) => {
            let token = state.auth_state.create_jwt(&profile.user_id).unwrap_or_default();
            Json(serde_json::json!({
                "status": "ok",
                "user_id": profile.user_id,
                "display_name": profile.display_name,
                "token": token,
            }))
        }
        _ => Json(serde_json::json!({"error": "用户名或密码错误"})),
    }
}

/// GET /api/auth/me — get current user from JWT
async fn auth_me(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
) -> Json<serde_json::Value> {
    let auth_header = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok());

    let user_id = match state.auth_state.extract_user(auth_header) {
        Some(uid) => uid,
        None => return Json(serde_json::json!({"error": "未登录或 token 无效"})),
    };

    match state.memory.users.get(&user_id).await {
        Some(profile) => Json(serde_json::json!({
            "user_id": profile.user_id,
            "display_name": profile.display_name,
            "grade_level": profile.grade_level,
            "wechat_bound": profile.wechat_openid.is_some(),
        })),
        None => Json(serde_json::json!({"error": "用户不存在"})),
    }
}

// ── QR Login Handlers ──────────────────────────────────────────────────

/// POST /api/auth/qr/request — generate QR login token
async fn auth_qr_request(
    State(state): State<Arc<AppState>>,
    _body: axum::body::Bytes,
) -> Json<serde_json::Value> {
    let qr = state.auth_state.create_qr_token().await;
    Json(serde_json::json!({
        "token": qr.token,
        "expires_in": 300,
        "status": "pending",
    }))
}

/// GET /api/auth/qr/status/:token — poll QR login status
async fn auth_qr_status(
    State(state): State<Arc<AppState>>,
    Path(token): Path<String>,
) -> Json<serde_json::Value> {
    match state.auth_state.get_qr_status(&token).await {
        Some(auth::QrLoginStatus::Confirmed) => {
            match state.auth_state.complete_qr_login(&token).await {
                Some(jwt) => Json(serde_json::json!({
                    "status": "confirmed",
                    "token": jwt,
                })),
                None => Json(serde_json::json!({
                    "status": "confirmed",
                    "error": "JWT generation failed",
                })),
            }
        }
        Some(status) => Json(serde_json::json!({
            "status": serde_json::to_string(&status).unwrap_or_default().trim_matches('"'),
        })),
        None => Json(serde_json::json!({"status": "expired"})),
    }
}

/// POST /api/auth/qr/confirm — confirm QR login from phone
async fn auth_qr_confirm(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    let qr_token = body["qr_token"].as_str().unwrap_or("");

    // User must be already logged in on the phone
    let auth_header = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok());
    let user_id = match state.auth_state.extract_user(auth_header) {
        Some(uid) => uid,
        None => return Json(serde_json::json!({"error": "请先在手机上登录"})),
    };

    match state.auth_state.confirm_qr(qr_token, &user_id).await {
        Ok(()) => Json(serde_json::json!({"status": "ok"})),
        Err(e) => Json(serde_json::json!({"error": e})),
    }
}

// ── WeChat Binding Handlers ─────────────────────────────────────────────

/// POST /api/auth/wechat-bind — request a 6-digit bind code
async fn wechat_bind_request(
    State(state): State<Arc<AppState>>,
    Json(body): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    // Accept either raw JWT or "Bearer <jwt>" in the token field
    let raw = body["token"].as_str().unwrap_or("");
    let jwt = raw.strip_prefix("Bearer ").unwrap_or(raw);
    let user_id = match state.auth_state.verify_jwt(jwt) {
        Ok(uid) => uid,
        Err(_) => return Json(serde_json::json!({"error": "请先登录"})),
    };
    let code = state.auth_state.create_bind_code(&user_id).await;
    Json(serde_json::json!({
        "status": "ok",
        "bind_code": code,
        "expires_in": 600,
        "instructions": "请向 TAIS 微信公众号发送这 6 位数字完成绑定",
    }))
}

/// GET /api/auth/wechat/bind-status — poll binding status
async fn auth_wechat_bind_status(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
) -> Json<serde_json::Value> {
    let auth_header = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok());
    let user_id = match state.auth_state.extract_user(auth_header) {
        Some(uid) => uid,
        None => return Json(serde_json::json!({"error": "请先登录"})),
    };

    match state.auth_state.get_bind_status(&user_id).await {
        Some(auth::BindStatus::Confirmed) => {
            let openid = state.auth_state.get_bound_openid(&user_id).await;
            Json(serde_json::json!({
                "status": "confirmed",
                "wechat_openid": openid,
            }))
        }
        Some(status) => Json(serde_json::json!({
            "status": serde_json::to_string(&status).unwrap_or_default().trim_matches('"'),
        })),
        None => Json(serde_json::json!({"status": "expired"})),
    }
}

// ── Chat & Login Pages ────────────────────────────────────────────────

/// GET /login — login/register page
async fn login_page() -> Html<String> {
    Html(crate::chat::render_login())
}

/// GET /chat — interactive WebSocket chat page
async fn chat_page(
    State(state): State<Arc<AppState>>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Html<String> {
    // Check for token in query param first, then Authorization header
    let token = params.get("token");
    let user_id = if let Some(t) = token {
        state.auth_state.verify_jwt(t).ok()
    } else {
        None
    };
    let session_id = uuid::Uuid::new_v4().to_string();

    // If logged in, link session and get display_name
    let display_name = if let Some(ref uid) = user_id {
        state.memory.link_session_user(&session_id, uid).await;
        state.memory.users.get(uid).await
            .and_then(|p| p.display_name)
    } else {
        None
    };

    let html = crate::chat::render(&session_id, user_id.as_deref(), display_name.as_deref());
    Html(html)
}

// ── Memory ──────────────────────────────────────────────────────────────

async fn memory_list_sessions(
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let sessions = state.memory.list_sessions().await;
    Json(serde_json::json!({
        "total": sessions.len(),
        "sessions": sessions,
    }))
}

async fn memory_get_session(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
) -> Json<serde_json::Value> {
    let ctx = state.memory.get_context(&session_id).await;
    let turns = state.memory.get_history(&session_id).await;
    Json(serde_json::json!({
        "session_id": session_id,
        "total_turns": ctx.total_turns,
        "concepts": ctx.concepts_discussed,
        "summary": ctx.summary,
        "turns": turns,
    }))
}

async fn memory_search(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Json<serde_json::Value> {
    let query = params.get("q").map(|s| s.as_str()).unwrap_or("");
    if query.is_empty() {
        return Json(serde_json::json!({"error": "missing query parameter 'q'"}));
    }
    let results = state.memory.search(query).await;
    Json(serde_json::json!({
        "query": query,
        "results": results,
    }))
}

// ── WebSocket (with Memory context) ────────────────────────────────────

async fn ws_session(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
) -> axum::response::Response {
    ws.on_upgrade(move |socket| handle_ws(socket, state, session_id))
}

async fn handle_ws(mut socket: WebSocket, state: Arc<AppState>, session_id: String) {
    state.active_ws_count.fetch_add(1, Ordering::Relaxed);
    let socket = Arc::new(tokio::sync::Mutex::new(socket));
    let gene = GeneProfile::default();

    let context_prompt = state.memory.build_context_prompt(&session_id).await;
    if !context_prompt.is_empty() {
        let _ = socket.lock().await
            .send(Message::Text(format!("📋 欢迎回来！我回顾了之前的对话：\n{context_prompt}").into()))
            .await;
    }

    let mut current_user_id: Option<String> = None;
    let mut current_display: Option<String> = None;
    let mut current_concept: Option<String> = None;
    let mut session_mode = SessionMode::General;
    let mut welcomed = false;

    while let Some(Ok(msg)) = socket.lock().await.recv().await {
        if let Message::Text(text) = msg {
            let text = text.to_string();
            // Check for identity JSON message
            if let Ok(ident) = serde_json::from_str::<serde_json::Value>(&text) {
                if ident.get("type").and_then(|v| v.as_str()) == Some("identify") {
                    current_user_id = ident.get("user_id").and_then(|v| v.as_str()).map(String::from);
                    current_display = ident.get("display_name").and_then(|v| v.as_str()).map(String::from);
                    if let Some(ref uid) = current_user_id {
                        state.memory.link_session_user(&session_id, uid).await;
                        state.memory.users.get_or_create(uid).await;
                    }
                    let ack = serde_json::json!({
                        "type": "identity_ack",
                        "user_id": current_user_id,
                        "display_name": current_display,
                    });
                    let _ = socket.lock().await.send(Message::Text(ack.to_string().into())).await;
                    // Send welcome now that we know the user's name
                    if !welcomed {
                        welcomed = true;
                        let name = current_display.as_deref().unwrap_or("访客");
                        let welcome = format!(
                            "💬 欢迎 **{name}**！当前模式: **通用模式**\n\n\
                            🎯 **三种模式**:\n\
                            ├─ 💬 通用模式 — 自然对话，直接回答\n\
                            ├─ 📚 学习模式 — TAIS 技能胶囊，苏格拉底追问\n\
                            └─ ⚙️ 命令模式 — `llm`, `passwd`, `habits`, `status`\n\n\
                            输入模式名即可切换。`help` 查看详情。"
                        );
                        let _ = socket.lock().await.send(Message::Text(welcome.into())).await;
                    }
                    continue;
                }
            }

            if !gene::GeneGateway::check_safety(&text, &gene) {
                let _ = socket.lock().await
                    .send(Message::Text("⚠️ 我无法直接提供答案。让我们一步步思考：".into()))
                    .await;
                continue;
            }

            // ── Mode switch detection ────────────────────────────────
            let lower = text.to_lowercase();
            if contains_any(&lower, &["切换到学习", "学习模式", "教学", "恢复教学", "进入教学"]) {
                session_mode = SessionMode::Learning;
                let resp = format!("🟢 **已切换到学习模式**。我会用苏格拉底式追问引导你思考。\n当前概念: {}\n直接问问题即可。",
                    current_concept.as_deref().unwrap_or("未设定"));
                let _ = socket.lock().await.send(Message::Text(resp.into())).await;
                continue;
            }
            if contains_any(&lower, &["切换到命令", "命令模式", "管理模式", "admin", "指令模式", "不是学习", "不是教学"]) {
                session_mode = SessionMode::Command;
                let resp = "⚙️ **已切换到命令模式**。你可以输入指令来操作 TAIS。\n\
                    输入 `help` 查看所有可用命令。\n\
                    输入「通用模式」或「学习模式」切换。";
                let _ = socket.lock().await.send(Message::Text(resp.into())).await;
                continue;
            }
            if contains_any(&lower, &["通用模式", "切换到通用", "聊天模式", "一般模式", "对话模式", "通用", "general"]) {
                session_mode = SessionMode::General;
                let resp = "💬 **已切换到通用模式**。我会自然地回答你的问题，不追问也不限指令。\n\
                    你可以直接提问、聊天、或输入指令。\n\
                    输入「学习模式」或「命令模式」切换。";
                let _ = socket.lock().await.send(Message::Text(resp.into())).await;
                continue;
            }

            // ── Route by persistent mode ─────────────────────────────
            match session_mode {
                SessionMode::Command => {
                    let response = handle_command_full(
                        &state, &session_id, &text, &gene,
                        current_user_id.as_deref(), current_display.as_deref(),
                        &mut session_mode, &mut current_concept,
                    ).await;
                    state.memory.record_exchange_with_user(
                        &session_id, &text, &response,
                        None, None, None,
                    ).await;
                    let _ = socket.lock().await.send(Message::Text(response.into())).await;
                }
                SessionMode::General => {
                    // Heartbeat: "still working" every 2 minutes
                    let hb_socket = socket.clone();
                    let heartbeat = tokio::spawn(async move {
                        loop {
                            tokio::time::sleep(std::time::Duration::from_secs(120)).await;
                            let _ = hb_socket.lock().await.send(Message::Text(
                                "⏳ 任务仍在执行中，请耐心等待...".into()
                            )).await;
                        }
                    });
                    let response = handle_general(
                        &state, &session_id, &text, &gene,
                        current_user_id.as_deref(), current_display.as_deref(),
                        &mut current_concept,
                    ).await;
                    heartbeat.abort();
                    // Send any agent progress messages
                    if !response.0.is_empty() {
                        for p in &response.0 {
                            let _ = socket.lock().await.send(Message::Text(p.clone().into())).await;
                        }
                    }
                    let response = response.1;
                    state.memory.record_exchange_with_user(
                        &session_id, &text, &response,
                        current_concept.as_deref(), None, None,
                    ).await;
                    let _ = socket.lock().await.send(Message::Text(response.into())).await;
                }
                SessionMode::Learning => {
                    // Try to extract concept
                    let (_, extracted) = detect_intent(&text);
                    if let Some(ref c) = extracted {
                        current_concept = Some(c.clone());
                    }
                    let concept = current_concept.as_deref().unwrap_or("当前主题");

                    let strategy = pick_strategy(&text);
                    let input = serde_json::json!({
                        "student_query": text,
                        "concept": concept,
                        "strategy": strategy,
                        "context": context_prompt,
                    });

                    match state.skills_bus.execute("tais-socratic-tutor", input, &gene).await {
                        Ok(output) => {
                            let wrapped = gene::GeneGateway::wrap(&output, &gene);
                            let response = wrapped["content"]
                                .as_str()
                                .unwrap_or("请再试一次")
                                .to_string();
                            state.memory.record_exchange_with_user(
                                &session_id, &text, &response,
                                Some(concept), None, None,
                            ).await;
                            let _ = socket.lock().await.send(Message::Text(response.into())).await;
                        }
                        Err(e) => {
                            let _ = socket.lock().await.send(Message::Text(format!("系统错误: {e}").into())).await;
                        }
                    }
                }
            }
        }
    }

    state.active_ws_count.fetch_sub(1, Ordering::Relaxed);
}

// ── Helpers ────────────────────────────────────────────────────────────────

/// Check if `text` contains any of the given keywords.
fn contains_any(text: &str, keywords: &[&str]) -> bool {
    keywords.iter().any(|kw| text.contains(kw))
}

// ── Intent Detection ───────────────────────────────────────────────────────

#[derive(Debug, PartialEq)]
enum Intent {
    Command,
    Question,
}

/// Persistent session mode — used by handle_ws and handle_command_full
#[derive(Debug, PartialEq)]
enum SessionMode { Learning, Command, General }

fn detect_intent(text: &str) -> (Intent, Option<String>) {
    let lower = text.to_lowercase();

    if lower.contains('?') || lower.contains('？')
        || lower.starts_with("what") || lower.starts_with("how") || lower.starts_with("why")
        || contains_any(&lower, &["什么是", "怎么", "为什么", "如何", "解释", "教我", "学习",
            "不懂", "不明白", "帮我理解", "请问", "我想知道", "说明", "讲一下", "介绍一下"])
    {
        return (Intent::Question, extract_concept(text));
    }

    (Intent::Command, None)
}

/// Try to extract a concept from the user's message.
fn extract_concept(text: &str) -> Option<String> {
    // Remove question marks and common prefixes
    let cleaned = text
        .replace('？', " ")
        .replace('?', " ")
        .replace('，', " ")
        .replace(',', " ");

    // Look for quotes 「」
    if let Some(start) = cleaned.find('「') {
        if let Some(end) = cleaned[start..].find('」') {
            return Some(cleaned[start + 3..start + end].to_string());
        }
    }

    // Keywords: after "什么是", "关于", "解释", "怎么" etc.
    let prefixes = ["什么是", "关于", "解释一下", "怎么理解", "教我", "介绍一下"];
    for prefix in &prefixes {
        if let Some(pos) = cleaned.find(prefix) {
            let after = cleaned[pos + prefix.len()..].trim();
            // Take first 1-2 words as concept
            let words: Vec<&str> = after.split_whitespace().take(3).collect();
            if !words.is_empty() {
                return Some(words.join(" "));
            }
        }
    }

    None
}

/// Pick a teaching strategy based on user's message.
fn pick_strategy(text: &str) -> &str {
    let lower = text.to_lowercase();
    if lower.contains("对吗") || lower.contains("对不对") || lower.contains("是不是") {
        "counterexample"
    } else if lower.contains("例子") || lower.contains("比如") || lower.contains("类比") {
        "analogy"
    } else if lower.contains("怎么做") || lower.contains("步骤") || lower.contains("过程") {
        "scaffold"
    } else {
        "clarification"
    }
}

/// Handle a command in command mode. Returns (response, new_mode_if_changed).
async fn handle_command_full(
    state: &Arc<AppState>,
    session_id: &str,
    text: &str,
    gene: &GeneProfile,
    user_id: Option<&str>,
    display_name: Option<&str>,
    session_mode: &mut SessionMode,
    current_concept: &mut Option<String>,
) -> String {
    let lower = text.to_lowercase();
    let name = display_name.unwrap_or("访客");
    let sid = &session_id[..8.min(session_id.len())];

    // ── Personal LLM helper ────────────────────────────────────────
    let personal_llm = if let Some(uid) = user_id {
        state.memory.users.get_llm_config(uid).await
    } else { None };

    // ── help ───────────────────────────────────────────────────────
    if lower == "help" || lower == "?" || lower == "？" || lower == "你能做什么"
       || lower.starts_with("help ") || lower.starts_with("? ") || lower.starts_with("？ ") {
        return format!(
            "🔧 **TAIS 命令列表**\n\n\
            **💬 通用模式**（推荐日常使用）\n\
            ├─ `通用模式` / `聊天模式` — 自然对话，不追问不限指令\n\
            ├─ 适合日常提问、闲聊、时间查询等\n\n\
            **📚 教学模式**（引导式学习）\n\
            ├─ `学习模式` / `教学模式` — 切换到苏格拉底追问引导\n\
            ├─ `concept <概念>` — 设定当前学习主题\n\n\
            **⚙️ 命令模式**（系统控制）\n\
            ├─ `命令模式` / `管理模式` — 切换到命令模式（当前模式）\n\
            ├─ `help` — 显示此帮助\n\
            ├─ `status` — 查看系统运行状态\n\
            ├─ `habits` — 查看 7 个习惯胶囊状态\n\
            ├─ `gene` — 查看当前基因人格\n\
            ├─ `gene <scholar|mentor|hacker>` — 设置 AI 人格\n\
            ├─ `skills` — 列出 35 个可用技能\n\
            ├─ `tools` — 列出 MCP 工具\n\
            ├─ `memory` ✧ `sessions` — 记忆与会话\n\
            ├─ `evolution` — 查看进化引擎状态\n\
            ├─ `crystallize <name> <desc>` — 结晶当前技能为 SOP\n\
            ├─ `checkpoint <内容>` — 设置工作记忆\n\
            ├─ `clear` — 清除工作记忆\n\
            ├─ `llm` — LLM 状态\n\
            ├─ `llm list` — 列出所有 LLM 配置\n\
            ├─ `llm add <名> <provider> <模型> <url> <key>` — 添加 LLM\n\
            ├─ `llm del <id>` — 删除单个 LLM\n\
            ├─ `llm clear` — 删除所有系统 LLM\n\
            ├─ `llm set default <名>` — 设为默认 LLM\n\
            ├─ `whoami` — 查看当前用户\n\
            ├─ `passwd <新密码>` — 修改密码\n\
            ├─ `set grade <年级>` — 设置年级 (如: 高一, 初二)\n\
            ├─ `set style <风格>` — 设置学习风格 (visual/verbal/hands-on)\n\
            └─ `学习模式` ✧ `通用模式` — 切换模式\n\n\
            👤 用户: {name}\n📝 会话: {sid}\n🧬 基因: {gene_name}\n🔄 模式: {mode}",
            name = name,
            sid = sid,
            gene_name = gene.personality,
            mode = if *session_mode == SessionMode::Command { "命令" } else { "学习" },
        );
    }

    // ── status ─────────────────────────────────────────────────────
    if lower.contains("status") {
        let info = state.agent_loop.get_state().await;
        return format!(
            "📊 **系统状态**\n\
            用户: {name}\n会话: {sid}\n基因: {gene_name}\n模式: {mode}\n\
            总轮次: {total}\n成功: {ok} / 失败: {fail}\n\
            快速路径: {fast}\n部署策略: {deploy}\n\
            工作记忆: {cp}",
            name = name, sid = sid,
            gene_name = gene.personality,
            mode = if *session_mode == SessionMode::Command { "命令" } else { "学习" },
            total = info.total_rounds, ok = info.successful_rounds, fail = info.failed_rounds,
            fast = info.fast_path_rounds, deploy = info.deployed_strategies,
            cp = state.agent_loop.get_checkpoint().await
                .map(|c| c.key_info).unwrap_or_default(),
        );
    }

    // ── habits ─────────────────────────────────────────────────────
    if lower.contains("habit") {
        let states = state.habit_engine.get_all_states().await;
        let mut s = String::from("🔄 **习惯引擎 (7胶囊)**\n\n");
        for st in &states {
            let icon = if st.is_auto { "🟢" } else if st.weight > 0.6 { "🔵" } else { "🟡" };
            let warn = if st.weight < THETA_RETRAIN { " ⚠️退化" } else { "" };
            s.push_str(&format!(
                "{} {}: {:.2} streak={} s={}/f={}{}\n",
                icon, st.rule_id, st.weight, st.streak, st.success_count, st.failure_count, warn,
            ));
        }
        return s;
    }

    // ── gene / 基因 ────────────────────────────────────────────────
    if lower == "gene" || lower == "基因" {
        return format!(
            "🧬 **当前基因人格**: {}\n思维: {}\n风控: {}\n行为: {}\n\n可用人格: scholar(学者) | mentor(导师) | hacker(黑客)\n切换: `gene hacker`",
            gene.personality, gene.thinking, gene.risk_level, gene.behavior,
        );
    }
    // Actually GeneProfile doesn't support mutation directly — just report
    if lower.starts_with("gene ") {
        let g = lower.trim_start_matches("gene ").trim();
        if ["scholar", "mentor", "hacker"].contains(&g) {
            return format!("✅ 基因人格设置: **{}**\n（注：当前版本基因在会话级固定，新会话生效）", g);
        }
    }

    // ── concept ────────────────────────────────────────────────────
    if lower.starts_with("concept ") || lower.starts_with("概念 ") {
        let c = lower.trim_start_matches("concept ").trim_start_matches("概念 ").trim().to_string();
        let resp = format!("✅ 学习主题已设定: **{}**\n之后的提问将围绕此概念展开。", c);
        *current_concept = Some(c);
        return resp;
    }

    // ── skills ─────────────────────────────────────────────────────
    if lower == "skills" || lower == "技能" {
        let rules = state.habit_engine.get_rules().await;
        let mut s = format!("📦 **可用技能胶囊 (35个)**\n\n");
        s.push_str("**TAIS 教学 (7)**\n");
        s.push_str("├─ tais-workflow | tais-learning-analyst | tais-socratic-tutor\n");
        s.push_str("├─ tais-resource-pusher | tais-skill-coach\n");
        s.push_str("├─ tais-feedback-collector | tais-evolution\n\n");
        s.push_str("**OO 工程 (14)**: prd-generator → code-scaffold\n");
        s.push_str("**基因 (7)**: personality → heredity\n");
        s.push_str("**习惯 (7)**\n");
        for r in &rules {
            s.push_str(&format!("├─ {}: {}\n", r.id, r.name));
        }
        s.push_str("\n💡 用 `tools` 查看 MCP 工具列表");
        return s;
    }

    // ── tools ──────────────────────────────────────────────────────
    if lower == "tools" || lower == "工具" {
        let tools = state.mcp_gateway.list_tools().await;
        let mut s = format!("🔌 **MCP 工具 ({}个)**\n\n", tools.len());
        for (i, t) in tools.iter().take(10).enumerate() {
            s.push_str(&format!("{}. {} — {}\n", i + 1, t.name, t.description));
        }
        if tools.len() > 10 {
            s.push_str(&format!("... 还有 {} 个工具", tools.len() - 10));
        }
        return s;
    }

    // ── direct answer ──────────────────────────────────────────────
    if lower.starts_with("direct ") || lower.starts_with("直接回答 ") || lower.starts_with("直接回答：") {
        let q = lower.trim_start_matches("direct ")
            .trim_start_matches("直接回答 ")
            .trim_start_matches("直接回答：").trim();
        return format!(
            "📝 **直接回答模式**\n\n你的问题: {q}\n\n💡 当前 LLM 未配置，无法生成实时回答。\n配置 LLM 后（`tais.toml` 或环境变量），我会用 AI 直接回答。\n\n可配置: OpenAI / Anthropic / Ollama / DeepSeek",
            q = safe_truncate(q, 200),
        );
    }

    // ── memory / sessions ──────────────────────────────────────────
    if lower.contains("memory") || lower == "记忆" {
        let sessions = state.memory.list_sessions().await;
        return format!(
            "🧠 **会话记忆**\n活跃会话: {} 个\n当前会话: {sid}\n用户: {name}\n\n💡 用 `sessions` 查看所有会话",
            sessions.len(), sid = sid, name = name,
        );
    }
    if lower == "sessions" {
        let sessions = state.memory.list_sessions().await;
        let mut s = format!("📋 **活跃会话 ({}个)**\n\n", sessions.len());
        for sid_item in sessions.iter().take(10) {
            let short = &sid_item[..8.min(sid_item.len())];
            let turns = state.memory.session_turns(sid_item).await;
            s.push_str(&format!("├─ {}... ({} 轮)\n", short, turns));
        }
        if sessions.len() > 10 { s.push_str(&format!("... 还有 {} 个\n", sessions.len() - 10)); }
        return s;
    }

    // ── evolution ──────────────────────────────────────────────────
    if lower.contains("evolution") || lower.contains("进化") {
        return format!(
            "🧬 **进化引擎**\n状态: 等待中 (需 ≥50 会话触发)\n阈值: composite < 0.6\n\
            指标权重: LE:0.35 TE:0.25 SA:0.20 RE:0.10 TS:0.10\n\
            审查: 教师闸门开启 (auto_deploy=false)",
        );
    }

    // ── checkpoint ─────────────────────────────────────────────────
    if lower.starts_with("checkpoint ") {
        let ki = text.trim_start_matches("checkpoint ").trim().to_string();
        state.agent_loop.update_checkpoint(&ki, None).await;
        return format!("✅ 工作记忆已设置: {}", if ki.len() > 100 { safe_truncate(&ki, 100) } else { &ki });
    }
    if lower == "clear" || lower == "清除" || lower == "清空" {
        state.agent_loop.clear_checkpoint().await;
        return "✅ 工作记忆已清除".to_string();
    }

    // ── crystallize ────────────────────────────────────────────────
    if lower.starts_with("crystallize ") {
        let rest = text.trim_start_matches("crystallize ").trim();
        let parts: Vec<&str> = rest.splitn(2, ' ').collect();
        let name = parts.first().unwrap_or(&"unnamed");
        let desc = parts.get(1).unwrap_or(&"no description");
        let skills_dir = std::path::PathBuf::from("memory/skills");
        match skills::crystallizer::crystallize_skill(
            &skills_dir, name, desc, "自动结晶",
            &["待补充"], &["待验证"],
        ) {
            Ok(r) => {
                state.agent_loop.reload_skills().await;
                return format!("✅ 技能已结晶: {}\n文件: {}\n索引更新: {}", r.skill_name, r.sop_path, r.index_updated);
            }
            Err(e) => return format!("❌ 结晶失败: {e}"),
        }
    }

    // ── llm ────────────────────────────────────────────────────────
    // ── llm clear ──────────────────────────────────────────────────
    if lower == "llm clear" {
        let configs = state.llm_router.list_configs().await;
        let mut deleted = 0;
        for c in &configs {
            match state.llm_router.delete_config(&c.id).await {
                Ok(_) => deleted += 1,
                Err(e) => tracing::warn!("Failed to delete {}: {e}", c.name),
            }
        }
        return format!("✅ 已删除 {} 个系统 LLM 配置。个人 LLM 不受影响。", deleted);
    }

    if lower == "llm" || lower == "llm status" {
        let status = state.llm_router.status().await;
        let configs = state.llm_router.list_configs().await;
        let mut s = String::new();
        // Personal LLM
        if let Some(ref pl) = personal_llm {
            s.push_str(&format!("🧑 **你的 LLM**: {} / {} | base: {}\n",
                pl.provider, pl.model, pl.base_url));
        } else {
            s.push_str("🧑 **你的 LLM**: 未配置 (在仪表盘设置)\n");
        }
        s.push_str(&format!("\n🖥️ **系统**: {} 已连接 / {} 活跃 / {} 总计\n",
            status.connected_count, status.active_configs, status.total_configs));
        for (i, c) in configs.iter().enumerate() {
            let def = if c.is_default { " ★默认" } else { "" };
            s.push_str(&format!("{}. {} [{}/{}]{}\n",
                i + 1, c.name, c.provider, c.model, def));
        }
        return s;
    }
    if lower == "llm list" {
        let configs = state.llm_router.list_configs().await;
        let mut s = String::from("📋 **LLM 配置**\n\n");
        // Personal config first
        if let Some(ref pl) = personal_llm {
            let key = if pl.api_key.len() > 8 { format!("{}...", &pl.api_key[..8]) } else { "***".into() };
            s.push_str(&format!("🧑 **你的 LLM** (个人)\n   {} | model={} | base={} | key={}\n\n",
                pl.provider, pl.model, pl.base_url, key));
        }
        // System configs
        s.push_str("🖥️ **系统 LLM**:\n");
        for (i, c) in configs.iter().enumerate() {
            let key = if c.api_key.len() > 8 { format!("{}...", &c.api_key[..8]) } else { "***".into() };
            s.push_str(&format!("{}. `{}` {} | model={} | key={}\n",
                i + 1, c.name, c.provider, c.model, key));
        }
        if configs.is_empty() && personal_llm.is_none() { s.push_str("(空) 在 Dashboard 或个人仪表盘配置 LLM\n"); }
        return s;
    }
    if lower.starts_with("llm add ") {
        // Parse: llm add <name> <provider> <model> <base_url> <api_key>
        let rest = text.trim_start_matches("llm add ").trim();
        let parts: Vec<&str> = rest.splitn(5, ' ').collect();
        if parts.len() < 5 {
            return "用法: `llm add <名称> <openai|anthropic|ollama> <模型名> <base_url> <api_key>`".into();
        }
        let provider = match parts[1] {
            "openai" => llm::ProviderType::OpenAI,
            "anthropic" => llm::ProviderType::Anthropic,
            "ollama" => llm::ProviderType::Ollama,
            _ => return "❌ provider 必须是: openai, anthropic, ollama".into(),
        };
        return match state.llm_router.create_config(llm::LlmConfigRequest {
            name: parts[0].into(), provider, model: parts[2].into(),
            base_url: parts[3].into(), api_key: parts[4].into(),
            params: llm::LlmParams::default(), is_default: false, is_active: true,
        }).await {
            Ok(c) => format!("✅ LLM 配置已创建: {} (id={})", c.name, &c.id[..8]),
            Err(e) => format!("❌ 创建失败: {e}"),
        };
    }
    if lower.starts_with("llm del ") {
        let id = text.trim_start_matches("llm del ").trim();
        return match state.llm_router.delete_config(id).await {
            Ok(_) => format!("✅ LLM 配置已删除: {id}"),
            Err(e) => format!("❌ 删除失败: {e}"),
        };
    }
    if lower.starts_with("llm set default ") {
        let name = text.trim_start_matches("llm set default ").trim();
        let configs = state.llm_router.list_configs().await;
        if let Some(c) = configs.iter().find(|c| c.name == name || c.id.starts_with(name)) {
            return match state.llm_router.update_config(&c.id, llm::LlmConfigRequest {
                name: c.name.clone(), provider: c.provider.clone(),
                base_url: c.base_url.clone(), api_key: c.api_key.clone(),
                model: c.model.clone(), params: c.params.clone(),
                is_default: true, is_active: true,
            }).await {
                Ok(_) => format!("✅ 默认 LLM 设为: {}", c.name),
                Err(e) => format!("❌ 设置失败: {e}"),
            };
        } else {
            return format!("❌ 未找到配置: {name}\n用 `llm list` 查看所有配置")
        }
    }

    // ── set grade / set style ─────────────────────────────────────
    if lower.starts_with("set grade ") {
        let g = text.trim_start_matches("set grade ").trim();
        if let Some(uid) = user_id {
            if let Some(mut p) = state.memory.users.get(uid).await {
                p.grade_level = Some(g.to_string());
                state.memory.users.update(p).await;
                return format!("✅ 年级已设为: {g}");
            }
        }
        return "❌ 无法保存（请先登录）".into();
    }
    if lower.starts_with("set style ") {
        let s = text.trim_start_matches("set style ").trim();
        if let Some(uid) = user_id {
            if let Some(mut p) = state.memory.users.get(uid).await {
                p.preferred_style = Some(s.to_string());
                state.memory.users.update(p).await;
                return format!("✅ 学习风格已设为: {s}");
            }
        }
        return "❌ 无法保存（请先登录）".into();
    }

    // ── whoami / passwd ────────────────────────────────────────────
    if lower == "whoami" || lower == "我是谁" {
        return format!(
            "👤 用户: {name}\n🆔 User ID: {uid}\n🧬 基因: {gene}\n🔄 模式: 命令\n📝 会话: {sid}",
            name = display_name.unwrap_or("未知"),
            uid = user_id.unwrap_or("unknown"),
            gene = gene.personality,
            sid = &session_id[..8.min(session_id.len())],
        );
    }
    if lower.starts_with("passwd ") {
        let new_pw = text.trim_start_matches("passwd ").trim();
        if new_pw.len() < 6 {
            return "❌ 密码至少6位".into();
        }
        if let Some(uid) = user_id {
            let hash = match state.auth_state.hash_password(new_pw) {
                Ok(h) => h,
                Err(e) => return format!("❌ 加密失败: {e}"),
            };
            // Update in-memory
            if let Some(mut profile) = state.memory.users.get(uid).await {
                profile.password_hash = Some(hash.clone());
                state.memory.users.update(profile).await;
            }
            // Update SQLite
            if let Some(ref pool) = state.db_pool {
                let _ = sqlx::query(
                    "UPDATE users SET password_hash = ?1, last_active = datetime('now') WHERE id = ?2"
                ).bind(&hash).bind(uid).execute(pool).await;
            }
            return format!("✅ 密码已更新 (用户: {})", display_name.unwrap_or(uid));
        } else {
            return "❌ 无法识别当前用户（未登录？）".into();
        }
    }

    // ── memory hot management ──────────────────────────────────────
    if lower == "memory" || lower == "记忆" {
        let hm = memory::hot::HotMemory::new();
        let size = hm.size();
        return format!("🧠 **热记忆** ({}/500 字符)\n\n{}\n\n管理: `memory add <事实>` | `memory rm <关键词>` | `memory list`",
            size, hm.read());
    }
    if lower == "memory list" {
        return memory::hot::HotMemory::new().read();
    }
    if lower.starts_with("memory add ") {
        let fact = text.trim_start_matches("memory add ").trim();
        match memory::hot::HotMemory::new().add(fact) {
            Ok(()) => return format!("✅ 已添加: {}", fact),
            Err(e) => return format!("❌ {}: {}", e, fact),
        }
    }
    if lower.starts_with("memory rm ") || lower.starts_with("memory remove ") {
        let kw = text.trim_start_matches("memory rm ").trim_start_matches("memory remove ").trim();
        match memory::hot::HotMemory::new().remove(kw) {
            Ok(()) => return format!("✅ 已移除包含「{}」的条目", kw),
            Err(e) => return format!("❌ {}", e),
        }
    }

    // ── Default response for unknown commands ──────────────────────
    format!(
        "⚙️ 收到: 「{}」\n\
        未识别的指令。输入 `help` 查看所有命令。",
        if text.len() > 80 { &text[..80] } else { text }
    )
}

fn no_progress(answer: String) -> (Vec<String>, String) {
    (vec![], answer)
}

/// Safe truncation at char boundaries (not byte boundaries).
fn safe_truncate(s: &str, max_chars: usize) -> &str {
    if s.len() <= max_chars { return s; }
    let mut end = max_chars;
    while end > 0 && !s.is_char_boundary(end) { end -= 1; }
    &s[..end]
}

/// Extract a skill name from user query (e.g. "查询武汉天气" → "weather_query")
fn extract_skill_name(text: &str) -> String {
    let lower = text.to_lowercase();
    if lower.contains("天气") || lower.contains("weather") { return "weather_query".into(); }
    if lower.contains("新闻") || lower.contains("news") { return "news_fetch".into(); }
    if lower.contains("搜索") || lower.contains("search") { return "web_search".into(); }
    if lower.contains("计算") || lower.contains("calc") { return "calculator".into(); }
    if lower.contains("翻译") || lower.contains("translate") { return "translator".into(); }
    if lower.contains("爬虫") || lower.contains("scrape") || lower.contains("抓取") { return "web_scraper".into(); }
    if lower.contains("文件") || lower.contains("file") { return "file_ops".into(); }
    // Generic: use first 2 words as skill name
    let words: Vec<&str> = text.split_whitespace().take(2).collect();
    let name = words.join("_").to_lowercase();
    if name.len() > 3 { name } else { "agent_skill".into() }
}

/// Agent tool-using loop — LLM can execute code to answer questions.
async fn run_agent_loop(
    state: &Arc<AppState>,
    user_query: &str,
    name: &str,
    time_str: &str,
    history: &str,
) -> (Vec<String>, String) {
    let mut progress = Vec::new();
    let context_section = if history.is_empty() {
        String::new()
    } else {
        format!("\n**最近的对话上下文**:\n{history}\n请基于以上上下文回答。如果用户提到\"上文\"、\"刚才\"、\"之前\"等，请引用上下文中的内容。\n")
    };
    let sys = format!(
        "# 角色: TAIS 自进化 AI Agent\n\
        你有完整的执行权限: 代码运行、文件读写、浏览器控制、系统操作。\n\
        永远不要说\"做不到\"——用工具探测，用结果说话。用中文回复。\n\n\
        ## 行动原则\n\
        - 探测优先: 失败时先收集信息(日志/状态)，关键发现记入工作记忆，再决定重试还是换方案\n\
        - 失败升级: 1次→读错误理解原因，2次→探测环境，3次→深度分析后换方案或问用户\n\
        - 禁止无信息重复: 永远不要用同样的参数重试同样的操作\n\n\
        ## 工具\n\
        **Python 代码块** ```python (已预导入以下函数):\n\
        file_read(path,start,count,keyword), file_write(path,content,mode)\n\
        file_patch(path,old,new), ask_user(question,candidates)\n\
        HTTP: urllib.request (标准库)\n\n\
        **浏览器操控** ```browser:\n\
        new_tab(url), page_info(), capture_screenshot()\n\
        click_at_xy(x,y), js(script), http_get(url), wait_for_load()\n\n\
        用户: {name} | UTC: {time}{context}\n\
        ⚠️ 代码必须在 ```块中，否则不执行。先给计划再执行。",
        name = name, time = time_str, context = context_section,
    );

    // Step 1: planning — show the plan to the user
    progress.push("📋 **任务规划**:".into());
    let plan_prompt = format!(
        "用户需求: {query}\n\n请先给出一个简短的执行计划（3-5步），然后再编写代码。计划格式:\n📋 计划:\n1. xxx\n2. xxx\n然后写代码。",
        query = user_query
    );
    // Multi-turn execution loop (max 3 rounds)
    let mut response = match state.llm_router.chat_simple(&sys, &plan_prompt).await {
        Ok(r) => {
            if let Some(plan_end) = r.find("```") {
                let plan = r[..plan_end].trim();
                if !plan.is_empty() { progress.push(plan.to_string()); }
            }
            r
        }
        Err(e) => return (progress, format!("❌ LLM 调用失败: {e}")),
    };

    // Multi-turn execution loop: keep executing code blocks until LLM produces only text
    for _round in 0..3 {
        // Try browser code first
        if let Some(code) = extract_code_block(&response, "browser") {
            progress.push(format!("🌐 操控浏览器:\n```python\n{}\n```",
                if code.len() > 300 { format!("{}...({} 行)", safe_truncate(&code, 300), code.lines().count()) } else { code.clone() }));
            let output = execute_browser_harness(&code).await;
            progress.push(format!("🌐 浏览器输出:\n```\n{}\n```",
                if output.len() > 500 { format!("{}...", safe_truncate(&output, 500)) } else { output.clone() }));
            let followup = format!("浏览器操作结果:\n[Browser Output]\n{}\n[/Browser Output]\n\n继续执行或总结结果回答用户: 「{}」",
                output, user_query);
            match state.llm_router.chat_simple(&sys, &followup).await {
                Ok(r) => { response = r; continue; }
                Err(_) => return (progress, format!("浏览器操作结果:\n{}", output)),
            }
        }
        // Then Python code
        if let Some(code) = extract_code_block(&response, "python") {
            progress.push(format!("⚡ 执行代码 ({} 行):\n```python\n{}\n```",
                code.lines().count(),
                if code.len() > 200 { format!("{}...", safe_truncate(&code, 200)) } else { code.clone() }));
            let output = execute_python_code(&code).await;
            progress.push(format!("📤 执行结果:\n```\n{}\n```",
                if output.len() > 500 { format!("{}...", safe_truncate(&output, 500)) } else { output.clone() }));
            let followup = format!("代码执行结果:\n[Code Output]\n{}\n[/Code Output]\n\n继续执行或总结回答用户: 「{}」",
                output, user_query);
            match state.llm_router.chat_simple(&sys, &followup).await {
                Ok(r) => { response = r; continue; }
                Err(_) => return (progress, format!("代码执行结果:\n{}", output)),
            }
        }
        // Then shell
        if let Some(code) = extract_code_block(&response, "powershell")
            .or_else(|| extract_code_block(&response, "bash"))
        {
            progress.push(format!("⚡ 执行脚本:\n```\n{}\n```",
                if code.len() > 200 { safe_truncate(&code, 200) } else { &code }));
            let output = execute_shell_code(&code).await;
            let disp_out = if output.len() > 500 { format!("{}...", safe_truncate(&output, 500)) } else { output.clone() };
            progress.push(format!("📤 脚本输出:\n```\n{}\n```", disp_out));
            let followup = format!("脚本执行结果:\n{}\n\n继续或总结回答: 「{}」", output, user_query);
            match state.llm_router.chat_simple(&sys, &followup).await {
                Ok(r) => { response = r; continue; }
                Err(_) => return (progress, format!("脚本输出:\n{}", output)),
            }
        }
        break; // No more code blocks — exit loop
    }
    (progress, response)
}

/// Extract a code block of given language from LLM response.
fn extract_code_block(response: &str, lang: &str) -> Option<String> {
    // Find ```lang or ```lang (case-insensitive, optional space before lang)
    let lower_resp = response.to_lowercase();
    let lang_lower = lang.to_lowercase();
    // Try all possible opening patterns
    let patterns = [
        format!("```{}", lang_lower),
        format!("``` {}", lang_lower),
        format!("```{}\r", lang_lower),
    ];
    for pattern in &patterns {
        if let Some(start) = lower_resp.find(pattern) {
            let code_start = start + pattern.len();
            let after_open = &response[code_start..];
            // Skip newline after opening marker
            let code_begin = after_open.find('\n').map(|n| n + 1).unwrap_or(0);
            // Find closing ```
            let remaining = &after_open[code_begin..];
            if let Some(code_end) = remaining.find("\n```") {
                return Some(remaining[..code_end].trim().to_string());
            }
            if let Some(code_end) = remaining.find("```") {
                return Some(remaining[..code_end].trim().to_string());
            }
        }
    }
    None
}

/// Prepend TAIS agent helpers to Python code.
fn prepend_helpers(code: &str) -> String {
    let helpers = include_str!("../../assets/agent_helpers.py");
    format!("{}\n{}", helpers, code)
}

/// Execute Python code and return stdout.
async fn execute_python_code(code: &str) -> String {
    let full_code = prepend_helpers(code);
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(60),
        tokio::process::Command::new("python")
            .args(["-c", &full_code])
            .env("PYTHONIOENCODING", "utf-8")
            .env("PYTHONUTF8", "1")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()
    ).await;
    let result = match result {
        Ok(r) => r,
        Err(_) => return "⏱️ Python 执行超时 (60s)".into(),
    };
    match result {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let stderr = String::from_utf8_lossy(&out.stderr);
            if stdout.is_empty() && stderr.is_empty() {
                "(无输出)".into()
            } else if stdout.is_empty() {
                format!("(stderr): {}", stderr.trim())
            } else if stderr.is_empty() {
                stdout.trim().to_string()
            } else {
                format!("{}\n(stderr): {}", stdout.trim(), stderr.trim())
            }
        }
        Err(e) => format!("执行失败: {e}"),
    }
}

/// Execute browser-harness code (Python with browser helpers).
async fn execute_browser_harness(code: &str) -> String {
    let result = tokio::process::Command::new("browser-harness")
        .args(["-c", code])
        .env("PYTHONIOENCODING", "utf-8")
        .output()
        .await;
    match result {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let stderr = String::from_utf8_lossy(&out.stderr);
            if stdout.is_empty() && stderr.is_empty() { "(无输出)".into() }
            else if stderr.is_empty() { stdout.trim().to_string() }
            else if stdout.is_empty() { format!("(stderr): {}", stderr.trim()) }
            else { format!("{}\n(stderr): {}", stdout.trim(), stderr.trim()) }
        }
        Err(e) => format!("browser-harness 执行失败: {e}\n提示: 确保 Chrome 远程调试已开启 (chrome://inspect/#remote-debugging)"),
    }
}

/// Execute shell code (PowerShell on Windows, bash on Unix).
async fn execute_shell_code(code: &str) -> String {
    let (shell, arg) = if cfg!(windows) {
        ("powershell", "-Command")
    } else {
        ("bash", "-c")
    };
    let result = tokio::process::Command::new(shell)
        .args([arg, code])
        .output()
        .await;
    match result {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            if stdout.is_empty() { "(无输出)".into() } else { stdout.trim().to_string() }
        }
        Err(e) => format!("执行失败: {e}"),
    }
}

/// Handle natural conversation in General mode — no Socratic, no command parsing.
async fn handle_general(
    state: &Arc<AppState>,
    session_id: &str,
    text: &str,
    gene: &GeneProfile,
    user_id: Option<&str>,
    display_name: Option<&str>,
    current_concept: &mut Option<String>,
) -> (Vec<String>, String) {
    let lower = text.to_lowercase();
    let name = display_name.unwrap_or("访客");
    let now = chrono::Utc::now();
    let time_str = now.format("%Y-%m-%d %H:%M UTC").to_string();

    // ── Check if user has personal LLM or system LLM ──────────────
    let has_personal = if let Some(uid) = user_id {
        state.memory.users.get_llm_config(uid).await.is_some()
    } else { false };
    let has_system = {
        let s = state.llm_router.status().await;
        s.active_configs > 0
    };
    let llm_available = has_personal || has_system;

    // ── Quick commands that also work in general mode ──────────────
    if lower == "help" || lower == "?" || lower == "？" {
        return no_progress(format!(
            "💬 **通用模式** — 我是你的 AI 助手，可以自然对话。\n\n\
            **快捷指令**（通用模式下也可用）:\n\
            ├─ `help` — 此帮助\n\
            ├─ `status` — 系统状态\n\
            ├─ `habits` — 习惯引擎\n\
            ├─ `gene` — 基因人格\n\
            ├─ `学习模式` — 切换教学引导\n\
            ├─ `命令模式` — 切换指令控制\n\
            ├─ `time` / `时间` — 当前时间\n\
            └─ 直接提问或聊天即可\n\n\
            👤 {name} | 🧬 {gene} | 💬 通用模式",
            name = name, gene = gene.personality,
        ));
    }

    if lower == "status" {
        let info = state.agent_loop.get_state().await;
        return no_progress(format!("📊 总轮次: {} | 成功: {} | 快速路径: {} | 模式: 通用",
            info.total_rounds, info.successful_rounds, info.fast_path_rounds));
    }

    if lower == "habits" {
        let states = state.habit_engine.get_all_states().await;
        let mut s = String::from("🔄 习惯状态: ");
        for st in &states {
            let icon = if st.is_auto { "🟢" } else { "🔵" };
            s.push_str(&format!("{}={:.2} ", st.rule_id, st.weight));
        }
        return no_progress(s);
    }

    if lower == "gene" {
        return no_progress(format!("🧬 当前基因: {} | 思维: {} | 风控: {} | 行为: {}",
            gene.personality, gene.thinking, gene.risk_level, gene.behavior));
    }

    if lower == "time" || lower == "时间" || lower.contains("几点") || lower.contains("时间") {
        return no_progress(format!("🕐 当前 UTC 时间: {time}\n📅 日期: {date}",
            time = time_str,
            date = now.format("%Y-%m-%d %A").to_string(),
        ));
    }

    // ── Try to extract concept for context ─────────────────────────
    if let Some(c) = extract_concept(text) {
        *current_concept = Some(c);
    }

    // ── Agent Tool-Using Loop (GenericAgent-inspired) ─────────────
    if llm_available && text.len() > 2 && !lower.starts_with("help")
       && !lower.starts_with("status") && !lower.starts_with("habits")
       && !lower.starts_with("gene") && !lower.starts_with("time")
       && lower != "?" && lower != "？" && lower != "你好" && lower != "hi"
    {
        // Auto-register personal LLM if needed
        if has_personal && !has_system {
            if let Some(uid) = user_id {
                if let Some(cfg) = state.memory.users.get_llm_config(uid).await {
                    let provider = match cfg.provider.as_str() {
                        "anthropic" => llm::ProviderType::Anthropic,
                        "ollama" => llm::ProviderType::Ollama,
                        _ => llm::ProviderType::OpenAI,
                    };
                    let _ = state.llm_router.create_config(llm::LlmConfigRequest {
                        name: format!("个人LLM-{}", uid), provider,
                        base_url: cfg.base_url.clone(), api_key: cfg.api_key.clone(),
                        model: cfg.model.clone(), params: llm::LlmParams::default(),
                        is_default: true, is_active: true,
                    }).await;
                }
            }
        }

        // Hot memory (Hermes-style): small facts injected every turn
        let hot_memory = memory::hot::HotMemory::new();
        let hot_mem_str = hot_memory.to_prompt();

        // Recent turns (last 3 exchanges only, for immediate context)
        let context = state.memory.get_context(session_id).await;
        let recent_turns = if context.total_turns > 0 {
            let turns: Vec<String> = context.recent_turns.iter().rev().take(6).rev().map(|t| {
                let role = if t.role == memory::TurnRole::Student { "用户" } else { "AI" };
                format!("[{}]: {}", role, safe_truncate(&t.content, 200))
            }).collect();
            turns.join("\n")
        } else { String::new() };
        let history = format!("{}{}", hot_mem_str, recent_turns);

        // ── Check SkillLoader for matching crystallized skills ─────
        let skill_hint = {
            let loader = state.agent_loop.skill_loader().await;
            let skills = loader.resolve(Some(&text), 2);
            if skills.is_empty() { String::new() } else {
                format!("\n**相关技能 (已结晶的 SOP)**:\n{skills}\n如适用，请直接调用已结晶的函数。\n")
            }
        };
        let enhanced_query = format!("{}{}", skill_hint, text);

        let result = {
            let state = state.clone();
            let enhanced_query = enhanced_query.clone();
            let name = name.to_string();
            let time_str = time_str.clone();
            let history = history.clone();
            tokio::task::spawn(async move {
                run_agent_loop(&state, &enhanced_query, &name, &time_str, &history).await
            }).await.unwrap_or_else(|e| {
                tracing::error!("Agent loop panicked: {e}");
                (vec!["⚠️ 内部错误，已恢复。".into()], "请重试。".into())
            })
        };

        // ── Auto-crystallize ONLY on successful executions ──────────
        let had_code = result.0.iter().any(|p| p.contains("Agent 执行代码") || p.contains("操控浏览器"));
        let had_output = result.0.iter().any(|p| p.contains("执行结果") || p.contains("浏览器输出"));
        let is_error = result.1.len() < 10 || result.1.contains("失败") || result.1.contains("Error");
        if had_code && had_output && !is_error {
            let skill_name = extract_skill_name(&text);
            // Only crystallize known pattern types, skip conversational queries
            let known = ["weather_query", "news_fetch", "web_search", "calculator", "translator", "web_scraper", "file_ops"];
            if known.contains(&skill_name.as_str()) {
                let desc = format!("{}: {}", skill_name, safe_truncate(text, 60));
                let skills_dir = std::path::PathBuf::from("memory/skills");
                let loader = skills::loader::SkillLoader::new(skills_dir.clone());
                if loader.match_skills(&[&skill_name]).is_empty() {
                    let _ = skills::crystallizer::crystallize_skill(
                        &skills_dir, &skill_name, &desc, "Agent 自动结晶",
                        &["待后续优化"], &["能正确返回结果"],
                    );
                    state.agent_loop.reload_skills().await;
                }
            }
        }

        return result;
    }

    // ── Fallback responses (LLM not configured or failed) ──────────
    if lower.contains("?") || lower.contains("？")
        || lower.starts_with("what") || lower.starts_with("怎么") || lower.starts_with("如何")
        || lower.starts_with("为什么") || lower.contains("什么是")
    {
        // Only mention LLM unavailability once per concept
        let concept_hint = current_concept.as_deref().unwrap_or("");
        let llm_hint = if !concept_hint.is_empty() {
            format!("💡 配置 LLM 后我可以直接回答。\n当前学习主题: {concept_hint}\n输入 `help` 查看功能，或切换到「学习模式」让我引导你思考。")
        } else {
            "💡 配置 LLM 后我可以直接回答你的问题。\n输入 `help` 查看我能做什么。".to_string()
        };
        return no_progress(format!(
            "🤔 关于「{q}」\n\n{llm_hint}",
            q = if text.len() > 120 { &text[..120] } else { text },
            llm_hint = llm_hint,
        ));
    }

    // ── Greeting / casual ──────────────────────────────────────────
    if lower.contains("hello") || lower.contains("你好") || lower.contains("hi")
        || lower == "嗨" || lower.contains("hey")
    {
        return no_progress(format!(
            "👋 你好 {name}！我是 TAIS。\n当前: 💬 通用模式 | 🧬 {gene}\n有什么我可以帮你的？输入 `help` 看功能。",
            name = name, gene = gene.personality,
        ));
    }

    // ── Default natural response ───────────────────────────────────
    no_progress(format!(
        "💬 收到。「{q}」\n输入 `help` 查看我能做什么，或直接告诉我你的需求。",
        q = if text.len() > 60 { &text[..60] } else { text },
    ))
}

// ── Shared Knowledge ────────────────────────────────────────────────────

async fn shared_knowledge_list(
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let knowledge = state.memory.shared.list_knowledge().await;
    Json(serde_json::json!(knowledge))
}

async fn shared_knowledge_upsert(
    State(state): State<Arc<AppState>>,
    Json(req): Json<memory::KnowledgeNode>,
) -> Json<serde_json::Value> {
    state.memory.shared.upsert_knowledge(req).await;
    Json(serde_json::json!({"status": "ok"}))
}

async fn shared_knowledge_search(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Json<serde_json::Value> {
    let q = params.get("q").map(|s| s.as_str()).unwrap_or("");
    let results = state.memory.shared.search_knowledge(q).await;
    Json(serde_json::json!(results))
}

// ── Shared FAQs ────────────────────────────────────────────────────────

async fn shared_faqs_list(
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let faqs = state.memory.shared.top_faqs(50).await;
    Json(serde_json::json!(faqs))
}

async fn shared_faqs_add(
    State(state): State<Arc<AppState>>,
    Json(req): Json<memory::FaqEntry>,
) -> Json<serde_json::Value> {
    state.memory.shared.add_faq(req).await;
    Json(serde_json::json!({"status": "ok"}))
}

async fn shared_faqs_search(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Json<serde_json::Value> {
    let q = params.get("q").map(|s| s.as_str()).unwrap_or("");
    let results = state.memory.shared.search_faqs(q).await;
    Json(serde_json::json!(results))
}

// ── Shared Stats ───────────────────────────────────────────────────────

async fn shared_stats(
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let stats = state.memory.shared.get_stats().await;
    Json(serde_json::json!(stats))
}

// ── Shared Strategies ──────────────────────────────────────────────────

async fn shared_strategies_list(
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let strategies = state.memory.shared.top_strategies(50).await;
    Json(serde_json::json!(strategies))
}

async fn shared_strategies_add(
    State(state): State<Arc<AppState>>,
    Json(req): Json<memory::StrategyEntry>,
) -> Json<serde_json::Value> {
    state.memory.shared.add_strategy(req).await;
    Json(serde_json::json!({"status": "ok"}))
}

// ── User Memory ────────────────────────────────────────────────────────

async fn user_list(
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let users = state.memory.users.list_users().await;
    Json(serde_json::json!(users))
}

async fn user_profile(
    State(state): State<Arc<AppState>>,
    Path(user_id): Path<String>,
) -> Json<serde_json::Value> {
    match state.memory.users.get(&user_id).await {
        Some(profile) => Json(serde_json::json!(profile)),
        None => Json(serde_json::json!({"error": "user not found"})),
    }
}

async fn user_mastery(
    State(state): State<Arc<AppState>>,
    Path(user_id): Path<String>,
) -> Json<serde_json::Value> {
    match state.memory.users.get(&user_id).await {
        Some(profile) => Json(serde_json::json!({
            "user_id": user_id,
            "mastery": profile.mastery,
            "weakest": profile.weakest_concepts(5),
            "misconceptions": profile.active_misconceptions(),
        })),
        None => Json(serde_json::json!({"error": "user not found"})),
    }
}

// ── User Sessions ──────────────────────────────────────────────────────

async fn user_sessions(
    State(state): State<Arc<AppState>>,
    Path(user_id): Path<String>,
) -> Json<serde_json::Value> {
    let sessions = state.memory.users.get_user_sessions(
        &state.memory.session_users,
        &user_id,
    ).await;

    let mut session_data = Vec::new();
    for sid in &sessions {
        let ctx = state.memory.get_context(sid).await;
        session_data.push(serde_json::json!({
            "session_id": sid,
            "total_turns": ctx.total_turns,
            "concepts": ctx.concepts_discussed,
            "last_active": ctx.last_active,
        }));
    }

    Json(serde_json::json!({
        "user_id": user_id,
        "total_sessions": sessions.len(),
        "sessions": session_data,
    }))
}

// ── User LLM Config ────────────────────────────────────────────────────

async fn user_llm_get(
    State(state): State<Arc<AppState>>,
    Path(user_id): Path<String>,
) -> Json<serde_json::Value> {
    // Try in-memory first
    match state.memory.users.get_llm_config(&user_id).await {
        Some(config) => return Json(serde_json::json!(config)),
        None => {
            // Try SQLite fallback
            if let Some(ref pool) = state.db_pool {
                if let Ok(Some(meta_str)) = sqlx::query_scalar::<_, String>(
                    "SELECT metadata FROM users WHERE id = ?1"
                ).bind(&user_id).fetch_optional(pool).await {
                    if let Ok(cfg) = serde_json::from_str::<memory::UserLlmConfig>(&meta_str) {
                        // Restore to memory
                        state.memory.users.set_llm_config(&user_id, cfg.clone()).await;
                        return Json(serde_json::json!(cfg));
                    }
                }
            }
            Json(serde_json::json!({"llm_config": null, "note": "using system default"}))
        }
    }
}

async fn user_llm_set(
    State(state): State<Arc<AppState>>,
    Path(user_id): Path<String>,
    Json(req): Json<memory::UserLlmConfig>,
) -> Json<serde_json::Value> {
    // Save to in-memory
    let result = state.memory.users.set_llm_config(&user_id, req.clone()).await;
    // Persist to SQLite
    if let Some(ref pool) = state.db_pool {
        if let Ok(json) = serde_json::to_string(&req) {
            let _ = sqlx::query(
                "UPDATE users SET metadata = ?1, last_active = datetime('now') WHERE id = ?2"
            ).bind(&json).bind(&user_id).execute(pool).await;
        }
        // Also register as system config for actual LLM calls
        let provider = match req.provider.as_str() {
            "anthropic" => llm::ProviderType::Anthropic,
            "ollama" => llm::ProviderType::Ollama,
            _ => llm::ProviderType::OpenAI,
        };
        let _ = state.llm_router.create_config(llm::LlmConfigRequest {
            name: format!("个人LLM-{}", user_id),
            provider,
            base_url: req.base_url.clone(),
            api_key: req.api_key.clone(),
            model: req.model.clone(),
            params: llm::LlmParams::default(),
            is_default: true,
            is_active: true,
        }).await;
        // Remove previous personal config if exists
        let configs = state.llm_router.list_configs().await;
        for c in &configs {
            if c.name.starts_with("个人LLM-") && c.name != format!("个人LLM-{}", user_id) {
                let _ = state.llm_router.delete_config(&c.id).await;
            }
        }
    }
    match result {
        Some(_) => Json(serde_json::json!({"status": "ok", "user_id": user_id})),
        None => Json(serde_json::json!({"error": "user not found"})),
    }
}

// ── Per-User Dashboard ─────────────────────────────────────────────────

async fn user_dashboard(
    State(state): State<Arc<AppState>>,
    Path(user_id): Path<String>,
) -> Html<String> {
    let profile = state.memory.users.get(&user_id).await;
    let sessions = state.memory.users.get_user_sessions(
        &state.memory.session_users,
        &user_id,
    ).await;

    let html = if let Some(ref p) = profile {
        dashboard::render_user_home(p, sessions.len())
    } else {
        format!(
            "<!DOCTYPE html><html><head><title>TAIS — Not Found</title></head>\
            <body style='font-family:sans-serif;padding:40px;text-align:center;background:#0f172a;color:#e2e8f0;'>\
            <h2>Student Not Found</h2><p>User ID: {user_id}</p>\
            <p>No data yet</p></body></html>"
        )
    };

    Html(html)
}

// ── Task Orchestration Handlers ──────────────────────────────────────────

async fn task_list(
    State(state): State<Arc<AppState>>,
    Path(workflow_id): Path<String>,
) -> Json<serde_json::Value> {
    let tasks = state.task_manager.list_by_workflow(&workflow_id).await;
    Json(serde_json::json!({
        "workflow_id": workflow_id,
        "total": tasks.len(),
        "tasks": tasks,
    }))
}

async fn task_create(
    State(state): State<Arc<AppState>>,
    Path(workflow_id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    let name = body["name"].as_str().unwrap_or("untitled");
    let desc = body["description"].as_str().unwrap_or("");
    let agent = body["agent"].as_str();
    let deps: Vec<String> = body["dependencies"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();

    let task = state.task_manager.create(&workflow_id, name, desc, agent, deps).await;
    Json(serde_json::json!(task))
}

async fn task_update(
    State(state): State<Arc<AppState>>,
    Path(task_id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    let name = body["name"].as_str().map(String::from);
    let desc = body["description"].as_str().map(String::from);
    let agent = body["agent"].as_str().map(String::from);
    let deps: Option<Vec<String>> = body["dependencies"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect());

    match state.task_manager.update(&task_id, name, desc, agent, deps).await {
        Some(task) => Json(serde_json::json!(task)),
        None => Json(serde_json::json!({"error": "task not found"})),
    }
}

async fn task_delete(
    State(state): State<Arc<AppState>>,
    Path(task_id): Path<String>,
) -> Json<serde_json::Value> {
    let removed = state.task_manager.remove(&task_id).await;
    Json(serde_json::json!({"removed": removed, "task_id": task_id}))
}

async fn task_start(
    State(state): State<Arc<AppState>>,
    Path(task_id): Path<String>,
) -> Json<serde_json::Value> {
    match state.task_dispatcher.dispatch(&task_id).await {
        Ok(task) => Json(serde_json::json!({
            "status": "dispatched",
            "task": task,
            "note": "Task spawned on tokio green thread. Poll GET /api/tasks/{task_id} for status."
        })),
        Err(e) => Json(serde_json::json!({"error": e})),
    }
}

async fn task_complete(
    State(state): State<Arc<AppState>>,
    Path(task_id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    let result = body["result"].as_str().unwrap_or("completed");
    match state.task_manager.complete(&task_id, result).await {
        Ok(task) => Json(serde_json::json!(task)),
        Err(e) => Json(serde_json::json!({"error": e})),
    }
}

async fn task_interrupt(
    State(state): State<Arc<AppState>>,
    Path(task_id): Path<String>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Json<serde_json::Value> {
    let reason = params.get("reason").map(|s| s.as_str()).unwrap_or("manual_interrupt");
    match state.task_manager.interrupt(&task_id, reason).await {
        Ok(task) => Json(serde_json::json!(task)),
        Err(e) => Json(serde_json::json!({"error": e})),
    }
}

async fn task_summary(
    State(state): State<Arc<AppState>>,
    Path(workflow_id): Path<String>,
) -> Json<serde_json::Value> {
    let summary = state.task_manager.workflow_summary(&workflow_id).await;
    Json(serde_json::json!(summary))
}

fn render_user_dashboard(profile: &memory::UserProfile, sessions: &[String]) -> String {
    let name = profile.display_name.as_deref().unwrap_or(&profile.user_id);
    let grade = profile.grade_level.as_deref().unwrap_or("N/A");
    let style = profile.preferred_style.as_deref().unwrap_or("N/A");

    let mut mastery_rows = String::new();
    for (concept, m) in &profile.mastery {
        let pct = (m.level * 100.0) as u32;
        let color = if m.level >= 0.85 { "#4CAF50" } else if m.level >= 0.6 { "#FF9800" } else if m.level >= 0.3 { "#FF5722" } else { "#9E9E9E" };
        mastery_rows.push_str(&format!(
            "<tr><td>{concept}</td><td><div style='background:{color};width:{pct}%;height:20px;border-radius:3px;min-width:4px;'></div></td><td>{pct}%</td><td>{}</td></tr>",
            m.exposures
        ));
    }

    let mut mis_rows = String::new();
    for m in profile.active_misconceptions() {
        mis_rows.push_str(&format!(
            "<tr><td>{}</td><td>{}</td><td>{}x</td></tr>",
            m.related_concept, m.description, m.occurrences
        ));
    }

    let llm_info = if let Some(ref cfg) = profile.llm_config {
        format!("{} / {}", cfg.provider, cfg.model)
    } else {
        "System Default".into()
    };

    let session_list: String = sessions.iter()
        .map(|s| format!("<li style='margin:4px 0;color:#94a3b8;font-size:13px;'>{s}</li>"))
        .collect::<Vec<_>>()
        .join("");

    format!("<!DOCTYPE html>
<html lang='zh-CN'>
<head>
<meta charset='UTF-8'>
<meta name='viewport' content='width=device-width, initial-scale=1.0'>
<title>TAIS — {name}</title>
<style>
* {{ margin:0; padding:0; box-sizing:border-box; }}
body {{ font-family: -apple-system, sans-serif; background:#0f172a; color:#e2e8f0; padding:20px; }}
h1 {{ color:#38bdf8; margin-bottom:4px; }}
.subtitle {{ color:#94a3b8; margin-bottom:24px; }}
.card {{ background:#1e293b; border-radius:12px; padding:20px; margin-bottom:20px; border:1px solid #334155; }}
.card h2 {{ color:#38bdf8; margin-bottom:12px; font-size:18px; }}
.grid {{ display:grid; grid-template-columns: repeat(auto-fit, minmax(150px, 1fr)); gap:16px; margin-bottom:20px; }}
.stat {{ background:#1e293b; border-radius:10px; padding:16px; text-align:center; border:1px solid #334155; }}
.stat .value {{ font-size:32px; font-weight:700; color:#38bdf8; }}
.stat .label {{ font-size:13px; color:#94a3b8; margin-top:4px; }}
table {{ width:100%; border-collapse:collapse; font-size:14px; }}
th {{ text-align:left; color:#94a3b8; padding:8px 12px; border-bottom:1px solid #334155; }}
td {{ padding:8px 12px; border-bottom:1px solid #1e293b; }}
</style>
</head>
<body>
<h1>{name}</h1>
<p class='subtitle'>Grade: {grade} | Style: {style} | LLM: {llm_info}</p>

<div class='grid'>
    <div class='stat'><div class='value'>{session_count}</div><div class='label'>Sessions</div></div>
    <div class='stat'><div class='value'>{total_turns}</div><div class='label'>Turns</div></div>
    <div class='stat'><div class='value'>{concept_count}</div><div class='label'>Concepts</div></div>
    <div class='stat'><div class='value'>{mis_count}</div><div class='label'>Misconceptions</div></div>
</div>

<div class='card'>
    <h2>Concept Mastery</h2>
    <table>
    <tr><th>Concept</th><th>Mastery</th><th>%</th><th>Attempts</th></tr>
    {mastery_rows}
    </table>
</div>

<div class='card'>
    <h2>Active Misconceptions</h2>
    <table>
    <tr><th>Concept</th><th>Error</th><th>Count</th></tr>
    {mis_rows}
    </table>
</div>

<div class='card'>
    <h2>Session History</h2>
    <ul style='margin-top:8px;padding-left:16px;'>
    {session_list}
    </ul>
</div>

</body></html>",
        name = name,
        grade = grade,
        style = style,
        llm_info = llm_info,
        session_count = sessions.len(),
        total_turns = profile.total_turns,
        concept_count = profile.mastery.len(),
        mis_count = profile.active_misconceptions().len(),
        mastery_rows = mastery_rows,
        mis_rows = mis_rows,
        session_list = session_list,
    )
}

// ── Agent Loop Handlers ───────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct AgentProposeRequest {
    student_id: String,
    concept: String,
    mastery_level: f64,
    weak_points: Vec<String>,
    learning_style: Option<String>,
    history: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AgentRunRequest {
    student_id: String,
    concept: String,
    mastery_level: f64,
    student_query: String,
    weak_points: Option<Vec<String>>,
    learning_style: Option<String>,
    history: Option<String>,
}

/// POST /api/agent/propose — analyze state and propose next teaching task
async fn agent_propose(
    State(state): State<Arc<AppState>>,
    Json(req): Json<AgentProposeRequest>,
) -> Json<serde_json::Value> {
    let student_state = agent::StudentState {
        student_id: req.student_id,
        concept: req.concept,
        mastery_level: req.mastery_level,
        weak_points: req.weak_points,
        learning_style: req.learning_style.unwrap_or_else(|| "inquiry".into()),
        session_count: 0,
        last_activity: chrono::Utc::now().to_rfc3339(),
    };

    match state.agent_loop.proposer.propose(&student_state, &req.history.unwrap_or_default()).await {
        Ok(proposal) => Json(serde_json::json!({"status":"ok","proposal":proposal})),
        Err(e) => Json(serde_json::json!({"status":"error","message":e.to_string()})),
    }
}

/// POST /api/agent/run — run one iteration of the agent loop
async fn agent_run(
    State(state): State<Arc<AppState>>,
    Json(req): Json<AgentRunRequest>,
) -> Json<serde_json::Value> {
    let student_state = agent::StudentState {
        student_id: req.student_id,
        concept: req.concept,
        mastery_level: req.mastery_level,
        weak_points: req.weak_points.unwrap_or_default(),
        learning_style: req.learning_style.unwrap_or_else(|| "inquiry".into()),
        session_count: 0,
        last_activity: chrono::Utc::now().to_rfc3339(),
    };

    match state.agent_loop.run_one(
        &student_state,
        &req.student_query,
        &req.history.unwrap_or_default(),
    ).await {
        Ok(rating) => {
            let loop_state = state.agent_loop.get_state().await;
            Json(serde_json::json!({
                "status": "ok",
                "rating": rating,
                "loop_state": loop_state,
            }))
        }
        Err(e) => Json(serde_json::json!({"status":"error","message":e.to_string()})),
    }
}

/// GET /api/agent/status — get current agent loop stats
async fn agent_status(
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let stats = state.agent_loop.get_stats().await;
    Json(serde_json::json!({"status":"ok","stats":stats}))
}

/// POST /api/agent/reset — reset agent loop state
async fn agent_reset(
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    state.agent_loop.reset().await;
    Json(serde_json::json!({"status":"ok","message":"agent loop reset"}))
}

// ── LLM Proxy ──────────────────────────────────────────────────────────────

/// POST /api/llm/fetch-models — proxy request to get available models (bypass CORS)
async fn fetch_llm_models(
    Json(body): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    let base_url = body["base_url"].as_str().unwrap_or("").trim_end_matches('/');
    let api_key = body["api_key"].as_str().unwrap_or("");

    if base_url.is_empty() {
        return Json(serde_json::json!({"error": "base_url required"}));
    }

    let client = reqwest::Client::new();
    let mut models: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    // Try 1: GET /models (OpenAI-compatible)
    let mut req = client.get(format!("{}/models", base_url))
        .header("Content-Type", "application/json");
    if !api_key.is_empty() {
        req = req.header("Authorization", format!("Bearer {}", api_key));
    }
    match req.timeout(std::time::Duration::from_secs(10)).send().await {
        Ok(resp) => {
            if resp.status().is_success() {
                if let Ok(data) = resp.json::<serde_json::Value>().await {
                    if let Some(arr) = data.get("data").and_then(|d| d.as_array()) {
                        models = arr.iter()
                            .filter_map(|m| m.get("id").and_then(|id| id.as_str()).map(String::from))
                            .collect();
                    }
                }
            }
            if models.is_empty() {
                errors.push(format!("/models returned {} models", models.len()));
            }
        }
        Err(e) => errors.push(format!("/models: {e}")),
    }

    // Try 2: Test connectivity with chat completion
    let test_connected = if models.is_empty() {
        let test_body = serde_json::json!({
            "model": "deepseek-chat",
            "messages": [{"role": "user", "content": "hi"}],
            "max_tokens": 5,
        });
        let mut req = client.post(format!("{}/chat/completions", base_url))
            .json(&test_body);
        if !api_key.is_empty() {
            req = req.header("Authorization", format!("Bearer {}", api_key));
        }
        match req.timeout(std::time::Duration::from_secs(15)).send().await {
            Ok(resp) => {
                let status = resp.status().as_u16();
                if status == 200 || status == 400 {
                    // 400 = bad request (e.g. model not found) but API IS reachable
                    if status == 200 { true }
                    else {
                        // Model name might be wrong, but connection works
                        let body = resp.text().await.unwrap_or_default();
                        errors.push(format!("chat test: HTTP {} {}", status,
                            if body.len() > 100 { &body[..100] } else { &body }));
                        true
                    }
                } else {
                    errors.push(format!("chat test HTTP {}", status));
                    false
                }
            }
            Err(e) => {
                errors.push(format!("chat test: {e}"));
                false
            }
        }
    } else { true };

    // Build known model list based on provider
    if models.is_empty() {
        // Detect provider from URL
        let known = if base_url.contains("deepseek") {
            vec!["deepseek-chat", "deepseek-reasoner"]
        } else if base_url.contains("openai") {
            vec!["gpt-4o", "gpt-4-turbo", "gpt-4", "gpt-3.5-turbo"]
        } else if base_url.contains("anthropic") {
            vec!["claude-opus-4-6", "claude-sonnet-4-6", "claude-haiku-4-5"]
        } else if base_url.contains("ollama") || base_url.contains("11434") {
            vec!["llama3", "mistral", "gemma3", "qwen3"]
        } else {
            vec!["deepseek-chat", "deepseek-reasoner", "gpt-4o", "gpt-3.5-turbo",
                 "claude-sonnet-4-6", "claude-haiku-4-5"]
        };
        models = known.into_iter().map(String::from).collect();
    }

    if test_connected {
        let msg = if errors.is_empty() {
            format!("✅ 连接成功 — {} 个模型可用", models.len())
        } else {
            format!("✅ 连接成功 ({} 个备选模型。获取列表: {})", models.len(), errors.join("; "))
        };
        Json(serde_json::json!({"ok": true, "models": models, "count": models.len(), "message": msg, "source": if errors.is_empty() { "api" } else { "known" }}))
    } else {
        Json(serde_json::json!({"ok": false, "error": errors.join("; "), "models": models, "count": models.len(), "source": "known_fallback"}))
    }
}

// ── Habit Engine API ────────────────────────────────────────────────────────

/// GET /api/habits/list — list all 7 habit rules
async fn habit_list(
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let rules = state.habit_engine.get_rules().await;
    let rules_json: Vec<serde_json::Value> = rules.iter().map(|r| {
        serde_json::json!({
            "id": r.id,
            "name": r.name,
            "description": r.description,
            "trigger_type": format!("{:?}", r.trigger_type),
            "condition": format!("{:?}", r.condition),
            "action": format!("{:?}", r.action),
            "learning_rate": r.learning_rate,
            "decay_rate": r.decay_rate,
        })
    }).collect();
    Json(serde_json::json!({"habits": rules_json, "count": rules_json.len()}))
}

/// GET /api/habits/state — all 7 habit states
async fn habit_all_states(
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let states = state.habit_engine.get_all_states().await;
    let states_json: Vec<serde_json::Value> = states.iter().map(|s| {
        serde_json::json!({
            "rule_id": s.rule_id,
            "weight": format!("{:.4}", s.weight),
            "success_count": s.success_count,
            "failure_count": s.failure_count,
            "streak": s.streak,
            "is_auto": s.is_auto,
            "last_triggered": s.last_triggered.format("%Y-%m-%d %H:%M:%S").to_string(),
        })
    }).collect();
    Json(serde_json::json!({"states": states_json, "count": states_json.len()}))
}

/// GET /api/habits/{id}/status — single habit state
async fn habit_status(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Json<serde_json::Value> {
    match state.habit_engine.get_state(&id).await {
        Some(s) => Json(serde_json::json!({
            "rule_id": s.rule_id,
            "weight": format!("{:.4}", s.weight),
            "success_count": s.success_count,
            "failure_count": s.failure_count,
            "streak": s.streak,
            "is_auto": s.is_auto,
            "is_stable": s.weight > THETA_STABLE,
            "last_triggered": s.last_triggered.format("%Y-%m-%d %H:%M:%S").to_string(),
        })),
        None => Json(serde_json::json!({"error": "Habit not found"})),
    }
}

/// POST /api/habits/{id}/trigger — manually trigger a habit
async fn habit_trigger(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    match state.habit_engine.trigger(&id, body).await {
        Ok(log) => Json(serde_json::json!({
            "id": log.id,
            "rule_id": log.rule_id,
            "triggered_at": log.triggered_at.format("%Y-%m-%d %H:%M:%S").to_string(),
            "action_result": log.action_result,
            "success": log.success,
            "duration_ms": log.duration_ms,
        })),
        Err(e) => Json(serde_json::json!({"error": e})),
    }
}

/// GET /api/habits/{id}/logs — habit execution history
async fn habit_logs(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Json<serde_json::Value> {
    let limit: usize = params.get("limit")
        .and_then(|v| v.parse().ok())
        .unwrap_or(50);
    let logs = state.habit_engine.get_logs(Some(&id), limit).await;
    let logs_json: Vec<serde_json::Value> = logs.iter().map(|l| {
        serde_json::json!({
            "id": l.id,
            "rule_id": l.rule_id,
            "triggered_at": l.triggered_at.format("%Y-%m-%d %H:%M:%S").to_string(),
            "context": l.context,
            "action_result": l.action_result,
            "success": l.success,
            "duration_ms": l.duration_ms,
        })
    }).collect();
    Json(serde_json::json!({"logs": logs_json, "count": logs_json.len()}))
}

// ── Skill Crystallization API ──────────────────────────────────────────────

/// POST /api/skills/crystallize — distill a teaching pattern into a reusable SOP
async fn skill_crystallize(
    State(state): State<Arc<AppState>>,
    Json(body): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    let skill_name = body.get("skill_name").and_then(|v| v.as_str()).unwrap_or("");
    let description = body.get("description").and_then(|v| v.as_str()).unwrap_or("");
    let strategy = body.get("strategy").and_then(|v| v.as_str()).unwrap_or("");
    let pitfalls: Vec<&str> = body.get("pitfalls")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|s| s.as_str()).collect())
        .unwrap_or_default();
    let indicators: Vec<&str> = body.get("success_indicators")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|s| s.as_str()).collect())
        .unwrap_or_default();

    if skill_name.is_empty() || description.is_empty() || strategy.is_empty() {
        return Json(serde_json::json!({"error": "skill_name, description, and strategy are required"}));
    }

    let skills_dir = std::path::PathBuf::from("memory/skills");
    match skills::crystallizer::crystallize_skill(
        &skills_dir, skill_name, description, strategy,
        &pitfalls, &indicators,
    ) {
        Ok(result) => {
            // Reload skill loader to pick up the new skill
            state.agent_loop.reload_skills().await;
            Json(serde_json::json!({
                "status": "ok",
                "skill_name": result.skill_name,
                "sop_path": result.sop_path,
                "index_updated": result.index_updated,
                "message": result.message,
            }))
        }
        Err(e) => Json(serde_json::json!({"error": e})),
    }
}

/// GET /api/skills/crystallized — list self-evolved skills
async fn skill_crystallized_list(
    _state: State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let skills_dir = std::path::PathBuf::from("memory/skills");
    let skills = skills::crystallizer::list_crystallized_skills(&skills_dir);
    Json(serde_json::json!({"skills": skills, "count": skills.len()}))
}

/// POST /api/skills/reload — reload skill index
async fn skill_reload(
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    state.agent_loop.reload_skills().await;
    Json(serde_json::json!({"status": "ok", "message": "Skill index reloaded"}))
}

// ── Working Checkpoint API ─────────────────────────────────────────────────

/// GET /api/agent/checkpoint — get current working checkpoint
async fn agent_get_checkpoint(
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    match state.agent_loop.get_checkpoint().await {
        Some(cp) => Json(serde_json::json!({
            "key_info": cp.key_info,
            "related_sop": cp.related_sop,
            "passed_sessions": cp.passed_sessions,
            "is_stale": cp.is_stale(),
            "created_at": cp.created_at.format("%Y-%m-%d %H:%M:%S").to_string(),
        })),
        None => Json(serde_json::json!({"checkpoint": null})),
    }
}

/// POST /api/agent/checkpoint — set working checkpoint
async fn agent_set_checkpoint(
    State(state): State<Arc<AppState>>,
    Json(body): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    let key_info = body.get("key_info").and_then(|v| v.as_str()).unwrap_or("");
    let related_sop = body.get("related_sop").and_then(|v| v.as_str());
    state.agent_loop.update_checkpoint(key_info, related_sop).await;
    Json(serde_json::json!({"status": "ok", "message": "Checkpoint updated"}))
}

/// DELETE /api/agent/checkpoint — clear working checkpoint
async fn agent_clear_checkpoint(
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    state.agent_loop.clear_checkpoint().await;
    Json(serde_json::json!({"status": "ok", "message": "Checkpoint cleared"}))
}
