# zhihu-rust

用 Rust + GPUI 重写的知乎客户端（Android 优先，桌面同步）。

参考实现：[zly2006/zhihu-plus-plus](https://github.com/zly2006/zhihu-plus-plus)（Kotlin Multiplatform / Compose）。
本仓库由 [1qingfeng-1/template](https://github.com/1qingfeng-1/template) 初始化。

## 状态

**M0 已完成（GO）**：桌面与 Android 真机可运行，中文渲染、虚拟列表、触摸滚动、主题切换、软键盘输入、安全区均已验证。

**M1 已完成**：API 签名、HTTP 客户端、SQLite、Cookie 导入、真实热榜/日报。

**M2 已完成**：HTML → AST 富文本渲染、详情页路由、长文滚动、远程图片加载、返回键导航。

**M3 已完成（2026-10-28 真机验收通过）**：账号校验、点赞/关注、搜索建议、关键词屏蔽、评论、通知中心、Binance 风格主界面（深色 + 黄色强调 + 底部导航）均已验证。

**M4 功能完成（不含写作/草稿/导出）**：评论分页/子评论、完整搜索、用户+话题屏蔽、阅读设置与默认 Tab、图片磁盘缓存、视频播放、TTS、扫码/手机号登录、通知分页、深链与分享均已实现；真机平台门禁待设备接入。

**M5 进行中（2026-10-28）**：桌面自适应双栏与键盘快捷键、本地崩溃报告、运行诊断、发布流水线（签名 + CI + 图标）；M4 真机门禁仍待设备接入。

- 桌面宽窗口（≥900px）左侧栏 + 右侧内容；`Ctrl+K` 搜索、`Ctrl+1..5` 切 Tab、
  `Ctrl+R` 刷新、`Ctrl+D` 主题、`Esc` 返回。
- 本地崩溃日志（不包含 Cookie/正文、不上传）与运行诊断页。
- 可选本地推荐：关闭/混合/本地三种排序，主题/作者/标题特征，默认关闭、纯本机。
- 富文本排版：标准 / 宽松 / 紧凑三套预设，统一栏宽、行高、引用、代码与图片样式。
- 推荐流双列瀑布流、无限滚动、任意位置下拉刷新；条目右滑可“不感兴趣/屏蔽作者/屏蔽话题”。
- 详情页显示作者与发布/编辑时间，支持赞同/反对；评论支持默认/最新/最早/最多赞同排序。
- 用户头像与用户主页（资料、关注/粉丝、回答列表、关注/取关），从 Feed/详情/评论/通知进入。
- 评论支持赞同/反对/喜欢（带图标）、盖楼缩进与回复颜色标签、0 回复隐藏入口；回答与评论可复制 Markdown。
- TTS 设置（语速/音调/音量/音色）与歌词式同步高亮朗读：当前句黄色、自动滚动、点击跳读、底部播放条。
- 详情“选择”模式可拖选复制，松开左键自动复制并弹窗；代码块/公式块一键复制。
- 问题页查看全部回答（默认/最新/最多赞同）；日报在应用内阅读。
- 侧栏整体可折叠为图标栏，“浏览/我的”分组也可单独折叠。
- 评论返回正文消失的问题已修复。

- 重写规划：[docs/REWRITE_PLAN.md](docs/REWRITE_PLAN.md)
- M0 报告：[docs/M0_REPORT.md](docs/M0_REPORT.md)
- M1 报告：[docs/M1_REPORT.md](docs/M1_REPORT.md)
- M2 报告：[docs/M2_REPORT.md](docs/M2_REPORT.md)
- M3 报告：[docs/M3_REPORT.md](docs/M3_REPORT.md)
- M4 计划：[docs/M4_PLAN.md](docs/M4_PLAN.md)
- M4 进展：[docs/M4_REPORT.md](docs/M4_REPORT.md)
- M5 计划：[docs/M5_PLAN.md](docs/M5_PLAN.md)
- M5 进展：[docs/M5_REPORT.md](docs/M5_REPORT.md)
- P2 评估（本地推荐 / NLP）：[docs/M5_P2_EVALUATION.md](docs/M5_P2_EVALUATION.md)
- 发布与签名：[docs/RELEASE.md](docs/RELEASE.md)

| Binance 风格主页 | 点赞 / 关注 / 评论 |
| --- | --- |
| ![Home](docs/m3-android-home.png) | ![Actions](docs/m3-android-actions.png) |

> M4 新增：视频播放、TTS 朗读、手机/扫码登录（含风控引导）、图片磁盘缓存与全屏查看、
> 阅读历史、深链分享、Capture 阶段滑动切 Tab。
> 详见 [docs/M4_REPORT.md](docs/M4_REPORT.md)。

| 桌面双栏 | 评论返回后正文保留 |
| --- | --- |
| ![Desktop](docs/m5-desktop-home.png) | ![Comment back](docs/m5-desktop-comment-back.png) |

| 运行诊断 | 本地崩溃日志 |
| --- | --- |
| ![Diagnostics](docs/m5-desktop-diagnostics.png) | ![Crash](docs/m5-desktop-crash.png) |

> M5 新增：桌面双栏与快捷键、本地崩溃报告、运行诊断、发布流水线与应用图标、
> 富文本排版三套预设（标准/宽松/紧凑）。
> 详见 [docs/M5_REPORT.md](docs/M5_REPORT.md)。

## 技术栈（已验证组合）

| 组件 | 版本 |
| --- | --- |
| Rust | 1.85+ |
| `gpui`（`gpui-pre`） | `=0.3.6` |
| `gpui-kit` | 0.6.5（`vendor/gpui-kit`，上游 `13c716b`） |
| `gpui-mobile` | 0.1.0（`vendor/gpui-mobile`，上游 `5fd54f4`） |
| `ratex-gpui` | 0.4.1（独立公式排版，KaTeX 解析器移植） |
| Gradle / AGP / JDK | 9.4.1 / 9.1.0 / 21 |

> `gpui-kit` 最新 0.8.x 依赖 `gpui-pre =0.3.10`，与 `gpui-mobile` 的 `=0.3.6` 冲突。升级必须两边一起回归。
> 依赖获取说明见 [vendor/README.md](vendor/README.md)。

## 目录

```text
crates/zhihu-types   领域类型
crates/zhihu-api     HTTP / 签名 / 认证（M1）
crates/zhihu-content HTML → AST / Markdown / 公式（M1）
crates/zhihu-store   SQLite / 缓存 / 屏蔽词（M1）
crates/zhihu-core    无 UI 的服务层与状态
crates/zhihu-ui      GPUI 页面与组件
crates/zhihu-platform 平台能力抽象
apps/desktop         桌面入口
apps/mobile          Android / iOS 入口
android/             Gradle APK 宿主
vendor/              固定版本的上游源码快照
```

## 开发

```bash
cargo check
cargo test
cargo run -p zhihu-desktop
```

### Windows PC 版

```bash
cargo build --release -p zhihu-desktop
```

产物：`target/release/zhihu-desktop.exe`（单文件，无需安装）。
数据目录：`%APPDATA%\zhihu-rust`；可用 `ZHIHU_DATA_DIR` 覆盖。
本仓库同时提供打包好的目录与 zip：

- `dist/zhihu-desktop-windows-x64/zhihu-desktop.exe`
- `dist/zhihu-desktop-windows-x64.zip`

使用说明见包内 `README.txt`。桌面宽窗口自动切换双栏；快捷键见包内说明。

### Android

前置：Rust `aarch64-linux-android` target、`cargo-ndk`、Android SDK 36、NDK 28、JDK 17+。

```bash
export ANDROID_HOME="C:/Users/13736/AppData/Local/Android/Sdk"
export ANDROID_NDK_HOME="$ANDROID_HOME/ndk/28.2.13676358"
export JAVA_HOME="C:/Program Files/AndroidStudio/jbr"

# 1. 交叉编译
cargo ndk -t arm64-v8a -P 26 -o android/app/src/main/jniLibs build -p zhihu-mobile

# 2. 去符号 + 删除依赖 crate 的副产物
"$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/windows-x86_64/bin/llvm-strip.exe" \
  --strip-all android/app/src/main/jniLibs/arm64-v8a/libzhihu_mobile.so
rm -f android/app/src/main/jniLibs/arm64-v8a/libgpui_mobile.so

# 3. 打包安装
(cd android && cmd //c ".\\gradlew.bat assembleDebug --no-daemon")
adb install -r android/app/build/outputs/apk/debug/app-debug.apk
adb shell am start -n dev.zhihurs.app/dev.gpui.mobile.GpuiActivity
```

> `-P 26` 必须与 `minSdk 26` 一致：NDK 只在 API 26+ 的 sysroot 中提供 `libnativewindow.so`。
> Gradle 使用阿里云 Maven 镜像与腾讯 Gradle 镜像（`android/settings.gradle.kts`、`gradle-wrapper.properties`）。

## 许可证

AGPL-3.0。参考项目的 API 常量、签名实现与测试语料移植后仍受其 AGPL 许可约束。
“知乎”为第三方商标，本项目与知乎官方无关。
