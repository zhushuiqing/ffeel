#!/usr/bin/env bash
# ============================
# post-build.sh
# 通用构建后处理：检测构建系统，验证产物
# 被 .claude/settings.json PostToolUse 引用
#
# 自定义：修改 PRODUCT_PATTERNS 数组适配项目产物路径
# ============================
set -euo pipefail

PROJECT_DIR="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$PROJECT_DIR"

echo "=== post-build 检查 ==="

# ---- Tauri 桌面应用 ----
if [ -f "src-tauri/tauri.conf.json" ]; then
    APP_NAME=$(node -e "const p=require('./src-tauri/tauri.conf.json'); console.log(p.productName||'app')" 2>/dev/null || echo "app")
    RELEASE_DIR="src-tauri/target/release"

    for bundle in "$RELEASE_DIR/bundle"/*/; do
        [ -d "$bundle" ] || continue
        TYPE=$(basename "$bundle")
        ITEM=$(ls -t "$bundle"* 2>/dev/null | head -1)
        if [ -n "$ITEM" ]; then
            SIZE=$(du -sh "$ITEM" 2>/dev/null | cut -f1)
            echo "[$TYPE] $(basename "$ITEM") ($SIZE)"
        fi
    done

    # 至少找到二进制
    if [ -f "$RELEASE_DIR/$APP_NAME" ]; then
        echo "[binary] $RELEASE_DIR/$APP_NAME ($(du -sh "$RELEASE_DIR/$APP_NAME" | cut -f1))"
    elif ! ls "$RELEASE_DIR/bundle/"*/"$APP_NAME"* 2>/dev/null | grep -q .; then
        echo "WARN: 未找到构建产物"
    fi
fi

# ---- Rust CLI / lib ----
if [ -f "Cargo.toml" ] && [ ! -f "src-tauri/tauri.conf.json" ]; then
    APP_NAME=$(node -e "const p=require('child_process').execSync('cargo metadata --no-deps --format-version 1',{encoding:'utf8'}); const m=JSON.parse(p); console.log(m.packages[0]?.name||'app')" 2>/dev/null || echo "app")
    TARGET_DIR="target/release"
    if [ -f "$TARGET_DIR/$APP_NAME" ]; then
        echo "[binary] $TARGET_DIR/$APP_NAME ($(du -sh "$TARGET_DIR/$APP_NAME" | cut -f1))"
    elif [ -f "$TARGET_DIR/lib${APP_NAME}.so" ] || [ -f "$TARGET_DIR/${APP_NAME}.dll" ] || [ -f "$TARGET_DIR/lib${APP_NAME}.dylib" ]; then
        echo "[lib] 构建产物在 $TARGET_DIR/"
    fi
fi

# ---- Go ----
if [ -f "go.mod" ]; then
    BIN=$(ls -t 2>/dev/null | head -1 || echo "")
    for f in *; do
        [ -x "$f" ] && [ ! -d "$f" ] && file "$f" | grep -q "executable" && echo "[binary] $f ($(du -sh "$f" | cut -f1))"
    done || true
fi

# ---- Node ----
if [ -f "package.json" ]; then
    if [ -d "dist" ]; then
        echo "[dist] dist/ ($(du -sh dist | cut -f1))"
    fi
    if [ -d "build" ]; then
        echo "[build] build/ ($(du -sh build | cut -f1))"
    fi
    if [ -d "out" ]; then
        echo "[out] out/ ($(du -sh out | cut -f1))"
    fi
fi

echo "=== post-build 完成 ==="
