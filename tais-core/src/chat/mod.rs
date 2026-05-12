// Chat UI — WebSocket chat + Login/Register page
//
// GET /login  — Login/Register page
// GET /chat   — WebSocket chat with collapsible session history sidebar

pub fn render_login() -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>TAIS — 登录</title>
<style>
* {{ margin: 0; padding: 0; box-sizing: border-box; }}
body {{
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', 'PingFang SC', 'Microsoft YaHei', sans-serif;
    background: #0d1117; color: #c9d1d9; min-height: 100vh; display: flex;
    align-items: center; justify-content: center;
}}
.container {{
    background: #161b22; border: 1px solid #30363d; border-radius: 12px;
    padding: 32px; width: 100%; max-width: 400px;
}}
.container h1 {{ font-size: 20px; color: #58a6ff; text-align: center; margin-bottom: 4px; }}
.container .subtitle {{ color: #8b949e; font-size: 13px; text-align: center; margin-bottom: 24px; }}
.tabs {{ display: flex; margin-bottom: 20px; border-bottom: 1px solid #30363d; }}
.tab {{
    flex: 1; text-align: center; padding: 8px; cursor: pointer; font-size: 14px;
    color: #8b949e; border-bottom: 2px solid transparent; transition: all 0.2s;
}}
.tab.active {{ color: #58a6ff; border-bottom-color: #58a6ff; }}
.tab:hover {{ color: #c9d1d9; }}
.form {{ display: none; }}
.form.active {{ display: block; }}
.form-group {{ margin-bottom: 16px; }}
.form-group label {{ display: block; font-size: 13px; color: #8b949e; margin-bottom: 6px; }}
.form-group input {{
    width: 100%; padding: 10px 12px; background: #0d1117; border: 1px solid #30363d;
    border-radius: 6px; color: #c9d1d9; font-size: 14px; font-family: inherit; outline: none;
}}
.form-group input:focus {{ border-color: #58a6ff; }}
.btn {{
    width: 100%; padding: 10px; border: none; border-radius: 6px; font-size: 14px;
    font-weight: 600; cursor: pointer; transition: background 0.2s; margin-bottom: 8px;
}}
.btn-primary {{ background: #238636; color: #fff; }}
.btn-primary:hover {{ background: #2ea043; }}
.btn-primary:disabled {{ background: #21262d; color: #484f58; cursor: not-allowed; }}
.error {{ color: #f85149; font-size: 13px; margin-top: 8px; display: none; text-align: center; }}
.back-link {{ display: block; text-align: center; margin-top: 16px; color: #8b949e; font-size: 13px; text-decoration: none; }}
.back-link:hover {{ color: #58a6ff; }}
</style>
</head>
<body>

<div class="container">
    <h1>&#129504; TAIS</h1>
    <p class="subtitle">Teacher-AI-Student 教学系统</p>

    <div class="tabs">
        <div class="tab active" onclick="switchTab('login')">登录</div>
        <div class="tab" onclick="switchTab('register')">注册</div>
    </div>

    <form id="loginForm" class="form active" onsubmit="handleLogin(event)">
        <div class="form-group">
            <label>用户名</label>
            <input type="text" id="loginUsername" placeholder="输入用户名" required>
        </div>
        <div class="form-group">
            <label>密码</label>
            <input type="password" id="loginPassword" placeholder="输入密码" required>
        </div>
        <button type="submit" class="btn btn-primary" id="loginBtn">登录</button>
        <div class="error" id="loginError"></div>
    </form>

    <form id="registerForm" class="form" onsubmit="handleRegister(event)">
        <div class="form-group">
            <label>用户名 *</label>
            <input type="text" id="regUsername" placeholder="字母数字下划线" required>
        </div>
        <div class="form-group">
            <label>显示名称</label>
            <input type="text" id="regDisplay" placeholder="可选，如：小明">
        </div>
        <div class="form-group">
            <label>密码 * (至少6位)</label>
            <input type="password" id="regPassword" placeholder="至少6位" required minlength="6">
        </div>
        <button type="submit" class="btn btn-primary" id="regBtn">注册</button>
        <div class="error" id="regError"></div>
    </form>

    <a href="javascript:history.back()" class="back-link" style="display:none">&#127968; 返回</a>
</div>

<script>
// Auto-login: if already logged in (localStorage), redirect to chat
(function() {{
    const token = localStorage.getItem('tais_token');
    if (token) {{
        window.location.href = '/chat?token=' + encodeURIComponent(token);
    }}
}})();
function switchTab(name) {{
    document.querySelectorAll('.tab').forEach(t => t.classList.remove('active'));
    document.querySelectorAll('.form').forEach(f => f.classList.remove('active'));
    if (name === 'login') {{
        document.querySelectorAll('.tab')[0].classList.add('active');
        document.getElementById('loginForm').classList.add('active');
    }} else {{
        document.querySelectorAll('.tab')[1].classList.add('active');
        document.getElementById('registerForm').classList.add('active');
    }}
    document.getElementById('loginError').style.display = 'none';
    document.getElementById('regError').style.display = 'none';
}}

async function handleLogin(e) {{
    e.preventDefault();
    const btn = document.getElementById('loginBtn');
    const errEl = document.getElementById('loginError');
    btn.disabled = true; btn.textContent = '登录中...';
    errEl.style.display = 'none';
    try {{
        const resp = await fetch('/api/auth/login', {{
            method: 'POST', headers: {{ 'Content-Type': 'application/json' }},
            body: JSON.stringify({{
                username: document.getElementById('loginUsername').value,
                password: document.getElementById('loginPassword').value,
            }}),
        }});
        const data = await resp.json();
        if (data.token) {{
            localStorage.setItem('tais_token', data.token);
            localStorage.setItem('tais_user', data.user_id);
            localStorage.setItem('tais_display', data.display_name || data.user_id);
            // Set cookie for auto-login (30 days)
            document.cookie = 'tais_token=' + data.token + ';path=/;max-age=' + (86400*30) + ';SameSite=Lax';
            window.location.href = '/chat?token=' + encodeURIComponent(data.token);
        }} else {{
            errEl.textContent = data.error || '登录失败'; errEl.style.display = 'block';
        }}
    }} catch (ex) {{
        errEl.textContent = '网络错误: ' + ex.message; errEl.style.display = 'block';
    }}
    btn.disabled = false; btn.textContent = '登录';
}}

async function handleRegister(e) {{
    e.preventDefault();
    const btn = document.getElementById('regBtn');
    const errEl = document.getElementById('regError');
    btn.disabled = true; btn.textContent = '注册中...';
    errEl.style.display = 'none';
    const username = document.getElementById('regUsername').value.trim();
    const display = document.getElementById('regDisplay').value.trim();
    const password = document.getElementById('regPassword').value;
    if (password.length < 6) {{
        errEl.textContent = '密码至少6位'; errEl.style.display = 'block';
        btn.disabled = false; btn.textContent = '注册'; return;
    }}
    try {{
        const body = {{ username, password }};
        if (display) body.display_name = display;
        const resp = await fetch('/api/auth/register', {{
            method: 'POST', headers: {{ 'Content-Type': 'application/json' }},
            body: JSON.stringify(body),
        }});
        const data = await resp.json();
        if (data.token) {{
            localStorage.setItem('tais_token', data.token);
            localStorage.setItem('tais_user', data.user_id);
            localStorage.setItem('tais_display', data.display_name || data.user_id);
            document.cookie = 'tais_token=' + data.token + ';path=/;max-age=' + (86400*30) + ';SameSite=Lax';
            window.location.href = '/chat?token=' + encodeURIComponent(data.token);
        }} else {{
            errEl.textContent = data.error || '注册失败'; errEl.style.display = 'block';
        }}
    }} catch (ex) {{
        errEl.textContent = '网络错误: ' + ex.message; errEl.style.display = 'block';
    }}
    btn.disabled = false; btn.textContent = '注册';
}}
</script>

</body>
</html>"#
    )
}

pub fn render(session_id: &str, user_id: Option<&str>, display_name: Option<&str>) -> String {
    let name = display_name.unwrap_or(user_id.unwrap_or("访客"));
    let greeting = format!("{name} 的 TAIS 会话");
    let user_id_attr = user_id.unwrap_or("");
    let display_attr = display_name.unwrap_or("");
    let dashboard_link = if user_id_attr.is_empty() {
        "/".to_string()
    } else {
        format!("/dashboard/{}", user_id_attr)
    };

    format!(
        r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>{greeting}</title>
<!-- Markdown + LaTeX + Mermaid -->
<link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/katex@0.16/dist/katex.min.css">
<script src="https://cdn.jsdelivr.net/npm/marked/marked.min.js"></script>
<script src="https://cdn.jsdelivr.net/npm/katex@0.16/dist/katex.min.js"></script>
<script src="https://cdn.jsdelivr.net/npm/katex@0.16/dist/contrib/auto-render.min.js"></script>
<script src="https://cdn.jsdelivr.net/npm/mermaid@10/dist/mermaid.min.js"></script>
<script>mermaid.initialize({{ startOnLoad: false, theme: 'dark' }});</script>
<style>
* {{ margin: 0; padding: 0; box-sizing: border-box; }}
body {{
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', 'PingFang SC', 'Microsoft YaHei', sans-serif;
    background: #0d1117; color: #c9d1d9; height: 100vh; display: flex;
}}
/* ── Sidebar ──────────────────────────────────────────────── */
.sidebar {{
    background: #161b22; border-right: 1px solid #30363d;
    width: 280px; flex-shrink: 0; display: flex; flex-direction: column;
    transition: width 0.25s ease, opacity 0.25s ease;
    overflow: hidden;
}}
.sidebar.collapsed {{ width: 0; opacity: 0; border-right: none; }}
.sidebar-header {{
    padding: 14px 16px; border-bottom: 1px solid #30363d;
    display: flex; justify-content: space-between; align-items: center;
    flex-shrink: 0;
}}
.sidebar-header h2 {{ font-size: 14px; color: #58a6ff; white-space: nowrap; }}
.sidebar-header .sidebar-toggle {{
    background: none; border: 1px solid #30363d; color: #8b949e;
    cursor: pointer; font-size: 16px; padding: 2px 8px; border-radius: 4px;
    line-height: 1.4; transition: all 0.2s;
}}
.sidebar-header .sidebar-toggle:hover {{ color: #c9d1d9; border-color: #58a6ff; }}
.sidebar-session-list {{
    flex: 1; overflow-y: auto; padding: 8px;
}}
.sidebar-session-list .empty {{
    color: #484f58; font-size: 13px; text-align: center; padding: 20px 8px;
}}
.sidebar-session-list .new-chat-btn {{
    display: block; width: 100%; padding: 8px; margin-bottom: 8px;
    background: #238636; color: #fff; border: none; border-radius: 6px;
    font-size: 13px; cursor: pointer; font-weight: 600; transition: background 0.2s;
}}
.sidebar-session-list .new-chat-btn:hover {{ background: #2ea043; }}
.session-item {{
    padding: 10px 12px; border-radius: 6px; cursor: pointer; margin-bottom: 4px;
    transition: background 0.15s; border: 1px solid transparent;
    white-space: nowrap; overflow: hidden;
}}
.session-item:hover {{ background: #21262d; }}
.session-item.active {{ background: #1f6feb22; border-color: #1f6feb55; }}
.session-item .session-title {{ font-size: 13px; color: #c9d1d9; overflow: hidden; text-overflow: ellipsis; }}
.session-item .session-meta {{ font-size: 11px; color: #484f58; margin-top: 2px; display: flex; gap: 8px; }}
.session-item.active .session-title {{ color: #58a6ff; }}
/* ── Main Chat Area ───────────────────────────────────────── */
.chat-area {{
    flex: 1; display: flex; flex-direction: column; min-width: 0;
}}
.chat-header {{
    background: #161b22; border-bottom: 1px solid #30363d;
    padding: 12px 20px; display: flex; justify-content: space-between;
    align-items: center; flex-shrink: 0;
}}
.chat-header h1 {{ font-size: 16px; color: #58a6ff; }}
.chat-header .info {{ color: #8b949e; font-size: 12px; }}
.chat-header nav {{ display: flex; gap: 12px; align-items: center; }}
.chat-header a {{ color: #8b949e; text-decoration: none; font-size: 13px; }}
.chat-header a:hover {{ color: #58a6ff; }}
.chat-header .user-badge {{
    background: #1f6feb33; color: #58a6ff; font-size: 12px; padding: 4px 10px;
    border-radius: 12px; border: 1px solid #1f6feb55;
}}
.toggle-sidebar-btn {{
    background: none; border: 1px solid #30363d; color: #8b949e;
    font-size: 15px; cursor: pointer; padding: 2px 7px; border-radius: 4px;
    margin-right: 8px; transition: all 0.2s; line-height: 1.4;
}}
.toggle-sidebar-btn:hover {{ color: #c9d1d9; border-color: #58a6ff; }}
/* Messages area */
.messages {{
    flex: 1; overflow-y: auto; padding: 20px; display: flex;
    flex-direction: column; gap: 12px;
}}
.msg {{ max-width: 72%; padding: 10px 14px; border-radius: 12px; line-height: 1.55; font-size: 14px; word-wrap: break-word; }}
.msg.student {{ align-self: flex-end; background: #1f6feb33; border: 1px solid #1f6feb55; border-bottom-right-radius: 4px; }}
.msg.tutor {{ align-self: flex-start; background: #21262d; border: 1px solid #30363d; border-bottom-left-radius: 4px; }}
.msg .role {{ font-size: 11px; font-weight: 600; margin-bottom: 4px; opacity: 0.8; }}
.msg.student .role {{ color: #58a6ff; }}
.msg.tutor .role {{ color: #7ee787; }}
.msg .time {{ font-size: 10px; color: #484f58; margin-top: 4px; text-align: right; }}
.msg.system {{ align-self: center; background: transparent; color: #8b949e; font-size: 12px; max-width: 90%; text-align: center; border: none; }}
.typing {{ align-self: flex-start; padding: 8px 14px; color: #8b949e; font-size: 13px; display: none; }}
.typing.active {{ display: block; }}
/* Input area */
.input-area {{
    background: #161b22; border-top: 1px solid #30363d; padding: 12px 20px;
    display: flex; gap: 10px; flex-shrink: 0;
}}
.input-area textarea {{
    flex: 1; background: #0d1117; border: 1px solid #30363d; border-radius: 8px;
    color: #c9d1d9; padding: 10px 14px; font-size: 14px; font-family: inherit;
    resize: none; min-height: 42px; max-height: 120px; outline: none;
}}
.input-area textarea:focus {{ border-color: #58a6ff; }}
.input-area button {{
    background: #238636; color: #fff; border: none; border-radius: 8px;
    padding: 10px 20px; font-size: 14px; cursor: pointer; font-weight: 600;
}}
.input-area button:hover {{ background: #2ea043; }}
.input-area button:disabled {{ background: #21262d; color: #484f58; cursor: not-allowed; }}
/* ── Markdown content ──────────────────────────────────────── */
.msg-content p {{ margin: 4px 0; }}
.msg-content pre {{ background: #0d1117; border: 1px solid #30363d; border-radius: 6px; padding: 12px; overflow-x: auto; margin: 8px 0; font-size: 12px; }}
.msg-content code {{ font-family: 'JetBrains Mono', 'Fira Code', 'Consolas', monospace; font-size: 12px; }}
.msg-content :not(pre) > code {{ background: #21262d; padding: 1px 5px; border-radius: 3px; }}
.msg-content table {{ border-collapse: collapse; margin: 8px 0; width: 100%; }}
.msg-content th, .msg-content td {{ border: 1px solid #30363d; padding: 6px 10px; text-align: left; }}
.msg-content th {{ background: #21262d; }}
.msg-content blockquote {{ border-left: 3px solid #58a6ff; padding-left: 12px; color: #8b949e; margin: 8px 0; }}
.msg-content ul, .msg-content ol {{ padding-left: 20px; margin: 4px 0; }}
.msg-content .katex-display {{ margin: 8px 0; }}
/* ── Copy button ────────────────────────────────────────────── */
.msg-bubble {{ position: relative; }}
.copy-btn {{
    position: absolute; top: 4px; right: 4px;
    background: #21262d; border: 1px solid #30363d; color: #8b949e;
    border-radius: 4px; padding: 2px 6px; font-size: 11px; cursor: pointer;
    opacity: 0; transition: opacity 0.15s;
}}
.msg-bubble:hover .copy-btn {{ opacity: 1; }}
.copy-btn:hover {{ background: #30363d; color: #c9d1d9; }}
/* Status bar */
.status-bar {{
    background: #161b22; border-top: 1px solid #30363d; padding: 4px 20px;
    font-size: 11px; color: #484f58; display: flex; justify-content: space-between;
    flex-shrink: 0;
}}
.status-bar .connected {{ color: #3fb950; }}
.status-bar .disconnected {{ color: #f85149; }}
@media (max-width: 700px) {{
    .sidebar {{ width: 0; opacity: 0; border-right: none; }}
    .sidebar.open-mobile {{ width: 260px; opacity: 1; border-right: 1px solid #30363d; position: absolute; z-index: 10; height: 100vh; }}
    .msg {{ max-width: 88%; }}
    .chat-header h1 {{ font-size: 14px; }}
}}
</style>
</head>
<body>

<!-- ── Sidebar ──────────────────────────────────────────────── -->
<div class="sidebar" id="sidebar">
    <div class="sidebar-header">
        <h2>&#128218; 会话历史</h2>
        <button class="sidebar-toggle" onclick="toggleSidebar()" title="折叠侧边栏">&laquo;</button>
    </div>
    <div class="sidebar-session-list" id="sessionList">
        <button class="new-chat-btn" onclick="newChat()">+ 新建对话</button>
        <div class="empty" id="sessionLoading">加载中...</div>
    </div>
</div>

<!-- ── Main Chat ────────────────────────────────────────────── -->
<div class="chat-area">
<div class="chat-header">
    <div style="display:flex;align-items:center;">
        <button class="toggle-sidebar-btn" onclick="toggleSidebar()" title="展开/折叠侧边栏" id="toggleBtn">&#9776;</button>
        <div>
            <h1>&#129504; {greeting}</h1>
            <span class="info">
                Session: <span id="sessionIdShort">{session_id_short}</span>
                &nbsp;|&nbsp; LLM: <span id="llmStatus" style="color:#8b949e">-</span>
                &nbsp;|&nbsp; 模式: <span id="modeBadge" style="color:#58a6ff">通用</span>
            </span>
        </div>
    </div>
    <nav>
        <a href="/" title="Dashboard 首页">&#127968; 首页</a>
        <a href="{dashboard_link}" title="个人仪表盘">&#128202; 仪表盘</a>
        <span class="user-badge" id="userBadge" style="display: none">&#128100; <span id="userName"></span></span>
        <a href="/login" id="loginLink">&#128273; 登录</a>
        <a href="javascript:void(0)" id="logoutLink" style="display: none" onclick="logout()">退出</a>
    </nav>
</div>

<div class="messages" id="messages">
    <div class="msg system">&#128075; 你好！我是 TAIS 苏格拉底式导师。<br>我不会直接给你答案，而是通过追问引导你自己发现。<br>请提出你的问题吧。</div>
</div>

<div class="typing" id="typing">&#129504; TAIS 正在思考...</div>

<div class="input-area">
    <textarea id="input" placeholder="输入你的问题..." rows="1"
        onkeydown="if(event.key==='Enter'&&!event.shiftKey){{event.preventDefault();send();}}"></textarea>
    <button id="sendBtn" onclick="send()">发送</button>
</div>

<div class="status-bar">
    <span id="connStatus" class="disconnected">&#9679; 连接中...</span>
    <span id="msgCount">0 条消息</span>
</div>
</div>

<script>
const serverSessionId = '{session_id}';
const serverUserId = '{user_id_attr}';
const serverDisplay = '{display_attr}';
let currentSessionId = serverSessionId;
let ws = null;
let msgCount = 0;
let reconnectTimer = null;
let currentUser = null;
let authToken = null;
let sidebarVisible = true;

// ── Auth init ──────────────────────────────────────────────────
function initAuth() {{
    const token = localStorage.getItem('tais_token');
    const userId = localStorage.getItem('tais_user');
    const display = localStorage.getItem('tais_display');

    if (token && userId) {{
        authToken = token;
        currentUser = {{ userId, displayName: display || userId }};
        // Ensure cookie exists (for server-side pages like dashboard)
        document.cookie = 'tais_token=' + token + ';path=/;max-age=' + (86400*30) + ';SameSite=Lax';
        document.getElementById('userName').textContent = display || userId;
        document.getElementById('userBadge').style.display = 'inline';
        document.getElementById('loginLink').style.display = 'none';
        document.getElementById('logoutLink').style.display = 'inline';
        document.title = (display || userId) + ' 的 TAIS 会话';
        const h1 = document.querySelector('.chat-header h1');
        if (h1) h1.textContent = '🧬 ' + (display || userId) + ' 的 TAIS 会话';
        // Check if user has personal LLM configured
        checkPersonalLlm(userId);
    }} else if (serverUserId) {{
        currentUser = {{ userId: serverUserId, displayName: serverDisplay || serverUserId }};
        document.getElementById('userName').textContent = serverDisplay || serverUserId;
        document.getElementById('userBadge').style.display = 'inline';
        document.getElementById('loginLink').style.display = 'none';
        document.getElementById('logoutLink').style.display = 'inline';
        const h1 = document.querySelector('.chat-header h1');
        if (h1) h1.textContent = '🧬 ' + (serverDisplay || serverUserId) + ' 的 TAIS 会话';
        checkPersonalLlm(serverUserId);
    }}
}}

// ── Status bar update ───────────────────────────────────────────
async function updateStatus() {{
    try {{
        const el = document.getElementById('llmStatus');
        if (!el) return;
        // Check personal LLM first, then system
        let hasLlm = false;
        if (currentUser) {{
            const resp = await fetch('/api/memory/users/' + currentUser.userId + '/llm');
            const data = await resp.json();
            if (data && data.provider) {{
                el.textContent = data.provider + '/' + data.model;
                el.style.color = '#3fb950';
                hasLlm = true;
            }}
        }}
        if (!hasLlm) {{
            const resp = await fetch('/api/llm/status');
            const data = await resp.json();
            if (data.active_configs > 0) {{
                el.textContent = '系统 (' + data.active_configs + ')';
                el.style.color = '#d29922';
            }} else {{
                el.textContent = '未配置';
                el.style.color = '#8b949e';
            }}
        }}
    }} catch(e) {{}}
}}

async function checkPersonalLlm(userId) {{
    // Skip if already redirected this session
    if (sessionStorage.getItem('llm_checked')) return;
    sessionStorage.setItem('llm_checked', '1');
    try {{
        const resp = await fetch('/api/memory/users/' + userId + '/llm');
        const data = await resp.json();
        if (!data || !data.provider) {{
            appendSystem('&#9888; 尚未配置个人 LLM，<a href="/dashboard/' + userId + '" style="color:#58a6ff">点击这里配置</a>（仅提示一次）');
        }}
    }} catch(e) {{}}
}}

function setModeBadge(mode) {{
    const el = document.getElementById('modeBadge');
    if (!el) return;
    const map = {{
        'learning': {{ text: '学习', color: '#d2a8ff' }},
        'command': {{ text: '命令', color: '#ffa657' }},
        'general': {{ text: '通用', color: '#58a6ff' }}
    }};
    const m = map[mode] || map['general'];
    el.textContent = m.text;
    el.style.color = m.color;
}}

function logout() {{
    localStorage.removeItem('tais_token');
    localStorage.removeItem('tais_user');
    localStorage.removeItem('tais_display');
    window.location.href = '/login';
}}

// ── Sidebar ────────────────────────────────────────────────────
function toggleSidebar() {{
    sidebarVisible = !sidebarVisible;
    const sidebar = document.getElementById('sidebar');
    if (sidebarVisible) {{
        sidebar.classList.remove('collapsed');
    }} else {{
        sidebar.classList.add('collapsed');
    }}
}}

async function loadSessions() {{
    if (!currentUser) return;
    const listEl = document.getElementById('sessionList');

    try {{
        const resp = await fetch('/api/memory/users/' + currentUser.userId + '/sessions');
        const data = await resp.json();
        if (!data.sessions || data.sessions.length === 0) {{
            listEl.innerHTML = '<button class="new-chat-btn" onclick="newChat()">+ 新建对话</button><div class="empty">暂无历史会话</div>';
            return;
        }}

        let html = '<button class="new-chat-btn" onclick="newChat()">+ 新建对话</button>';
        for (const s of data.sessions) {{
            const sidShort = s.session_id.substring(0, 8);
            const turns = s.total_turns || 0;
            const concepts = (s.concepts || []).slice(0, 2).join(', ');
            const activeClass = s.session_id === currentSessionId ? ' active' : '';
            html += `<div class="session-item${{activeClass}}" onclick="switchSession('${{s.session_id}}')" title="${{s.session_id}}">
                <div class="session-title">${{sidShort}}... (${{turns}} 轮)</div>
                <div class="session-meta">${{concepts || '新对话'}}</div>
            </div>`;
        }}
        listEl.innerHTML = html;
    }} catch (e) {{
        listEl.innerHTML = '<button class="new-chat-btn" onclick="newChat()">+ 新建对话</button><div class="empty">加载失败</div>';
    }}
}}

function newChat() {{
    // Navigate to a fresh chat
    window.location.href = '/chat' + (authToken ? '?token=' + authToken : '');
}}

function switchSession(sessionId) {{
    if (sessionId === currentSessionId) return;
    currentSessionId = sessionId;

    // Close existing WebSocket
    if (ws) {{ ws.close(); ws = null; }}
    if (reconnectTimer) {{ clearTimeout(reconnectTimer); reconnectTimer = null; }}

    // Update UI
    document.getElementById('sessionIdShort').textContent = sessionId.substring(0, 8);
    document.getElementById('messages').innerHTML = '<div class="msg system">&#128260; 正在加载会话...</div>';
    document.getElementById('msgCount').textContent = '0 条消息';

    // Highlight active session in sidebar
    document.querySelectorAll('.session-item').forEach(el => {{
        el.classList.remove('active');
        if (el.getAttribute('onclick') && el.getAttribute('onclick').includes(sessionId.substring(0,8))) {{
            el.classList.add('active');
        }}
    }});

    // Load history then connect
    loadHistory().then(connect);
    msgCount = 0;
}}

// ── Connect WebSocket ──────────────────────────────────────────
function connect() {{
    const proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
    const url = `${{proto}}//${{location.host}}/api/session/${{currentSessionId}}`;
    ws = new WebSocket(url);

    ws.onopen = () => {{
        document.getElementById('connStatus').innerHTML = '&#9679; 已连接';
        document.getElementById('connStatus').className = 'connected';
        document.getElementById('sendBtn').disabled = false;
        if (reconnectTimer) {{ clearTimeout(reconnectTimer); reconnectTimer = null; }}
        if (currentUser) {{
            ws.send(JSON.stringify({{ type: 'identify', user_id: currentUser.userId, display_name: currentUser.displayName }}));
        }}
    }};

    ws.onmessage = (evt) => {{
        hideTyping();
        try {{
            const json = JSON.parse(evt.data);
            if (json.type === 'identity_ack') {{
                appendSystem('&#128100; 已识别: ' + (json.display_name || json.user_id));
                if (json.display_name) {{
                    document.getElementById('userName').textContent = json.display_name;
                    document.getElementById('userBadge').style.display = 'inline';
                    document.getElementById('loginLink').style.display = 'none';
                    document.getElementById('logoutLink').style.display = 'inline';
                }}
                loadSessions();
                updateStatus();
                return;
            }}
        }} catch(e) {{}}
        appendMessage('tutor', evt.data);
        // Update mode badge if response contains mode switch keywords
        if (evt.data.includes('已切换到学习模式')) setModeBadge('learning');
        if (evt.data.includes('已切换到命令模式')) setModeBadge('command');
        if (evt.data.includes('已切换到通用模式')) setModeBadge('general');
        // Refresh session list every 3 messages
        if (document.querySelectorAll('.msg.tutor').length % 3 === 0) {{
            loadSessions();
        }}
    }};

    ws.onclose = () => {{
        document.getElementById('connStatus').innerHTML = '&#9679; 已断开';
        document.getElementById('connStatus').className = 'disconnected';
        document.getElementById('sendBtn').disabled = true;
        reconnectTimer = setTimeout(connect, 3000);
    }};

    ws.onerror = () => {{
        document.getElementById('connStatus').innerHTML = '&#9679; 连接错误';
        document.getElementById('connStatus').className = 'disconnected';
    }};
}}

// ── Load history ───────────────────────────────────────────────
async function loadHistory() {{
    try {{
        const resp = await fetch('/api/memory/sessions/' + currentSessionId);
        const data = await resp.json();
        if (data.turns && data.turns.length > 0) {{
            document.getElementById('messages').innerHTML = '';
            appendSystem('&#128218; 加载了 ' + data.turns.length + ' 条历史记录');
            for (const turn of data.turns) {{
                const role = turn.role === 'Student' ? 'student' : 'tutor';
                appendMessage(role, turn.content, turn.timestamp);
            }}
        }} else {{
            document.getElementById('messages').innerHTML = '<div class="msg system">&#128075; 新对话！请提出你的问题吧。</div>';
        }}
    }} catch (e) {{
        document.getElementById('messages').innerHTML = '<div class="msg system">&#128075; 你好！请提出你的问题。</div>';
    }}
}}

// ── Send message ────────────────────────────────────────────────
function send() {{
    const input = document.getElementById('input');
    const text = input.value.trim();
    if (!text) return;
    if (!ws || ws.readyState !== WebSocket.OPEN) {{
        appendSystem('&#9888; 连接已断开，正在重连...');
        connect();
        return;
    }}
    appendMessage('student', text, null, currentUser);
    ws.send(text);
    input.value = '';
    input.style.height = 'auto';
    showTyping();
}}

// ── UI helpers ─────────────────────────────────────────────────
function renderMd(text) {{
    try {{ return marked.parse(text, {{ breaks: true, gfm: true }}); }}
    catch(e) {{ return text.replace(/&/g,'&amp;').replace(/</g,'&lt;'); }}
}}

function appendMessage(role, content, ts, user) {{
    hideTyping();
    const container = document.getElementById('messages');
    const div = document.createElement('div');
    div.className = 'msg ' + role;
    let roleLabel = role === 'student' ? '&#127891; 你' : '&#129504; TAIS';
    if (role === 'student' && user) {{
        roleLabel = '&#127891; ' + (user.displayName || user.userId);
    }}
    const now = ts ? new Date(ts) : new Date();
    const time = now.toLocaleTimeString('zh-CN', {{ hour: '2-digit', minute: '2-digit' }});
    const md = renderMd(content);
    div.innerHTML = '<div class="role">' + roleLabel + '</div>'
        + '<div class="msg-bubble">'
        + '<button class="copy-btn" onclick="copyMsg(this)" title="复制">📋</button>'
        + '<div class="msg-content">' + md + '</div>'
        + '</div>'
        + '<div class="time">' + time + '</div>';
    container.appendChild(div);
    container.scrollTop = container.scrollHeight;
    msgCount++;
    document.getElementById('msgCount').textContent = msgCount + ' 条消息';
    // LaTeX
    if (typeof renderMathInElement !== 'undefined') {{
        try {{ renderMathInElement(div, {{ delimiters: [{{left:'$$',right:'$$',display:true}}, {{left:'$',right:'$',display:false}}] }}); }} catch(e) {{}}
    }}
    // Mermaid
    setTimeout(() => {{
        div.querySelectorAll('code.language-mermaid').forEach(el => {{
            try {{ mermaid.render('m-' + Math.random().toString(36).slice(2), el.textContent).then(svg => {{ el.parentElement.outerHTML = svg.svg; }}); }} catch(e) {{}}
        }});
    }}, 100);
}}

function copyMsg(btn) {{
    const text = btn.parentElement.querySelector('.msg-content').textContent;
    navigator.clipboard.writeText(text).then(() => {{
        btn.textContent = '✓';
        setTimeout(() => {{ btn.textContent = '📋'; }}, 1500);
    }});
}}

function appendSystem(text) {{
    const container = document.getElementById('messages');
    const div = document.createElement('div');
    div.className = 'msg system';
    div.innerHTML = text;
    container.appendChild(div);
    container.scrollTop = container.scrollHeight;
}}

function showTyping() {{ document.getElementById('typing').classList.add('active'); document.getElementById('messages').scrollTop = document.getElementById('messages').scrollHeight; }}
function hideTyping() {{ document.getElementById('typing').classList.remove('active'); }}

function escapeHtml(text) {{
    const map = {{ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }};
    return text.replace(/[&<>"']/g, c => map[c]);
}}

document.getElementById('input').addEventListener('input', function() {{
    this.style.height = 'auto';
    this.style.height = Math.min(this.scrollHeight, 120) + 'px';
}});

// ── Start ──────────────────────────────────────────────────────
initAuth();
loadSessions();
loadHistory().then(connect);
</script>

</body>
</html>"#,
        greeting = greeting,
        session_id_short = &session_id[..8.min(session_id.len())],
        session_id = session_id,
        user_id_attr = user_id_attr,
        display_attr = display_attr,
    )
}
