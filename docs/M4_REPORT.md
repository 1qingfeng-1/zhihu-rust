# M4 进展报告

> 日期：2026-10-28
> 范围：M4 不含写作 / 草稿 / 导出。
> 状态：M4 功能实现完成；真机 UI/平台门禁按用户要求延后到 M5。

---

## 1. 已完成

### 1.1 评论分页与子评论

- **实网发现**：`comment_v5/.../root_comment` 对当前账号返回 `data: []`
  （`paging.totals` 正常），但旧版 `/api/v4/{answers|articles}/{id}/comments`
  正常返回 20 条。客户端已切换到 v4 并在请求中带上内容页 Referer。
- 新增 v4 评论解析（`author.member`、`reply_to_author`、`like_count`、`paging.is_end`）。
- `comments(kind, id, offset)` 返回 `CommentsPage { items, is_end }`。
- 子回复：每条根评论可“查看回复/收起回复”，懒加载
  `/api/v4/comments/{id}/child_comments`，支持“加载更多回复”；
  v4 根评论不返回子回复数量，因此所有根评论都提供入口。
- 解析根评论内嵌 `child_comments` 与 `child_comment_next_offset`。
- 评论页支持“加载更多”，子评论缩进展示、作者标记、点赞数；未内嵌完的显示“还有 N 条回复”。
- 单元测试覆盖子评论与 `paging.is_end`。

### 1.2 完整搜索

- 签名 `search_v3`（`t/q/correction/offset/limit/filter_fields/lc_idx/show_all_topics`）。
- `data[].object` → FeedItem 映射，搜索结果可直接进入回答/文章详情。
- 结果分页：解析 `paging.is_end`，搜索页“加载更多”，offset 递增。
- 筛选栏：综合/回答/文章/视频（`vertical` + `vertical_info`），
  综合排序/最新/最多赞同（`sort`），切换即重新搜索。
- 搜索摘要会去掉 `<em>` 高亮标签并解码 HTML 实体。
- 匿名访问返回 403（需要登录 Cookie），失败时保留搜索建议列表并显示提示。
- 单元测试覆盖搜索对象与分页解析。
- 类型/排序筛选参数暂未暴露到 UI（搜索接口需要登录态才能验证参数语义）。

### 1.3 用户 / 话题屏蔽

- SQLite `block_users` / `block_topics` 读写；`FeedItem` 新增 `topics`。
- 推荐流按关键词、作者 id/name、话题名过滤；示例 Feed 同步过滤。
- 详情页“屏蔽”按钮；屏蔽页支持关键词、用户、话题三类管理。
- 话题从推荐流 `target.topics` / `question.topics` 解析。

### 1.4 阅读与外观设置

- `AppSettings { dark, font_size, line_height, tab }` 持久化到 SQLite。
- “我的”页：深色模式、字号（小/中/大）、行距（紧凑/标准/宽松）。
- 默认 Tab 记忆：切换 Tab 后写入设置，下次启动恢复（`ZHIHU_TAB` 环境变量优先）。
- 详情页正文、列表、代码块按设置缩放。

### 1.5 扫码登录

- 新客户端（避免主 Cookie jar 的合成 `d_c0` 污染）执行：
  - `/signin` → `/udid` → 验证码初始化 → `POST /api/v3/account/api/login/qrcode`
  - 轮询 `/api/v3/account/api/login/qrcode/{token}/scan_info`
- 成功后把 Cookie 导入主客户端、持久化并校验 `/api/v4/me`。
- UI：登录表单内“扫码登录” → 本地二维码矩阵 → 状态提示。
- Host 实测二维码获取成功（返回 `link`/`token`）。

### 1.6 手机号（短信验证码）登录

- 移植知乎 Android 客户端协议（`ZhihuPhoneLoginClient.kt`，AGPL-3.0）：
  - `/udid` 获取网页设备 Cookie `d_c0`；
  - 加密设备表单初始化访客：`POST /api/account/prod/init/udid_guest`（HMAC-SHA1 签名）；
  - `GET/PUT/POST /captcha` 处理图形验证码分支；
  - `POST /api/account/prod/auth/digits` 发送短信；
  - `POST /api/account/prod/sign_in` 用 `digits` 授权码换取 access token 与 Cookie。
- 新增 `phone_crypto`：请求体自定义块加密（查表 + 10 轮替换 + CBC 式链接）+ HMAC-SHA1。
- 登录成功后 Cookie 导入主客户端、写入 SQLite、校验 `/api/v4/me`。
- UI：手机号、图形验证码（如需）、短信验证码三段式表单。
- **Host 实测**：
  - 访客初始化通过（加密与 HMAC 签名正确）；
  - `/captcha` 状态查询通过，并在响应中返回 `capsion_ticket` Cookie；
  - 修正后每次响应自动吸收 `Set-Cookie`（此前遗漏导致“缺少验证码票据” 120002）；
  - 图形验证码 `PUT /captcha` 成功返回图片（5 KB 的 `img_base64`）。
  - 合成号码仍被风控拦截，完整短信登录需要用真实手机号在真机门禁验证
    （`cargo run -p zhihu-api --example phone_probe -- <手机号>`）。

### 1.7 TTS 朗读

- Android：`GpuiTts.java`（系统 TextToSpeech，中文），`GpuiActivity` 初始化/停止。
- Rust：`zhihu-ui::tts` 通过 JNI 调用。
- 桌面：调用系统语音（Windows PowerShell `System.Speech`，macOS `say`，Linux `spd-say`），
  朗读文本通过 stdin 传入，停止时终止子进程。
- 详情页“朗读/停止”按钮，返回时自动停止。
- PC 实测：点击“朗读”后生成 PowerShell 合成进程，按钮切换为“停止”，点击后进程结束。
- 修复 `GpuiActivity` 重复 `onDestroy`（TTS 与 MediaSession 合并，并 shutdown）。

### 1.8 图片磁盘缓存 / GIF

- GPUI 的 `img()` 本身支持 GIF 多帧与动画播放；缓存按原文件落盘，
  因此正文 GIF 无需额外桥接。

- URL → md5 文件名缓存在 `data_dir/images`，下载写入 `.part` 后原子改名。
- 同一 URL 并发请求去重；完成后发送 `ImageReady` / `ImageFailed`。
- 加载中/失败占位，失败 URL 不重复请求。
- 启动时按修改时间淘汰，最多保留 300 个文件。
- 视频封面（poster）同样进入图片管线。

### 1.9 视频播放

- HTML 解析新增 `Block::Video { src, poster }`，支持 `<video src|data-src|data-original|data-url>` 与 `figure` 内视频。
- 详情页渲染视频卡片（封面 + 播放按钮），点击进入全屏视频页。
- Android 使用 `gpui-mobile` `video_player`（MediaPlayer + SurfaceView 平台视图）：
  `set_url` → `show_surface` → `play`，`platform_view_element` 同步原生视图边界。
- 关闭视频/返回时释放播放器与平台视图；桌面显示“暂不支持内嵌视频”兜底。

### 1.10 通知分页与已读

- 接口改为 `notifications(offset)`，解析 `paging.is_end`。
- 通知页支持“加载更多”；未读圆点；标题栏“全部已读”。
- 已读会同步服务端：`POST /notifications/v3/timeline/entry/{comment|like|favlist_me|follow|invite}/actions/readall`，
  成功后显示“已同步已读状态”，失败显示错误。
- Host 实测：`mark all read OK`。

### 1.11 阅读历史与图片查看器

- 详情加载成功即写入 `read_history`（回答/文章、标题、时间，主键去重）。
- “我的 → 阅读历史”页：最近 200 条、相对时间、点击回到详情、支持清空。
- 详情正文图片点击进入全屏查看器（`ObjectFit::Contain`，点击左上角关闭）。
- 多图：全屏查看器支持左右按钮、横向滚轮/swipe 切图，并显示 `当前/总数`。
- 历史与图片查看器均为本地能力，无需登录即可验证。

### 1.12 首页滑动切 Tab（门禁回归修复）

- 原实现在内容容器上监听 `on_scroll_wheel`，气泡阶段晚于列表滚动，
  横滑会同时带动列表纵向偏移；且只允许 `tab <= 3`，无法滑入“我的”。
- 改为在内容区叠加 `canvas` 注册 **Capture 阶段** 的滚轮监听：
  - 横滑优先判定后调用 `stop_propagation()`，列表收不到该事件，不再纵向滚动；
  - 判定阈值 24px（横向占优 1.25 倍），比旧实现更灵敏；
  - 桌面滚轮没有 Started/Ended：每个滚轮 notch 独立判定，且 400ms 后自动解除
    latch，否则一次横滚会吞掉后续所有滚轮；
  - 桌面横滚方向与触屏不同，按 `delta.x` 方向映射上一/下一 Tab；
  - 支持 5 个 Tab（推荐/关注/热榜/日报/我的），在“我的”页左右滑同样生效；
  - 每次切 Tab 递增 `list_epoch`，新页面从顶部开始；
  - 保留 350ms 防连切与 500ms 点击抑制。

### 1.13 公式排版

- **独立成段的公式**用 `ratex-gpui`（RaTeX，KaTeX 解析器移植，约 660 条命令）
  真正排版为位图：`render::render_latex` → `RenderImage`，按公式/字号/颜色
  thread-local 缓存，避免每帧重排。
- 行内公式在文本流中保留 `zhihu-content::format_math` 的 Unicode 投影
  （希腊字母、运算符、分数、根号、上下标），避免拆散段落换行。
- DEMO 正文包含一个 `\frac{-b\pm\sqrt{b^2-4ac}}{2a}` 独立公式作为示例。
- 单测：`only_math` 识别 + 实际光栅化 `render_latex` 返回非零尺寸。

### 1.14 扫码登录风控处理

- 扫码启动失败时自动换新 Cookie 会话重试一次（间隔 1.2s）。
- 识别知乎 40352 / `need_login` 风控响应，解析 `message` 与 `redirect`。
- 登录页展示风控卡片：
  - “打开验证页面”调用系统浏览器打开 `redirect`（缺省 `account/risk_control`）；
  - “重试”重新获取二维码；
  - 提示可切换网络，或改用手机号登录 / Cookie 导入。
- 轮询阶段的 403 同样转入风控卡片。

### 1.15 深链与分享

- 支持 `zhihurs://answer/{id}`、`zhihurs://article/{id}`、
  `https://www.zhihu.com/answer/{id}`、`/question/{q}/answer/{id}`、
  `https://zhuanlan.zhihu.com/p/{id}`。
- Android Manifest 注册 `zhihurs` 自定义 scheme 与知乎回答/文章 https 过滤器。
- 冷启动读取 initial link，运行中通过 `onNewIntent` 回调，UI 轮询消费。
- 详情页“分享”：Android 调系统分享面板；桌面复制链接到剪贴板。

---

## 2. 验证

```bash
cargo nextest run --workspace
cargo clippy -p zhihu-api -p zhihu-core -p zhihu-store -p zhihu-types -p zhihu-content -p zhihu-ui --all-targets -- -D warnings
cargo ndk -t arm64-v8a -P 26 check -p zhihu-mobile
```

当前：**40 个测试通过**；clippy 无警告；Android 交叉编译通过；`assembleDebug` 打包成功
（`android/app/build/outputs/apk/debug/app-debug.apk`，约 149 MB debug 包）。

Host / PC 实测（真实 Cookie，`cargo run -p zhihu-core --example cookie_probe`）：

- `/api/v4/me`、推荐流、回答详情、v4 评论（20 条根评论、子回复接口）、通知列表均成功。
- 搜索：综合 14 / 回答 18 / 文章 18 / 最新 17 条。
- `POST .../actions/readall`：`mark all read OK`。
- 二维码登录获取 `link`/`token` 成功；手机号访客初始化、图形验证码成功。
- 桌面 UI：登录、真实推荐流、详情、搜索筛选、TTS、多图、历史、五 Tab 滑动已人工/自动验证。

---

## 3. 剩余

| 任务 | 状态 |
| --- | --- |
| M4 真机平台门禁 | 设备移除后暂缓；PC 侧已完成登录态验收 |
| 子回复分页 | 已完成（v4 child_comments） |
| 通知服务端已读 | 已完成并实测 |
| 有效 Cookie 全链路验收（任务 #10） | 已完成（PC，真实 Cookie） |
| 视频 / TTS / 扫码 / 手机号真机验证 | 待设备接入 |
| 深链冷启动与热启动验证 | 待设备接入 |
| 行内公式图片化 | 当前为 Unicode 投影；独立公式已用 RaTeX 排版 |
| 真机 UI 验证（评论回复、通知已读） | 待设备接入 |

> 真机 UI 验证（视频、TTS、扫码/手机号、深链、评论回复、通知已读）
> 按用户要求移入 M5；M4 的代码与 PC/Host 验收已完成。

> 手机号登录已实现完整协议，但发送真实短信需要用户本人手机号；
> M5 真机门禁时请用测试号走一遍“发送验证码 → 输入 → 登录”。

---

## 4. 门禁步骤

```bash
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

门禁清单：

1. M3 回归：首页滑动切 Tab、搜索输入法、详情点赞/关注/评论、通知页、屏蔽页、后台回前台。
2. 图片缓存：二次进入详情不重复下载（logcat 无重复请求），断网显示失败占位。
3. 视频：含视频回答点击播放、返回释放（旋转/后台恢复不残留 SurfaceView）。
4. TTS：朗读中文、停止、返回自动停止。
5. 扫码登录：二维码展示 → 手机确认 → 头像/昵称与推荐流加载。
6. 手机号登录：短信验证码与图形验证码分支。
7. 深链：`adb shell am start -a android.intent.action.VIEW -d "zhihurs://answer/<id>"`；
   冷启动与已运行两种状态。
