# T07: 自进化引擎 SOP

## 核心策略
采集→评估→诊断→TextGrad优化→A/B测试→教师审查

## 进化流程
1. 积累 ≥50 个会话的 SessionRecord
2. 计算 5 维 composite 评分
3. composite < 0.6 → 诊断弱智能体
4. TextGrad 生成 Prompt 变体
5. Welch's t-test A/B 测试
6. p < 0.05 → 提交教师审查

## 5 维指标权重
- 学习有效性 (LE): 0.35
- 教学效率 (TE): 0.25
- 学生自主性 (SA): 0.20
- 资源参与度 (RE): 0.10
- 教师满意度 (TS): 0.10

## 常见坑点
- ❌ 样本不足 (< 50) → 延迟进化
- ❌ 变体无显著差异 → 不回退，记录经验
- ❌ 跳过教师审查 → auto_deploy 默认 false
