## 本次更新

- 修复 Windows 11 上桌面歌词被桌面歌词设置浮窗遮挡的问题（桌面歌词始终保持在设置浮窗之上），并改进设置浮窗在 Windows 与 macOS 上的避让逻辑，左右空间不足时自动改放到歌词上方或下方。
- 修复 Windows 11 桌面歌词设置浮窗中调色器（`ColorPicker`）弹层被裁剪在浮窗边界内的问题，现可完整展开显示。
- 桌面歌词默认文字效果调整为描边。
- 扩展插件浮层接口，支持指定文字效果（`effect`）并实时感知 Windows 系统任务栏深浅主题（`enji.taskbarTheme` / `taskbarTheme` 事件），配合新版「任务栏歌词 1.1.0」实现纯色显示与深色任务栏白字 / 浅色任务栏黑字自动适配。

## What's new

- Fixed desktop lyrics being covered by the desktop lyrics settings utility window on Windows 11 (keeping lyrics above the settings window), and improved utility window placement on Windows and macOS to fall back above or below when left/right space is tight.
- Fixed the Windows 11 `ColorPicker` flyout inside the compact desktop lyrics settings window being clipped to the window bounds.
- Changed the default desktop lyrics text effect to Outline.
- Extended plugin overlay APIs with text effect selection (`effect`) and live Windows taskbar theme reporting (`enji.taskbarTheme` / `taskbarTheme` event), enabling Taskbar Lyrics 1.1.0 to render solid text in white on dark taskbars and black on light taskbars.
