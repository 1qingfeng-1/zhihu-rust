# M1 进展报告

> 日期：2026-10-27
> 目标：API / 签名 / 存储 / 路由基础设施，接入真实数据。
> 状态：**核心链路通过**；推荐流等登录态内容已实现代码但缺少可用 Cookie 做真机验证。

---

## 1. 已完成

### 1.1 `zhihu-api`

- 移植知乎 `zse v4` 加解密（`crates/zhihu-api/src/sign.rs`，AGPL 来源标注）。
  - `x-zse-93 = 101_3_3.0`
  - `x-zse-96 = 2.0_` + `encrypt_zse_v4(md5(zse93 + "+" + pathname + "+" + d_c0 [+ "+" + body]))`
- `ZhihuClient`：
  - `reqwest` + rustls + cookie jar
  - `bootstrap()` 获取首页 Cookie；缺失 `d_c0` 时生成设备 Cookie
  - `set_cookies()` / `cookie_value()` / `cookie_header()`
  - 公开接口：`hot_search()`、`daily_latest()`
  - 登录态接口：`feed_recommend()` / `feed_recommend_items()`、`me()`
- Feed JSON → 领域模型映射（answer / article / pin / zvideo）。
- 单元测试：MD5、pathname、zse header、feed 解析。

### 1.2 `zhihu-store`

- `rusqlite`（bundled）+ `user_version` 迁移。
- 表：`settings`、`accounts`、`cookies`、`feed_cache`、`content_cache`、`read_history`、`search_history`、`block_keywords/users/topics`、`drafts`。
- 设置读写、账号切换、Cookie 保存/加载、搜索历史。
- 4 个单元测试通过。

### 1.3 `zhihu-core`

- `AppServices`：Tokio 多线程运行时 + `async_channel` 事件回传 UI。
- `refresh_hot_search()` / `refresh_daily()` / `refresh_feed()`。
- Cookie 导入（`a=1; b=2`）并持久化到 SQLite；`is_logged_in()`。
- 测试：Cookie 导入 → 落库 → 重开 Store 后仍为登录态。

### 1.4 `zhihu-ui`

- 热榜页：真实 20 条热榜（排名、标题、热度）。
- 日报页：真实知乎日报列表。
- 登录面板：Cookie 导入输入框、导入/取消、状态提示。
- 推荐/关注页：已登录时请求推荐流，未登录显示占位内容。
- 底部状态栏展示数据来源/错误。

### 1.5 Android

- `rusqlite` bundled SQLite 交叉编译成功（NDK clang）。
- `reqwest` + rustls 在 Android 真机完成 HTTPS 请求。
- 屏蔽系统返回键退出（`onBackPressed` no-op + `enableOnBackInvokedCallback=false`）。
- 真机截图：
  - `docs/m1-android-hot.png`（热榜）
  - `docs/m1-android.png`（推荐占位）
  - `docs/m1-android-login.png`（登录面板）

---

## 2. 接口实测结论

| 接口 | 匿名 | 结论 |
| --- | --- | --- |
| `news-at.zhihu.com/api/4/stories/latest` | ✅ 200 | 日报可直接使用 |
| `api/v4/search/hot_search` | ✅ 200 | 热榜可直接使用 |
| `api/v4/search/suggest` | ✅ 200 | 搜索建议可直接使用 |
| `api/v4/search_v3` | ❌ 400 | 需要完整参数/签名，后续补 |
| `api/v3/feed/topstory/recommend` | ❌ 401 | **需要登录 Cookie** |
| `api/v4/questions/{id}` / `answers/{id}` | ❌ 403 | 需要登录 Cookie / 风控 |
| 带 `x-zse-96` 的 `hot_search` | ✅ 200 | 签名头被服务端接受 |

> 结论：公开内容可直接跑通；内容详情与推荐流必须实现登录（Cookie 导入已就绪，手机号/扫码登录在 M3）。

---

## 3. 已知问题

| 问题 | 影响 | 计划 |
| --- | --- | --- |
| Android Activity 重建后平台单例失效 | 高 | 切到 gpui-mobile `host` 入口，或让 `init_platform` 可替换；当前用返回键不退出规避 |
| 返回键暂不导航 | 中 | M2 路由接入后，把 Escape/back 映射到 `router.back()` |
| Cookie 明文存 SQLite | 高（安全） | M3 接入 Android Keystore / desktop keyring |
| 推荐流未做真机登录验证 | 中 | 需要有效 `z_c0`/`d_c0` Cookie |
| 搜索页未接入 | 中 | M2 补 `search_v3` 参数与结果渲染 |
| Debug APK 体积 | 低 | 后续 release + LTO |

---

## 4. 验证命令

```bash
cargo test --workspace
cargo clippy -p zhihu-api -p zhihu-core -p zhihu-store -p zhihu-types -p zhihu-ui --all-targets -- -D warnings

# 手动探测线上 API（匿名部分）
cargo run -p zhihu-api --example live_probe
```

---

## 5. 下一步（M2）

1. 富文本渲染：HTML → AST → GPUI（回答/文章详情）。
2. 路由与页面栈：问题/回答/文章/想法/用户/评论。
3. 登录态验证：导入真实 Cookie，跑通推荐流与详情页。
4. 图片加载与磁盘缓存。
5. 搜索页与热榜跳转。
6. Android 生命周期修复与返回键导航。
