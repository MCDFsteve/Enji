# Windows 11 歌词页布局循环回归

## 环境与复现

- 日期：2026-11-09。
- 物理 Windows 11 主机（`bern-pc`），系统版本 `10.0.26200.9711`；不是虚拟机。
- 在已登录的交互桌面会话中运行 Enji 0.3.9，使用 WinUI 3、Windows App Runtime 2.2（包版本 `6.5.0`）。
- 安装版和由最新源码构建的未修复版均通过实际控件命令复现：

```powershell
enji.exe --engine windows ui activate --target player_bar.open_now
```

原生 stderr：

```text
bern: unhandled WinUI error 0x802b0014: Layout cycle detected.  Layout could not complete.
```

Windows 事件记录的 `0xc000027b` 是外层 stowed exception，不能把它当作直接根因。

## 根因与修复

Bern 公共 Windows 文本组件的注音布局回调，在 `LayoutUpdated` 中写回行布局策略、行高和顶部边距。除了重复写入未变化属性，原生 XAML 还会把 double API 接收到的尺寸存成 float：真机记录中 `42.9` 读回为 `42.900002`，`52.8` 读回为 `52.799999`。用 double 精确比较会永远判定“还需修正”，不断重新布局，最终被 WinUI 的布局循环检测终止。

修复在公共组件中完成：仅写入真正改变的属性，尺寸比较采用原生 float 精度。注音、逐字高亮、动态字号、动画和换行均保留；没有关闭功能、延时或增加重试。

Bern 的 `ruby_layout_reaches_a_fixed_point` 测试抽取生产模板的真实 C++ 回调，模拟精确存储和原生 float 存储，覆盖空文本、空注音、注音切换、整数与小数字号、换行尺寸、显式行高和卸载。恢复原来的精确比较时该测试因不收敛失败，修复后通过。

## 逐字高亮的第二个崩溃

布局修复后，在播放期间切换到带逐字时间轴的《Dance In The Game》（`1963695758`），以及暂停在该曲首句时重新打开歌词页，均暴露出另一个原生异常：

```text
lit size=-0.312500,40.640625 place=0.000000
bern: unhandled WinUI error 0x80070057: Invalid argument to parameter Size. Value must be equal to or greater than 0.
```

首字的原生 caret 坐标可以因字形前缘而为负。旧实现把绝对横坐标直接作为 Composition 裁剪宽度，因而在进度为零时产生负宽度。

修复将该行的左边界保留在裁剪偏移中，以“当前边界减左边界”计算宽度，静态裁剪和所有动画关键帧使用相同坐标转换；注音仍使用原始文字坐标。没有捕获并忽略异常，也没有关闭逐字动画。

`timed_highlight_preserves_negative_text_origins` 抽取生产代码的行定位、裁剪和关键帧表达式，覆盖负、零、正起点、分数进度和多行。旧实现因负宽度失败，修改后通过。

## 真机验证记录

完整修复版在同一真实 WinUI 3 进程中运行超过 11 分钟，完成：

- 《Dance In The Game》日语逐字歌词：20 次打开/收起、20 次开关注音；从首字进度零开始播放，跨多句切换和字号变化，无异常。
- 《孤勇者 (Live版)》（`1985221664`）中文逐字歌词：播放中切换歌曲并再次开关页面 5 次；截图确认正文逐字高亮仍在工作。
- 《Summer》（`443242`）纯音乐：空注音、空逐字路径，“纯音乐，请欣赏”场景再开关页面 5 次。
- 从宽窗口缩小至紧凑布局，再恢复宽窗口，无进程退出。
- 查看日文、中文及缩放截图；桌面歌词的注音和高亮仍可见。
- 共 30 次开关歌词页。最终 stderr 只有 WinUI 3 启动信息，没有未处理异常；对应启动时间之后的 Application Error（事件 1000）中没有新的应用崩溃。

Bern 修复的自动回归及静态检查均通过：

- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo check --workspace --all-targets`
- `cargo fmt --all -- --check`

Enji 升级依赖和版本至 0.3.10 后，`cargo check --workspace --all-targets` 通过。Enji 自身的 `cargo fmt --all -- --check` 未通过：32 个现有 Rust 文件有格式差异，本次没有修改这些文件，也没有混入全仓格式化。

本次没有运行 Windows 10 或其他平台的应用，没有替换真机原先安装的程序。回归构建使用 0.3.9 版本标识，确认修复后将发行版本提升至 0.3.10；核心修复已经合入 [Bern 修复请求](https://github.com/AimesSoft/Bern/pull/59)，Enji 固定使用合并提交 `f1de4e50ca8cd974bf3edab674905f9c83da1afd`。截图和原始诊断日志保留在本地 `target/diagnostics/win11-lyrics/`，不提交崩溃转储或构建产物。
