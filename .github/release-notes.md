## 0.2.2 修正

- **Windows：歌单打开后马上显示歌曲。** 以前歌曲列表常常是空白的，要拉一下窗口或刷新几次才出来。
- **Windows：提示不再反复弹出。** 比如装好插件后，“已安装插件”的提示在播放时每隔几秒又出现一次。
- **Android：返回键不再直接回到桌面。** 播放页按返回会收起播放页，歌单、专辑和歌手页按返回会回到上一页。

## 0.2.1 修正与改进

- **去掉播放时的低沉噪音**：歌曲的采样率和设备不同时（例如 48 kHz 的歌在 Mac 扬声器上、或大多数歌在 iPhone 上），每一小段音频的接缝处都有一下爆音，连起来是一种低沉的嗡嗡声，有时左右声道还会错位。
- **歌单打开快得多**：一千多首的歌单第一次打开从约 4 秒缩短到 1 秒以内，再次打开立即显示。
- **登录只保留扫码**：手机上可以把二维码存到相册，再在网易云音乐 App 里扫一扫、从相册选这张图。原来的手机号和邮箱登录常被网易云要求额外验证而失败。
- **下载有反馈**：点下载会提示开始下载；播放页的下载按钮上有一圈进度，下载完成后按钮自动消失。
- **手机上的播放页**：从底部升起时不再露出一条，收起时不再在底部停留；歌单页的按钮排成两行，不再挤在一起；播放列表显示每首歌的封面。
- **提示**：同一时间只显示一条，手机上显示在迷你播放器上方，不再被标签栏挡住。

## 0.2.0 新增

- **下载**：把歌连同歌词和封面存到本机，下载过的歌在列表里都有标记。
- **系统的播放控件**：键盘上的媒体键、macOS 菜单栏、iPhone 的锁屏和控制中心、Windows 的媒体浮窗、Linux 桌面的媒体控件、Android 的通知栏，都能看到正在放的歌并控制播放。
- **桌面歌词**：电脑和 Android 上，歌词可以浮在所有窗口上面。鼠标移上去（Android 上点一下）就能切歌、暂停和锁定。
- **插件**：在设置里的插件商店安装插件，比如 Windows 任务栏歌词、跟着鼓点跳动的封面。
- **播放页**：写出歌曲的格式和码率；手机竖着拿时，封面下方显示正在唱的那一行。

## 下载

| 系统 | 文件 |
| --- | --- |
| macOS（Apple 芯片） | `.dmg` |
| iPhone / iPad（iOS 17 及以上） | `.ipa` |
| Windows（x64） | `-setup.exe` 安装版，或 `.zip` 免安装版 |
| Android 8.0 及以上 | `.apk` |
| Linux（Ubuntu 24.04 或更新） | `.deb`，或 `.tar.gz` |

**macOS**：应用没有经过 Apple 公证，第一次打开会被拦下。到“系统设置 → 隐私与安全性”底部点“仍要打开”即可。

**iPhone / iPad**：`.ipa` 没有开发者签名，用 AltStore、Sideloadly 等工具以自己的 Apple ID 签名后安装。

**Windows**：安装包没有代码签名，SmartScreen 提示时点“更多信息 → 仍要运行”。

**Android**：如果装过自己构建的测试版，需要先卸载再安装；之后的版本都能直接覆盖更新。

Linux 版在 Ubuntu 24.04 虚拟机（ARM64）里实际用过；这里发布的是 x64 版。

Enji 是第三方网易云音乐客户端，非网易官方出品，与网易公司无关，仅供学习交流。

## What's fixed in 0.2.2

- **Windows: a playlist shows its songs as soon as it opens.** The list was often blank until the window was resized or the playlist opened again.
- **Windows: notices no longer keep coming back.** After installing a plugin, for one, its notice showed again every few seconds during playback.
- **Android: Back no longer leaves the app from the playing page.** It closes the playing page, and goes back from a playlist, album or singer.

## What's new in 0.2.1

- **No more low buzz** while playing: when a song's sample rate differs from the device's (a 48 kHz song on Mac speakers, or most songs on an iPhone), every short piece of audio clicked at its seam, together a low buzz, and the left and right channels could swap.
- **Playlists open much faster**: a playlist of over a thousand songs first opens in under a second instead of about four, and at once after that.
- **Sign in with a QR code only**: on a phone, save the code to your photos, then scan it from the album in the NetEase Cloud Music app. Signing in with a phone number or an email address often failed when NetEase asked for more verification.
- **Downloads show what they are doing**: a notice when one starts, and a ring of progress on the playing page's download button, which goes away once the song is downloaded.
- **The playing page on a phone** no longer leaves a strip at the bottom when it rises or lingers when it sinks; a playlist's buttons sit in two rows instead of crowding one; the queue shows each song's cover.
- **Notices** show one at a time, above the mini player on a phone instead of under the tab bar.

## What's new in 0.2.0

- **Downloads**: keep songs on your device with their lyrics and covers; downloaded songs are marked in every list.
- **The system's media controls**: the media keys, the macOS menu bar, the iPhone lock screen and Control Center, the Windows media flyout, the Linux desktop's media controls and the Android notification show what's playing and control it.
- **Desktop lyrics** on computers and Android, floating above every window. Hover over them (tap on Android) to skip, pause or lock them.
- **Plugins** from the plugin market in Settings, such as lyrics on the Windows taskbar or a cover that pulses with the beat.
- **The playing page** shows the format and bitrate, and on a phone held upright, the line being sung under the cover.

## Download

macOS (Apple silicon): `.dmg`, not notarized; allow it under System Settings → Privacy & Security → Open Anyway. iPhone / iPad (iOS 17+): `.ipa`, not signed for distribution; install it with AltStore, Sideloadly or a similar tool, which signs it with your Apple ID. Windows (x64): `-setup.exe` or the portable `.zip`, unsigned. Android 8.0+: `.apk`. Linux (Ubuntu 24.04 or newer, x64): `.deb` or `.tar.gz`, used in an ARM64 Ubuntu 24.04 virtual machine.

Enji is an unofficial, third-party NetEase Cloud Music client, not affiliated with NetEase, for learning and technical exchange only.
