// Enji 插件 API v2；用于编辑器补全，不是运行时脚本。Enji 是仅供注解使用的类型命名空间。
declare namespace Enji {
  type Platform = "macos" | "windows" | "linux" | "android" | "ios" | "tvos" | "watchos";
  type Capability = "playback.state" | "playback.control" | "queue.page" | "lyrics.timeline"
    | "audio.levels" | "audio.loudness" | "audio.trackGain" | "plugin.settings" | "plugin.actions";
  type Permission = "audio" | "cover" | "overlay" | "lyrics" | "queue" | "playback_control" | "loudness" | "audio_gain";
  type PlayMode = "repeatAll" | "repeatOne" | "shuffle" | "heart";
  type SettingValue = boolean | number | string;
  type JsonValue = null | boolean | number | string | readonly JsonValue[] | { readonly [key: string]: JsonValue };

  interface Plugin {
    readonly id: string;
    readonly name: string;
    readonly version: string;
    readonly permissions: readonly string[];
    hasPermission(permission: string): boolean;
  }
  /** enji.song、歌曲动作目标和队列项共用；不含文件路径或音源地址。 */
  interface Track {
    readonly id: number;
    readonly name: string;
    readonly artists: string;
    readonly album: string;
    /** 秒。 */
    readonly duration: number;
    /** 曲目标识，可作书签键。不是响度策略的 sourceKey。 */
    readonly trackKey: string;
  }
  interface PlaybackState {
    readonly playing: boolean;
    /** 曲首起算的秒数。 */
    readonly position: number;
    /** 秒。 */
    readonly duration: number;
    readonly mode: PlayMode | null;
    /** 标识一次播放；没有可操作的播放时为 null。 */
    readonly playbackToken: string | null;
  }
  /** 状态无需权限；控制需 playback_control。true 仅表示已入队。 */
  interface Playback extends PlaybackState {
    pause(): boolean;
    resume(): boolean;
    /** resume 的别名，不按 ID 选歌。 */
    play(): boolean;
    next(): boolean;
    previous(): boolean;
    /** 非负有限秒数；用调用时快照的 token 操作，不修改别的播放。 */
    seekTo(seconds: number): boolean;
    /** seekTo 的别名。 */
    seek(seconds: number): boolean;
  }

  /** 旧 lyric 事件：当前行的简报，不是完整歌词时间轴。 */
  interface CurrentLyric {
    readonly index: number;
    readonly text: string;
    readonly translation: string;
    readonly next: string;
  }
  /** 旧 audio 事件：实时电平和节拍，不是整曲响度。 */
  interface Levels {
    readonly level: number;
    readonly bass: number;
    readonly beat: number;
    readonly bpm: number;
    readonly position: number;
  }
  /** 行内 Unicode 标量半开区间 [start, end)，用于 Array.from(text).slice(start, end)。 */
  interface TextRange {
    readonly start: number;
    readonly end: number;
  }
  interface LyricWord extends TextRange {
    /** 从曲首起算，不是从本行起算。单位为秒。 */
    readonly time: number;
    readonly duration: number;
  }
  interface Ruby extends TextRange { readonly reading: string; }
  interface LyricLine {
    /** 从曲首起算，单位为秒。 */
    readonly time: number;
    readonly text: string;
    readonly translation: string | null;
    readonly words: readonly LyricWord[];
    readonly ruby: readonly Ruby[];
  }
  interface LyricsChanged {
    readonly revision: number;
    readonly total: number;
    readonly playbackToken: string | null;
  }
  interface QueueChanged extends LyricsChanged {
    /** 队列显示顺序中的零基位置，没有当前项时为 null。 */
    readonly currentIndex: number | null;
  }
  interface QueuePage extends QueueChanged {
    readonly offset: number;
    readonly items: readonly Track[];
  }
  interface LyricsPage extends LyricsChanged {
    readonly playbackToken: string;
    readonly offset: number;
    readonly items: readonly LyricLine[];
  }
  type PageReply<P> =
    | { readonly requestId: string; readonly ok: true; readonly page: P; readonly error: null }
    | { readonly requestId: string; readonly ok: false; readonly page: null; readonly error: string };
  interface Queue {
    /** 默认 0/50，limit 为 1..100。返回 requestId，监听 queuePage；未发出时返回 null。 */
    requestPage(offset?: number, limit?: number): string | null;
  }
  interface Lyrics {
    /** 同样分页；绑定当前播放，监听 lyricsTimeline。不是 Promise。 */
    requestTimeline(offset?: number, limit?: number): string | null;
  }

  type LoudnessStatus = "pending" | "ready" | "measuring" | "unavailable" | "deferred" | "preview" | "disabled";
  interface Loudness {
    readonly playbackToken: string;
    /** 标识实际音源版本，不是 trackKey；未识别或试听时可空。 */
    readonly sourceKey: string | null;
    readonly status: LoudnessStatus;
    readonly source: "replaygain" | "measured" | null;
    readonly integratedLufs: number | null;
    readonly truePeakDbtp: number | null;
    readonly samplePeakDbfs: number | null;
    /** 宿主防削波后的建议，不等于已经生效。 */
    readonly recommendedGainDb: number | null;
    /** 输出已确认的本次实际增益；尚未确认时为 null，旁路的 0 dB 不是静音。 */
    readonly appliedGainDb: number | null;
    readonly bitPerfectBypass: boolean;
    readonly enabled: boolean;
    /** 内置为 null，否则是用户选中的策略插件 ID。 */
    readonly provider: string | null;
  }
  interface Audio {
    /** 仅 audio_gain；接受 -24..12 dB 的有限数。true 表示入队，不会中途调音量。 */
    suggestTrackGain(sourceKey: string, gainDb: number): boolean;
  }
  interface Settings {
    /** 仅本插件清单中声明的设置；未知 ID 返回 null。 */
    get(id: string): SettingValue | null;
  }
  type ActionEvent =
    | { readonly id: string; readonly context: "track"; readonly track: Track }
    | { readonly id: string; readonly context: "player"; readonly track: Track | null };
  interface Events {
    song: Track | null;
    playback: PlaybackState;
    lyric: CurrentLyric;
    audio: Levels;
    queueChanged: QueueChanged;
    queuePage: PageReply<QueuePage>;
    lyricsChanged: LyricsChanged;
    lyricsTimeline: PageReply<LyricsPage>;
    loudness: Loudness | null;
    settingChanged: { readonly id: string; readonly value: SettingValue };
    action: ActionEvent;
  }
  interface Host {
    readonly platform: Platform;
    /** Enji 应用版本，与 API 代数独立。 */
    readonly hostVersion: string;
    readonly apiVersion: 2;
    /** 区分宿主能力和插件权限；未知能力返回 false。旧版可能没有这个方法。 */
    supports(key: Capability): boolean;
    supports(key: string): boolean;
    on<K extends keyof Events>(event: K, handler: (data: Events[K]) => void): void;
    readonly song: Track | null;
    readonly playing: boolean;
    readonly loudness: Loudness | null;
    readonly playback: Playback;
    readonly queue: Queue;
    readonly lyrics: Lyrics;
    readonly audio: Audio;
    readonly settings: Settings;
  }

  interface OverlayLine {
    readonly text: string;
    readonly ruby?: string;
    readonly size?: number;
    readonly bold?: boolean;
    /** #rrggbb 或 #rrggbbaa。 */
    readonly color?: string;
  }
  interface OverlaySpec {
    readonly lines: readonly OverlayLine[];
    readonly place?: "free" | "taskbar";
    readonly locked?: boolean;
  }
  interface SettingBase { readonly id: string; readonly label: string; }
  type SettingDeclaration = SettingBase & (
    | { readonly type: "boolean"; readonly default: boolean }
    | { readonly type: "number"; readonly default: number; readonly min: number; readonly max: number }
    | { readonly type: "enum"; readonly default: string; readonly options: readonly string[] }
  );
  interface ActionDeclaration { readonly id: string; readonly label: string; readonly context: "track" | "player"; }
  /** 文件 pack.json 的形状；不是全局变量。 */
  interface Pack {
    readonly id: string;
    readonly name: string;
    readonly version: string;
    readonly description?: string;
    readonly author?: string;
    readonly main?: string;
    readonly icon?: string;
    readonly minHostVersion?: string;
    readonly permissions?: readonly Permission[];
    readonly platforms?: readonly Platform[];
    readonly settings?: readonly SettingDeclaration[];
    readonly actions?: readonly ActionDeclaration[];
  }
}

declare const enji: Enji.Host;
declare const plugin: Enji.Plugin;
declare const playback: Enji.Playback;
declare const queue: Enji.Queue;
declare const lyrics: Enji.Lyrics;
declare const audio: Enji.Audio;
declare const settings: Enji.Settings;
declare const cover: { pulse(strength?: number): boolean };
declare const overlay: { show(id: string, spec: Enji.OverlaySpec): boolean; hide(id: string): boolean };
declare const ui: { toast(message: string): void };
declare const storage: { get(key: string): Enji.JsonValue; set(key: string, value: Enji.JsonValue | undefined): void };

// 和编辑器的标准 Console 声明可合并；Enji 只保证以下三种方法。
interface Console {
  log(...values: unknown[]): void;
  warn(...values: unknown[]): void;
  error(...values: unknown[]): void;
}
declare var console: Console;
