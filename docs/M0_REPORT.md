# M0 技术验证报告

> 日期：2026-10-27
> 结论：**GO** — `gpui-pre` + `gpui-kit` + `gpui-mobile` 组合可在 Windows 桌面与 Android 真机运行，中文排版、触摸滚动、主题切换与软键盘输入均通过验证。

---

## 1. 验证环境

| 项 | 值 |
| --- | --- |
| 主机 | Windows 11 Pro 10.0.26300，x64 |
| Rust | 1.98.1 (stable-x86_64-pc-windows-msvc) |
| Android 设备 | 真机，arm64-v8a，Android 16 / API 36，1200×2670 @ 520dpi |
| Android SDK | `C:/Users/13736/AppData/Local/Android/Sdk`，platform 36 |
| Android NDK | 28.2.13676358 |
| JDK | Android Studio JBR 21.0.10 |
| Gradle / AGP | 9.4.1 / 9.1.0 |
| `gpui-pre` | 0.3.6 |
| `gpui-kit` | 0.6.5（vendored，rev `13c716b`） |
| `gpui-mobile` | 0.1.0（vendored，rev `5fd54f4`） |

---

## 2. 已验证项

### 2.1 桌面（Windows）

- `cargo build -p zhihu-desktop` 通过，窗口正常打开。
- 中文标题、卡片标题/摘要、标签（回答/文章/想法）正确渲染。
- `uniform_list` 虚拟列表正常显示。
- 截图：`docs/m0-desktop.png`。

### 2.2 Android 真机

| 项目 | 结果 | 证据 |
| --- | --- | --- |
| Rust → `aarch64-linux-android` 交叉编译 | ✅ | `cargo ndk -P 26` |
| Gradle 打包 / 安装 / 启动 | ✅ | `app-debug.apk`，`am start` |
| 中文渲染 | ✅ | 系统 `NotoSansCJK-Regular.ttc`，`docs/m0-android.png` |
| 虚拟列表 + 触摸滑动 | ✅ | `adb shell input swipe`，`docs/m0-android-scroll.png` |
| Tab 点击 | ✅ | 点击“推荐”后高亮切换 |
| 深色/浅色切换 | ✅ | 点击“深色”，`docs/m0-android-dark.png` |
| 状态栏 / 手势条安全区 | ✅ | 自定义 `WindowInsets` 查询，内容不再被系统栏遮挡 |
| 软键盘弹出 + 文本输入 | ✅ | `dumpsys input_method` 显示 `mInputShown=true`；输入 “hello” 成功，`docs/m0-android-ime*.png` |
| 中文 IME 连接 | ✅（连接层） | IME 处于中文模式，`GpuiInputActivity$InputProxy` 为当前 InputConnection |

> 说明：`adb shell input text` 只能注入 ASCII；“中文候选词上屏”属于系统 IME 行为，连接层已验证，最终需人工在设备上确认候选词交互。

---

## 3. 关键问题与解决

### 3.1 依赖获取

- GitHub 在本机不稳定，`gpui-kit 0.6.5` 在 crates.io 已被 yank。
- 处理：将 `gpui-kit`（rev `13c716b`）与 `gpui-mobile`（rev `5fd54f4`）源码放入 `vendor/`，用 path 依赖。
- `gpui-kit` 的 workspace 已裁剪，仅保留 `kit/base/component/component-macros/assets`，避免拉取 `quickjs-jit` 等无关 git 依赖。

### 3.2 版本冲突

- `gpui-kit 0.8.x` 要求 `gpui-pre =0.3.10`，`gpui-mobile` 要求 `=0.3.6`。
- 当前锁定为 **gpui-pre 0.3.6 + gpui-kit 0.6.5 + gpui-mobile HEAD**，三者一致。

### 3.3 Android 链接错误 `-lnativewindow`

- `libnativewindow.so` 仅存在于 NDK sysroot 的 API 26+ 目录。
- 解决：`cargo ndk -P 26`（与 `minSdk 26` 一致）。

### 3.4 字体解析 panic

- `sans-serif` 不是 GPUI 字体解析器认识的字体族。
- 解决：Android 使用 `Noto Sans CJK SC`；Windows 使用 `Microsoft YaHei`。

### 3.5 边到边安全区

- Android 15+ 强制 edge-to-edge，`AndroidApp::content_rect()` 返回全屏，insets 全为 0。
- 处理：在 vendored `gpui-mobile` 中新增：
  - `jni::query_system_bar_insets()`：通过 JNI 读取 `WindowInsets.Type.systemBars()`。
  - `AndroidWindow::set_safe_area_insets()`。
  - 全局 `gpui_mobile::safe_area_insets()` 的 Android 分支。
- `zhihu-ui` 根节点应用 `pt/pb/pl/pr`。

### 3.6 构建环境

- 源码盘 D: 空间紧张，cargo target 重定向到 `C:/cargo-target/zhihu-rust`（`.cargo/config.toml`，已 gitignore）。
- Gradle 发行版使用腾讯镜像；Maven 使用阿里云 `google` / `central` 镜像（`google()` 在本机不可达）。
- `android/local.properties` 指向本机 SDK（已 gitignore）。

---

## 4. 已知问题 / 后续

| 问题 | 影响 | 计划 |
| --- | --- | --- |
| 软键盘弹出时列表被遮挡（未按 IME insets 缩放） | 中 | M1：读取 `gpui_mobile::keyboard_height()`，给根节点加底部 padding 或缩小可视区 |
| Debug `.so` 62 MB，APK 65 MB | 中 | M1：`--release`（LTO + strip）并测量体积 |
| 首次冷启动偶发 `must construct App on main thread` | 高 | 观察 Activity 重建；备选：切换到 gpui-mobile `host` 入口或把 `init_platform` 改为可替换 |
| 应用图标 / 启动图仍为 GPUI 示例资源 | 低 | M1：替换品牌资源 |
| 未接入 HTTP / SQLite | — | M1：`reqwest` + `rusqlite` 冒烟测试 |
| 中文候选词人工验证 | 低 | 设备上人工输入一段中文确认 |

---

## 5. 复现命令

```bash
# 桌面
cargo run -p zhihu-desktop

# Android（Git Bash）
export ANDROID_HOME="C:/Users/13736/AppData/Local/Android/Sdk"
export ANDROID_NDK_HOME="$ANDROID_HOME/ndk/28.2.13676358"
export JAVA_HOME="C:/Program Files/AndroidStudio/jbr"

cargo ndk -t arm64-v8a -P 26 -o android/app/src/main/jniLibs build -p zhihu-mobile
"$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/windows-x86_64/bin/llvm-strip.exe" \
  --strip-all android/app/src/main/jniLibs/arm64-v8a/libzhihu_mobile.so
rm -f android/app/src/main/jniLibs/arm64-v8a/libgpui_mobile.so

(cd android && cmd //c ".\\gradlew.bat assembleDebug --no-daemon")
adb install -r android/app/build/outputs/apk/debug/app-debug.apk
adb shell am start -n dev.zhihurs.app/dev.gpui.mobile.GpuiActivity
```

> `libgpui_mobile.so` 是依赖 crate 的 cdylib 副产物，APK 只需要 `libzhihu_mobile.so`。

---

## 6. M0 验收结论

- 核心风险（GPUI 移动端可用性、中文、IME、触摸、安全区）已实际验证。
- 可以进入 M1：API / 存储 / 路由 / 基础组件。
- 建议优先处理：键盘遮挡、release 构建体积、Activity 重建路径。
