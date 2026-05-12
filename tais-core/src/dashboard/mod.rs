// Dashboard — self-contained HTML page showing system status, LLM configs, evolution overview

/// Render the full dashboard HTML page with live data
pub fn render(
    llm_status: &crate::llm::LlmStatus,
    metrics: Option<&crate::EvolutionMetrics>,
    skill_count: usize,
    active_ws_count: u32,
    evolution_rounds: usize,
) -> String {
    let llm_rows = render_llm_rows(llm_status);
    let metrics_panel = render_metrics_panel(metrics, evolution_rounds);

    format!(r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>TAIS Core Engine — Dashboard</title>
<style>
  * {{ margin: 0; padding: 0; box-sizing: border-box; }}
  body {{
    background: #0d1117; color: #c9d1d9; font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
    min-height: 100vh;
  }}
  .header {{
    background: #161b22; border-bottom: 1px solid #30363d; padding: 16px 24px;
    display: flex; justify-content: space-between; align-items: center;
  }}
  .header h1 {{ font-size: 20px; color: #58a6ff; }}
  .header .version {{ color: #8b949e; font-size: 13px; }}
  .grid {{
    display: grid; grid-template-columns: repeat(auto-fit, minmax(360px, 1fr));
    gap: 16px; padding: 24px;
  }}
  .card {{
    background: #161b22; border: 1px solid #30363d; border-radius: 8px; overflow: hidden;
  }}
  .card-header {{
    background: #21262d; padding: 12px 16px; border-bottom: 1px solid #30363d;
    font-weight: 600; font-size: 14px; display: flex; align-items: center; gap: 8px;
  }}
  .card-body {{ padding: 16px; }}
  .status-grid {{
    display: grid; grid-template-columns: 1fr 1fr; gap: 12px;
  }}
  .stat {{ padding: 12px; background: #0d1117; border-radius: 6px; border: 1px solid #21262d; }}
  .stat-label {{ font-size: 11px; color: #8b949e; text-transform: uppercase; }}
  .stat-value {{ font-size: 24px; font-weight: 600; color: #58a6ff; margin-top: 4px; }}
  .stat-value.green {{ color: #3fb950; }}
  .stat-value.orange {{ color: #d29922; }}
  .stat-value.red {{ color: #f85149; }}
  table {{ width: 100%; border-collapse: collapse; font-size: 13px; }}
  th {{ text-align: left; padding: 8px 12px; background: #21262d; color: #8b949e; font-weight: 500; font-size: 11px; text-transform: uppercase; }}
  td {{ padding: 8px 12px; border-bottom: 1px solid #21262d; }}
  tr:hover {{ background: #1c2128; }}
  .badge {{
    display: inline-block; padding: 2px 8px; border-radius: 12px; font-size: 11px; font-weight: 500;
  }}
  .badge-openai {{ background: #10a37f20; color: #10a37f; border: 1px solid #10a37f40; }}
  .badge-anthropic {{ background: #d4a57420; color: #d4a574; border: 1px solid #d4a57440; }}
  .badge-ollama {{ background: #58a6ff20; color: #58a6ff; border: 1px solid #58a6ff40; }}
  .badge-connected {{ background: #3fb95020; color: #3fb950; border: 1px solid #3fb95040; }}
  .badge-disconnected {{ background: #f8514920; color: #f85149; border: 1px solid #f8514940; }}
  .badge-default {{ background: #d2992220; color: #d29922; border: 1px solid #d2992240; }}
  .progress-bar {{
    height: 8px; background: #21262d; border-radius: 4px; overflow: hidden; margin-top: 8px;
  }}
  .progress-fill {{ height: 100%; border-radius: 4px; transition: width 0.3s; }}
  .metric-row {{ margin-bottom: 12px; }}
  .metric-label {{ display: flex; justify-content: space-between; font-size: 12px; margin-bottom: 4px; }}
  .metric-name {{ color: #8b949e; }}
  .metric-value {{ color: #c9d1d9; font-weight: 600; }}
  .empty {{ text-align: center; color: #484f58; padding: 32px; font-size: 14px; }}
  .footer {{ padding: 16px 24px; border-top: 1px solid #30363d; color: #484f58; font-size: 12px; text-align: center; }}
  .btn {{
    display: inline-block; padding: 6px 12px; border-radius: 6px; border: 1px solid #30363d;
    background: #21262d; color: #c9d1d9; cursor: pointer; font-size: 12px; text-decoration: none;
    transition: all 0.15s;
  }}
  .btn:hover {{ background: #30363d; border-color: #58a6ff; }}
  .btn-primary {{ background: #238636; border-color: #2ea043; color: #fff; }}
  .btn-primary:hover {{ background: #2ea043; }}
  @media (max-width: 768px) {{
    .grid {{ grid-template-columns: 1fr; padding: 12px; }}
    .status-grid {{ grid-template-columns: 1fr 1fr; }}
  }}
</style>
</head>
<body>
<div class="header">
  <div>
    <h1>🧬 TAIS Core Engine</h1>
    <span class="version">v0.1.0 · Rust 2024</span>
  </div>
  <div style="display:flex;gap:8px">
    <a href="/api/llm/configs" class="btn">API: LLM 配置</a>
    <a href="/api/evolution/metrics" class="btn">API: 进化指标</a>
    <a href="/api/health" class="btn">API: 健康检查</a>
  </div>
</div>

<div class="grid">
  <!-- System Status -->
  <div class="card">
    <div class="card-header">📊 系统状态</div>
    <div class="card-body">
      <div class="status-grid">
        <div class="stat">
          <div class="stat-label">运行状态</div>
          <div class="stat-value green">🟢 运行中</div>
        </div>
        <div class="stat">
          <div class="stat-label">已注册胶囊</div>
          <div class="stat-value">{skill_count}</div>
        </div>
        <div class="stat">
          <div class="stat-label">LLM 配置</div>
          <div class="stat-value">{total_configs}</div>
        </div>
        <div class="stat">
          <div class="stat-label">已连接 LLM</div>
          <div class="stat-value green">{connected}</div>
        </div>
        <div class="stat">
          <div class="stat-label">活跃 WebSocket</div>
          <div class="stat-value">{active_ws}</div>
        </div>
        <div class="stat">
          <div class="stat-label">进化轮次</div>
          <div class="stat-value orange">{evolution_rounds_count}</div>
        </div>
      </div>
    </div>
  </div>

  <!-- Metrics Panel -->
  {metrics_panel}

  <!-- LLM Configs -->
  <div class="card" style="grid-column: 1 / -1;">
    <div class="card-header">🧠 LLM 模型配置</div>
    <div class="card-body">
      {llm_rows}
    </div>
  </div>
</div>

<div class="footer">
  TAIS Core Engine · Teacher-AI-Student Self-Evolving System · 王新年 · 2026
</div>
</body>
</html>"#,
    total_configs = llm_status.total_configs,
    connected = llm_status.connected_count,
    active_ws = active_ws_count,
    skill_count = skill_count,
    evolution_rounds_count = evolution_rounds,
    llm_rows = llm_rows,
    metrics_panel = metrics_panel,
)
}

fn render_llm_rows(status: &crate::llm::LlmStatus) -> String {
    if status.models.is_empty() {
        return r#"<div class="empty">🧠 暂未配置 LLM 模型<br><br>
        <small>使用 POST /api/llm/configs 添加模型配置</small></div>"#.into();
    }

    let mut rows = String::from(r#"<table>
      <thead><tr>
        <th>名称</th><th>提供商</th><th>模型</th><th>状态</th><th>默认</th>
      </tr></thead><tbody>"#);

    for m in &status.models {
        let provider_badge = match m.provider_type {
            crate::llm::ProviderType::OpenAI => "badge-openai",
            crate::llm::ProviderType::Anthropic => "badge-anthropic",
            crate::llm::ProviderType::Ollama => "badge-ollama",
        };
        let conn_badge = if m.is_connected { "badge-connected" } else { "badge-disconnected" };
        let conn_text = if m.is_connected { "🟢 已连接" } else { "🔴 未连接" };
        let default_badge = if m.is_default { r#"<span class="badge badge-default">默认</span>"# } else { "" };

        rows.push_str(&format!(
            r#"<tr>
              <td><strong>{name}</strong></td>
              <td><span class="badge {pb}">{pt}</span></td>
              <td><code>{model}</code></td>
              <td><span class="badge {cb}">{ct}</span></td>
              <td>{db}</td>
            </tr>"#,
            name = m.name,
            pb = provider_badge,
            pt = m.provider_type.to_string(),
            model = m.model,
            cb = conn_badge,
            ct = conn_text,
            db = default_badge,
        ));
    }

    rows.push_str("</tbody></table>");
    rows
}

fn render_metrics_panel(
    metrics: Option<&crate::EvolutionMetrics>,
    rounds: usize,
) -> String {
    let m = match metrics {
        Some(m) => m,
        None => return format!(r#"<div class="card">
          <div class="card-header">📈 进化指标</div>
          <div class="card-body"><div class="empty">暂无数据 · 需要至少 {} 个会话</div></div>
        </div>"#, 50),
    };

    format!(r#"<div class="card">
      <div class="card-header">📈 进化指标 (综合评分: {:.2})</div>
      <div class="card-body">
        {metric_row}
        {metric_row_te}
        {metric_row_sa}
        {metric_row_re}
        {metric_row_ts}
        <div style="margin-top:12px; padding-top:12px; border-top:1px solid #21262d; font-size:11px; color:#8b949e">
          基于 {rounds} 轮进化数据
        </div>
      </div>
    </div>"#,
    m.composite,
    rounds = rounds,
    metric_row = render_metric("学习有效性", m.learning_effectiveness, 0.35),
    metric_row_te = render_metric("教学效率", m.teaching_efficiency, 0.25),
    metric_row_sa = render_metric("学生自主性", m.student_autonomy, 0.20),
    metric_row_re = render_metric("资源参与度", m.resource_engagement, 0.10),
    metric_row_ts = render_metric("教师满意度", m.teacher_satisfaction, 0.10),
    )
}

/// Render the personalized home page for a logged-in user
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
     .replace('<', "&lt;")
     .replace('>', "&gt;")
     .replace('"', "&quot;")
     .replace('\'', "&#39;")
}

pub fn render_user_home(
    profile: &crate::memory::UserProfile,
    session_count: usize,
) -> String {
    let name = profile.display_name.as_deref().unwrap_or(&profile.user_id);
    let grade = profile.grade_level.as_deref().unwrap_or("未设置");
    let style = profile.preferred_style.as_deref().unwrap_or("未设置");
    let total_turns = profile.total_turns;
    let concept_count = profile.mastery.len();
    let mis_count = profile.active_misconceptions().len();

    // Mastery rows
    let mut mastery_rows = String::new();
    for (concept, m) in &profile.mastery {
        let pct = (m.level * 100.0) as u32;
        let color = if m.level >= 0.85 { "#3fb950" } else if m.level >= 0.6 { "#d29922" } else if m.level >= 0.3 { "#f85149" } else { "#484f58" };
        mastery_rows.push_str(&format!(
            "<tr><td>{concept}</td><td><div style='background:{color};width:{pct}%;height:18px;border-radius:3px;min-width:4px;'></div></td><td style='font-size:12px;color:#8b949e'>{pct}% · {}次</td></tr>",
            m.exposures
        ));
    }
    if mastery_rows.is_empty() {
        mastery_rows = "<tr><td colspan='3' style='color:#484f58;text-align:center;padding:16px'>暂无数据 — 去 <a href='/chat' style='color:#58a6ff'>聊天</a> 开始学习</td></tr>".into();
    }

    // LLM config
    let llm_provider = profile.llm_config.as_ref().map(|c| c.provider.as_str()).unwrap_or("未配置");
    let llm_model = profile.llm_config.as_ref().map(|c| c.model.as_str()).unwrap_or("");
    let llm_url = profile.llm_config.as_ref().map(|c| c.base_url.as_str()).unwrap_or("");
    let llm_key_masked = profile.llm_config.as_ref().map(|c| {
        if c.api_key.len() > 8 { format!("{}...{}", &c.api_key[..4], &c.api_key[c.api_key.len()-4..]) }
        else if c.api_key.is_empty() { "未设置".into() }
        else { "****".into() }
    }).unwrap_or_else(|| "未设置".into());

    format!(r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>TAIS — {name}</title>
<style>
* {{ margin: 0; padding: 0; box-sizing: border-box; }}
body {{
    background: #0d1117; color: #c9d1d9; font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', 'PingFang SC', 'Microsoft YaHei', sans-serif;
    min-height: 100vh;
}}
.header {{
    background: #161b22; border-bottom: 1px solid #30363d; padding: 14px 24px;
    display: flex; justify-content: space-between; align-items: center;
}}
.header h1 {{ font-size: 18px; color: #58a6ff; }}
.header .user-info {{ display: flex; align-items: center; gap: 12px; }}
.header .user-badge {{
    background: #1f6feb33; color: #58a6ff; padding: 6px 14px; border-radius: 16px;
    font-size: 13px; border: 1px solid #1f6feb55;
}}
.header a {{ color: #8b949e; text-decoration: none; font-size: 13px; margin-left: 12px; }}
.header a:hover {{ color: #c9d1d9; }}
.grid {{
    display: grid; grid-template-columns: repeat(auto-fit, minmax(340px, 1fr));
    gap: 16px; padding: 24px; max-width: 1200px; margin: 0 auto;
}}
.card {{
    background: #161b22; border: 1px solid #30363d; border-radius: 10px; overflow: hidden;
}}
.card-header {{
    background: #21262d; padding: 12px 16px; border-bottom: 1px solid #30363d;
    font-weight: 600; font-size: 14px; display: flex; align-items: center; gap: 8px;
}}
.card-body {{ padding: 16px; }}
.stat-grid {{
    display: grid; grid-template-columns: 1fr 1fr 1fr; gap: 10px;
}}
.stat {{
    padding: 14px; background: #0d1117; border-radius: 8px; border: 1px solid #21262d;
    text-align: center;
}}
.stat-label {{ font-size: 11px; color: #8b949e; margin-bottom: 6px; }}
.stat-value {{ font-size: 26px; font-weight: 700; color: #58a6ff; }}
.stat-value.green {{ color: #3fb950; }}
.stat-value.orange {{ color: #d29922; }}
table {{ width: 100%; border-collapse: collapse; font-size: 13px; }}
th {{ text-align: left; padding: 8px 12px; background: #21262d; color: #8b949e; font-weight: 500; font-size: 11px; }}
td {{ padding: 8px 12px; border-bottom: 1px solid #21262d; }}
.form-group {{ margin-bottom: 14px; }}
.form-group label {{ display: block; font-size: 12px; color: #8b949e; margin-bottom: 4px; }}
.form-group input, .form-group select {{
    width: 100%; padding: 8px 12px; background: #0d1117; border: 1px solid #30363d;
    border-radius: 6px; color: #c9d1d9; font-size: 13px; font-family: inherit; outline: none;
}}
.form-group input:focus, .form-group select:focus {{ border-color: #58a6ff; }}
.form-row {{ display: grid; grid-template-columns: 1fr 1fr; gap: 10px; }}
.btn {{
    display: inline-block; padding: 8px 16px; border-radius: 6px; border: 1px solid #30363d;
    background: #21262d; color: #c9d1d9; cursor: pointer; font-size: 13px; text-decoration: none;
    transition: all 0.15s; font-family: inherit;
}}
.btn:hover {{ background: #30363d; border-color: #58a6ff; }}
.btn-primary {{ background: #238636; border-color: #2ea043; color: #fff; }}
.btn-primary:hover {{ background: #2ea043; }}
.btn-danger {{ background: transparent; border-color: #f8514930; color: #f85149; }}
.btn-danger:hover {{ background: #f8514910; }}
.llm-status {{ font-size: 12px; padding: 8px; border-radius: 4px; margin-top: 8px; }}
.llm-status.ok {{ background: #3fb95010; color: #3fb950; }}
.llm-status.none {{ background: #d2992210; color: #d29922; }}
.save-msg {{ font-size: 12px; margin-left: 10px; display: none; }}
.save-msg.ok {{ color: #3fb950; }}
.save-msg.err {{ color: #f85149; }}
.empty {{ text-align: center; color: #484f58; padding: 24px; font-size: 13px; }}
@media (max-width: 768px) {{
    .grid {{ grid-template-columns: 1fr; padding: 12px; }}
    .stat-grid {{ grid-template-columns: 1fr 1fr; }}
    .form-row {{ grid-template-columns: 1fr; }}
}}
</style>
</head>
<body>
<div class="header">
    <div>
        <h1>&#129504; TAIS — {name}</h1>
    </div>
    <div class="user-info">
        <span class="user-badge">&#128100; {name} · {grade} · {style}</span>
        <a href="/chat">&#128172; 聊天</a>
        <a href="javascript:void(0)" onclick="logout()" style="color:#f85149">退出</a>
    </div>
</div>

<div class="grid">
    <!-- User Stats -->
    <div class="card">
        <div class="card-header">&#128200; 学习概览</div>
        <div class="card-body">
            <div class="stat-grid">
                <div class="stat">
                    <div class="stat-label">会话数</div>
                    <div class="stat-value">{session_count}</div>
                </div>
                <div class="stat">
                    <div class="stat-label">对话轮次</div>
                    <div class="stat-value green">{total_turns}</div>
                </div>
                <div class="stat">
                    <div class="stat-label">学习概念</div>
                    <div class="stat-value orange">{concept_count}</div>
                </div>
            </div>
        </div>
    </div>

    <!-- Concept Mastery -->
    <div class="card">
        <div class="card-header">&#127891; 概念掌握度</div>
        <div class="card-body">
            <table>
            <tr><th>概念</th><th>掌握度</th><th>进度</th></tr>
            {mastery_rows}
            </table>
            {mis_section}
        </div>
    </div>

    <!-- Per-User LLM Config -->
    <div class="card" style="grid-column: 1 / -1;">
        <div class="card-header">&#9881; 我的 LLM 配置 <span style="font-weight:400;font-size:11px;color:#8b949e;margin-left:8px">(仅对你生效)</span></div>
        <div class="card-body">
            <form id="llmForm" onsubmit="return false">
                <div class="form-row">
                    <div class="form-group">
                        <label>LLM 提供商</label>
                        <select id="llmProvider" onchange="onProviderChange()">
                            <option value="openai" {sel_openai}>OpenAI 兼容</option>
                            <option value="anthropic" {sel_anthropic}>Anthropic</option>
                            <option value="ollama" {sel_ollama}>Ollama</option>
                        </select>
                    </div>
                    <div class="form-group">
                        <label>API Base URL</label>
                        <input type="text" id="llmUrl" value="{llm_url_val}" placeholder="https://api.openai.com/v1">
                    </div>
                </div>
                <div class="form-row">
                    <div class="form-group">
                        <label>API Key <span style="font-size:10px;color:#484f58">({llm_key_display})</span></label>
                        <input type="text" id="llmKey" value="{llm_key_val}" placeholder="sk-...">
                    </div>
                    <div class="form-group">
                        <label>模型 <span style="font-size:10px;color:#8b949e" id="llmModelHint">(先获取模型列表)</span></label>
                        <div style="display:flex;gap:6px;">
                            <select id="llmModel" style="flex:1">
                                <option value="">{llm_model_val}</option>
                            </select>
                            <button type="button" class="btn" id="llmFetchBtn" onclick="fetchModels()">获取模型</button>
                        </div>
                    </div>
                </div>
                <div style="display:flex;align-items:center;gap:8px;">
                    <button type="button" class="btn btn-primary" id="llmSaveBtn" onclick="saveLlmConfig()">保存配置</button>
                    <span class="save-msg" id="llmSaveMsg"></span>
                </div>
            </form>
            <div class="llm-status {llm_status_class}" id="llmStatus">
                {llm_status_text}
            </div>
        </div>
    </div>
</div>

<script>
const userId = '{user_id_js}';

function logout() {{
    localStorage.removeItem('tais_token');
    localStorage.removeItem('tais_user');
    localStorage.removeItem('tais_display');
    window.location.href = '/login';
}}

function onProviderChange() {{
    const p = document.getElementById('llmProvider').value;
    const urlEl = document.getElementById('llmUrl');
    if (!urlEl.value) {{
        if (p === 'openai') urlEl.value = 'https://api.openai.com/v1';
        else if (p === 'ollama') urlEl.value = 'http://localhost:11434/v1';
    }}
    document.getElementById('llmModel').innerHTML = '<option value="">点击"获取模型"</option>';
}}

async function fetchModels() {{
    const btn = document.getElementById('llmFetchBtn');
    const sel = document.getElementById('llmModel');
    const hint = document.getElementById('llmModelHint');
    const statusEl = document.getElementById('llmStatus');
    const url = document.getElementById('llmUrl').value.trim().replace(/\/+$/, '');
    const key = document.getElementById('llmKey').value.trim();

    if (!url) {{ hint.textContent = '请先填写 API Base URL'; return; }}
    btn.disabled = true; btn.textContent = '获取中...';
    hint.textContent = '正在通过服务器获取模型列表...';
    hint.style.color = '#d29922';

    try {{
        // Use server proxy to bypass CORS
        const resp = await fetch('/api/llm/fetch-models', {{
            method: 'POST', headers: {{'Content-Type':'application/json'}},
            body: JSON.stringify({{ base_url: url, api_key: key }})
        }});
        const data = await resp.json();
        if (data.ok && data.models && data.models.length > 0) {{
            sel.innerHTML = data.models.map(m => '<option value="' + m + '">' + m + '</option>').join('');
            sel.disabled = false;
            const src = data.source === 'api' ? ' (API)' : ' (备选)';
            hint.textContent = '✅ 已连接，' + data.count + ' 个模型可选' + src;
            hint.style.color = '#3fb950';
            statusEl.textContent = (data.message || '🟢 连接成功') ;
            statusEl.className = 'llm-status ok';
            const current = '{llm_model_val}';
            if (current) {{ for (let o of sel.options) if (o.value === current) o.selected = true; }}
        }} else {{
            throw new Error(data.error || 'No models returned');
        }}
    }} catch(e) {{
        hint.textContent = '❌ ' + e.message;
        hint.style.color = '#f85149';
        statusEl.textContent = '❌ ' + e.message;
        statusEl.className = 'llm-status none';
        const p = document.getElementById('llmProvider').value;
        const fb = ['deepseek-chat','deepseek-reasoner','gpt-4o','gpt-3.5-turbo',
            'claude-sonnet-4-6','claude-haiku-4-5','llama3','qwen3'];
        sel.innerHTML = '<option value="">-- 手动选择 --</option>' + fb.map(m => '<option value="' + m + '">' + m + '</option>').join('');
        sel.disabled = false;
    }}
    btn.disabled = false; btn.textContent = '获取模型';
}}

async function saveLlmConfig() {{
    const btn = document.getElementById('llmSaveBtn');
    const msg = document.getElementById('llmSaveMsg');
    const model = document.getElementById('llmModel').value.trim();
    if (!model) {{ msg.textContent = '请先获取并选择模型'; msg.className = 'save-msg err'; msg.style.display = 'inline'; return; }}
    btn.disabled = true; btn.textContent = '保存中...';
    msg.style.display = 'none';
    const apiKey = document.getElementById('llmKey').value.trim();
    const body = {{
        provider: document.getElementById('llmProvider').value,
        model: model,
        base_url: document.getElementById('llmUrl').value.trim(),
    }};
    if (apiKey) body.api_key = apiKey;
    try {{
        const resp = await fetch('/api/memory/users/' + userId + '/llm', {{
            method: 'PUT', headers: {{'Content-Type':'application/json'}}, body: JSON.stringify(body),
        }});
        const data = await resp.json();
        if (data.status === 'ok') {{
            msg.textContent = '✓ 已保存'; msg.className = 'save-msg ok';
            document.getElementById('llmStatus').textContent = '🟢 个人 LLM: ' + body.provider + ' / ' + body.model;
            document.getElementById('llmStatus').className = 'llm-status ok';
        }} else {{
            msg.textContent = '✗ ' + (data.error || '保存失败');
            msg.className = 'save-msg err';
        }}
        msg.style.display = 'inline';
    }} catch(ex) {{
        msg.textContent = '✗ 网络错误';
        msg.className = 'save-msg err';
        msg.style.display = 'inline';
    }}
    btn.disabled = false; btn.textContent = '保存配置';
    setTimeout(() => {{ msg.style.display = 'none'; }}, 3000);
}}

// Check for token from localStorage and restore if needed
(function() {{
    const token = localStorage.getItem('tais_token');
    if (token && !window.location.search.includes('token=')) {{
        // Keep the token in URL for page refreshes
        const url = new URL(window.location);
        if (!url.searchParams.get('token')) {{
            url.searchParams.set('token', token);
            window.history.replaceState({{}}, '', url);
        }}
    }}
}})();
</script>
</body>
</html>"#,
        name = name,
        grade = grade,
        style = style,
        session_count = session_count,
        total_turns = total_turns,
        concept_count = concept_count,
        mastery_rows = mastery_rows,
        mis_section = if mis_count > 0 {
            let mut rows = String::from("<tr><th colspan='3' style='margin-top:12px'>⚠️ 持续误解</th></tr>");
            for m in profile.active_misconceptions() {
                rows.push_str(&format!("<tr><td>{}</td><td colspan='2' style='color:#f85149'>{}</td></tr>", m.related_concept, m.description));
            }
            rows
        } else { String::new() },
        sel_openai = if llm_provider == "openai" { "selected" } else { "" },
        sel_anthropic = if llm_provider == "anthropic" { "selected" } else { "" },
        sel_ollama = if llm_provider == "ollama" { "selected" } else { "" },
        llm_model_val = html_escape(llm_model),
        llm_url_val = html_escape(llm_url),
        llm_key_val = html_escape(profile.llm_config.as_ref().map(|c| c.api_key.as_str()).unwrap_or("")),
        llm_key_display = llm_key_masked,
        llm_status_class = if profile.llm_config.is_some() { "ok" } else { "none" },
        llm_status_text = if profile.llm_config.is_some() {
            format!("🟢 已配置个人 LLM: {} / {}", llm_provider, if llm_model.is_empty() { "未指定" } else { llm_model })
        } else {
            "⚠️ 未配置个人 LLM — 点击获取模型按钮测试连接后保存".into()
        },
        user_id_js = profile.user_id,
    )
}

fn render_metric(name: &str, value: f64, weight: f64) -> String {
    let pct = (value * 100.0) as u32;
    let color = if value >= 0.7 { "#3fb950" } else if value >= 0.4 { "#d29922" } else { "#f85149" };
    format!(r#"<div class="metric-row">
      <div class="metric-label">
        <span class="metric-name">{name} (权重 {:.0}%)</span>
        <span class="metric-value">{:.2}</span>
      </div>
      <div class="progress-bar">
        <div class="progress-fill" style="width:{pct}%;background:{color}"></div>
      </div>
    </div>"#, weight * 100.0, value, pct = pct, color = color, name = name)
}
