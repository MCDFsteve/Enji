## 本次更新

- 升级至 Bern 0.3.1：桌面端改用 `DetailFooter` 固定底部播放栏，设置页、插件管理与各设置子页面统一改用经典卡片排版，并纳入 Windows 标题栏集成、切页内存泄漏消除、宽窗设置布局与桌面歌词浮窗修复。
- 新增歌单与本地音乐倒序排序；补齐桌面端（macOS / Windows / Linux）播放、切歌、按 5% 步进调音、播放方式与红心快捷键。
- 完善播放队列与搜索体验：支持下一首插队区间管理与一键清空插队，在随机/心动模式及重启恢复时保留插队顺序并改用流式队列写入；搜索页支持按分类缓存与失败后单分类在线重试。
- 扩展插件能力：向插件开放受控进程内 `bern.cli` 接口，并新增高性能桌面歌词实时变色与帧动画接口。
- 修复歌词加载时左侧区域偏移，以及队列冷启动重复写盘、一起听跟播队列旋转等边界问题。

## What's new

- Updated to Bern 0.3.1: pinned the desktop playback bar with `DetailFooter`, unified Settings, Plugin Management, and settings subpages with a classic card layout, and brought in Windows title bar integration, page-switch memory leak fixes, wide-window settings layout fixes, and desktop lyrics overlay fixes.
- Added reverse sorting for playlists and local music, and completed desktop keyboard shortcuts (macOS / Windows / Linux) for playback, track switching, 5% volume steps, play mode, and favorites.
- Improved playback queue and search: managed "Play Next" as a contiguous up-next section with one-click clear, preserved up-next order across shuffle, heartbeat mode, and app restarts with streaming queue persistence, and added per-category search caching with individual online retry.
- Expanded plugin APIs with scoped in-process `bern.cli` access plus high-performance real-time desktop lyrics color updates and animation frame callbacks.
- Fixed left-side layout shift while lyrics load, as well as duplicate cold-start queue writes, Listen Together queue rotation, and other queue edge cases.
