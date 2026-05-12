# TAIS — Teacher-AI-Student 自进化教学系统

[![Rust](https://img.shields.io/badge/Rust-2024%20Edition-orange.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

TAIS 是一个**自进化 AI Agent 教学系统**，借鉴 GenericAgent 的工具执行循环和 Hermes 的热记忆架构。三胶囊体系（能力×基因×习惯）驱动 Agent 行为，支持浏览器操控、代码执行、文件读写和技能自动结晶。

---

## 快速开始

```bash
# 编译
cargo build

# 运行（默认 localhost:8080）
cargo run

# 配置 LLM（3 种方式）
cargo run -- --llm-provider openai --llm-model gpt-4o --llm-api-key sk-xxx
# 或: 环境变量 TAIS_LLM_PROVIDER=openai
# 或: 打开 http://localhost:8080 在 Dashboard 配置

# 重置密码
cargo run -- --reset-password 用户名 新密码

# 直接运行（秒启动）
.\target\debug\tais-server.exe
```

打开 `http://localhost:8080` → 注册/登录 → 配置 LLM → 开始对话。

---

## 三种模式

| 模式 | 触发词 | 行为 |
|------|--------|------|
| 💬 **通用模式** | `通用模式` / `聊天模式` | AI Agent 自然对话，自动调用工具执行任务 |
| 📚 **学习模式** | `学习模式` | 苏格拉底追问引导，调用 TAIS 技能胶囊 |
| ⚙️ **命令模式** | `命令模式` | 系统管理：`llm`, `memory`, `habits`, `status`, `passwd` |

---

## Agent 能力

| 能力 | 说明 |
|------|------|
| Python 代码执行 | 自动注入 `file_read/write/patch`, `ask_user` helpers |
| 浏览器操控 | 通过 browser-harness 控制 Chrome：打开网页、执行 JS、截图 |
| 文件读写 | `file_read`, `file_write`, `file_patch` |
| 用户提问 | `ask_user` — Agent 中途向用户确认 |
| 技能结晶 | 成功任务自动生成可复用 SOP 文件 |
| 习惯引擎 | 7 个习惯胶囊 (H01-H07)，强化学习自动优化 |
| 热记忆 | ≤500 字符关键事实，每轮注入 |

---

## 命令模式速查

| 命令 | 功能 |
|------|------|
| `help` | 完整命令列表 |
| `status` | 系统运行状态 |
| `habits` | 7 习惯胶囊权重 |
| `gene` / `gene hacker` | 查看/设置 AI 人格 |
| `llm` / `llm list` / `llm add` | LLM 配置管理 |
| `memory` / `memory add` | 热记忆管理 |
| `whoami` / `passwd` | 用户信息/改密码 |
| `通用模式` / `学习模式` / `命令模式` | 模式切换 |

---

## 架构

```
用户 → WebSocket → handle_ws → 模式路由
                                    ├─ General  → Agent Loop (LLM + Code + Browser)
                                    ├─ Learning → Skills Bus (Socratic Tutor)
                                    └─ Command  → System Handlers

Agent Loop:
  LLM → 生成计划 + ```python/```browser 代码块
  → 自动执行 → 结果注入 LLM → 循环 (max 3轮)
  → 成功执行自动结晶为 Skill
```

### 核心模块

| 模块 | 文件 | 职责 |
|------|------|------|
| Agent Loop | `src/agent/loop.rs` | 双速循环 (fast/quality path) |
| API / WebSocket | `src/api/mod.rs` | 10+ REST 端点 + WS 会话 |
| Chat UI | `src/chat/mod.rs` | Markdown/LaTeX/Mermaid 渲染 |
| LLM Router | `src/llm/mod.rs` | OpenAI/Anthropic/Ollama 统一接口 |
| Habit Engine | `src/habit/` | 7 胶囊 + 强化学习 + 调度器 |
| Skill Loader | `src/skills/loader.rs` | L1 索引 → L3 SOP 按需加载 |
| Skill Crystallizer | `src/skills/crystallizer.rs` | Agent 成功任务 → 自动 SOP |
| Hot Memory | `src/memory/hot.rs` | Hermes 风格 ≤500 字符热记忆 |
| Evolution Engine | `src/evolution/` | TextGrad 自进化 |
| MCP Gateway | `src/mcp/` | 35 胶囊工具注册 + JSON-RPC |

---

## 项目结构

```
tais-core/
├── Cargo.toml
├── assets/agent_helpers.py    # Python 执行环境预导入函数
├── memory/
│   ├── skills/                # 技能 SOP 文件 (11 个)
│   │   └── _index.md          # L1 技能索引
│   └── tais_hot_memory.md     # 热记忆文件
├── migrations/0001_init.sql   # SQLite 表结构
└── src/
    ├── main.rs                # 入口 + CLI
    ├── lib.rs                 # 核心类型
    ├── agent/                 # Agent 闭环 (Proposer/Consumer/Rater/Deployer/Loop)
    ├── api/                   # HTTP + WebSocket
    ├── chat/                  # Web Chat UI (Markdown/LaTeX/Mermaid)
    ├── auth/                  # JWT + QR 登录 + 微信绑定
    ├── config.rs              # 配置加载 (tais.toml)
    ├── dashboard/             # Dashboard 首页
    ├── data/                  # SQLite 初始化
    ├── evolution/             # TextGrad 进化引擎
    ├── gene/                  # 基因网关 (Scholar/Mentor/Hacker)
    ├── habit/                 # 习惯引擎 (7 capsules)
    ├── llm/                   # LLM 路由器
    ├── mcp/                   # MCP 工具网关
    ├── memory/                # 三层记忆 (hot/checkpoint/shared/user)
    ├── orchestrator/          # DAG 工作流编排
    ├── skills/                # 技能总线 + 加载器 + 结晶器
    └── wechat/                # 企业微信机器人
```

---

## 浏览器操控 (Browser Harness)

```bash
# 安装 browser-harness
git clone https://github.com/browser-use/browser-harness
cd browser-harness && uv tool install -e .

# 启用 Chrome 远程调试
# chrome://inspect/#remote-debugging → 勾选 Allow remote debugging

# 然后在 TAIS 中说：
# "用浏览器打开中南民族大学官网，看看有哪些新闻"
```

Agent 会自动写 ` ```browser` 代码块操控 Chrome。

---

## 测试

```bash
cargo test
# 72+ tests, 0 failures
```

---

## License

MIT
