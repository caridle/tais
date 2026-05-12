// Evolution Collector — aggregate session data into class-level reports

use crate::SessionRecord;

/// Aggregate multiple sessions into a class-level summary
pub fn aggregate_sessions(sessions: &[SessionRecord]) -> ClassReport {
    let n = sessions.len() as f64;
    if n == 0.0 {
        return ClassReport::default();
    }

    let avg_improvement: f64 = sessions.iter().map(|s| s.post_score - s.pre_score).sum::<f64>() / n;
    let avg_dialogue_rounds: f64 = sessions.iter().map(|s| s.dialogue_rounds as f64).sum::<f64>() / n;
    let avg_breakthrough: f64 = sessions.iter().map(|s| s.breakthrough_count as f64).sum::<f64>() / n;
    let total_hitl: u32 = sessions.iter().map(|s| s.hitl_escalations).sum();
    let hitl_rate = total_hitl as f64 / n;
    let total_pushed: u32 = sessions.iter().map(|s| s.resources_pushed).sum();
    let total_clicked: u32 = sessions.iter().map(|s| s.resources_clicked).sum();
    let engagement = if total_pushed > 0 {
        total_clicked as f64 / total_pushed as f64
    } else {
        0.0
    };

    // Collect common stuck points
    let mut stuck_freq: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    for s in sessions {
        for point in &s.stuck_points {
            *stuck_freq.entry(point.clone()).or_insert(0) += 1;
        }
    }
    let mut common_stuck: Vec<_> = stuck_freq.into_iter().collect();
    common_stuck.sort_by_key(|(_, c)| std::cmp::Reverse(*c));
    let common_stuck: Vec<String> = common_stuck.into_iter().take(5).map(|(k, _)| k).collect();

    ClassReport {
        total_sessions: sessions.len() as u32,
        avg_improvement,
        avg_dialogue_rounds,
        avg_breakthrough_per_session: avg_breakthrough,
        hitl_escalation_rate: hitl_rate,
        resource_engagement: engagement,
        common_stuck_points: common_stuck,
    }
}

#[derive(Debug, Clone, Default)]
pub struct ClassReport {
    pub total_sessions: u32,
    pub avg_improvement: f64,
    pub avg_dialogue_rounds: f64,
    pub avg_breakthrough_per_session: f64,
    pub hitl_escalation_rate: f64,
    pub resource_engagement: f64,
    pub common_stuck_points: Vec<String>,
}
