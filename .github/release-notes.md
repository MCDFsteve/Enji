## 0.3.0 新增

- **电视**：Android TV 上用遥控器操作，左侧是导航栏，用手机扫码登录，直接装 `.apk`。Apple TV 版已经适配，但这一版还没有安装包。
- **一起听**：在设置里打开后，用网易云的房间和朋友一起听，切歌、暂停、拖进度都会同步。
- **心动模式**：在设置里打开后，它和列表循环、随机播放排在一起，从正在放的歌开始接上网易云推荐的歌。
- **听歌记录同步到网易云**：听过的歌计入网易云账号的听歌记录，可以在设置里关闭。
- **账号页**：点开设置里的账号，能看到等级和升级进度、听歌时长、每年听了多少、最常听的歌和勋章。
- **年度听歌报告**：在账号页按年份打开网易云的官方报告，新的一年出来后自动出现；不想要可以在设置里关掉。
- **接着听**：下次打开时，上次放的歌、播放列表和进度都还在，按播放就接着放。
- **关闭窗口时**：电脑上可以选退出还是留在后台。留在后台时音乐继续放，从任务栏托盘（macOS 是菜单栏）能控制。macOS 默认留在后台。
- **手机横屏**：导航移到左侧，播放页左右分栏。
- **Android 上的播放**：来电或其他 App 开始放音乐时会暂停，拔耳机也会暂停。

## 0.3.0 改进与修正

- **封面律动**：新版插件按真实的节拍跳动，不再动一会儿就停。到插件商店更新到 1.1.0。
- **插件**：改成弹窗显示，不再挤压设置页。
- **固定的返回按钮**：歌单、专辑、歌手、本地音乐和播放记录的返回按钮固定在左上角，列表再长也不用滚回顶部。
- **手机播放页**：封面下的歌词有固定位置，封面不再上下跳，底部按钮不再被挤小；没有歌词时底部按钮也不会消失。
- **Android：** 本地音乐和播放记录按返回回到上一层，不再退到桌面；扫码弹窗居中。
- **在线封面**：缓存不再被本地音乐扫描清掉，切歌时封面出来得更快。

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

## What's new in 0.3.0

- **TV**: Android TV with the remote, a rail of pages down the side and sign-in with your phone, from the same `.apk`. Apple TV is supported too, but this version has no package for it.
- **Listen Together**: once on in Settings, listen with a friend in NetEase's rooms: skipping, pausing and seeking stay in step.
- **Heart Mode**: once on in Settings, it sits with Repeat and Shuffle, going on from the playing song with songs NetEase picks.
- **Listening history on NetEase**: what you play counts on your NetEase account, which Settings can turn off.
- **Account page**: open your account in Settings for your level and how far the next one is, your listening time, year by year, your most played songs and your medals.
- **Annual Report**: open NetEase's own report by year from the account page. A new year shows up by itself, and Settings can turn it off.
- **Pick up where you left off**: the next time you open Enji, the last song, its queue and its place are still there.
- **When the Window Closes**: on a computer, choose to quit or stay in the background, where the music plays on with controls in the system tray (the menu bar on a Mac). A Mac stays by default.
- **Phones held sideways**: the pages move to a rail at the side, and the playing page splits in two.
- **Playing on Android**: music pauses for a call or another app, and when headphones come out.

## What's better and fixed in 0.3.0

- **Cover Pulse**: the new plugin follows the real beat and no longer stops after a while. Update it to 1.1.0 in the plugin market.
- **Plugins** open in a dialog and no longer squeeze the settings.
- **The way back stays**: on a playlist, album, singer, local music and history, the back button stays at the top corner however long the list.
- **The playing page on a phone**: the lines under the cover keep their place, so the cover no longer jumps and the buttons at the bottom are no longer squeezed, and they stay when there are no lyrics.
- **Android**: Back goes back from local music and history instead of leaving the app, and the sign-in code is centered.
- **Online covers** are no longer cleared by a scan of local music, so they show sooner when a song changes.

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
