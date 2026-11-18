## 本次更新

- 修复 Windows（WinUI 3）沉浸播放时鼠标静置后仍显示的问题；移动鼠标会恢复原有指针。macOS 恢复原生侧栏收折按钮，并修正全屏转场时的工具栏背景。
- 修复本地音乐的 GBK / GB18030 标签和 LRC 歌词乱码。升级后会自动重扫旧缓存。
- 修复平板底栏时间与窄侧栏登录按钮被挤成竖排的问题。
- 升级至 Bern 0.1.20。播放进度等高频变化按字段同步，列表只传输变化的记录。

## What's new

- On Windows (WinUI 3), the mouse pointer now hides when idle during immersive playback and restores its previous shape on movement. On macOS, the native sidebar toggle is restored and the toolbar background is fixed during fullscreen transitions.
- Fixed garbled GBK / GB18030 tags and LRC lyrics in local music. Existing cached metadata is rescanned after updating.
- Fixed vertically wrapped time labels in tablet bottom bars and sign-in buttons in narrow sidebars.
- Updated to Bern 0.1.20. High-frequency updates such as playback progress sync per field, and list updates send only changed records.
