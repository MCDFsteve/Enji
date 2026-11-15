## 本次更新

- **日语假名注音对齐修复**：重构注音对齐算法，保留罗马音音节与标点边界并采用全局拍数/音节核比例动态规划，彻底解决连续多段汉字词组把拗音拆散或将整串读音挤到末段汉字上的问题，消除同音助词提前误锚定，并补齐 `tch` 促音、小假名、英文粘连词、日文数字读音、括号行内附注以及逐字歌词（`yrc`）前置抢拍长行的区间匹配。
- **各平台注音渲染修复**：修复 Windows 歌词页在无逐字时间轴时不显示假名注音的问题，修复 macOS / iOS 在含 `々` 等跨回退字体词组（如「重々承知」）上字距被撑开与注音偏位的问题。
- **界面与交互优化**：优化手机竖屏播放页主控区纵向间距并消除播放页底栏图标切换时的布局跳动；升级至 Bern 0.1.18，修复 Windows 11 窗口拖动阻塞并支持高刷新率渲染，对齐 macOS 统一顶栏窗口按钮垂直中心。

## What's new

- **Japanese furigana alignment fixes**: replaces greedy character-stream matching with syllable-aware global dynamic programming, partitioning consecutive kanji phrases proportionally without tearing palatalized morae or crowding readings onto trailing kanji, avoiding accidental early particle anchors, and supporting `tch` geminates, small kana, glued English words, Japanese numeral readings, inline parenthetical glosses, and pickup-timed `yrc` line spans.
- **Cross-platform ruby rendering fixes**: fixes missing furigana on non-word-timed lines in the Windows lyrics view, and fixes widened character spacing and off-center ruby over mixed fallback-font spans (such as `々`) on macOS and iOS.
- **UI and frame-rate improvements**: refines vertical spacing and eliminates bottom-bar icon jitter on compact portrait Now Playing; updates to Bern 0.1.18 with smooth high-refresh window dragging on Windows 11 and centered unified-toolbar window controls on macOS.
