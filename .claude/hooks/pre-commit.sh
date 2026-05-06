#!/usr/bin/env bash
# ============================
# pre-commit.sh
# 通用 Git 提交前检查：自动检测项目类型并运行对应测试/lint
# 被 .claude/settings.json PreToolUse 引用
#
# 自定义：如需增删检查项，直接编辑下方各区块
# ============================
set -euo pipefail

PROJECT_DIR="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$PROJECT_DIR"

echo "=== pre-commit 检查 ==="
FAILED=0

# ---- 后端测试 ----
if [ -f "Cargo.toml" ]; then
    echo "[cargo] 运行 Rust 测试..."
    cargo test 2>&1 || { echo "FAIL: cargo test 失败"; FAILED=1; }
elif [ -f "go.mod" ]; then
    echo "[go] 运行 Go 测试..."
    go test ./... 2>&1 || { echo "FAIL: go test 失败"; FAILED=1; }
elif [ -f "pom.xml" ] || [ -f "build.gradle" ]; then
    echo "[java] 运行测试..."
    if [ -f "mvnw" ]; then ./mvnw test; else mvn test; fi 2>&1 || { echo "FAIL: 测试失败"; FAILED=1; }
fi

# ---- 前端测试 ----
if [ -f "package.json" ]; then
    HAS_SCRIPT=$(node -e "const p=require('./package.json'); console.log(p.scripts?.test ? 'yes' : 'no')" 2>/dev/null || echo "no")
    if [ "$HAS_SCRIPT" = "yes" ]; then
        echo "[npm] 运行测试..."
        npm test 2>&1 || { echo "FAIL: npm test 失败"; FAILED=1; }
    fi

    # TypeScript 类型检查（如果有 tsconfig）
    if [ -f "tsconfig.json" ] || [ -f "tsconfig.app.json" ]; then
        TSCONFIG="tsconfig.json"
        [ -f "tsconfig.app.json" ] && TSCONFIG="tsconfig.app.json"
        echo "[tsc] 类型检查..."
        npx tsc -b 2>&1 || { echo "FAIL: TypeScript 类型错误"; FAILED=1; }
    fi

    # ESLint
    if [ -f "eslint.config.js" ] || [ -f ".eslintrc" ] || [ -f ".eslintrc.json" ]; then
        echo "[eslint] 代码检查..."
        npx eslint . 2>&1 || echo "WARN: ESLint 有警告（不阻塞）"
    fi
fi

# ---- Python ----
if [ -f "setup.py" ] || [ -f "pyproject.toml" ] || [ -f "requirements.txt" ]; then
    if command -v pytest &>/dev/null && [ -d "tests" ]; then
        echo "[pytest] 运行 Python 测试..."
        python -m pytest 2>&1 || { echo "FAIL: pytest 失败"; FAILED=1; }
    fi
fi

if [ $FAILED -eq 1 ]; then
    echo "=== pre-commit 失败：存在未通过项 ==="
    exit 1
fi

echo "=== pre-commit 全部通过 ==="
