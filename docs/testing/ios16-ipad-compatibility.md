# iPadOS 16 兼容性修复记录

## 验证环境与边界

- 用户反馈：iPadOS 16.5、巨魔安装的 0.3.11。
- 本轮源码基线：Enji 0.3.12；不改变版本号、不发布安装包。
- 实际运行：VirtualBuddy 的 macOS 14.6.1、Xcode 15.4、既有 iPad Pro 11 英寸第四代模拟器，iOS 16.4。不是宿主新版模拟器，也不代表 16.5 真机或巨魔安装验证。
- 未迁移登录 cookie、未伪造登录状态。私人年度报告内容和需登录的最常听数据不在已验证范围。

## 分段控件

根因：iOS 16 兼容层把指针位置广播到整个窗口的 environment；抬手触发全窗口更新，在 UISegmentedControl 执行选择回调前，将其选中项重新写回旧值。因此点击浅色时，回调收到的仍可能是深色。

修复：环境只传递稳定的指针对象，只有光效子视图订阅位置变化；不自绘分段按钮，不缓存乐观选择值，也不关闭光效。最终完整应用的封面按住/松开对照截图确认触摸光效仍然响应。

真实应用诊断前端中，原生 HID 点击“自动／浅色／深色”往返成功；已对照原生控件 index、Binding setter 和引擎命令的日志。随后重新构建、安装无诊断注入的正式完整应用，真实点击得到 System → Dark → Light → Dark，并核对浅深色实际截图；没有通过 CLI 直接写入选中值代替点击。

## 触摸浏览歌词

根因：旧系统没有 SwiftUI 的 onScrollPhaseChange，原兼容分支直接返回 content，从未将触摸浏览状态通知应用。

修复：从滚动内容内部找到最近的原生 UIScrollView，观察它已有的 pan 手势；仅在拖动及惯性期间运行 CADisplayLink，读取真实 tracking/dragging/decelerating 状态。程序位移不会触发手动浏览，卸载和切后台会清理观察者。不增加竞争手势、不靠固定延时猜测结束。

使用生产桥接代码的 iOS 16.4 探针，已通过真实 HID 输入和截图验证：

- 手指拖动时 outer=active，文字由模糊变为清晰。
- 快速抬手后保持清晰；一次记录中抬手时间为 4592.527 秒，浏览结束为 4595.192 秒，覆盖约 2.66 秒惯性。
- 程序 scrollTo(45) 移动到目标行，outer/inner 均保持 idle。
- 嵌套横向滚动只让 inner=active，外层不受影响。
- 惯性期间移除内容后立即恢复 idle，无遗留活动状态。

完整应用使用实际歌曲《メドレーの観察》（3438048323）再次验收：真实拖动中 browsing=true、所有歌词 blur=0，截图中远离当前行的正文、注音和翻译均清晰；停止拖动及惯性后，播放中恢复 browsing=false、blur={0,1.2,3.5} 与当前行跟随，随后的程序滚动保持非浏览状态。暂停时保持清晰，恢复播放时退出浏览，符合应用既有逻辑。没有修改原有“播放中浏览结束三秒后恢复跟随”的规则。

旧 macOS 分支通过通知与生命周期夹具；另外在 macOS 27 的真实 NSWindow/NSHostingView 中强制选择旧分支复核，BernWatch 能收到离散滚轮 true/false，生成器同构的条件内容和多根 Group/ForEach 更新未丢失活动状态。显式模拟全部探针销毁重建可丢失原 live 阶段，但没有在现有声明的生成路径复现，不列为已确认生产缺陷。这不是旧版 macOS 的真实鼠标/触控板验收。

## 年度报告

根因：iPad 的 WKWebView 默认使用桌面浏览环境；设置手机 UA 不会把 navigator.platform 从 MacIntel 改成 iPad，远端页面因此未进入正确的移动版入口。

修复：Bern 增加与 UA 独立的 WebPage(content_mode: auto/mobile/desktop)，iOS 在创建 WKWebView 前设置 preferredContentMode；默认 auto 不改变既有行为。Enji 的年度报告显式使用 mobile，顶部另有始终由原生界面持有的关闭按钮。

同一 iOS 16.4 iPad、430×720 尺寸、相同手机 UA、不携带 cookie 的对照：默认模式只有背景与横幅，platform=MacIntel、网页关闭元素数为 0；mobile 模式 platform=iPad，出现完整年度报告入口、协议和进入按钮，网页关闭元素数为 1。已查看实际截图。

使用正式 BernWebPage 组件的无 cookie 探针，已真实点击网页自带关闭按钮并收到 web 关闭回调；打开本机不可连接地址后网页为空白，原生关闭栏仍可见，真实点击收到 native 关闭回调并返回。已查看这三种界面的截图。Enji 的已登录报告页面和私人报告内容未验收，不将组件探针替代为账户验收。

## 注音文字颜色

根因：注音经 Core Text 绘制，提前把 SwiftUI 语义 Color 桥接为 CGColor，丢失局部 SwiftUI 外观环境；普通 Text 没有这一步。

单独使用 UITraitCollection.performAsCurrent 的中间方案没有通过生产路径回归，已撤回。

最终方案：UIKit 绘制视图接收 SwiftUI 在完整局部环境中解析后的 tintColor；macOS 使用 Color.resolve(in:完整环境)。Core Text 字形、注音、彩色 emoji 和布局保持不变；mark 单独解析颜色，不在生产代码中截图采样或硬编码深浅黑白。颜色及 traits 改变时重绘。

生产代码抽取的 iOS 16.4 UI 探针 12 组原生像素断言全部通过，包括系统/局部浅深反向、同一视图动态切换、高对比、primary/secondary、透明色、独立 mark、正文和注音的彩色 emoji、折行截断、省略号及半字高亮。此次还修正了原先亮区叠加暗区使透明颜色变浓的问题，改为互斥剪裁。macOS 抽取生产代码回归通过，tvOS 16 Swift 6 类型检查通过但未实机运行。

最终完整应用的实际歌曲已核对截图：深色播放页中，注音开启时正文及假名均为白色，关闭注音后的普通文字仍为白色；全局浅色、播放页局部深色的相反环境也正常。仅构建 UI 探针不算运行验收，12 组断言的通过记录来自虚拟机中实际启动的探针。

## 完成的回归与落地

- iOS 16/17 双前端与指针订阅生成回归：1 项通过。
- 旧系统滚动生成、Swift 6 编译及生命周期夹具：4 项通过。
- 网页模式生成、默认行为及参数约束：4 项通过。
- 注音生成、原生排版及颜色回归：5 项通过；iOS 16.4 实际 UI 探针另有 12 组像素断言通过。
- 保留现有图标进度/角标依赖；修复其测试夹具访问空组件列表的问题，六个后端的生成回归均通过。
- 最终完整应用声明检查、双前端构建、安装及上述真实交互验收通过。
- Bern 最终提交的完整回归通过：`cargo fmt --all -- --check`、`cargo test --workspace --no-fail-fast -- --test-threads=1`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo check --workspace --all-targets`；只串行运行一轮，共用既有 target。
- [Bern 修复 PR](https://github.com/AimesSoft/Bern/pull/72) 已合入 master，提交 `44321c2b5ef813286fec8a190e9a5b571ed17918`。合并提交的完整源码树与验收提交 `00de620dfa6b39e27effaf7f4d25e5764a50c267` 一致，合并时三项 CI 均通过。
- Enji 固定到上述 Git 提交，恢复本地忽略的 Cargo 配置；锁文件在不加载本地 patch 的情况下生成，并通过依赖元数据核对，不依赖临时工作树。

原始诊断、日志与截图保存在忽略的 `target/diagnostics/ios16/`，不提交应用完整状态或构建产物。本轮不推送 Enji、不改变版本号、不发版；16.5 真机、巨魔安装和登录后的私人内容仍未验证。

## 后续：侧栏重复大标题

根因：侧栏已声明 `SidebarHeader(role: title)` 显示图标与名称，UIKit 生成器仍无条件追加系统导航大标题。改为标题头负责显示名称时不再生成系统大标题，保留图标旁的小标题；普通内容头部、无头部的侧栏及详情页标题不变。

[Bern 标题修复 PR](https://github.com/AimesSoft/Bern/pull/73) 已合入 master，Enji 依赖更新为 `37a347ba4f5d479821ce9e2b42af4bf612e6d2c3`。本次按不主动测试的约定未运行测试、编译或模拟器，也未重新打包；合并时三项自动 CI 尚未完成。前文的实际应用验收对应此前提交，不作为此次标题改动的运行验证。
