# Enji 插件 API v2 参考

插件是 Enji 加载的一份 JavaScript 脚本。脚本用 `enji.on` 接收事件，用 `enji.song` 等属性读最近一次快照，再向宿主请求操作。完整签名见 [编辑器类型声明 `enji.d.ts`](enji.d.ts)。

| 源码示例 | 用它做什么 |
| --- | --- |
| 只减不增：[pack.json](examples/attenuation-only/pack.json) · [main.js](examples/attenuation-only/main.js) | 最小响度策略。只申请 `audio_gain`，已知建议为正时改为 0 dB，不分析节拍。 |
| 歌曲书签：[pack.json](examples/bookmarks/pack.json) · [main.js](examples/bookmarks/main.js) | 记下每首歌的位置，在歌曲菜单回听；查看队列里的书签和书签处的歌词。文末有[五组 API 的操作对照](#用书签核对五组-api)。 |

## 加载一个插件

- **直接用示例源码**：在桌面版 Enji 的数据目录下新建 `plugins/<pack.json 中的 id>/`，把该示例的 `pack.json`、`main.js` 放进去。比如 macOS 的书签目录通常是 `~/Library/Application Support/Enji/plugins/example.playback-bookmarks/`；Windows 数据目录是 `%APPDATA%\Enji`，Linux 是 `$XDG_DATA_HOME/Enji`（未设置时为 `~/.local/share/Enji`）。重新打开 Enji，在「设置 → 插件」里启用。修改脚本后，关闭再启用插件以重新加载。
- **从文件导入**：「导入插件…」读取 `.enj`，即根目录有 `pack.json` 和入口脚本的 ZIP；不要把它们再套一层目录。导入后在同一面板启用。电视、手表界面没有这个文件导入按钮。本参考只提供源码；两个示例都不需要图标、商店索引或网络服务。
- **编辑器补全**：把 `enji.d.ts` 放在自己的 `main.js` 旁，在脚本顶部加 `/// <reference path="./enji.d.ts" />`。示例已经引用仓库里的声明。它是给编辑器用的，不要在运行时 `import` 或 `require`。启用 JS 类型提示时，建议编辑器只加载 ECMAScript 库（如 `lib: ["ES2020"]`）；浏览器 DOM、Node.js 模块不是插件 API。

### `pack.json`

| 字段 | 格式与约束 |
| --- | --- |
| `id`、`name`、`version` | 必填非空字符串。`id` 只用 ASCII 字母、数字、点、下划线和连字符，不能是 `.`、`..`。更新时保持 `id` 不变。 |
| `description`、`author` | 可选字符串，给用户看的简介、作者。 |
| `main`、`icon` | 包内相对文件名；`main` 默认 `main.js`，须为 UTF-8 文本。`icon` 可省略。不使用绝对路径、`..`、反斜杠或盘符。 |
| `permissions` | 权限字符串数组，只列确实需要的项，见下表。 |
| `platforms` | 可选数组，不写或空数组表示不限制。可用 `macos`、`windows`、`linux`、`android`、`ios`、`tvos`、`watchos`；Android TV 和 Wear OS 也属于 `android`。 |
| `minHostVersion` | **Enji 应用**的最低版本，可省略。须为无前缀、无范围、无多余前导零的三段稳定版号，如 `"0.3.19"`；不是 API 代数。当前宿主会在安装、更新和启动时判断兼容性。 |
| `settings`、`actions` | 可选数组，由宿主生成设置控件和菜单；结构见[设置与菜单动作](#设置与菜单动作)。 |

## 能力与权限

`enji.apiVersion` 在这个分支上是 `2`；`enji.hostVersion` 是实际应用版本字符串。**不能只凭应用版本号判断有无 v2**：旧宿主可能没有 `supports`，甚至与这个分支有相同的应用版本号。能力名区分大小写，未知能力返回 `false`。`supports` 回答“宿主会不会”，`plugin.hasPermission` 回答“本插件声明了哪项权限”。

```js
if (typeof enji.supports === "function" && enji.supports("queue.page") && plugin.hasPermission("queue")) {
  // 可以使用 queue.requestPage；拿到的是请求 ID，不是数组。
}
```

| `enji.supports(key)` 的 key | 清单权限 | 接口 |
| --- | --- | --- |
| `playback.state` | 无 | `enji.song`、`enji.playing`、`enji.playback`，`song` / `playback` 事件 |
| `playback.control` | `playback_control` | 暂停、继续、上下首、跳转 |
| `queue.page` | `queue` | `queue.requestPage`、`queuePage`、`queueChanged` |
| `lyrics.timeline` | `lyrics` | `lyrics.requestTimeline`、`lyricsTimeline`、`lyricsChanged`；旧 `lyric` 事件也需此权限 |
| `audio.levels` | `audio` | 旧 `audio` 事件：实时电平和节拍，无单独的 `audio.levels()` 方法 |
| `audio.loudness` | `loudness` **或** `audio_gain` | 只读 `enji.loudness`、`loudness` 事件 |
| `audio.trackGain` | `audio_gain` | `audio.suggestTrackGain`。还需用户选中该插件作为处理方式，建议才可能被采用。 |
| `plugin.settings` | 无额外权限 | 清单 `settings`、`settings.get`、`settingChanged` |
| `plugin.actions` | 无额外权限 | 清单 `actions`、`action`；动作内调用其他 API 仍需其权限。 |

旧的 `cover`、`overlay` 权限也仍可用。只关心整曲响度时**不要**申请 `audio`：`loudness`、`audio_gain` 都不会因此启动实时电平或节拍检测。

## 播放状态与控制

`enji.on(event, handler)` 注册回调，参数是事件数据，不是 `{type, data}` 的外层消息。每个启用的插件有自己的上下文；脚本顶层适合注册回调。快照不是会随时钟走动的计时器：在回调里读本次看到的状态。`playback`、`queue`、`lyrics`、`audio`、`settings` 这些全局对象也分别可从 `enji` 上取到。

- `enji.song`：当前曲目或 `null`。曲目、队列行、歌曲菜单的目标都用 `{id: number, name: string, artists: string, album: string, duration: number, trackKey: string}`；`artists` 是显示文字，`duration` 为秒。不提供本地路径或播放 URL。
- `enji.playback`：`{playing, position, duration, mode, playbackToken}` 及控制方法。`playing` 是布尔值（也可读 `enji.playing`）；`position`、`duration` 均为秒。`mode` 为 `repeatAll`、`repeatOne`、`shuffle`、`heart`。首次收到状态前，`playing` 为 `false`，两项时间为 `0`，`mode`、`playbackToken` 为 `null`。
- `enji.on("song", fn)` 收曲目，`enji.on("playback", fn)` 收播放状态字段；实际控制是否生效，以后续状态为准。
- `playback.pause()`、`resume()`（别名 `play()`）、`next()`、`previous()`、`seekTo(seconds)`（别名 `seek(seconds)`）返回布尔值。`seekTo` 接受从曲首起算的非负有限秒数，不接受毫秒或进度百分比。无权限、没有当前 token 或参数非法时返回 `false`；`true` 只表示请求已入队。`play()` 不用于播放某个 ID 的歌曲。

不要混用三个键：**`trackKey` 标识歌曲**（适合持久化书签）；**`playbackToken` 标识一次播放**（字符串，同一首重新播放也不能沿用旧 token）；**`sourceKey` 标识实际音源版本**（用于响度建议）。它们都是不透明值，不要拼装、解析或用歌名、队列下标替代。控制方法自动携带调用时的播放 token；宿主会丢弃旧 token 的请求。若从菜单或异步回包发起控制，插件仍须确认目标 `trackKey` 等于 `enji.song.trackKey`；跨事件还须核对最初捕获的 token，以免把过期操作绑定到新播放。

## 队列与歌词：发请求，再等对应事件

| 方法 | 回包事件 | 变更通知 |
| --- | --- | --- |
| `queue.requestPage(offset = 0, limit = 50)` | `queuePage` | `queueChanged`：`{revision, total, currentIndex, playbackToken}` |
| `lyrics.requestTimeline(offset = 0, limit = 50)` | `lyricsTimeline` | `lyricsChanged`：`{revision, total, playbackToken}` |

`offset` 是从 0 开始的安全整数，`limit` 是 **1 至 100** 的安全整数。两种请求合计每个插件实例最多 16 个在途。返回非空字符串 `requestId` 或 `null`，**不是 Promise**。参数非法、无权限、达到上限或歌词请求缺少当前播放时返回 `null`，不用等回包。回包只发给发起它的插件实例：先注册监听，再请求，用 `requestId` 对应本次操作。插件停用或重载后不要复用旧请求。

```ts
// 成功与失败都有四个字段，先判断 ok。
{ requestId: string, ok: true,  page: { offset, total, items, playbackToken, revision, currentIndex? }, error: null }
{ requestId: string, ok: false, page: null, error: string }
```

- `offset`、`total`、`revision` 均为数字，`total` 是整份列表的长度；`items` 只含本页，可为空。队列页按显示顺序返回曲目；仅队列带 `currentIndex`，是整份队列的零基位置，无当前项时为 `null`，与本页 `offset` 无关。队列的 `playbackToken` 可为 `null`。
- 歌词请求绑定当时的 `playbackToken`，成功页返回同一个 token；换歌后旧请求可能得到 `ok: false`。两种 `*Changed` 都是变更通知，不携带整份数据。只在需要时重新请求；翻页时比对同一列表的 `revision` 和相关播放 token，变了就丢弃旧结果。下一页从 `page.offset + page.items.length` 开始；空页或到达 `total` 就停。
- 一行歌词为 `{time, text, translation, words, ruby}`。`time` 从**曲首**起算，单位为秒；`text` 是原文，`translation` 是字符串或 `null`。`words` 是 `{time, duration, start, end}[]`，其中 `time` 也是曲首绝对秒、`duration` 是持续秒数。`ruby` 是 `{start, end, reading}[]`，`reading` 是读音。没有逐字时间或注音时，相应数组为空。
- `start`、`end` 均引用本行原文，按 **Unicode 标量的半开区间 `[start, end)`** 计数，不是 JS 的 UTF-16 下标，也不是字节或字形簇。比如 `A𠮷B` 的 `[1, 2)` 是完整的 `𠮷`：用 `Array.from(line.text).slice(start, end).join("")`，不要直接 `line.text.slice(start, end)`。

## 整曲响度与增益建议

`enji.loudness` 是最近一次响度快照；`enji.on("loudness", fn)` 收到同一形状的新数据。无读取权限时属性为 `null`，不会收到事件。有权限也须处理 `null`：例如还没有可识别的播放或尚未收到音源信息。对象字段如下，**未知值不是 0**。

| 字段 | 类型与含义 |
| --- | --- |
| `playbackToken`、`sourceKey` | 本次播放的字符串 token；实际音源版本键为 `string \| null`。版本尚未识别或试听时，不能假定有键。 |
| `status` | `pending` 等待资料，`measuring` 正在测量，`ready` 可用，`unavailable` 无可靠结果，`deferred` 本次推迟，`preview` 试听，`disabled` 归一化已关闭。不要假定它们依次到达。 |
| `source` | `"replaygain"` 表示标准曲目标签，`"measured"` 表示整曲测量；未知为 `null`。 |
| `integratedLufs` | 整曲综合响度（LUFS），`number \| null`。ReplayGain 标签不是测得的 LUFS。 |
| `truePeakDbtp`、`samplePeakDbfs` | 真峰值（dBTP）、样本峰值（dBFS），分别为 `number \| null`。二者不能互换。 |
| `recommendedGainDb` | `number \| null`。宿主经过防削波限制的建议增益（dB），不是实际已生效的值。 |
| `appliedGainDb`、`bitPerfectBypass` | 输出确认的本次实际增益（dB），未确认前为 `null`；布尔值表示已确认的位完美旁路，确认前为 `false`。`0 dB` 表示不增不减，不是静音。 |
| `enabled`、`provider` | `enabled` 是用户的归一化开关。`provider` 为内置时是 `null`，否则是用户选中的策略插件 ID；选择了插件不等于它已对本次播放给出建议。 |

内置策略对整曲测量以 **−18 LUFS** 为目标。使用 ReplayGain 时按可靠的曲目增益标签处理；不把来源或单位不明确的云端 `gain` 当作 LUFS 或增益。内置推荐和插件建议都受峰值上限约束：已知真峰值时留出余量；**不知道真峰值就不放大**，即使已有样本峰值。缺乏可靠资料时按安全值播放。

`audio.suggestTrackGain(sourceKey, gainDb): boolean` 接受当前响度对象给出的非空键和 **−24 至 +12 dB**（含端点）的有限数。它是建议，不是音量推子：

1. 只有声明 `audio_gain`、已启用且被用户选为响度处理方式的插件，其建议才会被宿主采用。`true` 表示 SDK 已排队，不承诺采用；切源、换策略、权限不符或没有可靠资料等情况下，可忽略建议。没有可用的插件建议就用内置安全值。
2. 建议按插件 ID、版本、设置值和音源版本缓存。设置改变前发出、改变后才返回的建议会被丢弃，不会记到新设置名下。宿主在起播时选择增益；晚来的测量、建议或拖动进度（seek）都不会让正在播放的歌中途变响。试听和实际位完美输出旁路，不改变用户的音量设置。
3. 后台只测已收齐且干净解码到 EOF 的音源。中断或残缺数据不当整曲，繁忙时可以推迟，不为测量重新下载。能在起播前识别音源版本（例如有有效的内容 MD5）时，可直接复用上次的结果。若只能读完整份数据才能得到内容摘要，下次起播前也未必能命中缓存；不会根据歌名、大小或码率猜测它是同一份录音。

要体验[只减不增策略](examples/attenuation-only/main.js)，启用插件，在设置中打开「响度归一化」，在「响度处理方式」选「只减不增」。它只在资料就绪且被选中时建议：安静歌曲即使允许放大也建议 0 dB，响的歌在插件建议范围内保留衰减。要比较策略，重新开始播放同一音源，不是拖回开头。

## 设置与菜单动作

```json
{
  "settings": [
    { "id": "hint", "label": "显示提示", "type": "boolean", "default": true },
    { "id": "lead", "label": "回听提前秒数", "type": "number", "min": 0, "max": 10, "default": 2 },
    { "id": "anchor", "label": "书签位置", "type": "enum", "options": ["此刻", "行首"], "default": "此刻" }
  ],
  "actions": [
    { "id": "save", "label": "留下书签", "context": "player" },
    { "id": "recall", "label": "查看歌曲书签", "context": "track" }
  ]
}
```

这是清单片段。`settings`、`actions` 各最多 64 项；同一数组内 `id` 唯一，遵循插件 ID 的字符规则且最多 64 字节，`label` 为 1 至 128 个字符。布尔默认值须是真正的布尔值；数值必须声明有限 `min < max`，默认值位于闭区间内；枚举 `options` 为 1 至 64 个互不重复的非空字符串（每项至多 128 个字符），默认值必须是其中之一。

- `settings.get(id)` 同步读取本插件已声明的有效值，返回 `boolean | number | string`；未知 ID 返回 `null`。用户设置由宿主管理和保存，脚本不能设值。同一插件更新后仍读取有效设置；旧值不符合新声明时回到新默认值。`enji.on("settingChanged", ({id, value}) => …)` 只在本插件的有效值变更后通知；此时 `get(id)` 已是新值。这不是新的播放快照。
- `enji.on("action", ({id, context, track}) => …)` 只投给声明该动作的已启用插件。`player` 对应播放页「…」中的动作，`track` 对应歌曲行菜单的「插件操作…」。`track` 上下文给**实际点击的行**，不是自动指向当前歌曲；`player` 没有目标时 `track` 可为 `null`。用 `id` 分派逻辑，不要比对显示标签。

## 原有接口速查

| 入口 | 说明 |
| --- | --- |
| `plugin.id`、`name`、`version`、`permissions`、`hasPermission(name)` | 本插件的身份与声明。`enji.platform` 是平台名。 |
| `enji.on("lyric", fn)` | 需 `lyrics`。当前行的 `{index, text, translation, next}`；后三项是显示字符串，没有翻译可为空串。与 `lyricsTimeline` 中可为 `null` 的翻译不同。 |
| `enji.on("audio", fn)` | 需 `audio`。播放中约每秒十次，并在节拍时送达 `{level, bass, beat, bpm, position}`；`beat` 是 0 到 1 的强度，拍间为 0，`position` 是秒。不是每一音频帧的时钟，也不是 LUFS。 |
| `cover.pulse(strength?)` | 需 `cover`。封面跳动强度 0 到 1，默认 0.5。返回是否已排队。 |
| `overlay.show(id, spec)`、`overlay.hide(id)` | 需 `overlay`，返回是否已排队。`spec` 为 `{lines, place?, locked?}`；最多四行，每行 `{text, size?, bold?, color?, ruby?}`。颜色为 `#rrggbb` 或 `#rrggbbaa`，`place` 为 `free`（默认）或 `taskbar`，`locked` 默认 `true`。实际显示位置依平台。 |
| `storage.get(key)`、`storage.set(key, value)` | 本插件按键保存 JSON 数据，跨启停保留；读不存在的键返回 `null`。适合书签，不替代用户可编辑的 `settings`。 |
| `ui.toast(message)`、`console.log/warn/error(...)` | 显示简短提示，或写宿主日志；无需额外权限。 |

## 用书签核对五组 API

在支持上述能力的 Enji 中启用「歌曲书签」。选一个至少两首歌的队列，让有时间轴歌词的 A 排在前三首。暂停在 A 的 **0:20** 左右；下面是操作和应看到的结果，不需要控制台。

| API 组 | 操作 | 可观察结果 |
| --- | --- | --- |
| 菜单动作 `plugin.actions` | 在播放页「…」选「在此刻留下书签」。再从未播放的 B 的歌曲行打开「插件操作… → 回到这首歌的书签」。 | 第一次提示记下 **A** 及位置；第二次显示 **B** 自己的书签或「没有书签」。A 不跳转，也不会切到 B。 |
| 播放 `playback.state/control` | 拖离保存位置，只从 **A 自己**的歌曲行选「回到这首歌的书签」。 | 出现跳转请求提示，实际进度回到保存的约 0:20。插件不会在分页回包中发起跳转。 |
| 设置 `plugin.settings` | 在「设置 → 插件 → 歌曲书签」把「回听提前秒数」改成 5，再从 A 的行回到书签。 | 改值出现「回听提前量」提示，跳转约到 0:15；书签仍是 0:20。关掉再启用插件，生效值和书签仍在。 |
| 队列 `queue.page` | 在播放页选「查看队首三首的书签」。改动队列后再点一次。 | 按当前显示顺序列出最多三首、各自的书签或「无书签」，并显示队列总数。不会把请求 ID 当作列表。 |
| 歌词 `lyrics.timeline` | 在播放页选「查看书签处的歌词」，可在第 50 行之后留下新书签再查看。 | 按保存的秒数（不是提前量）找到当时那行；正在唱的词有逐字数据时显示词，行有注音时显示首个读音。无歌词或书签在第一行前有独立提示。换歌或歌词修订时不拼接旧页。 |
