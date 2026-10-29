# M2 进展报告

> 日期：2026-10-27
> 目标：富文本渲染、详情页路由、图片。
> 状态：**核心链路通过**（离线演示 + 真机验证）；真实回答/文章正文需要登录 Cookie。

---

## 1. 已完成

### 1.1 内容管线 `zhihu-content`

- `scraper` + `html5ever` 解析知乎 HTML。
- 白名单 AST：
  - 块级：段落、标题（h1–h6）、引用、有序/无序列表、代码块、图片、表格、分割线。
  - 行内：文本、粗体、斜体、行内代码、链接、图片、公式、换行。
- 清洗：丢弃 `script/style`，未知标签展开为文本，链接解包 `link.zhihu.com/?target=`。
- 知乎公式图片（`/equation?tex=...`）转为数学文本。
- `to_plain_text()` 用于摘要/搜索。
- 测试：行内/块级/脚本清洗/纯文本投影。

### 1.2 详情 API

- `ZhihuClient::answer_detail(id)` / `article_detail(id)`。
- `ContentDetail`：标题、作者、正文 HTML、赞数、评论数、类型。
- DTO 映射 + 单元测试。

### 1.3 富文本渲染 `zhihu-ui`

- `StyledText` + `TextRun` 多风格行内排版：
  - 粗体、斜体、链接（蓝色下划线）、行内代码、公式、图片占位。
- 块级组件：标题、引用（左边框）、列表（• / 1.）、代码块、表格、分割线、图片。
- 远程图片：`gpui::img()` + `gpui-pre-reqwest-client`，桌面与 Android 均实测加载成功。

### 1.4 路由与交互

- `Route` 栈：`Answer` / `Article` / `Other`。
- Feed 卡片点击进入详情；示例数据进入内置演示正文。
- 详情页：
  - 返回按钮；
  - Android 返回键（映射为 Escape）返回上一页；
  - 加载中 / 错误 + 重试；
  - 长文滚动。
- 未登录点击真实卡片时提示“需要登录 Cookie”，不崩溃。

### 1.5 Android 实测

| 项目 | 结果 | 截图 |
| --- | --- | --- |
| 富文本详情 | ✅ | `docs/m2-android-detail.png` |
| 长文滚动 | ✅ | `docs/m2-android-detail-scroll.png` |
| 远程图片加载 | ✅ | `docs/m2-android-image.png` |
| 返回按钮 / 返回键 | ✅ | `docs/m2-android-back.png` |

---

## 2. 已知问题

| 问题 | 影响 | 计划 |
| --- | --- | --- |
| 真实回答/文章未真机验证 | 中 | 需要有效 Cookie；导入后点真实 Feed 卡片 |
| 行内链接不可点击 | 中 | M3：按 run 命中测试或段落级链接列表 |
| 图片无磁盘缓存 | 中 | M3：`ImageService` + LRU |
| 图片失败无占位 | 低 | 增加骨架/失败态 |
| 表格仅纯文本 | 低 | 后续做网格 |
| 热榜/日报条目未跳转 | 中 | 热榜→搜索页；日报→文章详情 |
| 搜索页未实现 | 中 | M3 |
| 评论、点赞、关注未实现 | 高 | M3 |

---

## 3. 验证

```bash
cargo test --workspace
cargo clippy -p zhihu-api -p zhihu-core -p zhihu-store -p zhihu-types -p zhihu-content -p zhihu-ui --all-targets -- -D warnings
```

当前测试：API 7、内容 3、核心 1、存储 4。

---

## 4. 下一步（M3）

1. 登录：真实 Cookie 验证推荐流；手机号/扫码登录。
2. 互动：点赞、关注、收藏、评论列表与回复。
3. 通知中心、屏蔽词/用户/话题。
4. 搜索页与热榜跳转。
5. 图片磁盘缓存、链接点击、代码高亮。
