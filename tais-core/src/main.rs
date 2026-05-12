// TAIS Core Engine — entry point
//
// Starts the axum HTTP/WebSocket server with all subsystems:
//   - MCP Gateway (tool registration + dispatch)
//   - Orchestrator (workflow DAG generation + execution)
//   - Evolution Engine (TextGrad-style self-optimization)
//   - Skills Bus (TAIS skill dispatch)
//   - Gene Gateway (persona injection at decision points)
//   - LlmRouter (OpenAI / Anthropic / Ollama unified LLM layer)
//   - Dashboard (system status HTML page)
//
// Usage:
//   cargo run                          # Start server on 0.0.0.0:9527
//   cargo run -- --reset-password caridle1 newpass  # Reset user password
//   TAIS_PORT=8080 cargo run           # Custom port
//   TAIS_AUTO_DEPLOY=true cargo run    # Enable auto-deploy (DANGER)

use argon2::{PasswordHasher, password_hash::SaltString, password_hash::rand_core::OsRng};
use std::sync::Arc;
use std::sync::atomic::AtomicU32;
use tokio::net::TcpListener;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // ── CLI: --reset-password <username> <newpassword> ──────────────
    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 4 && args[1] == "--reset-password" {
        let username = &args[2];
        let new_password = &args[3];
        // Hash the new password
        let salt = SaltString::generate(&mut OsRng);
        let hash = argon2::Argon2::default()
            .hash_password(new_password.as_bytes(), &salt)
            .map(|h| h.to_string())
            .map_err(|e| anyhow::anyhow!("Hash error: {e}"))?;
        // Update SQLite
        let config = tais_core::config::Config::load();
        if let Some(pool) = tais_core::data::init_db(&config.database).await {
            let result = sqlx::query(
                "INSERT OR REPLACE INTO users (id, display_name, password_hash, first_seen, last_active)
                 VALUES (?1, ?1, ?2, datetime('now'), datetime('now'))"
            ).bind(username).bind(&hash).execute(&pool).await;
            match result {
                Ok(_) => println!("✅ Password reset for '{}' — new password: {}", username, new_password),
                Err(e) => eprintln!("❌ Failed: {e}"),
            }
        } else {
            eprintln!("❌ Database not available. Ensure tais.db exists (start server once first).");
        }
        return Ok(());
    }

    // Initialize tracing
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| "tais_core=info,tower_http=info".into()))
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!("🧬 TAIS Core Engine v0.2.0 starting...");

    // Load configuration
    let config = tais_core::config::Config::load();

    // ── Database initialization ──
    let db_pool = tais_core::data::init_db(&config.database).await;
    let db_pool = match db_pool {
        Some(ref pool) => {
            tracing::info!("Database initialized (max {} connections)", config.database.max_connections);
            Some(pool.clone())
        }
        None => {
            tracing::info!("Running in memory-only mode (no database)");
            None
        }
    };

    // ── Initialize subsystems ──

    // MCP Gateway
    let mcp_gateway = Arc::new(tais_core::mcp::Gateway::new());
    for (name, desc) in all_capsule_tools() {
        mcp_gateway.register_skill_tool(tais_core::McpTool {
            name,
            description: desc,
            parameters: serde_json::json!({"type": "object"}),
        }).await;
    }

    // Orchestrator
    let orchestrator = Arc::new(
        tais_core::orchestrator::Orchestrator::new().with_mcp(mcp_gateway.clone())
    );

    // Evolution Engine
    let evolution_engine = Arc::new(tais_core::evolution::EvolutionEngine::new(
        config.evolution.threshold,
        config.evolution.min_sessions,
    ));

    // Skills Bus
    let skills_bus = Arc::new(tais_core::skills::SkillsBus::new());

    // LLM Router
    let llm_router = Arc::new(tais_core::llm::LlmRouter::new());
    tracing::info!("LLM Router initialized (0 configs — use POST /api/llm/configs to add)");

    // Register all 7 built-in TAIS skills
    {
        let count = tais_core::skills::implementations::all_tais_skills(llm_router.clone()).len();
        for skill in tais_core::skills::implementations::all_tais_skills(llm_router.clone()) {
            let name = skill.name().to_string();
            let def = skill.definition().clone();
            // Install definition first
            if let Err(e) = skills_bus.install(def).await {
                tracing::warn!("Failed to install skill {name}: {e}");
                continue;
            }
            // Then register
            if let Err(e) = skills_bus.register(skill).await {
                tracing::warn!("Failed to register skill {name}: {e}");
            } else {
                tracing::info!("✓ Registered: {name}");
            }
        }
        tracing::info!("TAIS skills loaded: {count} installed & registered");
    }

    // Auto-configure LLM from environment (only if explicitly set)
    // Disabled by default — use Dashboard or `llm add` command instead
    if std::env::var("TAIS_AUTO_LLM").is_ok() {
        if let Ok(key) = std::env::var("DEEPSEEK_API_KEY") {
            if !key.is_empty() {
                let _ = llm_router.create_config(tais_core::llm::LlmConfigRequest {
                    name: "DeepSeek".into(), provider: tais_core::llm::ProviderType::OpenAI,
                    base_url: "https://api.deepseek.com/v1".into(), api_key: key,
                    model: "deepseek-chat".into(), params: tais_core::llm::LlmParams::default(),
                    is_default: true, is_active: true,
                }).await;
            }
        }
    }

    // WeChat Bot
    let wechat_token = std::env::var("WECHAT_TOKEN").unwrap_or_else(|_| "tais_wechat_token".into());
    let wechat_bot = Arc::new(tokio::sync::Mutex::new(
        tais_core::wechat::WechatBot::new(&wechat_token, None, None)
    ));
    tracing::info!("WeChat Bot initialized (token={}...)", &wechat_token[..8.min(wechat_token.len())]);

    // ── Habit Engine ──
    let habit_engine = Arc::new(tais_core::habit::HabitEngine::new());
    for rule in tais_core::habit::rules::all_habit_rules() {
        let name = rule.name.clone();
        let id = rule.id.clone();
        match habit_engine.register(rule).await {
            Ok(()) => tracing::info!("✓ Habit registered: {id} — {name}"),
            Err(e) => tracing::warn!("Failed to register habit {id}: {e}"),
        }
    }
    if let Some(ref pool) = db_pool {
        habit_engine.with_db(pool.clone()).await;
    }
    tracing::info!("Habit Engine initialized with 7 capsules");

    // Start periodic scheduler for H01 daily review (runs in background)
    let he = habit_engine.clone();
    tokio::spawn(async move {
        he.start_scheduler().await;
    });
    tracing::info!("Habit scheduler started (checks every 60s)");

    // Memory (with optional DB persistence)
    let memory = if let Some(ref pool) = db_pool {
        Arc::new(tais_core::memory::SessionMemory::new().with_db(pool.clone()))
    } else {
        Arc::new(tais_core::memory::SessionMemory::new())
    };
    tracing::info!("Conversation memory initialized");

    // Auth
    let jwt_secret = std::env::var("TAIS_JWT_SECRET")
        .unwrap_or_else(|_| uuid::Uuid::new_v4().to_string());
    let token_expiry = std::env::var("TAIS_TOKEN_EXPIRY_HOURS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(24);
    let auth_state = Arc::new(tais_core::auth::AuthState::new(&jwt_secret, token_expiry));
    tracing::info!("Auth system initialized (JWT expiry={token_expiry}h)");

    tracing::info!(
        "Total capsules registered: 35 (14 OO + 7 Gene + 7 TAIS + 7 Habit)",
    );

    let tm = Arc::new(tais_core::orchestrator::task::TaskManager::new());
    let task_dispatcher = Arc::new(tais_core::orchestrator::task::TaskDispatcher::new(
        tm.clone(),
        skills_bus.clone(),
    ));

    // ── Build AgentLoop ──
    let agent_loop = Arc::new(tais_core::agent::AgentLoop::new(
        Arc::new(tais_core::agent::Proposer::new(llm_router.clone())),
        Arc::new(tais_core::agent::Consumer::new(skills_bus.clone())),
        Arc::new(tais_core::agent::Rater::new(llm_router.clone())),
        Arc::new(tais_core::agent::Deployer::new(evolution_engine.clone())),
        habit_engine.clone(),
    ));

    // ── Build app state ──
    let state = Arc::new(tais_core::api::AppState {
        orchestrator,
        mcp_gateway,
        evolution_engine,
        skills_bus,
        llm_router,
        wechat_bot,
        memory,
        task_manager: tm,
        task_dispatcher,
        agent_loop,
        auth_state,
        habit_engine,
        db_pool,
        active_ws_count: AtomicU32::new(0),
    });

    // ── Start server ──
    let router = tais_core::api::build_router(state);
    let addr = format!("{}:{}", config.server.host, config.server.port);
    let listener = TcpListener::bind(&addr).await?;

    tracing::info!("🚀 TAIS server listening on http://{addr}");
    tracing::info!("   GET  /                          — Dashboard 首页");
    tracing::info!("   POST /api/workflow/generate      — Generate teaching workflow");
    tracing::info!("   POST /api/workflow/execute       — Execute workflow");
    tracing::info!("   GET  /api/evolution/metrics      — View evolution metrics");
    tracing::info!("   POST /api/evolution/review       — Teacher review evolution");
    tracing::info!("   GET  /api/skills/list            — List TAIS skills");
    tracing::info!("   GET  /api/mcp/tools              — List MCP tools");
    tracing::info!("   GET  /api/llm/configs            — List LLM configs");
    tracing::info!("   POST /api/llm/configs            — Create LLM config");
    tracing::info!("   PUT  /api/llm/configs/:id        — Update LLM config");
    tracing::info!("   DEL  /api/llm/configs/:id        — Delete LLM config");
    tracing::info!("   POST /api/llm/configs/:id/test   — Test LLM connection");
    tracing::info!("   GET  /login                     — Login / Register page");
    tracing::info!("   GET  /chat                      — Web Chat UI");
    tracing::info!("   POST /api/auth/register         — User registration");
    tracing::info!("   POST /api/auth/login            — User login (JWT)");
    tracing::info!("   POST /api/auth/qr/request       — QR login: request token");
    tracing::info!("   GET  /api/auth/qr/status/:token — QR login: poll status");
    tracing::info!("   POST /api/auth/qr/confirm       — QR login: confirm");
    tracing::info!("   POST /api/auth/wechat/bind      — Request WeChat binding code");
    tracing::info!("   GET  /api/auth/wechat/bind-status — Poll binding status");
    tracing::info!("   GET  /api/auth/me               — Current user info");
    tracing::info!("   WS   /api/session/:id            — Student real-time session");

    axum::serve(listener, router).await?;

    Ok(())
}

/// Register all 35 capsules as MCP tools
fn all_capsule_tools() -> Vec<(String, String)> {
    vec![
        // OO Capability Capsules (14)
        ("oo-prd-generator".into(), "PRD生成器：一句话需求→8节结构化PRD".into()),
        ("oo-user-story".into(), "User Story拆解：PRD→14条INVEST Story".into()),
        ("oo-use-case".into(), "Use Case建模：Story→PlantUML用例图+规约".into()),
        ("oo-domain-model".into(), "领域模型：Use Case→DDD战术设计".into()),
        ("oo-class-diagram".into(), "类图：领域模型→OO类图+SOLID检查".into()),
        ("oo-sequence-diagram".into(), "时序图：Use Case→PlantUML时序图".into()),
        ("oo-state-diagram".into(), "状态图：类图→UML状态机".into()),
        ("oo-activity-diagram".into(), "活动图：Use Case→泳道活动图".into()),
        ("oo-component-diagram".into(), "组件图：类图→组件图+接口+技术栈".into()),
        ("oo-ui-prototype".into(), "UI原型：Story→HTML原型+页面清单".into()),
        ("oo-api-spec".into(), "API规范：时序图→OpenAPI 3.0 YAML".into()),
        ("oo-db-schema".into(), "DB Schema：领域模型→DDL+ER图".into()),
        ("oo-design-pattern".into(), "设计模式：类图→GoF 23模式推荐".into()),
        ("oo-code-scaffold".into(), "代码骨架：设计产出→DDD分层项目骨架".into()),
        // Gene Capsules (7)
        ("gene-personality".into(), "人格基因：Big Five性格模板".into()),
        ("gene-thinking".into(), "思维基因：第一性原理/类比/系统思维".into()),
        ("gene-decision".into(), "决策基因：ρ风险/τ阈值/α精度".into()),
        ("gene-behavior".into(), "行为基因：δ直白/σ简洁/γ主动".into()),
        ("gene-risk-control".into(), "风控基因：安全红线/合规底线".into()),
        ("gene-evolution".into(), "进化基因：成功模式/教训库".into()),
        ("gene-heredity".into(), "遗传基因：交叉/突变/选择算子".into()),
        // TAIS Teaching Capsules (7)
        ("tais-workflow".into(), "教学工作流编排器".into()),
        ("tais-learning-analyst".into(), "学情分析师".into()),
        ("tais-socratic-tutor".into(), "苏格拉底式导师".into()),
        ("tais-resource-pusher".into(), "个性化资源推送员".into()),
        ("tais-skill-coach".into(), "技能教练".into()),
        ("tais-feedback-collector".into(), "反馈采集器".into()),
        ("tais-evolution".into(), "自进化引擎".into()),
        // Habit Capsules (7)
        ("habit-review".into(), "复盘习惯：每日定时总结".into()),
        ("habit-error-handling".into(), "容错习惯：错误模式识别+策略切换".into()),
        ("habit-communication".into(), "沟通习惯：确认理解+总结共识".into()),
        ("habit-documentation".into(), "文档习惯：自动更新日志".into()),
        ("habit-optimization".into(), "优化习惯：自我审查+对比历史".into()),
        ("habit-security".into(), "安全习惯：高风险操作检查清单".into()),
        ("habit-collaboration".into(), "协作习惯：握手协议+任务拆解".into()),
        // Auto-Evolution Tools (GenericAgent-inspired)
        ("crystallize_skill".into(), "技能结晶：成功教学→可复用SOP文件".into()),
        ("update_working_checkpoint".into(), "工作记忆：短记事本<200 tokens".into()),
    ]
}
