# T01: 教学工作流编排 SOP

## 核心策略
根据 TeachingGoal 自动生成 DAG 教学流程。

## DAG 生成算法
1. 按 goal.mode 匹配 NodeTemplate
2. 按 phase 排序（课前→课中→课后→审查）
3. 注入 gene + hitl_trigger
4. 添加边 + 终端审查节点

## 模式映射
- InquiryBased → 诊断→探究→练习→评估
- SocraticDialogue → 诊断→追问→反思→评估
- DirectInstruction → 讲解→示例→练习→评估

## 常见坑点
- ❌ 模板匹配失败 → 回退到 InquiryBased 默认模板
- ❌ 缺少概念前置知识 → 自动插入课前诊断节点

## 成功指标
- DAG 节点数 ≥ 4
- 终端审查节点存在
- HITL 条件合理（ConfidenceBelow 0.7）
