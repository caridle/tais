// Habit capsule definitions — the 7 built-in habits (H01-H07)
//
// Each function returns a HabitRule with the parameters from the PRD/IMPL spec:
//
//   H01 复盘      Periodic      ScheduledTime{23,0}  SummarizeDecisions     eta=0.10 lambda=0.05
//   H02 容错      EventDriven   ErrorPattern{3}       SwitchStrategy         eta=0.15 lambda=0.08
//   H03 沟通      EventDriven   DialogueEnded          ConfirmUnderstanding   eta=0.08 lambda=0.03
//   H04 文档      EventDriven   OutputChanged          UpdateLogs             eta=0.05 lambda=0.10
//   H05 优化      EventDriven   EvolutionTriggered     SelfReview             eta=0.12 lambda=0.05
//   H06 安全      Conditional   HighRiskOperation      SafetyChecklist        eta=0.20 lambda=0.02
//   H07 协作      Conditional   MultiAgentMode         HandshakeAndDecompose  eta=0.08 lambda=0.06

use crate::habit::state::*;

pub fn h01_review() -> HabitRule {
    HabitRule {
        id: "H01".into(),
        name: "复盘习惯".into(),
        description: "每日23:00定时总结当日决策和错误模式，生成复盘报告".into(),
        trigger_type: TriggerType::Periodic,
        condition: HabitCondition::ScheduledTime { hour: 23, minute: 0 },
        action: HabitAction::SummarizeDecisions,
        learning_rate: 0.10,
        decay_rate: 0.05,
    }
}

pub fn h02_error_handling() -> HabitRule {
    HabitRule {
        id: "H02".into(),
        name: "容错习惯".into(),
        description: "连续3次同类错误时自动切换教学策略，调整决策阈值".into(),
        trigger_type: TriggerType::EventDriven,
        condition: HabitCondition::ErrorPattern { consecutive_errors: 3 },
        action: HabitAction::SwitchStrategy,
        learning_rate: 0.15,
        decay_rate: 0.08,
    }
}

pub fn h03_communication() -> HabitRule {
    HabitRule {
        id: "H03".into(),
        name: "沟通习惯".into(),
        description: "对话结束时确认学生理解程度，总结对话共识".into(),
        trigger_type: TriggerType::EventDriven,
        condition: HabitCondition::DialogueEnded,
        action: HabitAction::ConfirmUnderstanding,
        learning_rate: 0.08,
        decay_rate: 0.03,
    }
}

pub fn h04_documentation() -> HabitRule {
    HabitRule {
        id: "H04".into(),
        name: "文档习惯".into(),
        description: "教学产出变更时自动更新日志和版本标注".into(),
        trigger_type: TriggerType::EventDriven,
        condition: HabitCondition::OutputChanged,
        action: HabitAction::UpdateLogs,
        learning_rate: 0.05,
        decay_rate: 0.10,
    }
}

pub fn h05_optimization() -> HabitRule {
    HabitRule {
        id: "H05".into(),
        name: "优化习惯".into(),
        description: "进化引擎触发时自我审查当前状态，对比历史版本".into(),
        trigger_type: TriggerType::EventDriven,
        condition: HabitCondition::EvolutionTriggered,
        action: HabitAction::SelfReview,
        learning_rate: 0.12,
        decay_rate: 0.05,
    }
}

pub fn h06_security() -> HabitRule {
    HabitRule {
        id: "H06".into(),
        name: "安全习惯".into(),
        description: "高风险操作前强制执行安全检查清单".into(),
        trigger_type: TriggerType::Conditional,
        condition: HabitCondition::HighRiskOperation,
        action: HabitAction::SafetyChecklist,
        learning_rate: 0.20,
        decay_rate: 0.02,
    }
}

pub fn h07_collaboration() -> HabitRule {
    HabitRule {
        id: "H07".into(),
        name: "协作习惯".into(),
        description: "多Agent协作时执行握手协议和任务拆解".into(),
        trigger_type: TriggerType::Conditional,
        condition: HabitCondition::MultiAgentMode,
        action: HabitAction::HandshakeAndDecompose,
        learning_rate: 0.08,
        decay_rate: 0.06,
    }
}

/// Returns all 7 built-in habit rules
pub fn all_habit_rules() -> Vec<HabitRule> {
    vec![
        h01_review(),
        h02_error_handling(),
        h03_communication(),
        h04_documentation(),
        h05_optimization(),
        h06_security(),
        h07_collaboration(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_rules_have_unique_ids() {
        let rules = all_habit_rules();
        let mut ids: Vec<&str> = rules.iter().map(|r| r.id.as_str()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 7, "All 7 habits must have unique IDs");
    }

    #[test]
    fn test_h01_is_periodic() {
        let rule = h01_review();
        assert_eq!(rule.id, "H01");
        assert_eq!(rule.trigger_type, TriggerType::Periodic);
        assert_eq!(rule.learning_rate, 0.10);
        assert_eq!(rule.decay_rate, 0.05);
    }

    #[test]
    fn test_h02_error_pattern() {
        let rule = h02_error_handling();
        assert_eq!(rule.id, "H02");
        assert_eq!(rule.trigger_type, TriggerType::EventDriven);
    }

    #[test]
    fn test_h06_is_conditional() {
        let rule = h06_security();
        assert_eq!(rule.id, "H06");
        assert_eq!(rule.trigger_type, TriggerType::Conditional);
        assert_eq!(rule.learning_rate, 0.20);
    }

    #[test]
    fn test_learning_rates_valid() {
        for rule in all_habit_rules() {
            assert!(rule.learning_rate > 0.0 && rule.learning_rate <= 1.0,
                "{} learning rate {} out of range", rule.id, rule.learning_rate);
            assert!(rule.decay_rate > 0.0 && rule.decay_rate <= 1.0,
                "{} decay rate {} out of range", rule.id, rule.decay_rate);
        }
    }
}
