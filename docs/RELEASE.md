# 发布与签名

> 适用：M5 发布流水线（任务 #26）。
> 目标：一条 tag 触发 Android APK 与桌面安装包，签名材料不入库。

## 1. 版本号

| 变量 | 来源 | 示例 |
| --- | --- | --- |
| `ZHIHU_VERSION` | tag（`v` 前缀会被去掉） | `v1.1.0` → `1.1.0` |
| `ZHIHU_VERSION_CODE` | CI 的 `github.run_number` | `42` |

本地构建可显式设置：

```bash
ZHIHU_VERSION=1.1.0 ZHIHU_VERSION_CODE=42 ./gradlew assembleRelease
```

## 2. Android 签名

签名读取顺序：环境变量 → `android/keystore.properties`（已 gitignore）→ 调试密钥。

| 环境变量 | properties 键 |
| --- | --- |
| `ZHIHU_KEYSTORE_FILE` | `storeFile` |
| `ZHIHU_KEYSTORE_PASSWORD` | `storePassword` |
| `ZHIHU_KEY_ALIAS` | `keyAlias` |
| `ZHIHU_KEY_PASSWORD` | `keyPassword` |

生成本地密钥（只做一次，妥善备份）：

```bash
keytool -genkeypair -v \
  -keystore android/release.jks \
  -alias zhihu-rust \
  -keyalg RSA -keysize 4096 -validity 10000 \
  -dname "CN=zhihu-rust, OU=dev, O=zhihu-rust, L=, S=, C=CN"
```

`android/keystore.properties` 示例：

```properties
storeFile=release.jks
storePassword=******
keyAlias=zhihu-rust
keyPassword=******
```

> 未配置签名时 `assembleRelease` 会自动改用 debug 密钥，仅供冒烟测试，**不可用于分发**。

## 3. GitHub Actions

`.github/workflows/release.yml` 在推送 `v*` tag 时执行：

1. **android**：`cargo ndk` 构建 `arm64-v8a` → `llvm-strip` → `assembleRelease` →
   上传 `zhihu-<tag>-android-arm64.apk`。
2. **desktop**：Windows / Linux 构建 `zhihu-desktop` →
   上传 `zhihu-desktop-windows-x64.zip`、`zhihu-desktop-linux-x64.tar.gz`。
3. **publish**：汇总产物 + `git-cliff` changelog，创建 GitHub Release。

需要在仓库 Secrets 中配置：

| Secret | 说明 |
| --- | --- |
| `ZHIHU_KEYSTORE_BASE64` | `base64 -w0 android/release.jks` |
| `ZHIHU_KEYSTORE_PASSWORD` | 密钥库口令 |
| `ZHIHU_KEY_ALIAS` | 别名 |
| `ZHIHU_KEY_PASSWORD` | 密钥口令 |

```bash
base64 -w0 android/release.jks > keystore.b64
gh secret set ZHIHU_KEYSTORE_BASE64 < keystore.b64
gh secret set ZHIHU_KEYSTORE_PASSWORD
gh secret set ZHIHU_KEY_ALIAS
gh secret set ZHIHU_KEY_PASSWORD
```

缺失 keystore secret 时流水线仍会产出 APK（debug 签名），但只应作内部测试。

## 4. 本地发布构建

```bash
export ANDROID_HOME="C:/Users/13736/AppData/Local/Android/Sdk"
export ANDROID_NDK_HOME="$ANDROID_HOME/ndk/28.2.13676358"
export JAVA_HOME="C:/Program Files/AndroidStudio/jbr"

cargo ndk -t arm64-v8a -P 26 -o android/app/src/main/jniLibs build -p zhihu-mobile --release
"$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/windows-x86_64/bin/llvm-strip.exe" \
  --strip-all android/app/src/main/jniLibs/arm64-v8a/libzhihu_mobile.so
rm -f android/app/src/main/jniLibs/arm64-v8a/libgpui_mobile.so
(cd android && cmd //c ".\\gradlew.bat assembleRelease --no-daemon")

cargo build --release -p zhihu-desktop
# Windows 便携版（静态 C 运行库，不依赖 VCRUNTIME140.dll）：
# RUSTFLAGS="-C target-feature=+crt-static" cargo build --release -p zhihu-desktop
```

产物：

- `android/app/build/outputs/apk/release/app-release.apk`
- `target/release/zhihu-desktop.exe`（Windows）
- 便携包：`dist/zhihu-desktop-windows-x64/` 与 `.zip`（exe + README + LICENSE）

## 5. 发布前检查

- [ ] `cargo fmt --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo nextest run --workspace`
- [ ] Android 真机回归（M5 门禁清单见 `docs/M5_PLAN.md`）
- [ ] 桌面宽/窄窗口、快捷键、崩溃日志页
- [ ] 版本号与 tag 一致
- [ ] 商店素材：图标（`res/mipmap-*`）、截图、隐私说明（无遥测、本地存储）

## 6. 隐私与数据

- 应用不上传崩溃日志、阅读历史、屏蔽列表或本地推荐反馈；全部存于本机 `data_dir`。
- 崩溃日志只含 panic 消息、线程、位置与回溯，不含 Cookie 或正文。
- 本地推荐默认关闭；关闭时不采集，开启后只保存作者 id、主题/标题特征与权重。
- 卸载应用会清除本地数据；无云端账号同步。

## 7. 商店素材（草稿）

- 应用名：知乎 Rust
- 简短描述：无广告、可离线的知乎第三方客户端，Rust + GPUI 构建。
- 完整描述要点：
  - 推荐 / 关注 / 热榜 / 日报，问答与文章富文本、公式、图片、视频；
  - 评论与子回复、搜索筛选、通知、阅读历史；
  - 深色/浅色、字号/行距、关键词/用户/话题屏蔽；
  - Cookie / 扫码 / 手机号登录；TTS 朗读；
  - 本地崩溃日志与诊断，无遥测、不上传阅读数据。
- 截图清单：首页、详情、评论、搜索、我的、桌面双栏（平板/桌面渠道）。
- 隐私说明：无账号体系、无云同步、无广告 SDK；网络仅用于访问知乎公开接口与图片。
- 应用图标：`android/app/src/main/res/mipmap-*/ic_launcher*.png`，
  自适应图标 `mipmap-anydpi-v26/ic_launcher.xml`。

