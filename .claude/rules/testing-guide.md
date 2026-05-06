# .claude 测试指南

## hooks/ — 直接运行脚本
```bash
# 提交前检查
bash .claude/hooks/pre-commit.sh

# 构建后验证
bash .claude/hooks/post-build.sh
```

## skills/ — 对话中触发
```
/deploy      一键构建安装包
/local-test  启动本地测试环境
```

## agents/ — 对话中触发
```
审查一下代码  → 调用 code-reviewer agent
```

## rules/ — AI 自动遵循（无需手动测试）
code-style.md / testing.md / api-conventions.md 在 AI 编写代码/测试/API 时自动生效。

## settings.json hooks — 通过特定操作触发
1. 编辑 .env 文件 → 被 PreToolUse① 拦截
2. 说 "帮我提交代码" → 触发 PreToolUse⑧（运行 pre-commit.sh）
3. 说 "构建安装包" → 触发 PostToolUse⑨（运行 post-build.sh）
4. 输入 rm -rf / → 被 PreToolUse② 拦截
5. 输入 git status → PreToolUse③ 自动放行
6. 编辑 .tsx 文件 → PostToolUse④ 自动格式化
7. 运行任意命令 → PostToolUse⑤ 记录到 command_log.txt
8. 上下文压缩后 → PostCompact⑦ 自动注入 compaction-context.md
