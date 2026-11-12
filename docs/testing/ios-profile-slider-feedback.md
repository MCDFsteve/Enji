# iOS 个人资料与滑杆反馈修复记录

## 状态

2026-11-12：源码已实现并提交，**尚未构建、运行测试或进行设备验收**。本轮没有测试授权，没有发布新版。框架修复：https://github.com/AimesSoft/Bern/pull/70 ，提交 `614f982ac546f793bf5b9114824dce9e8d9ec18e`；PR 尚未合并。

## 退出登录右偏

个人资料的声明本来就是 `Spacer → Text → Spacer`。SwiftUI 后端原先把文字和它后面的 Spacer 折叠成左对齐无限宽文字，破坏左右对称分配。

修复在 Bern 的共享行布局判定：多个 Spacer 保留各自的空间，不借给文字；条件分支和重复项也参与判定。SwiftUI、Compose 使用同一判定，不在应用按钮上加偏移。新增生成与 IR 回归用例，未执行。

## iOS 26 以上滑杆反馈

使用系统 SwiftUI `sensoryFeedback(.selection, trigger:)`，仅 iOS 26 以上启用，遵循系统设置与硬件支持。不使用自制震动发生器，也不把仅在 visionOS 播放的 `.press(.slider)` / `.release(.slider)` 当作 iPhone 接口。

- 拖动开始时请求反馈，拖动跨反馈档位时再次请求。
- 有 step 时使用 step 档位；连续值按轨道百分比分反馈档位，不改变实际 seek 或音量精度。
- 只有系统 Slider 的交互 binding 触发；初始化、播放自然推进、快照回写和程序控制不触发。
- 集中覆盖受控值 Slider、草稿绑定 Slider、媒体进度条。只读 Progress 不震动。
- 旧 iOS、macOS、tvOS 不新增反馈行为。模拟器不能证明真机触觉手感。

官方资料：
- https://developer.apple.com/documentation/swiftui/sensoryfeedback/selection
- https://developer.apple.com/documentation/swiftui/view/sensoryfeedback(_:trigger:)

## 未解决：入场后封面再次上移

**尚未确定根因，不能视为已修复。** 静态路径为 shell 的 `move(bottom, sheet)` → BernWholeWindow → BernEntrance。整页安全区通过 geometry 回调回写状态，入场依赖窗口级 settled，但这些代码不足以证明此次二次位移一定来自安全区晚更新。

当前歌词区固定 68、说明行固定 18；封面图片按固定比例占位，图片下载本身不决定占位。未采用延时、二次补位或修改弹簧参数掩盖问题。

后续获运行授权后，应在当前源码构建上记录首次入场期间的窗口 bounds、安全区、封面与控制区布局，区分布局变化和绘制位移，再落地根因修复。现有 target 中旧 Swift 生成文件不是当前源码，不能作为当前版本的运行证据。
