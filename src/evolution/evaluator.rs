// Evolution Evaluator — compute multi-dimensional metrics from session data

use crate::{EvolutionMetrics, SessionRecord};

/// Compute the 5-dimensional evolution metrics from session records.
pub fn compute_metrics(sessions: &[SessionRecord]) -> EvolutionMetrics {
    if sessions.is_empty() {
        return EvolutionMetrics {
            learning_effectiveness: 0.0,
            teaching_efficiency: 0.0,
            student_autonomy: 0.0,
            resource_engagement: 0.0,
            teacher_satisfaction: 0.0,
            composite: 0.0,
        };
    }

    let n = sessions.len() as f64;

    // 1. Learning Effectiveness (normalized gain)
    let learning_effectiveness: f64 = sessions
        .iter()
        .map(|s| {
            let gain = s.post_score - s.pre_score;
            if s.pre_score >= 1.0 {
                0.0
            } else {
                gain / (1.0 - s.pre_score).max(0.01)
            }
        })
        .sum::<f64>()
        / n;

    // 2. Teaching Efficiency (breakthroughs per dialogue round)
    let teaching_efficiency: f64 = sessions
        .iter()
        .map(|s| {
            if s.dialogue_rounds == 0 {
                0.0
            } else {
                s.breakthrough_count as f64 / s.dialogue_rounds as f64
            }
        })
        .sum::<f64>()
        / n;

    // 3. Student Autonomy (1 - hitl escalation rate)
    let total_hitl: u32 = sessions.iter().map(|s| s.hitl_escalations).sum();
    let student_autonomy = 1.0 - (total_hitl as f64 / n).min(1.0);

    // 4. Resource Engagement (click-through rate)
    let total_pushed: u32 = sessions.iter().map(|s| s.resources_pushed).sum();
    let total_clicked: u32 = sessions.iter().map(|s| s.resources_clicked).sum();
    let resource_engagement = if total_pushed > 0 {
        total_clicked as f64 / total_pushed as f64
    } else {
        1.0 // no resources pushed = no penalty
    };

    // 5. Teacher Satisfaction (manual rating)
    let teacher_satisfaction: f64 = sessions
        .iter()
        .filter_map(|s| s.teacher_rating)
        .sum::<f64>()
        / sessions.iter().filter(|s| s.teacher_rating.is_some()).count().max(1) as f64;

    // Composite score (weighted average)
    let composite = 0.35 * learning_effectiveness
        + 0.25 * teaching_efficiency
        + 0.20 * student_autonomy
        + 0.10 * resource_engagement
        + 0.10 * teacher_satisfaction;

    EvolutionMetrics {
        learning_effectiveness,
        teaching_efficiency,
        student_autonomy,
        resource_engagement,
        teacher_satisfaction,
        composite,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_session(post: f64, rounds: u32, breakthroughs: u32, hitl: u32) -> SessionRecord {
        SessionRecord {
            session_id: uuid::Uuid::new_v4(),
            student_id: "test".into(),
            workflow_id: uuid::Uuid::new_v4(),
            pre_score: 0.4,
            post_score: post,
            dialogue_rounds: rounds,
            breakthrough_count: breakthroughs,
            hitl_escalations: hitl,
            resources_pushed: 3,
            resources_clicked: 2,
            stuck_points: vec![],
            teacher_rating: Some(8.0),
            created_at: chrono::Utc::now().naive_utc(),
        }
    }

    #[test]
    fn test_perfect_session() {
        let sessions = vec![make_session(1.0, 5, 5, 0)];
        let metrics = compute_metrics(&sessions);
        assert!(metrics.learning_effectiveness > 0.9);
        assert!(metrics.teaching_efficiency > 0.9);
        assert_eq!(metrics.student_autonomy, 1.0);
    }

    #[test]
    fn test_poor_session() {
        let sessions = vec![make_session(0.4, 10, 1, 3)];
        let metrics = compute_metrics(&sessions);
        assert!(metrics.learning_effectiveness < 0.1);
        assert!(metrics.teaching_efficiency < 0.2);
        assert_eq!(metrics.student_autonomy, 0.0); // capped at 0
    }
}
