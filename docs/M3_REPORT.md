# M3 进展报告（阶段检查点）

> 日期：2026-10-27
> 目标：登录校验、互动、搜索、屏蔽系统。
> 状态：账号校验 / 点赞关注 / 搜索建议 / 关键词屏蔽已实现；评论、通知、手机号/扫码登录待做。
> 工作方式：按用户要求，逻辑改动以单元测试为主，真机验证按批次进行（本轮为一次批量验证）。

---

## 1. 已完成

### 1.1 账号校验

- `GET /api/v4/me`（签名 + Cookie）→ `AccountProfile`。
- `AppServices::validate_login()`：校验成功后把昵称/url_token 写入 SQLite；失败发送 `AccountError`。
- 启动时若存在 `z_c0` 自动校验；登录按钮显示昵称（≤6 字），失败显示“登录失效”。
- 修复了占位账号名 “default” 被误显示为登录名的问题。
- **真机登录态验证仍需要有效 Cookie**。

### 1.2 点赞 / 关注

- API：
  - `POST /api/v4/answers/{id}/voters` body `{"type":"up"|"down"|"neutral"}`
  - `POST /api/v4/articles/{id}/voters` body `{"voting":1|0}`
  - `POST/DELETE /api/v4/members/{token}/followers`
  - 统一 POST/DELETE 签名：body 参与 `x-zse-96`，并带 `x-xsrftoken`（取 `_xsrf` Cookie）。
- UI：详情页固定操作栏（在滚动容器外），赞同/关注按钮，乐观更新 + 失败提示。
- **重要修复**：操作按钮最初放在 `overflow_y_scroll` 容器内，触摸点击会被滚动容器吞掉；移到滚动容器外后正常。
- 演示正文支持本地点赞/关注切换（无需登录），真机已验证。

### 1.3 搜索

- 公开接口 `GET /api/v4/search/suggest?q=...`。
- 搜索框输入 + “搜索”按钮 → 搜索页展示建议列表。
- 真机验证：输入 `rust` 返回 10 条真实建议。
- 完整 `search_v3` 结果、结果点击跳转待后续。

### 1.4 评论与通知

- 评论：`/api/v4/comment_v5/{answers|articles}/{id}/root_comment`，签名 + Cookie；解析作者、内容、点赞、子评论数、时间；详情页“评论”按钮进入。
- 通知：`api.zhihu.com/notifications/v3/message/v3`，解析 actor/title/text/read/created；主页“消息”入口。（线上可用性待有效 Cookie 验证）
- 演示正文自带两条示例评论，便于无登录时验收评论 UI。

### 1.5 Binance 风格主界面

- 深色优先：`#0B0E11` 背景、`#181A20` 卡片、`#F0B90B` 黄色强调色。
- 顶部品牌 + 消息入口 + 头像；圆角搜索栏 + 黄色搜索按钮。
- 底部导航：推荐 / 关注 / 热榜 / 日报 / 我的（替代原顶部 Tab）。
- “我的”页：账号卡、登录 Cookie 表单、深色模式、屏蔽词、消息中心、关于。
- 详情/搜索/评论/通知页统一为同一套颜色。

### 1.6 交互修复

- 搜索框点击时强制 `blur + focus`，修复 Android IME 被系统收起后无法重新唤起的问题（待真机确认）。
- 主页支持左右滑动切换 推荐/关注/热榜/日报（水平位移 > 60px 且主导轴为水平）。
- 点击任意底部 Tab 会先关闭登录/屏蔽词面板，修复“切 Tab 页面不变”。
- Android Resume 时强制重绘（`force_redraw`），缓解后台切回黑屏。

### 1.7 关键词屏蔽

- SQLite `block_keywords(pattern, is_regex, enabled)`。
- 匹配标题、摘要、作者名；支持子串与正则。
- 作用于：推荐流、热榜、日报、示例 Feed。
- UI：底部“屏蔽词”入口 → 添加/删除关键词，变更后自动刷新数据。
- 真机验证：添加 `Rust` 后，示例 Feed 中所有 Rust 相关条目被过滤。
- 单元测试覆盖子串/正则匹配与持久化。

---

## 2. 验证

```bash
cargo test --workspace
cargo clippy -p zhihu-api -p zhihu-core -p zhihu-store -p zhihu-types -p zhihu-content -p zhihu-ui --all-targets -- -D warnings
```

当前测试：**20 个**（nextest：API 8、内容 3、核心 3、存储 5、其他），全部通过。

本批次真机截图：

| 项目 | 截图 |
| --- | --- |
| 点赞 / 关注 | `docs/m3-android-actions.png` |
| 搜索建议 | `docs/m3-android-search.png` |
| 屏蔽词管理 | `docs/m3-android-blocklist-added.png` |
| 屏蔽后信息流 | `docs/m3-android-blocklist-filtered.png` |

---

## 2.1 真机验收（2026-10-28）

| 项目 | 结果 | 截图 |
| --- | --- | --- |
| Binance 风格主页 / 底部导航 | 通过 | `docs/m3-android-home.png` |
| 搜索框唤起输入法 | 通过（`mInputShown=true`） | `docs/m3-search-ime.png` |
| 左右滑动切换 Tab | 通过，单次滑动只切一格 | `docs/m3-swipe-left.png` / `docs/m3-swipe-right.png` |
| 登录表单打开时切 Tab | 通过，自动关闭表单并切换 | `docs/m3-overlay-tab.png` |
| 我的页 / 登录表单 / 设置 | 通过 | `docs/m3-android-profile.png` |
| 点赞 / 关注 / 评论按钮 | 通过 | `docs/m3-android-actions.png` |
| 评论页（演示数据） | 通过 | `docs/m3-android-comments.png` |
| 消息页（未登录提示） | 通过 | `docs/m3-android-notifications.png` |
| 后台切回前台 | 通过，force_redraw 后正常绘制 | `docs/m3-lifecycle-resume.png` |

> 真实 Cookie 下的推荐流、点赞、关注、评论和通知仍需一次登录态验收。

---

## 3. 已知问题

| 问题 | 影响 | 计划 |
| --- | --- | --- |
| 点赞/关注/评论/通知未用真实 Cookie 验证 | 高 | 导入有效 Cookie 后做一次登录态验收 |
| 评论只有第一页根评论，无子评论/分页 | 中 | 任务 #9：评论回复与分页 |
| 手机号/扫码登录未实现 | 中 | M4（Cookie 导入已可用） |
| 完整 `search_v3` 结果未实现 | 中 | 后续 |
| 用户/话题屏蔽未实现 | 中 | 后续 |
| Activity 被系统彻底销毁后重建仍可能复用旧平台 | 中 | 切 gpui-mobile `host` 入口；Resume 已加 `force_redraw` |
| 搜索框/输入框沿用 gpui-component 默认边框，深色下偏亮 | 低 | 后续统一样式 |

---

## 4. 下一步

1. 登录态验收：导入有效 Cookie，验证推荐流、点赞、关注、评论、通知。
2. 评论回复与分页（任务 #9）。
3. 手机验证码 / 扫码登录（需 WebView/相机桥）。
4. Android `host` 入口评估，彻底解决 Activity 重建。
5. 搜索完整结果与热榜/日报跳转。
