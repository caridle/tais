// Concrete TAIS skill implementations (all 7 TAIS skills)
//
// T00: Socratic Tutor        — 苏格拉底式导师
// T01: Workflow Orchestrator  — 教学工作流编排
// T02: Learning Analyst       — 学情分析
// T03: Resource Pusher        — 个性化资源推送
// T04: Skill Coach            — 技能教练
// T05: Feedback Collector     — 反馈采集
// T06: Evolution Engine       — 自进化引擎

use crate::*;
use async_trait::async_trait;
use std::sync::Arc;

// ── Helpers ───────────────────────────────────────────────────────────

fn make_def(
    name: &str, display: &str, desc: &str,
    cat: skills::SkillCategory,
    binds: Vec<&str>,
    schema: serde_json::Value,
    prompt: &str,
) -> skills::SkillDefinition {
    skills::SkillDefinition {
        name: name.into(), display_name: display.into(),
        version: "1.0.0".into(), description: desc.into(),
        category: cat,
        binds: binds.into_iter().map(String::from).collect(),
        input_schema: schema, system_prompt: prompt.into(),
        installed_at: chrono::Utc::now().to_rfc3339(),
    }
}

async fn llm_or_fallback(
    llm: &Arc<llm::LlmRouter>, system: &str, prompt: &str,
    fallback: serde_json::Value,
) -> Result<serde_json::Value, skills::SkillError> {
    match llm.chat_simple(system, prompt).await {
        Ok(resp) => {
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&resp) {
                Ok(parsed)
            } else {
                Ok(fallback)
            }
        }
        Err(_) => {
            tracing::warn!("LLM unavailable — using fallback");
            Ok(fallback)
        },
    }
}

// ═══════════════════════════════════════════════════════════════════════
// T00: Socratic Tutor — 苏格拉底式导师
// ═══════════════════════════════════════════════════════════════════════

pub struct SocraticTutor {
    def: skills::SkillDefinition,
    llm: Arc<llm::LlmRouter>,
}

impl SocraticTutor {
    pub fn new(llm: Arc<llm::LlmRouter>) -> Self {
        Self {
            def: make_def(
                "tais-socratic-tutor", "苏格拉底式导师",
                "通过反问引导而非直接给答案，培养学生独立思考能力",
                skills::SkillCategory::Teaching,
                vec!["socratic"],
                serde_json::json!({
                    "type": "object",
                    "required": ["student_query"],
                    "properties": {
                        "student_query": {"type": "string", "description": "学生的问题"},
                        "concept": {"type": "string"},
                        "strategy": {"type": "string", "enum": ["clarification","counterexample","analogy","scaffold"]},
                        "context": {"type": "string"}
                    }
                }),
                "你是一位苏格拉底式导师。永远不直接给答案，用追问引导学生自己发现。\
                 每次不超过3个追问。输出JSON：{\"content\":..., \"strategy\":..., \"question_count\":..., \"confidence\":..., \"progress_rounds\":..., \"error_rate\":...}",
            ),
            llm,
        }
    }
}

#[async_trait]
impl skills::TaisSkill for SocraticTutor {
    fn name(&self) -> &str { "tais-socratic-tutor" }
    fn definition(&self) -> &skills::SkillDefinition { &self.def }

    async fn execute(
        &self, input: serde_json::Value, _gene: &GeneProfile,
    ) -> Result<serde_json::Value, skills::SkillError> {
        let query = input["student_query"].as_str().unwrap_or("未知问题");
        let concept = input["concept"].as_str().unwrap_or("当前主题");
        let strategy = input["strategy"].as_str().unwrap_or("clarification");
        let context = input["context"].as_str().unwrap_or("");
        let prompt = format!(
            "学生正在学习「{concept}」，策略「{strategy}」。\n上下文：{context}\n\n学生问题：{query}\n\n请生成苏格拉底式追问（JSON）："
        );

        llm_or_fallback(&self.llm, &self.def.system_prompt, &prompt,
            fallback_socratic(query, concept, strategy),
        ).await
    }

    fn should_escalate(&self, output: &serde_json::Value) -> Option<HitlTrigger> {
        if output["confidence"].as_f64().unwrap_or(1.0) < 0.5 {
            Some(HitlTrigger {
                condition: HitlCondition::ConfidenceBelow(0.5),
                action: HitlAction::EscalateToTeacher,
            })
        } else { None }
    }
}

fn fallback_socratic(query: &str, concept: &str, strategy: &str) -> serde_json::Value {
    let (content, qc) = match strategy {
        "clarification" => (
            format!("🤔 关于「{concept}」，你说「{query}」——能用更具体的例子说明吗？\n追问：如果不考虑现有知识，你会从哪个角度重新看待这个问题？"),
            2,
        ),
        "counterexample" => (
            format!("🧐 你提到「{query}」。请考虑：如果条件反过来，结果还会一样吗？\n追问：什么情况下你的结论可能不成立？"),
            2,
        ),
        "analogy" => (
            format!("💡 「{query}」让我想到一个类比……你能自己找一个生活中的例子来说明「{concept}」吗？\n追问：你的类比和原问题有什么本质区别？"),
            2,
        ),
        "scaffold" => (
            format!("🪜 关于「{concept}」，拆开来看：\n1. 前提条件是什么？\n2. 核心机制是什么？\n3. 后果是什么？\n你先回答第一步。"),
            3,
        ),
        _ => (
            format!("🤔 你说「{query}」，换个角度——去掉术语，普通人怎样理解「{concept}」？"),
            1,
        ),
    };
    serde_json::json!({
        "content": content, "strategy": strategy, "question_count": qc,
        "confidence": 0.8, "progress_rounds": 0, "error_rate": 0.0
    })
}

// ═══════════════════════════════════════════════════════════════════════
// T01: Workflow Orchestrator
// ═══════════════════════════════════════════════════════════════════════

pub struct WorkflowOrchestrator {
    def: skills::SkillDefinition,
    llm: Arc<llm::LlmRouter>,
}

impl WorkflowOrchestrator {
    pub fn new(llm: Arc<llm::LlmRouter>) -> Self {
        Self { def: make_def(
            "tais-workflow", "教学工作流编排器",
            "根据教学目标和学生画像，自动编排教学节点序列（DAG）",
            skills::SkillCategory::Orchestration, vec!["workflow"],
            serde_json::json!({"type":"object","required":["goal"],"properties":{"goal":{"type":"string"},"student_level":{"type":"string"},"duration_min":{"type":"integer"}}}),
            "你是教学设计专家。根据教学目标设计DAG流程。输出JSON：{\"nodes\":[{\"id\":...,\"agent\":...,\"name\":...}],\"edges\":[[from,to],...]}",
        ), llm }
    }
}

#[async_trait]
impl skills::TaisSkill for WorkflowOrchestrator {
    fn name(&self) -> &str { "tais-workflow" }
    fn definition(&self) -> &skills::SkillDefinition { &self.def }
    async fn execute(&self, input: serde_json::Value, _gene: &GeneProfile) -> Result<serde_json::Value, skills::SkillError> {
        let goal = input["goal"].as_str().unwrap_or("通用教学");
        llm_or_fallback(&self.llm, &self.def.system_prompt,
            &format!("请为教学目标「{goal}」设计教学流程"),
            serde_json::json!({"nodes":[{"id":"n1","agent":"tais-socratic-tutor","name":"概念引入"},{"id":"n2","agent":"tais-socratic-tutor","name":"深度追问"},{"id":"n3","agent":"tais-feedback-collector","name":"效果反馈"}],"edges":[["n1","n2"],["n2","n3"]],"confidence":0.75}),
        ).await
    }
    fn should_escalate(&self, _: &serde_json::Value) -> Option<HitlTrigger> { None }
}

// ═══════════════════════════════════════════════════════════════════════
// T02: Learning Analyst
// ═══════════════════════════════════════════════════════════════════════

pub struct LearningAnalyst {
    def: skills::SkillDefinition,
    llm: Arc<llm::LlmRouter>,
}

impl LearningAnalyst {
    pub fn new(llm: Arc<llm::LlmRouter>) -> Self {
        Self { def: make_def(
            "tais-learning-analyst", "学情分析师",
            "分析学生对话历史，识别知识掌握度、薄弱点和学习风格",
            skills::SkillCategory::Analysis, vec!["analyst"],
            serde_json::json!({"type":"object","required":["conversation_history"],"properties":{"conversation_history":{"type":"string"},"concept":{"type":"string"}}}),
            "你是学情分析专家。分析学生对话，输出JSON：{mastery_level, weak_points[], learning_style, recommended_strategy}",
        ), llm }
    }
}

#[async_trait]
impl skills::TaisSkill for LearningAnalyst {
    fn name(&self) -> &str { "tais-learning-analyst" }
    fn definition(&self) -> &skills::SkillDefinition { &self.def }
    async fn execute(&self, input: serde_json::Value, _gene: &GeneProfile) -> Result<serde_json::Value, skills::SkillError> {
        let history = input["conversation_history"].as_str().unwrap_or("");
        let concept = input["concept"].as_str().unwrap_or("未知");
        llm_or_fallback(&self.llm, &self.def.system_prompt,
            &format!("分析学生对「{concept}」的理解。对话：{history}"),
            serde_json::json!({"concept":concept,"mastery_level":0.5,"weak_points":["需更多数据"],"learning_style":"inquiry","recommended_strategy":"clarification","confidence":0.6}),
        ).await
    }
    fn should_escalate(&self, o: &serde_json::Value) -> Option<HitlTrigger> {
        if o["mastery_level"].as_f64().unwrap_or(1.0) < 0.3 {
            Some(HitlTrigger { condition: HitlCondition::ConfidenceBelow(0.3), action: HitlAction::EscalateToTeacher })
        } else { None }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// T03: Resource Pusher
// ═══════════════════════════════════════════════════════════════════════

pub struct ResourcePusher {
    def: skills::SkillDefinition,
    llm: Arc<llm::LlmRouter>,
}

impl ResourcePusher {
    pub fn new(llm: Arc<llm::LlmRouter>) -> Self {
        Self { def: make_def(
            "tais-resource-pusher", "个性化资源推送员",
            "根据学生薄弱点推荐学习资源",
            skills::SkillCategory::Resource, vec!["resource"],
            serde_json::json!({"type":"object","required":["weak_points"],"properties":{"weak_points":{"type":"array","items":{"type":"string"}},"concept":{"type":"string"}}}),
            "你是学习资源推荐专家。推荐3-5个资源。输出JSON：{\"resources\":[{\"title\":...,\"type\":\"article|video|exercise\",\"url\":...,\"why\":...}]}",
        ), llm }
    }
}

#[async_trait]
impl skills::TaisSkill for ResourcePusher {
    fn name(&self) -> &str { "tais-resource-pusher" }
    fn definition(&self) -> &skills::SkillDefinition { &self.def }
    async fn execute(&self, input: serde_json::Value, _gene: &GeneProfile) -> Result<serde_json::Value, skills::SkillError> {
        let weak = input["weak_points"].as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(", "))
            .unwrap_or_else(|| "通用".into());
        let concept = input["concept"].as_str().unwrap_or("当前主题");
        llm_or_fallback(&self.llm, &self.def.system_prompt,
            &format!("推荐「{concept}」资源，薄弱点：{weak}"),
            serde_json::json!({"resources":[{"title":format!("{concept}入门"),"type":"article","url":"#","why":"基础"},{"title":format!("{concept}练习"),"type":"exercise","url":"#","why":"巩固"}],"confidence":0.7}),
        ).await
    }
    fn should_escalate(&self, _: &serde_json::Value) -> Option<HitlTrigger> { None }
}

// ═══════════════════════════════════════════════════════════════════════
// T04: Skill Coach
// ═══════════════════════════════════════════════════════════════════════

pub struct SkillCoach {
    def: skills::SkillDefinition,
    llm: Arc<llm::LlmRouter>,
}

impl SkillCoach {
    pub fn new(llm: Arc<llm::LlmRouter>) -> Self {
        Self { def: make_def(
            "tais-skill-coach", "技能教练",
            "生成针对性练习题，批改并给出改进建议",
            skills::SkillCategory::Coaching, vec!["coach"],
            serde_json::json!({"type":"object","required":["action","concept"],"properties":{"action":{"type":"string","enum":["generate_exercise","grade_answer"]},"concept":{"type":"string"},"student_answer":{"type":"string"},"difficulty":{"type":"string"}}}),
            "你是技能教练。输出JSON：{\"exercise\":...,\"grading\":...,\"feedback\":...,\"difficulty\":...,\"confidence\":...}",
        ), llm }
    }
}

#[async_trait]
impl skills::TaisSkill for SkillCoach {
    fn name(&self) -> &str { "tais-skill-coach" }
    fn definition(&self) -> &skills::SkillDefinition { &self.def }
    async fn execute(&self, input: serde_json::Value, _gene: &GeneProfile) -> Result<serde_json::Value, skills::SkillError> {
        let action = input["action"].as_str().unwrap_or("generate_exercise");
        let concept = input["concept"].as_str().unwrap_or("当前主题");
        let prompt = if action == "grade_answer" {
            format!("批改「{concept}」答案：{}", input["student_answer"].as_str().unwrap_or(""))
        } else {
            format!("为「{concept}」生成练习题")
        };
        llm_or_fallback(&self.llm, &self.def.system_prompt, &prompt,
            serde_json::json!({"exercise":format!("用自己的话解释「{concept}」并举实际例子"),"grading":null,"feedback":"请提交后获得反馈","difficulty":"medium","confidence":0.8}),
        ).await
    }
    fn should_escalate(&self, _: &serde_json::Value) -> Option<HitlTrigger> { None }
}

// ═══════════════════════════════════════════════════════════════════════
// T05: Feedback Collector
// ═══════════════════════════════════════════════════════════════════════

pub struct FeedbackCollector {
    def: skills::SkillDefinition,
    llm: Arc<llm::LlmRouter>,
}

impl FeedbackCollector {
    pub fn new(llm: Arc<llm::LlmRouter>) -> Self {
        Self { def: make_def(
            "tais-feedback-collector", "反馈采集器",
            "采集学生对教学的反馈，分析教学效果和满意度",
            skills::SkillCategory::Feedback, vec!["feedback"],
            serde_json::json!({"type":"object","required":["session_data"],"properties":{"session_data":{"type":"string"},"student_id":{"type":"string"}}}),
            "你是教学反馈分析师。输出JSON：{comprehension, engagement, satisfaction, key_improvements[], summary}",
        ), llm }
    }
}

#[async_trait]
impl skills::TaisSkill for FeedbackCollector {
    fn name(&self) -> &str { "tais-feedback-collector" }
    fn definition(&self) -> &skills::SkillDefinition { &self.def }
    async fn execute(&self, input: serde_json::Value, _gene: &GeneProfile) -> Result<serde_json::Value, skills::SkillError> {
        let data = input["session_data"].as_str().unwrap_or("无数据");
        llm_or_fallback(&self.llm, &self.def.system_prompt,
            &format!("分析教学：{data}"),
            serde_json::json!({"comprehension":0.7,"engagement":0.6,"satisfaction":0.7,"key_improvements":["增加互动","更具体例子"],"summary":"教学效果良好，建议增加交互练习","confidence":0.65}),
        ).await
    }
    fn should_escalate(&self, o: &serde_json::Value) -> Option<HitlTrigger> {
        if o["satisfaction"].as_f64().unwrap_or(1.0) < 0.4 {
            Some(HitlTrigger { condition: HitlCondition::ConfidenceBelow(0.4), action: HitlAction::EscalateToTeacher })
        } else { None }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// T06: Evolution Engine
// ═══════════════════════════════════════════════════════════════════════

pub struct EvolutionSkill {
    def: skills::SkillDefinition,
    llm: Arc<llm::LlmRouter>,
}

impl EvolutionSkill {
    pub fn new(llm: Arc<llm::LlmRouter>) -> Self {
        Self { def: make_def(
            "tais-evolution", "自进化引擎",
            "分析教学历史，提出策略优化建议和自我改进方案",
            skills::SkillCategory::Evolution, vec!["evolution"],
            serde_json::json!({"type":"object","required":["session_history"],"properties":{"session_history":{"type":"string"},"target_metric":{"type":"string"}}}),
            "你是教学策略进化专家。输出JSON：{improvements:[{area,suggestion,expected_gain}], priority, rationale}",
        ), llm }
    }
}

#[async_trait]
impl skills::TaisSkill for EvolutionSkill {
    fn name(&self) -> &str { "tais-evolution" }
    fn definition(&self) -> &skills::SkillDefinition { &self.def }
    async fn execute(&self, input: serde_json::Value, _gene: &GeneProfile) -> Result<serde_json::Value, skills::SkillError> {
        let history = input["session_history"].as_str().unwrap_or("");
        llm_or_fallback(&self.llm, &self.def.system_prompt,
            &format!("优化教学策略：{history}"),
            serde_json::json!({"improvements":[
                {"area":"提问策略","suggestion":"增加元认知追问","expected_gain":0.15},
                {"area":"反馈速度","suggestion":"缩短等待时间","expected_gain":0.08}
            ],"priority":"medium","rationale":"提问深度是最大瓶颈","confidence":0.6}),
        ).await
    }
    fn should_escalate(&self, _: &serde_json::Value) -> Option<HitlTrigger> { None }
}

// ═══════════════════════════════════════════════════════════════════════
// Builder: all 7 TAIS skills
// ═══════════════════════════════════════════════════════════════════════

pub fn all_tais_skills(llm: Arc<llm::LlmRouter>) -> Vec<Box<dyn skills::TaisSkill>> {
    vec![
        Box::new(SocraticTutor::new(llm.clone())),
        Box::new(WorkflowOrchestrator::new(llm.clone())),
        Box::new(LearningAnalyst::new(llm.clone())),
        Box::new(ResourcePusher::new(llm.clone())),
        Box::new(SkillCoach::new(llm.clone())),
        Box::new(FeedbackCollector::new(llm.clone())),
        Box::new(EvolutionSkill::new(llm.clone())),
    ]
}
