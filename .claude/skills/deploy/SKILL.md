---
name: deploy
description: 一键构建 Mac/Windows 安装包，含前置校验、测试、构建、产物验证
metadata:
  requires:
    bins: ["cargo", "node", "npm"]
  cliHelp: "npm run tauri build"
---

# 一键部署技能

> 生成 Mac（.dmg）和 Windows（.msi/.exe）安装包。

## 流程

1. 前置校验
2. 运行测试
3. 构建安装包
4. 验证产物

---

## 1. 前置校验

确认环境就绪：

```bash
# Rust 工具链
rustc --version && cargo --version

# Node.js
node --version && npm --version

# Tauri CLI
npx @tauri-apps/cli --version
```

**需要：**
- Rust ≥ 1.77.2（Cargo.toml 中 `rust-version`）
- Node.js ≥ 18
- macOS：Xcode Command Line Tools（`xcode-select --install`）
- Windows：Visual Studio Build Tools + WebView2（Win10+ 内置）

---

## 2. 运行测试

构建前确保所有测试通过：

```bash
# Rust 后端测试
cd src-tauri && cargo test && cd ..

# TypeScript 前端测试
npx vitest run

# TypeScript 类型检查（对应 beforeBuildCommand 中的 tsc -b）
npx tsc -b
```

---

## 3. 构建安装包

### macOS（当前机器）

```bash
npm run tauri build
```

产物位于：`src-tauri/target/release/bundle/`
- `.dmg` — 拖拽安装
- `.app` — 直接运行的应用程序包

构建参数说明（`tauri.conf.json` 中已配置）：
- `bundle.targets: "all"`：生成所有支持格式
- `bundle.windows.wix.language: "zh-CN"`：Windows 中文安装包
- `bundle.windows.nsis.installMode: "currentUser"`：当前用户安装
- `beforeBuildCommand: "npm run build"`：自动执行前端构建

### Windows（交叉构建）

macOS 上交叉构建 Windows 包需要额外配置。推荐两种方式：

**方式 A：Windows 虚拟机/远程**
```powershell
# Windows 机器上
git clone <repo>
cd ffeel
npm install
npm run tauri build
```

**方式 B：macOS 构建 Windows 目标（需要 mingw-w64）**
```bash
# 安装 mingw-w64 交叉编译工具
brew install mingw-w64

# 添加 Windows 目标
rustup target add x86_64-pc-windows-gnu

# 构建（需额外配置 linker）
cargo build --target x86_64-pc-windows-gnu --release
```

> 实际推荐使用方式 A — Windows CI runner 或本地 Windows 环境。Tauri 2.0 Windows 构建依赖 WebView2 + Windows SDK，交叉编译不稳定。

---

## 4. 验证产物

### macOS

```bash
# 检查 .dmg 是否存在
ls -lh src-tauri/target/release/bundle/dmg/*.dmg

# 检查 .app bundle 结构
ls -lh src-tauri/target/release/bundle/macos/*.app

# 显示应用基本信息
mdls src-tauri/target/release/bundle/macos/*.app
```

### Windows

```bash
# 检查 .msi（WiX）
ls -lh src-tauri/target/release/bundle/msi/*.msi

# 检查 .exe（NSIS）
ls -lh src-tauri/target/release/bundle/nsis/*.exe
```

---

## 完整一键命令

```bash
# macOS 全流程：测试 → 构建
(cd src-tauri && cargo test) && npx vitest run && npm run tauri build
```

## 版本说明

- 版本号定义在 `src-tauri/tauri.conf.json` 的 `version` 字段（当前 `0.1.0`）
- 构建前确认版本号正确
- 产物命名格式：`{productName}_{version}_{arch}.{dmg|msi|exe}`
