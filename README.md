# Kvmddc

跨平台多主机显示器切换与键鼠共享桌面工具（macOS + Windows），基于 **Rust + Tauri v2 + Three.js** 构建。

## 核心特性

- **硬件输入源切换与状态回读**：macOS 原生 `IOAVService` 与 Windows `DxVA2` 硬件通道直接读写显示器输入源、亮度、音量与色温预设，内置状态机去抖与冷却保护。
- **跨系统键鼠共享守护**：托管编排 Deskflow 服务端与客户端，自动生成自签名 TLS 身份指纹、双向屏幕拓扑连接、跨系统修饰键对齐（`⌘` / `Ctrl`），并按两端逻辑与物理分辨率比例自动补偿跨屏鼠标移动速度。
- **单屏与盒盖自动锁边**：实时感知笔记本开合盖状态与主显示器当前输入源，单屏工作时自动锁定外侧屏幕边缘，防止光标滑入不可见桌面。
- **纯局域网控制面**：主机间通过 Bearer 鉴权纯局域网控制通道自动发现本机网卡地址、同步配对指纹、共享图像剪贴板与校验式文件互传，拒绝绑定公网或虚拟组网接口。
- **三维桌面主舞台与菜单栏面板**：内置三维工作室桌面视图与轻量菜单栏控制面板，支持全局快捷键切换、亮度音量调节与不断电待机唤醒。

## 目录结构

- `crates/kvmddc-core`：配置模型、SQLite 切屏状态机、Deskflow 配置生成、局域网控制协议与魔术包唤醒
- `crates/kvmddc-platform`：macOS（`IOKit` / `CoreGraphics` / `AppKit`）与 Windows（`Win32` / `DxVA2` / `CCD`）原生硬件与显示适配层
- `src-tauri`：Tauri v2 守护运行时、多窗口与托盘管理、前端命令桥接
- `ui`：主舞台三维视图、菜单栏控制面板、边缘选择器与屏幕指示条前端源码

## 构建与运行

前置依赖：
- Rust 工具链（`1.78+`）
- Deskflow（macOS 安装于 `/Applications/Deskflow.app`，Windows 安装于默认路径或在配置中指定）

```bash
# 运行全部单元测试
cargo test --workspace

# 编译正式版可执行文件
cargo build --release --bin kvmddc
```

首次启动会在用户本地目录 `~/.kvmddc/` 下自动生成权限受限的本地配置与自签名证书（仅保存在本机，不纳入版本控制）。

## 开源协议

MIT License
