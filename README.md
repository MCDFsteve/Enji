<div align="center">

<img src="assets/icon/enji.png" width="112" alt="Enji Music 图标">

<h1>Enji Music</h1>

<p><b>用 Rust 写成的网易云音乐客户端，每个平台都是原生应用。</b></p>

<p>
<img alt="Rust" src="https://img.shields.io/badge/Rust-1.99-CE422B?style=for-the-badge&logo=rust&logoColor=white">
<a href="https://github.com/AimesSoft/Bern"><img alt="Built with Bern" src="https://img.shields.io/badge/Built%20with-Bern-222222?style=for-the-badge"></a>
<img alt="NetEase Cloud Music" src="https://img.shields.io/badge/%E7%BD%91%E6%98%93%E4%BA%91%E9%9F%B3%E4%B9%90-%E5%9C%A8%E7%BA%BF%20%2B%20%E6%9C%AC%E5%9C%B0-E60026?style=for-the-badge">
</p>

<p>
<img alt="macOS · SwiftUI" src="https://img.shields.io/badge/macOS-SwiftUI-555555?style=flat-square&logo=apple&logoColor=white">
<img alt="iOS · SwiftUI" src="https://img.shields.io/badge/iOS-SwiftUI-555555?style=flat-square&logo=apple&logoColor=white">
<img alt="Windows · WinUI 3" src="https://img.shields.io/badge/Windows-WinUI%203-555555?style=flat-square&logo=windows11&logoColor=white">
<img alt="Android · Jetpack Compose" src="https://img.shields.io/badge/Android-Compose-555555?style=flat-square&logo=android&logoColor=white">
<img alt="Linux · GTK 4" src="https://img.shields.io/badge/Linux-GTK%204-555555?style=flat-square&logo=gtk&logoColor=white">
<img alt="MCP built in" src="https://img.shields.io/badge/AI-MCP%20built%20in-555555?style=flat-square">
<a href="LICENSE"><img alt="License: MIT" src="https://img.shields.io/badge/license-MIT-E60026?style=flat-square"></a>
</p>

<p><b>简体中文</b> · <a href="README.en.md">English</a></p>

</div>

<table>
<tr>
<td width="50%" align="center"><a href="docs/screenshots/macos-charts.png"><img src="docs/screenshots/macos-charts.png" alt="macOS 上的排行榜页：左侧边栏，网格排列的榜单封面，底部播放条"></a><br><sub>macOS · 排行榜</sub></td>
<td width="50%" align="center"><a href="docs/screenshots/macos-now-playing.png"><img src="docs/screenshots/macos-now-playing.png" alt="macOS 上的播放页：左边封面和播放控制，右边逐行歌词与中文翻译，背后是模糊的封面"></a><br><sub>macOS · 播放页</sub></td>
</tr>
<tr>
<td width="50%" align="center"><a href="docs/screenshots/windows-charts.png"><img src="docs/screenshots/windows-charts.png" alt="Windows 上的排行榜页：左侧导航栏，网格排列的榜单封面，底部播放条"></a><br><sub>Windows · 排行榜</sub></td>
<td width="50%" align="center"><a href="docs/screenshots/windows-now-playing.png"><img src="docs/screenshots/windows-now-playing.png" alt="Windows 上的播放页：左边封面和播放控制，右边逐行歌词与中文翻译，背后是模糊的封面"></a><br><sub>Windows · 播放页</sub></td>
</tr>
</table>

<table>
<tr>
<td align="center"><a href="docs/screenshots/iphone-duo-outer-charts.png"><img src="docs/screenshots/iphone-duo-outer-charts.png" width="190" alt="iPhone Duo 外屏上的排行榜，系统的竖向标签栏在右侧"></a></td>
<td align="center"><a href="docs/screenshots/iphone-duo-outer-now-playing.png"><img src="docs/screenshots/iphone-duo-outer-now-playing.png" width="190" alt="iPhone Duo 外屏上的播放页：大封面、进度条和播放控制"></a></td>
<td align="center"><a href="docs/screenshots/android-charts.png"><img src="docs/screenshots/android-charts.png" width="150" alt="Android 上的排行榜，底部是迷你播放器和导航栏"></a></td>
<td align="center"><a href="docs/screenshots/android-lyrics.png"><img src="docs/screenshots/android-lyrics.png" width="150" alt="Android 上的播放页：歌词代替封面显示，逐行歌词带中文翻译，下方是进度条和播放控制"></a></td>
</tr>
<tr>
<td colspan="2" align="center"><sub>iPhone Duo 外屏</sub></td>
<td colspan="2" align="center"><sub>Android</sub></td>
</tr>
</table>

<p align="center"><a href="docs/screenshots/iphone-duo-inner-now-playing.png"><img src="docs/screenshots/iphone-duo-inner-now-playing.png" width="640" alt="iPhone Duo 展开后的内屏播放页：封面在左半屏，歌词在右半屏，中间隔着铰链"></a><br><sub>iPhone Duo 展开：封面和歌词分在铰链两侧</sub></p>

## 特色

- **轻。** macOS 版 `.app` 13.9 MiB，压缩后 7 MiB；停在排行榜页时占用 61 MiB 内存。
- **有景深的歌词。** 正在唱的一行最清楚，上下几行逐渐虚化，背后铺着模糊的封面。
- **本地音乐也有歌词和封面。** 本机的 FLAC、MP3 会自动配上网易云里同一首歌的歌词和封面，和在线歌单在一个曲库里搜索。关掉网易云，Enji 就是一个纯本地播放器。
- **AI 能直接用。** 自带 MCP 服务器和命令行。Claude 这样的 AI 可以搜歌、放歌、切换页面，看的是界面上的控件，而不是截图。

## 功能

- **听网易云**：每日推荐、推荐歌单和 60 多个排行榜，歌单、专辑、歌手都能点进去看。不登录也能看排行榜、搜歌、听免费歌曲。
- **登录**：扫码、手机验证码或密码登录，登录信息只保存在你自己的设备上。
- **我的歌单**：我喜欢的音乐、自建和收藏的歌单；可以新建歌单、把歌加进去、给歌点红心。
- **搜索**：网易云和本机的歌一起搜，有热搜榜和搜索记录。
- **歌词**：跟着歌逐行滚动，带中文翻译，点一行就跳到那里；也能看歌曲评论。
- **音质**：标准、较高、极高、无损、Hi-Res 可选（320k 以上需要网易云会员）。
- **本地音乐**：MP3、FLAC、M4A、WAV 等常见格式都能放。电脑上添加文件夹；iPhone 上把歌放进“文件”App 的 Enji 文件夹；Android 上选一个文件夹。
- **播放记录**：最近听过什么，每首歌听了几次。
- **外观**：浅色、深色或跟随系统，中文和英文界面。

## 交给 AI

每次构建都会在 `target/bern/<平台>/bin/` 生成 `enji` 命令行和 `enji-mcp`，`skills/enji/SKILL.md` 里是全部命令的说明。

```sh
enji search search --text "Yesterday Once More"
enji enji play --source Search --index 1
enji state
```

## 从源码构建

```sh
git clone https://github.com/MCDFsteve/Enji.git
cd Enji
cargo build --manifest-path vendor/bern/Cargo.toml -p bern-cli --release
vendor/bern/target/release/bern build --target macos --release
open target/bern/macos/Enji.app
```

`--target` 可以换成 `windows`、`linux`、`android` 或 `ios`，需要在对应平台的主机上构建。

| 目标 | 需要 |
| --- | --- |
| macOS / iOS | Xcode 26 或更新版本；iOS 另需 `rustup target add aarch64-apple-ios aarch64-apple-ios-sim` |
| Windows | Visual Studio C++ 生成工具与 Windows SDK |
| Android | JDK 21、Android SDK 与 NDK，`rustup target add aarch64-linux-android` |
| Linux | GTK 4 与 libadwaita 开发包 |

`vendor/bern/` 是一份 Bern 快照，带有 Enji 用到、上游还没有的封面光效和模糊背景，来源见 [UPSTREAM.md](vendor/bern/UPSTREAM.md)。

## 现状

macOS 和 Windows 实机、iPhone Duo 模拟器、Android 模拟器上实际用过浏览、搜索、播放和歌词。Linux 版只通过了 `bern check`。

Enji 不是网易云音乐的官方客户端，与网易公司无关。

## 开源协议与致谢

[MIT](LICENSE)。

- [Bern](https://github.com/AimesSoft/Bern)：界面框架
- [ncm-api-rs](https://github.com/SPlayer-Dev/ncm-api-rs)：网易云音乐接口
- [Misuzu Music](https://github.com/AimesSoft/MisuzuMusic)：随机播放、本地歌曲识别和封面光效的做法
- [symphonia](https://github.com/pdeljanov/Symphonia)、[rodio](https://github.com/RustAudio/rodio)、[lofty](https://github.com/Serial-ATA/lofty-rs)：解码、播放和标签
- [Fluent UI System Icons](https://github.com/microsoft/fluentui-system-icons)：图标里的均衡器图形
