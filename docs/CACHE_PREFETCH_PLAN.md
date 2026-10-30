# Feed Cache & Prefetch Plan

## Problem

推荐流、关注流、热榜、日报都只在用户切换到对应 tab 时才发请求，首次切换必然等待一次网络往返；缓存虽然已经存在，但只作为“请求失败后的兜底”，没有参与首屏渲染。

## Goals

- 切 tab 时先出内容，再后台刷新（stale-while-revalidate）。
- 冷启动后温和预热公共数据，不抢占首帧、不放大风控风险。
- 请求去重、失败静默，只有用户主动进入的 tab 才展示错误。

## Strategy

1. **Cache-first**
   - 推荐流：`refresh_feed` 在发请求前先 `try_send(UiEvent::FeedCached)`，UI 直接渲染缓存快照，网络返回后替换；缓存事件不设置错误横幅，也不关闭分页。
   - 热榜 / 日报：首次请求前先读取 `cache_feed("hot")` / `cache_feed("daily")` 快照并下发，成功后覆盖写回。
   - 缓存使用现有的 `Store::cache_feed` / `cached_feed`，无需 schema 迁移。
2. **Delayed prefetch**
   - 首帧后约 1.2s（splash 淡出阶段）调用一次 `AppServices::prefetch_startup`。
   - 只预取公开数据（热榜、日报）；推荐流仅在存在 `z_c0` 会话时预取；关注流不在启动阶段预取。
3. **Dedupe & failure policy**
   - `hot_fetching` / `daily_fetching` / `feed_fetching` 原子标志保证同一数据只有一个在途请求。
   - 预取失败不产生 UI 错误；用户主动刷新/进入 tab 时才将错误写入对应状态。
4. **Follow-up（尚未实现）**
   - 关注流：进入“关注”时预取另一个子 tab（推荐 ↔ 动态）。
   - 公共数据 TTL（热榜 3–5 分钟、日报按天）：当前由 in-flight 去重和手动刷新控制，后续可按 `fetched_at` 跳过过新的缓存刷新。
   - 预取耗时可接入诊断面板（`hot_loaded_ms` / `feed_loaded_ms` 已有基础）。

## Implemented

- `crates/zhihu-core/src/lib.rs`
  - `UiEvent::FeedCached(Vec<FeedItem>)`
  - `cached_hot` / `cached_daily` helpers（复用 `Store::cached_feed`）
  - `refresh_hot_search` / `refresh_daily`：缓存先行、成功写快照、in-flight 去重
  - `refresh_feed`：缓存先行 + 去重，网络失败仍回退 `FeedOffline`
  - `AppServices::prefetch_startup()`：一次性预热热榜/日报，登录后预热推荐流
- `crates/zhihu-ui/src/lib.rs`
  - `UiEvent::FeedCached` 处理：渲染快照、清除 loading、不写错误
  - 250ms tick 中在启动 1.2s 后触发 `prefetch_startup`

## Validation

- `cargo check -p zhihu-core -p zhihu-ui`
- `cargo nextest run -p zhihu-core -p zhihu-store`（27 passed）
- 设备侧待验证：冷启动后直接切热榜/日报应立即有内容；断网启动推荐流应展示缓存快照且无错误横幅。
