## 0.3.10 紧急修复

- **Windows 11 歌词页闪退**：修复打开歌词页、开关日语注音时触发的布局循环崩溃。
- **逐字歌词闪退**：修复部分字形在首字开始高亮时产生负裁剪宽度，导致切歌或打开歌词页立即退出的问题。
- 注音、逐字高亮和字号动画均保留。建议 Windows 用户尽快更新。

已在 Windows 11 物理机上复现并验证：日语逐字歌词、中文逐字歌词、纯音乐、30 次开关歌词页、20 次开关注音及窗口缩放，同一进程运行超过 11 分钟无新崩溃。此次未运行 Windows 10 应用。

## Emergency fixes in 0.3.10

- **Windows 11 lyrics-page crashes**: fixes a layout cycle when opening lyrics or toggling Japanese readings.
- **Word-by-word lyrics crashes**: fixes negative clipping widths at the start of certain glyphs, which could close the app when changing songs or opening lyrics.
- Readings, word-by-word highlighting and font-size animations remain enabled. Windows users are encouraged to update.

Reproduced and verified on a physical Windows 11 PC with Japanese and Chinese word-timed lyrics, instrumental music, 30 page open/close cycles, 20 reading toggles and window resizing. The same process ran for over 11 minutes without a new crash. Windows 10 was not run for this fix.

## 0.3.9 修正

- **注音不再改变原文字体**：Apple 平台开启日语假名注音时，保留原来的系统字体、字重和中文、日文字形，不再变成细衬线体。
- **电视歌词字号一致**：修正 Apple TV 大标题的字号映射，开关注音不再把原文放大。

已在 iPhone 17（iOS 26.5）和 Apple TV 4K（tvOS 27.0）模拟器中实际检查注音开关、重启保持和字体；逐字高亮用另一首有逐字时间的歌曲验证。测试的日语歌曲不含逐字时间，尚未覆盖日语注音与逐字高亮同时播放。长读音需要额外空间，仍可能正常换行。

## What's fixed in 0.3.9

- **Readings keep the original font**: on Apple platforms, Japanese furigana preserves the system font, its weight, and Chinese/Japanese fallback glyphs instead of switching to a thin serif face.
- **Consistent TV lyric sizes**: corrects the Apple TV large-title mapping so turning furigana on does not enlarge the base text.

The furigana toggle, persistence after restart, and fonts were checked in the iPhone 17 (iOS 26.5) and Apple TV 4K (tvOS 27.0) simulators. Word-by-word highlighting was checked using a separate song with word timing. The Japanese samples had no word timing, so simultaneous furigana and word-by-word playback remains unverified. Long readings need extra space and can still wrap normally.

## 0.3.8 新增

- **日语歌词假名注音**：根据歌曲提供的罗马音，为日语歌词的汉字标注平假名。歌词页、封面下的歌词、电视和桌面歌词共用这项设置，可以在设置里关闭。

## 0.3.8 修正

- **Apple TV 打包**：修正 tvOS 字体映射导致的编译失败，Apple TV 版也包含日语歌词注音。

- **注音跟随歌词高亮**：注音颜色与原文保持一致，换行、字号变化和切换歌词时同步过渡，不改变原文的逐字时间。
- **原生界面**：更新到 Bern 0.1.12，包含导航、气泡、安全区背景和系统兼容性修正。

## What's new in 0.3.8

- **Japanese lyric readings**: hiragana readings above kanji, aligned from the song's romanized lyrics when available. The lyrics page, cover lyrics, TV and desktop lyrics share the setting, which can be turned off in Settings.

## What's fixed in 0.3.8

- **Apple TV packaging**: fixes the tvOS font mapping that blocked compilation; the Apple TV version also includes Japanese lyric readings.

- **Readings follow the lyrics**: colours and highlights stay in step with the original text through wrapping, font-size changes and line transitions, without changing its word timing.
- **Native interface**: updated to Bern 0.1.12, including navigation, popover, safe-area background and compatibility fixes.

## 0.3.6 修正

- **Windows 切换页面更顺**：推荐、搜索和歌单这类每行都有右键菜单的列表，打开时卡住的时间短了一些。

## 0.3.5 新增

- **逐字歌词更省资源**：歌词页和桌面歌词的每个字由界面自己点亮，不用再每秒刷新 25 次。
- **Mac 安装包更小**：磁盘映像小了约四分之一。

## 0.3.5 修正

- **播放页不再跳动**：电脑和平板上，歌手下面的音质和“试听片段”一开始就留好了位置，加载出来时不再把页面撑开。

## 0.3.4 新增

- **Android 图标跟随系统取色**：Android 13 及以上打开“图标匹配主题”后，Enji 的图标会跟随壁纸颜色。

## 0.3.4 修正

- **播放更省资源**：开着逐字歌词和桌面歌词时不再卡顿掉帧，Mac 上的 CPU 占用大幅下降，Windows 也一样。
- **逐字歌词**：边缘改成硬切，不再是软边。
- **Linux 播放页点不动**：进度条、暂停、上一首和下一首都能点了。
- **Ubuntu 上的主题色**：开关、滑块和按钮不再显示系统的橙红色，而是 Enji 自己的红色。
- **Windows 10 无法添加本地音乐文件夹**：点“添加文件夹”会打开选择窗口了。

## 0.3.3 新增

- **逐字歌词更细腻**：每个字从左到右逐渐亮起，歌词页、封面下的歌词和桌面歌词都一样。macOS 14、iOS 17 及更早的系统上，整个字淡入。
- **热搜榜可以单独关闭**：在设置的可选功能里，关掉后搜索页只留搜索记录。
- **Windows 安装程序**：换成简洁的单窗口界面，安装包从约 33 MB 缩小到约 6 MB，首次安装可以自己选位置。
- **Android 安装包小了很多**：从约 28 MB 缩小到约 7 MB。
- **iOS 16**：iPhone 和 iPad 最低支持到 iOS 16，还是同一个 `-ios.ipa`。
- **Mac 的磁盘映像**：打开后有背景，把应用拖进“应用程序”即可。

## 0.3.3 修正

- **Windows 10 打不开**：不用另外安装运行时也能正常打开。
- **语言恢复默认**：在应用里选的语言，重启后不再变回系统语言。
- **版本号**：设置里一直显示 0.2.0，现在是真实版本。
- **扫码登录**：弹窗里的二维码不再被切掉，底部按钮也不再被截断。
- **取不到音源**：网易云偶尔拒绝取歌曲地址，放歌和“一起听”切歌时会报错。现在调整了请求方式，也会自动重试。
- **封面**：下载失败或卡住时自动重试，不再一直空着。
- **Windows**：播放页底部的按钮间距正常了；Windows 10 上的图标、空白网格和启动崩溃都已修复。
- **手机**：播放列表的弹窗不再超出屏幕。

## 0.3.2 新增

- **逐字歌词**：网易云给了逐字时间的歌，唱到哪个字亮到哪个字，歌词页、封面下的歌词和桌面歌词都一样。可以在设置里关掉。
- **私人漫游**：登录后首页有入口，歌放完自动接下一首，不想听就点“不喜欢”。设置里可以打开“启动时打开私人漫游”。
- **USB 独占输出**：Android 14 及以上，插 USB 解码器时按歌曲原本的采样率输出，不经过系统重采样。需要手机厂商开放这项功能。打开后系统音量不再生效，请先调低设备音量。
- **Apple TV 安装包**：`-tvos.ipa`，没有开发者签名，需要自己签名安装。
- **更多架构**：Mac 增加 Intel 版，Windows 和 Linux 增加 ARM64 版。Android 按架构拆成三个 `.apk`，下载更小。

## 0.3.2 修正

- **Android：** 在播放页点空白处不再点到下面的页面。
- **Android 平板：** 底部的播放条和音量正常显示，能打开播放页；播放页不再压到状态栏和任务栏，收起按钮也回来了。
- **iPad：** 播放页的收起按钮不再压在状态栏的图标上。
- **Linux：** `.deb` 的文件名和其他安装包统一。

## 0.3.1 新增

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

## 0.3.1 改进与修正

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
| macOS（Apple 芯片） | `-macos-arm64.dmg` |
| macOS（Intel） | `-macos-x86_64.dmg` |
| iPhone / iPad（iOS 16 及以上） | `-ios.ipa` |
| Apple TV（tvOS 18 及以上） | `-tvos.ipa` |
| Windows（x64） | `-windows-x64-setup.exe` 安装版，或 `.zip` 免安装版 |
| Windows（ARM64） | `-windows-arm64-setup.exe` 安装版，或 `.zip` 免安装版 |
| Android 8.0 及以上 | 手机、平板和电视大多用 `-android-arm64-v8a.apk`；很旧的 32 位设备用 `-android-armeabi-v7a.apk`；模拟器和 x86 设备用 `-android-x86_64.apk` |
| Linux（Ubuntu 24.04 或更新） | `-linux-x64` 或 `-linux-arm64` 的 `.deb`，或 `.tar.gz` |

**macOS**：应用没有经过 Apple 公证，第一次打开会被拦下。到“系统设置 → 隐私与安全性”底部点“仍要打开”即可。

**iPhone / iPad / Apple TV**：`.ipa` 没有开发者签名，用 AltStore、Sideloadly 等工具以自己的 Apple ID 签名后安装。

**Windows**：安装包没有代码签名，SmartScreen 提示时点“更多信息 → 仍要运行”。

**Android**：如果装过自己构建的测试版，需要先卸载再安装；之后的版本都能直接覆盖更新。

Enji 是第三方网易云音乐客户端，非网易官方出品，与网易公司无关，仅供学习交流。

## What's new in 0.3.4

- **The Android icon follows the system's colours**: on Android 13 and later, with themed icons on, Enji's icon takes the wallpaper's colours.

## What's fixed in 0.3.4

- **Lighter playback**: word-by-word lyrics and desktop lyrics no longer stutter, with much lower CPU use on the Mac and on Windows.
- **Word-by-word lyrics**: the edge is a hard cut, no longer soft.
- **Linux player page**: the progress bar, pause, previous and next respond to clicks again.
- **Theme colour on Ubuntu**: switches, sliders and buttons show Enji's own red, no longer the system's orange.
- **Adding a local music folder on Windows 10**: Add Folder opens the folder picker again.

## What's new in 0.3.3

- **Finer word-by-word lyrics**: each character lights from left to right as it is sung, on the lyrics page, under the cover and in desktop lyrics. On macOS 14, iOS 17 and earlier a character fades in whole.
- **Trending searches can go on their own**: under the optional features in Settings. With them off, the search page keeps only your search history.
- **Windows setup**: a clean single window, about 6 MB instead of 33 MB, and a first installation can pick its folder.
- **A much smaller Android package**: about 7 MB instead of 28 MB.
- **iOS 16**: iPhone and iPad now go down to iOS 16, in the same `-ios.ipa`.
- **The Mac disk image** opens with a background: drag the app onto Applications.

## What's fixed in 0.3.3

- **Windows 10 not opening**: it opens without installing a runtime.
- **The language going back**: the language chosen in the app stays through a restart.
- **The version**: Settings always said 0.2.0, and now shows the real one.
- **QR sign-in**: the code is no longer cut off in its dialog, nor the button below it.
- **Songs that would not load**: NetEase sometimes refuses a song's address, which showed as an error when playing or skipping in Listen Together. Enji now asks differently and tries again.
- **Covers** try again when a download fails or stalls, instead of staying blank.
- **Windows**: the buttons at the bottom of the playing page are spaced properly, and on Windows 10 the icons, blank grids and a crash at startup are fixed.
- **Phones**: the queue popover no longer runs off the screen.

## What's new in 0.3.2

- **Word-by-word lyrics**: on songs NetEase times word by word, the line lights up as it is sung, on the lyrics page, under the cover and in desktop lyrics. Settings can turn it off.
- **Roaming**: signed in, start it from the home page. One song follows another with no end, and Dislike moves on. Settings can open it when the app starts.
- **USB exclusive output**: on Android 14 and later, a USB DAC plays each song at its own sample rate, without the system resampling it. It needs the phone's maker to allow it. Once on, system volume no longer applies, so turn the device down first.
- **Apple TV package**: `-tvos.ipa`, not signed for distribution, so sign it yourself to install it.
- **More architectures**: Intel Macs, and ARM64 for Windows and Linux. Android comes as three smaller `.apk` files, one for each architecture.

## What's fixed in 0.3.2

- **Android**: a press on a blank part of the playing page no longer reaches the page under it.
- **Android tablets**: the player bar and the volume show at the bottom, so the playing page can be opened, and it no longer runs under the status bar and the taskbar. Its close button is back too.
- **iPad**: the playing page's close button no longer sits on the status bar's icons.
- **Linux**: the `.deb` is named like the other packages.

## What's new in 0.3.1

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

## What's better and fixed in 0.3.1

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

macOS: `-macos-arm64.dmg` for Apple silicon, `-macos-x86_64.dmg` for Intel, not notarized; allow it under System Settings → Privacy & Security → Open Anyway. iPhone / iPad (iOS 16+): `-ios.ipa`. Apple TV (tvOS 18+): `-tvos.ipa`. Neither is signed for distribution; install them with AltStore, Sideloadly or a similar tool, which signs them with your Apple ID. Windows: `-windows-x64-setup.exe` or `-windows-arm64-setup.exe`, or the portable `.zip`, unsigned. Android 8.0+: `-android-arm64-v8a.apk` for most phones, tablets and TVs, `-android-armeabi-v7a.apk` for very old 32-bit devices, `-android-x86_64.apk` for emulators and x86 devices. Linux (Ubuntu 24.04 or newer): `-linux-x64` or `-linux-arm64`, as `.deb` or `.tar.gz`.

Enji is an unofficial, third-party NetEase Cloud Music client, not affiliated with NetEase, for learning and technical exchange only.
