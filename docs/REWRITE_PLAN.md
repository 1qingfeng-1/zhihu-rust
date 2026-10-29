# 知乎 Rust 客户端重写规划

> 目标仓库：`zhihu-rust`（已用 [1qingfeng-1/template](https://github.com/1qingfeng-1/template) 初始化，AGPL-3.0）
> 参考实现：[zly2006/zhihu-plus-plus](https://github.com/zly2006/zhihu-plus-plus)
> 技术栈：Rust + GPUI（`gpui-pre`）+ `gpui-kit` + `gpui-mobile`
> 文档日期：2026-10-27

---

## 0. 结论摘要

| 项目 | 建议 |
| --- | --- |
| 首发平台 | **Android（arm64-v8a，minSdk 26）+ Windows 桌面开发版**；iOS 后置（需要 macOS 构建机） |
| 首发范围 | **只读浏览 MVP**：首页/关注/热榜/日报、问题/回答/文章/想法、评论、搜索、用户页、收藏夹、历史、深浅色 |
| 第二阶段 | 登录（Cookie 导入 → 手机验证码 / 扫码）、点赞/关注/评论/收藏、通知、设置、屏蔽词 |
| 第三阶段 | 创作（回答/想法）、草稿、Markdown 编辑器、导出、TTS、视频、本地推荐 / NLP 过滤 |
| UI 方案 | `gpui-kit`（`gpui-component`）+ 自研移动端组件；不使用 Compose / Flutter |
| 数据层 | `reqwest` + `serde` + `rusqlite`（SQLite），沿用参考项目的 API 协议与 `zse96` 签名 |
| 工期量级 | 1–2 人：**M0 技术验证 2 周**，只读 MVP 约 **8–10 周**，核心功能对齐约 **4–6 个月**（不含本地推荐/NLP 等实验功能） |
| 最大风险 | `gpui-mobile` 仍是早期项目（0.1.0，2026-10 仍活跃）；中文 IME、WebView/视频/相机桥接、富文本 + LaTeX 渲染需要专项验证 |

**先做 2 周技术验证（M0），通过后再投入 UI 开发。** M0 不通过时的退路见 §12。

---

## 1. 目标与非目标

### 1.1 目标

1. 用 Rust 重写一个可日常使用的知乎客户端，移动端体验优先。
2. 保留参考项目的核心价值：无广告、屏蔽系统、隐私、本地推荐（可后置）。
3. 单一 Rust 代码库同时产出 Android / Windows / macOS / Linux，iOS 预留。
4. 头less 核心（API、状态、存储）与 UI 解耦，保证可测试性。
5. 二进制体积与启动速度优于原 Android 客户端。

### 1.2 非目标（首版）

- 不复刻参考项目的全部 80+ 页面与实验性功能。
- 不实现服务端爬虫、AIGC 投票后端、遥测。
- 不在首版支持 iOS（代码结构预留，但需要 Mac 才能验证）。
- 不保证与知乎官方 API 的长期兼容；签名/风控变化属于持续维护成本。

### 1.3 法律与许可

- 参考项目为 **AGPL-3.0**，其 `rs-zse-sign`、HTML 解析规则、接口常量等如被移植，本项目必须保持 **AGPL-3.0**（当前模板已设为 AGPL-3.0）。
- `gpui-mobile` 为 GPL-3.0 / AGPL-3.0 / Apache-2.0 三选一；`gpui-kit`、`gpui-component` 为 Apache-2.0。
- “知乎”是第三方商标。应用名、图标、包名应避免让用户误认为是官方客户端（建议名：`Zhihu Rust` / `知了`，包名 `dev.<owner>.zhihurs`）。

---

## 2. 参考项目分析

### 2.1 现状

参考项目已从早期 Flutter 迁移为 **Kotlin Multiplatform + Compose Multiplatform**：

- 约 1100 个文件，`shared/src` 约 422 个文件。
- 目标：Android（Full/Lite 两个变体）、JVM 桌面（Windows/Linux）、macOS、iOS 骨架。
- 共享层包结构：
  - `data/`：DTO、API 客户端、Feed/内容模型、日报、通知、收藏
  - `account/`：Cookie / 手机验证码 / 扫码登录、Token 刷新
  - `viewmodel/`：分页、Feed、评论、过滤、本地推荐、通知
  - `ui/`：页面与组件（`ui/` + `ui/article` + `ui/components` + `ui/subscreens`）
  - `markdown/`、`filter/`、`nlp/`、`reading/`、`updater/`
  - `shared-local-db/`：Room/SQLite，屏蔽词、屏蔽记录、本地内容缓存
- 已有 Rust 资产（可直接复用/移植）：
  - `rs-zse-sign`：**zse v4 加解密**（AGPL），已能生成知乎签名密文
  - `rs-hf-tokenizer`、`sentence_embeddings`：端侧 NLP 相关
  - `aigc-vote-server`：独立 Rust 服务，与本客户端无强依赖
- 测试资产丰富：`jvmTest/resources` 中有真实 API JSON、HTML 语料、公式语料，可作为 Rust 移植的 golden test 输入。

### 2.2 关键协议

- Web API 为主：`www.zhihu.com/api/v4/*`、`api.zhihu.com/*`、`daily.zhihu.com/api/4/*`。
- 签名：`x-zse-93` + `x-zse-96`（`2.0_` + `encrypt_zse_v4(md5(sign_source))`）；Token 刷新使用 `x-zse-83: 3_3.0` + zse v4 加密表单。
- 登录：
  - Cookie 导入（最容易，MVP 可用）
  - 手机验证码：`/api/v3/oauth/captcha/v2`、`/api/v3/oauth/sign_in`
  - 扫码：`/api/v3/account/api/login/qrcode`
  - 401 后调用 `/api/account/prod/token/refresh`
- 客户端需要维护 `d_c0`、`udid`、Cookie、UA 等指纹字段。

> 结论：**API 层是“移植”而不是“重新逆向”**。先把参考项目的 Kotlin 数据层逐文件映射为 Rust，能显著降低风险。

---

## 3. 技术栈与版本锁定

### 3.1 已验证的版本组合（关键）

`gpui-kit` 最新版与 `gpui-mobile` **不兼容**：

| 组件 | 版本 | 说明 |
| --- | --- | --- |
| `gpui-pre`（别名 `gpui`） | `=0.3.6` | Zed GPUI 的预发布包 |
| `gpui-kit` | **vendored** `vendor/gpui-kit`，上游 rev `13c716b`（v0.6.5） | 该 rev 依赖 `gpui-pre =0.3.6`；crates.io 上的 0.6.5 已被 yank |
| `gpui-mobile` | **vendored** `vendor/gpui-mobile`，上游 rev `5fd54f4` | crates.io 上的 0.1.0 只是占位包 |
| `gpui-pre-reqwest-client` | `=0.3.6` | 官方 HTTP 客户端适配 |
| Rust | 1.85+（edition 2024 可用） | 模板为 edition 2021，可先保持 |

> 注意：`gpui-kit 0.8.x` 依赖 `gpui-pre =0.3.10`，与 `gpui-mobile` 的 `=0.3.6` 直接冲突。M0 必须锁定上述三元组；若要升级，需要整体升级两个仓库并回归触摸/IME/渲染。
>
> **当前实现**：由于本机访问 GitHub 不稳定且 `gpui-kit 0.6.5` 在 crates.io 已 yank，两个依赖以源码快照形式放在 `vendor/`，用 path 依赖引入。详见 `vendor/README.md`。网络可用后可切回 git 依赖（保持同一 rev）。

`gpui-mobile` 官方示例本身就用 `gpui-kit`（`default-features = false, features = ["component"]`），证明组合可行。

### 3.2 依赖映射

| 参考项目（Kotlin） | Rust 选型 | 备注 |
| --- | --- | --- |
| Ktor Client | `reqwest` + `gpui-pre-reqwest-client` | rustls；按账号维护 Cookie |
| kotlinx.serialization | `serde` + `serde_json` | DTO 保留原始字段，缺字段用 `Option` |
| Room / SQLite | `rusqlite`（bundled）+ 手写迁移 | 跨 Android/桌面最稳；FTS5 做本地搜索 |
| Coil 图片加载 | 自研 `ImageCache`（`reqwest` + `image` + `gif`）+ 磁盘 LRU | GPUI 只负责解码后的 `Image` |
| Compose UI | `gpui` + `gpui-kit` / `gpui-component` | 移动端组件需自研 |
| Navigation Compose | 自研 typed Router | 栈式导航 + 深链 + 状态恢复 |
| Lifecycle/ViewModel | GPUI `Entity` + `Context` + 服务层 | 页面状态放 Entity，业务逻辑放 `zhihu-core` |
| jsoup / ksoup | `scraper` / `html5ever` | 知乎正文为 HTML，需要 AST 化 |
| markdown-renderer | `pulldown-cmark` / `comrak` + 自研排版 | 编辑器与阅读共用 |
| LaTeX 渲染 | 先文本回退；P1 评估 `latex2mathml` + SVG 或 KaTeX 移植 | **专项 spike** |
| Media3/ExoPlayer | Android 原生播放器（gpui-mobile `video_player` 桥） | 桌面后续接 `gstreamer`/`mpv` |
| Android WebView | gpui-mobile `webview` 桥 / JNI | 登录、风控、部分文章兜底 |
| ZXing 扫码 | gpui-mobile `camera` + ML Kit/ZXing（JNI） | 扫码登录后置 |
| TTS | Android `TextToSpeech` / 桌面系统 TTS | 通过 Platform trait |
| ONNX Runtime | `ort`（可选 feature） | 本地 NLP 过滤，最后阶段 |
| 日志 | `tracing` + `tracing-subscriber` + `tracing-android` | |
| 错误 | `thiserror`（库）+ `anyhow`（应用） | |

### 3.3 构建工具链

- Android：`cargo-ndk` + Gradle（`NativeActivity` / `GpuiInputActivity` 宿主），先只出 `arm64-v8a`。
- 桌面：`cargo run -p zhihu-desktop`；发布用 `cargo-dist` 或 `cargo-bundle`。
- 当前 Windows 机器已安装 Android SDK/NDK，但未装 `aarch64-linux-android` target 与 `cargo-ndk`，M0 补齐。

---

## 4. 代码架构

### 4.1 Workspace 布局

```text
zhihu-rust/
├─ Cargo.toml                 # workspace
├─ crates/
│  ├─ zhihu-types/            # DTO + 领域模型（纯数据，无 IO）
│  ├─ zhihu-api/              # HTTP、认证、签名、分页、接口封装
│  ├─ zhihu-content/          # HTML→AST、清洗、Markdown、公式、过滤器
│  ├─ zhihu-store/            # SQLite、迁移、Repository
│  ├─ zhihu-core/             # 服务层、AppState、Router、无 UI ViewModel
│  ├─ zhihu-ui/               # GPUI 组件、页面、主题、图片缓存
│  ├─ zhihu-platform/         # Platform trait + desktop/android 实现
│  └─ zhihu-recommend/        # 可选：本地推荐 / 向量过滤
├─ apps/
│  ├─ desktop/                # 桌面可执行文件
│  └─ mobile/                 # cdylib/staticlib；android_main / ios_main
├─ android/                   # Gradle 宿主工程（APK 壳、资源、清单）
├─ docs/
└─ xtask/                     # 构建、资源生成、schema 校验
```

依赖方向必须单向：

```text
types ← api ← core ← ui
          ↖ store ↙
content ←───────┘
platform ← ui / apps
```

`zhihu-core` 不依赖 GPUI；`zhihu-ui` 不直接发 HTTP。这样核心逻辑可以在普通 `cargo test` 中测试。

### 4.2 运行时与并发

- 网络：`reqwest` + 专用 Tokio runtime；请求结果通过 channel / `cx.update` 回到 UI。
- UI：GPUI 主线程；所有 UI 状态变更走 `Entity::update`。
- 分页：`Pager<T>` 状态机（`Idle/Loading/Loaded/Error/End`），每页请求可取消。
- 缓存：内存 LRU（图片/正文）+ SQLite（列表/详情/历史）。
- 取消：页面销毁时 drop `Task` / 取消令牌，避免回写已卸载页面。

### 4.3 服务层

```rust
pub struct AppServices {
    pub api: Arc<ZhihuApi>,          // 按账号切换 Cookie
    pub store: Arc<Store>,
    pub images: Arc<ImageService>,
    pub platform: Arc<dyn Platform>,
    pub router: Entity<Router>,      // 由 UI 层注入
    pub settings: Arc<Settings>,
}
```

通过 GPUI `Global` 注入；测试时构造内存版 `Store` 与 mock `Api`。

---

## 5. 数据与 API 层

### 5.1 分层

```text
HTTP (reqwest)
  → 签名/认证 middleware（Cookie、x-zse-93/96、401 refresh、重试）
  → DTO（serde，保留 raw JSON）
  → 领域模型（类型化：FeedItem、Answer、Article、Question、Pin、Comment、Person…）
  → ViewModel（展示字段、过滤状态、分页游标）
```

### 5.2 API 模块（按参考项目映射）

- `auth`：cookie 导入、验证码、扫码、token refresh、`/api/v4/me`
- `feed`：推荐（web/android/local/hybrid）、关注、热榜、日报
- `content`：问题、回答、文章、想法、视频 play_info
- `comment`：一级/二级评论、点赞、排序
- `people`：用户主页、关注/粉丝、创作、收藏夹
- `search`：搜索、热搜、建议、筛选
- `notification`：消息、已读、设置
- `collection`：收藏夹、内容、创建、收藏动作
- `interaction`：赞同/反对、关注、评论、分享
- `history`：在线历史、本地历史
- `write`：发布回答/想法、草稿、图片上传

首版只需实现 `auth(cookie)`、`feed`、`content`、`comment`、`people`、`search`、`collection`、`history`。

### 5.3 签名与风控

1. 直接移植 `rs-zse-sign`（cipher）为 `zhihu-api::sign`，保持 AGPL 头。
2. 把 `ZhihuFetchSignature.kt` 的 `sign_source` 拼接、MD5、zse93/83 头逻辑逐行移植。
3. 用参考项目 `jvmTest/resources/real-api` 的请求/响应做回归测试。
4. 设计 `Fingerprint` 抽象：`d_c0`、`udid`、UA、`x-requested-with` 等集中管理，便于风控变化时快速调整。

### 5.4 本地存储

SQLite 表（首版）：

| 表 | 用途 |
| --- | --- |
| `accounts` | 账号元数据、当前账号 |
| `cookies` | 加密后的 Cookie / token（密钥走系统安全存储） |
| `feed_cache` | 首页/关注列表快照（按账号 + 模式） |
| `content_cache` | 回答/文章/想法正文与元数据 |
| `read_history` | 本地阅读历史、进度 |
| `search_history` | 搜索历史 |
| `block_keywords` / `block_users` / `block_topics` / `blocked_records` | 屏蔽系统 |
| `collections` | 收藏夹缓存 |
| `drafts` | 草稿 |
| `settings` | KV 设置（主题、字号、推荐模式等） |
| `notification_state` | 红点、已读游标 |

- 迁移：`user_version` + 顺序 SQL 脚本，启动时事务执行。
- 全文搜索：`FTS5` 索引 `content_cache` 的标题/正文。
- 敏感数据：Android 用 Keystore/EncryptedSharedPreferences（JNI），桌面用 `keyring`；SQLite 中只存密文。

---

## 6. 内容渲染（最大工作量之一）

知乎正文是 HTML，参考项目同时有 Markdown 编辑器与 LaTeX 渲染。Rust 端需要自建富文本管线：

```text
HTML string
  → html5ever 解析
  → 白名单清洗（去掉脚本/广告/追踪/盐选推广节点）
  → 归一化 AST（Block/Inline：段落、标题、图片、链接、引用、列表、代码、表格、公式、分割线）
  → GPUI Element 树（虚拟化、懒加载图片）
```

要点：

- **图片**：`<figure>`/`<img>` 提取 `data-original`/`src`，统一走 `ImageService`（磁盘缓存 + 解码 + 尺寸占位 + 查看器）。
- **公式**：先把 `\(...\)` / `\[...\]` 节点保留为 `Math`，MVP 显示原文/简化排版；P1 单独 spike（`latex2mathml` + SVG，或引入 KaTeX WASM/离屏渲染）。
- **代码块**：`syntect`（`gpui-component` 已带 feature）。
- **视频**：识别 `video` 节点，调平台播放器覆盖层（不内嵌解码）。
- **链接**：站内链接走 Router，站外走系统浏览器。
- **表情**：复用参考项目 `misc/emojis` 资源（AGPL），映射 `[惊喜]` 等文本标记。
- **字体**：Android 系统 CJK 字体回退；桌面内置 Noto Sans CJK 子集。需要验证 `cosmic-text` 的中文换行、标点挤压与粗体回退。
- **选择/复制**：GPUI 文本选择能力需确认；首版至少支持整段复制与链接点击。

编辑器（P3）：纯文本编辑 + Markdown 工具栏 + 预览，图片走系统选择器上传。不要一开始做 WYSIWYG。

---

## 7. 平台层

### 7.1 抽象

```rust
pub trait Platform: Send + Sync {
    fn data_dir(&self) -> PathBuf;
    fn cache_dir(&self) -> PathBuf;
    fn secure_store(&self) -> &dyn SecureStore;
    fn open_url(&self, url: &str);
    fn share_text(&self, text: &str);
    fn copy_text(&self, text: &str);
    fn safe_area(&self) -> Insets;
    fn set_system_bars(&self, style: SystemBarStyle);
    fn toast(&self, msg: &str);
    fn pick_image(&self) -> Task<Option<PathBuf>>;
    fn scan_qr(&self) -> Task<Option<String>>;
    fn speak(&self, text: &str);
    fn stop_speaking(&self);
    fn play_video(&self, url: &str);
    fn open_webview(&self, url: &str) -> Task<WebViewResult>;
    fn on_back(&self, cb: Box<dyn Fn() -> bool>);  // 返回是否已消费
}
```

### 7.2 Android

- 宿主：`GpuiInputActivity`（支持 IME 组合输入）+ `NativeActivity` 清单配置。
- `.so` 由 `cargo-ndk` 产出，Gradle 负责打包、签名、资源、启动图、深链。
- JNI 桥：优先使用 `gpui-mobile` 已有能力（剪贴板、文件选择、相机、通知、视频、WebView、深链、分享、TTS 需自建）。
- 生命周期：`InitWindow`、前后台、配置变更（深色模式、字体缩放）、旋转。
- 返回键 / 预测性返回：Router 优先消费。
- 安全区：刘海、手势条、状态栏/导航栏颜色。

### 7.3 桌面

- GPUI 原生窗口；`gpui-kit` 组件可直接使用。
- 桌面布局：左侧栏 + 列表/详情双栏（`AdaptiveListDetail` 思路）。
- Windows 首发，用于开发调试；macOS/Linux 后续。

### 7.4 iOS（后置）

- `gpui-mobile` 提供 `ios_main` / staticlib；需要 Mac + Xcode + XcodeGen。
- 代码上只需保证 `apps/mobile` 的入口按 `cfg(target_os)` 分支，不阻塞 Android。

---

## 8. 路由、状态与交互

### 8.1 路由

```rust
pub enum Route {
    Home, Follow, Hot, Daily, Notifications, Profile,
    Question { id: u64 },
    Answer { question_id: u64, answer_id: u64 },
    Article { id: u64 },
    Pin { id: u64 },
    Person { token: String },
    Collection { id: u64 },
    Comments { target: TargetRef },
    Search { query: Option<String> },
    Settings, SettingsAppearance, SettingsReading, SettingsFilter, Login,
    // ...
}
```

- `Router` 持有 `Vec<Route>` + 每个 Tab 的独立栈。
- 深链解析：`https://www.zhihu.com/question/{q}/answer/{a}`、`/p/{article}`、`/pin/{id}`、`/people/{token}`、`zhihu://...`。
- 启动时恢复上次 Tab 与栈（可配置初始页）。
- Android 返回键、手势返回、桌面 Alt+Left 统一映射到 `router.back()`。

### 8.2 移动端交互

- 底部导航：首页 / 关注 / 热榜 / 消息 / 我的（可配置）。
- 顶部 Tab + 横滑切换；下拉刷新；触底加载；滚动隐藏工具栏。
- 回答切换：上下滑 / 左右滑（参考项目两种模式），支持“下一个回答”。
- 双击点赞 / 双击打开评论（可配置）。
- 底部 Sheet：分享、评论、收藏、更多操作。
- 沉浸式阅读：隐藏干扰元素、字号/行距/段距调节、阅读进度条。
- 离线：列表与正文缓存，弱网提示。

### 8.3 主题

- 浅色/深色/跟随系统；自定义主色。
- 设计 token：背景/表面/文字/次要文字/分割线/主色/危险色/圆角/间距/字号。
- 中文排版：正文 16–17sp，行高 1.6–1.8，段距可调。

---

## 9. 里程碑

> 估算按 1–2 名熟悉 Rust 的开发者，包含测试与联调，不含不可控的 API 风控变更。

### M0：技术验证（1–2 周）— Go / No-Go

> **状态：已完成（2026-10-27），结论 GO。** 详见 [M0_REPORT.md](M0_REPORT.md)。

交付：

1. workspace 骨架（types/api/store/content/core/ui/platform/apps）。
2. 在 Windows 桌面跑通 `gpui-kit` 中文列表页。
3. 在 Android 真机/模拟器跑通 `gpui-mobile` hello world。
4. 验证清单：
   - 中文字体、换行、粗体/斜体
   - 软键盘中文输入、光标、选择、复制粘贴
   - 触摸滚动/惯性、长列表性能（1000 项）
   - 图片异步加载 + 磁盘缓存
   - HTTPS 请求知乎公开接口（带签名）
   - SQLite 读写与迁移
   - 安全区、深色模式、返回键
5. 输出《M0 验证报告》：阻塞项、补丁、是否继续。

**通过标准**：上述 7 项中至少 6 项无阻塞；IME 与滚动必须有可用方案。

### M1：基础设施（2–3 周）

> **状态：进行中（2026-10-27）。** 已完成签名、HTTP 客户端、SQLite、Cookie 导入、热榜/日报真实数据；推荐流与详情待有效 Cookie 验证。详见 [M1_REPORT.md](M1_REPORT.md)。

- `zhihu-types` / `zhihu-api`：Cookie 登录、签名、Feed/内容/评论 DTO。
- `zhihu-store`：首版 schema、设置、缓存、历史。
- `zhihu-core`：AppState、Router、Pager、错误模型、日志。
- `zhihu-ui`：主题、基础组件（AppBar、Tab、列表、卡片、加载/错误/空态）。
- 首页推荐流（只读）+ 回答正文（纯文本/图片）跑通。
- CI：fmt / clippy / test / Android 构建。

### M2：只读 MVP（4–5 周）

> **状态：核心完成（2026-10-27）。** 已完成 HTML→AST 富文本、详情页路由、长文滚动、远程图片、返回键；真实正文与搜索待 Cookie/后续。详见 [M2_REPORT.md](M2_REPORT.md)。

页面：首页、关注、热榜、日报、问题、回答、文章、想法、评论、用户主页、收藏夹、搜索、历史、设置（外观/阅读）。

能力：富文本渲染、图片查看器、视频入口、下拉刷新、分页、深链、离线缓存、深浅色。

**验收**：不登录即可浏览主要公开内容；连续滚动 30 分钟无崩溃/明显卡顿；冷启动 < 2s（中端机）。

### M3：账号与互动（3–4 周）

> **状态：已完成并通过真机验收（2026-10-28）。** 账号校验、点赞/关注、搜索建议、关键词屏蔽、评论、通知中心、Binance 风格主界面均已验证；真实 Cookie 登录态、手机号/扫码登录、评论回复与分页待后续。详见 [M3_REPORT.md](M3_REPORT.md)。

- Cookie 导入 → 手机验证码登录 → 扫码登录。
- 点赞/反对、关注、评论/回复、收藏、分享。
- 通知中心、红点、已读。
- 屏蔽词（正则）、屏蔽用户/话题、屏蔽记录。
- 多账号切换、Token 自动刷新。

**验收**：登录态可完成“刷首页 → 看回答 → 点赞 → 评论 → 收到通知”的闭环。

### M4：创作与进阶（4–5 周）

- 写回答/想法、草稿、图片上传、Markdown 编辑与预览。
- 导出 Markdown/HTML/PDF、收藏夹导出。
- TTS 朗读、阅读进度、沉浸式阅读。
- 公式渲染（若 M0 未解决则在此专项）。
- 视频播放完善、GIF、多图滑动。

### M5：差异化与发布（3–4 周）

- 本地推荐 / 混合推荐模式（依赖参考项目的爬虫/权重设计，需单独评估）。
- AIGC 标记与投票（可选后端）。
- NLP 语义屏蔽（`ort` + tokenizer，可选 feature，控制包体）。
- 桌面端双栏布局、键盘快捷键。
- 无障碍、性能剖析、崩溃上报（本地）、发布流水线、签名与商店素材。

---

## 10. 功能优先级矩阵

| 功能 | MVP | P1 | P2 | 说明 |
| --- | :---: | :---: | :---: | --- |
| 首页推荐 / 关注 / 热榜 / 日报 | ✅ | | | 公开接口 |
| 问答 / 文章 / 想法 / 评论 | ✅ | | | 富文本是重点 |
| 搜索 / 用户页 / 收藏夹 / 历史 | ✅ | | | |
| 图片查看 / 深链 / 主题 | ✅ | | | |
| 登录（Cookie / 手机 / 扫码） | | ✅ | | Cookie 先行 |
| 点赞 / 关注 / 评论 / 收藏 | | ✅ | | |
| 通知 / 屏蔽系统 | | ✅ | | |
| 写作 / 草稿 / 导出 | | | ✅ | |
| TTS / 视频 / 公式 | | ✅ | | 公式风险高 |
| 本地推荐 / 混合模式 | | | ✅ | 依赖爬虫 |
| NLP 语义过滤 | | | ✅ | 可选 feature |
| AIGC 投票 | | | ✅ | 依赖服务端 |
| 多账号 / 数据迁移 | | ✅ | | |
| 桌面双栏 / 快捷键 | | | ✅ | |

---

## 11. 测试与 CI

### 11.1 测试金字塔

1. **单元测试**：签名（与 Kotlin/参考输出对齐）、URL 构造、AST 转换、分页状态机、过滤器。
2. **Golden 测试**：复用参考项目 `jvmTest/resources` 的 JSON/HTML/公式语料；`insta` 快照。
3. **API 契约测试**：录制响应 → mock server（`wiremock`），不访问真实知乎。
4. **UI 测试**：`gpui-kit` 的 `test-support`（headless 渲染、输入模拟、快照）。
5. **设备测试**：Android 真机/模拟器 smoke（启动、滚动、登录页、返回键、深色模式），截图对比。
6. **性能基准**：`criterion`（解析/渲染/AST），设备端帧率与内存。

### 11.2 CI

- `cargo fmt --check`、`cargo clippy -D warnings`、`cargo nextest`。
- `cargo deny`（许可证/漏洞）、`typos`。
- Android job：安装 NDK + `cargo-ndk`，构建 `arm64-v8a`，Gradle `assembleDebug`，上传 APK。
- 桌面 job：Windows + Linux 构建。
- iOS job：macOS runner（可后置，手动触发）。
- 真实 API 测试仅在 `live` feature + 手动触发，避免 CI 被封。

模板已包含 `build.yml`、`deny.toml`、`typos`、`cliff.toml`，在其基础上扩展即可。

---

## 12. 风险与应对

| 风险 | 概率/影响 | 应对 |
| --- | --- | --- |
| `gpui-mobile` 早期项目，API 不稳定 | 高 / 高 | 锁定 git rev；fork 后打补丁；M0 先验证；保留退路 |
| 版本冲突：`gpui-kit` 0.8 (gpui-pre 0.3.10) vs `gpui-mobile` (=0.3.6) | 确定 / 高 | 使用已验证组合：gpui-pre 0.3.6 + gpui-kit rev `13c716b` + gpui-mobile HEAD；升级需整体回归 |
| 中文 IME / 候选词 / 选择 | 中 / 高 | M0 专项；必要时自建 `InputConnection` 桥；参考 gpui-mobile 的 `GpuiInputActivity` |
| 富文本 + LaTeX 渲染工作量大 | 高 / 中 | AST 先行；公式 MVP 文本回退；P1 专项 spike |
| WebView / 视频 / 相机 / TTS 桥接不完整 | 中 / 高 | 平台 trait 隔离；缺什么补什么；WebView 用于登录兜底 |
| 知乎风控 / 签名算法变化 | 中 / 高 | 签名与指纹集中管理；错误可观测；保留 WebView 兜底；不承诺永久可用 |
| 列表性能 / 内存（大量图片） | 中 / 中 | 虚拟列表、图片尺寸解码、磁盘 LRU、取消不可见请求 |
| 无障碍（GPUI 支持有限） | 高 / 中 | 首版保证可点击区域与字号；后续补 accessibility tree |
| iOS 无法在 Windows 验证 | 确定 / 中 | iOS 后置；入口与 platform 层预留 |
| 许可证 / 商标 | 中 / 中 | 全项目 AGPL-3.0；应用名与官方区分；保留版权头 |
| 工期被“功能对齐”拖长 | 高 / 高 | 严格按 MVP 矩阵；每周可运行版本；不做 WYSIWYG / 本地爬虫等大坑 |

### M0 不通过时的退路

1. **换 UI 栈**：`gpui` 桌面 + `winit`/`android-activity` 自绘（工作量大）；或 `slint`（移动端成熟度更高，但生态与 GPUI 不同）。
2. **Kotlin UI + Rust 核心**：保留现有 Compose UI，通过 JNI/UniFFI 调用 Rust 核心（最稳妥，但不是“纯 Rust 重写”）。
3. **先桌面后移动**：把 `gpui-kit` 桌面版做完整，移动端等待 `gpui-mobile` 成熟。

---

## 13. 待确认决策

1. **iOS 是否在首年目标内？** 若需要，必须准备 macOS 构建机与开发者账号。
2. **首版是否接受“只能浏览、不能登录互动”？** 建议接受，可把 MVP 提前 4 周。
3. **是否保留 AGPL-3.0？** 若移植参考项目的签名/解析代码，事实上必须保留。
4. **应用命名与包名**：避免与官方混淆。
5. **本地推荐 / NLP 是否在路线图内？** 建议 P2，独立 feature 控制包体。

---

## 14. 首批任务（可直接开 issue / PR）

1. `chore: workspace 骨架与版本锁定`（gpui-pre 0.3.6 / gpui-kit rev / gpui-mobile rev）
2. `spike: Android hello world + 中文 IME + 滚动`（M0 核心）
3. `feat(api): 移植 zse96 签名与 Cookie 客户端`
4. `feat(api): Feed/Answer/Article/Question DTO 与分页`
5. `feat(store): SQLite schema、迁移、设置与缓存`
6. `feat(content): HTML → AST 清洗与基础渲染`
7. `feat(ui): 主题 token 与基础组件`
8. `feat(ui): 首页信息流（只读）`
9. `feat(ui): 回答/文章详情 + 图片查看器`
10. `ci: Android 构建流水线 + 设备 smoke test`

---

## 15. 目录与命名约定

- crate 前缀统一 `zhihu-`；应用二进制 `zhihu-desktop`、`zhihu-mobile`。
- 领域模型不使用 API DTO 的可空字段；DTO 单独放 `zhihu-types::dto`。
- 所有移植自参考项目的文件保留 AGPL 版权头与来源说明。
- 错误：库层 `thiserror`，应用层 `anyhow`；UI 只展示用户可读错误。
- 日志：禁止记录 Cookie/Token/正文；请求日志脱敏。
- 每个 PR 必须：`cargo fmt`、`clippy`、相关测试、截图（UI 变更）。

---

## 附录 A：参考项目 vs 本项目

| 维度 | zhihu-plus-plus | zhihu-rust |
| --- | --- | --- |
| 语言 | Kotlin Multiplatform | Rust |
| UI | Compose Multiplatform | GPUI + gpui-kit |
| 移动端 | Android 原生（成熟） | gpui-mobile（早期） |
| 桌面 | JVM Compose（实验） | GPUI 原生 |
| 核心复用 | — | 移植 API/签名/解析/测试语料 |
| 体积 | Lite APK < 4 MB | 目标 < 20 MB（含字体/图标） |
| 许可证 | AGPL-3.0 | AGPL-3.0 |

## 附录 B：关键外部参考

- 参考项目：<https://github.com/zly2006/zhihu-plus-plus>
- GPUI：<https://github.com/zed-industries/zed/tree/main/crates/gpui>
- gpui-kit：<https://github.com/longbridge/gpui-kit>
- gpui-mobile：<https://github.com/itsbalamurali/gpui-mobile>
- gpui-kit 文档：<https://gpui-kit.com>
