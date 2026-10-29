# Vendored dependencies

These directories are pinned source snapshots of upstream projects that are not
reliably fetchable in this environment.

| Directory | Upstream | Commit | License |
| --- | --- | --- | --- |
| `gpui-kit/` | https://github.com/longbridge/gpui-kit | `13c716b687b47f96a677aaf46c8fa341fa7208da` (v0.6.5) | Apache-2.0 |
| `gpui-mobile/` | https://github.com/itsbalamurali/gpui-mobile | `5fd54f4554263ae06753e90472c7f2e2ff5d7495` | GPL-3.0 / AGPL-3.0 / Apache-2.0 |

## Why

- `gpui-mobile` publishes only a name-reservation placeholder to crates.io.
- `gpui-kit` 0.6.5 is yanked on crates.io, and GitHub is unreachable here.
- `gpui-kit` 0.8.x depends on `gpui-pre =0.3.10`, which conflicts with
  `gpui-mobile`'s `gpui-pre =0.3.6`.

## Modifications

- `gpui-kit/Cargo.toml`: the workspace member list was trimmed to
  `kit` / `base` / `component` / `component-macros` / `assets`. The removed
  members (story, shell, examples) pull git-only dependencies such as
  `quickjs-jit` that are irrelevant to this app.
- `gpui-mobile` (Android safe area, necessary on Android 15+ edge-to-edge):
  - `src/android/jni.rs`: `query_system_bar_insets()` reads
    `WindowInsets.Type.systemBars()` via JNI and is used by `InitWindow` /
    `WindowResized` instead of the (all-zero) `content_rect`.
  - `src/android/window.rs`: `AndroidWindow::set_safe_area_insets()`.
  - `src/lib.rs`: Android branch for the global `safe_area_insets()`.

These patches are small; upstream them if possible. Re-apply after updating
either snapshot.

## Updating

1. Download the archive at the desired commit.
2. Replace the directory contents (keep the workspace member trimming above).
3. Update the commit and the dependency path if the crate layout changed.
4. Bump both `gpui-kit` and `gpui-mobile` together, and re-verify the
   `gpui-pre` pin matches in both manifests.
