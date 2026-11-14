# 更新日志 (Changelog)

本项目所有面向功能、交互与稳定性演进的版本变更均记录于此。
版本号遵循 [语义化版本 2.0.0](https://semver.org/lang/zh-CN/)，格式参考 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)。

## [0.3.0] - 2026-11-14

### 新增 (Added)

- **三主机桌面集群与修订号状态同步**：完整接入 `MacBook Pro`、`Windows 台式机` 与 `Mac Studio` 三机拓扑，新增带单调修订号（`shared_settings_rev`）的集群配置同步机制，在全网对等节点间自动对齐键鼠直连源、主 DDC 控制节点、边缘切屏策略、跨系统修饰键映射与内屏高度比例。
- **外设热插拔自动接管键鼠源**：基于 macOS `IOHIDManager` 与 Windows `RawInput` 实现物理外接键鼠热插拔追踪器（过滤内置触控板、虚拟 HID 与扩展坞空壳节点，内置防抖与重连宽限窗口），支持外设换插后自动切换全局 Deskflow 键鼠服务端主机。
- **多主机分辨率与高刷模式管理**：新增跨平台显示模式枚举与切换能力（macOS CoreGraphics HiDPI 模式与 Windows CCD/GDI 接口），支持在控制面直接切换各主机的逻辑分辨率与刷新率（优先匹配 `75Hz` 高刷 HiDPI 档位），并自动记忆用户在系统设置中主动调整的有效桌面分辨率。
- **三维舞台 Mac Studio 硬件建模**：在主舞台三维桌面视图中新增 `Mac Studio` 铝合金机身模型、状态卡片与三机实时信号指示。
- **macOS Deskflow 原生低延迟垫片（`tis_mainthread_shim.m`）**：通过 `DYLD_INSERT_LIBRARIES` 为托管的 `deskflow-core` 注入三项底层增强：
  - 将后台线程调用的 `HIToolbox / TIS` 键盘布局接口同步派发至主队列，消除 macOS 26 下后台线程查询输入法导致的崩溃；
  - 接管套接字创建与连接流程，强制启用 `TCP_NODELAY` 并标记 `SO_NET_SERVICE_TYPE = NET_SERVICE_TYPE_VO`（Wi-Fi WMM 语音级高优先级队列），同时声明 `NSActivityLatencyCritical` 与 `QOS_CLASS_USER_INTERACTIVE` 防止系统定时器合并；
  - 支持内屏光标物理软围栏（`confine_builtin.flag`）与盒盖无屏状态下的合成桌面几何兜底。

### 优化 (Changed)

- **Ctrl 边缘选机浮窗交互重做**：
  - 紧贴触发侧屏幕边缘弹出，支持 HiDPI 多显示器坐标夹紧与首击直达响应（`acceptFirstMouse`）；
  - 新增鼠标滚轮 / 触控板上下滚动选机与指针悬停高亮选择，局部更新选中态而不打断滚轮焦点；
  - 支持松开 `Ctrl` 键、左键单击或 macOS 下按住 `Ctrl` 左键点击任意设备立即丝滑切屏（单次触发防重入）；
  - 支持点击浮窗外任意桌面空白处、卡片外留白、`Esc` 键或失焦取消本次切换，并在松开本轮 `Ctrl` 前抑制重复弹出。
- **跨机键鼠流延迟与移动比例调优**：
  - 默认边缘切屏驻留延迟由 `250ms` 收紧至 `120ms`，Deskflow 拓扑热重载窗口由 `600ms` 压缩至 `80ms`；
  - Windows 端 `deskflow-core.exe` 启动优先级提升至 `ABOVE_NORMAL_PRIORITY_CLASS`；
  - 跨机鼠标速度补偿改为按操作系统配对精细计算：`macOS <-> macOS`（如 `MacBook` 与 `Mac Studio`）保持原生 `1:1` 指针手感，`macOS <-> Windows` 按两端逻辑宽度与物理像素密度自动换算比例。
- **纯局域网多网卡优选策略**：macOS 端在扩展坞瞬态网卡与主网卡并存时优先锁定主接口（`en0`），全平台严格拦截 `100.64.0.0/10` 虚拟组网地址进入控制面或 mDNS 候选列表。

### 修复 (Fixed)

- **修复盒盖笔记本与三机切换时的键鼠焦点回弹死锁**：
  - `/v1/switch` 控制协议新增 `sync_input_focus` 原子语义，远程切屏时同步迁移 Deskflow 键鼠焦点；
  - 修正盒盖 `MacBook`（无内屏且不持有 27" 主屏画面）的可达性判定，避免误判为可见屏幕；
  - 区分用户主动切回与 Deskflow 异常重置，修复从 `Mac Studio` 主动切往其他主机时被逃逸保护逻辑拉回 `Mac Studio` 的问题；
  - 当原焦点所在笔记本处于盒盖状态且用户通过显示器物理按键切走画面时，自动将键鼠焦点跟随 `0x60` 当前输入源迁移至目标主机。
- **修复开盖笔记本外接副屏未显示时的不可见扩展桌面陷阱**：当 `MacBook` USB-C 视频线保持握手但 27" 主屏切往其他主机时，自动启用内屏边缘软围栏，防止光标滑入不可见的扩展桌面区域。
- **修复显示器休眠重连时的断联假象分辨率污染**：自动过滤 `640x480` 至 `1024x768` 的临时回退分辨率模式，避免覆盖用户已保存的正式桌面档位。

## [0.2.0] - 2026-11-12

### 新增 (Added)

- **跨平台 DDC/CI 硬件控制引擎**：基于 macOS `IOAVService` 与 Windows `DxVA2` 实现显示器输入源（`0x60`）、亮度（`0x10`）、音量（`0x62`）与色温预设回读及控制，内置 SQLite 持久化切屏状态机与写入冷却保护。
- **Deskflow 托管编排与单屏锁边**：自动生成自签名 TLS 证书指纹、服务端屏幕拓扑配置、跨系统修饰键对齐与盒盖单屏锁边守护。
- **纯局域网安全控制面与协同工具**：提供 Bearer 鉴权 HTTP 控制接口、对等节点自动发现配对、跨机图片剪贴板同步、SHA-256 校验文件互传与魔术包（WoL）远程唤醒。
- **三维工作室界面与菜单栏控制中心**：提供基于 Three.js 的三维桌面舞台、轻量托盘控制面板、屏幕顶部状态指示条与全局热键切屏。

[0.3.0]: https://github.com/yanmoais/Kvmddc/compare/626ad8f...v0.3.0
[0.2.0]: https://github.com/yanmoais/Kvmddc/commits/626ad8f
