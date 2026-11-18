/// <reference path="../../enji.d.ts" />
// @ts-check
(function () {
  const needs = ["playback.state", "playback.control", "queue.page", "lyrics.timeline", "plugin.settings", "plugin.actions"];
  if (typeof enji.supports !== "function" || !needs.every(key => enji.supports(key))) {
    ui.toast("歌曲书签需要支持插件 API v2 的 Enji。");
    return;
  }

  /** @param {number} seconds */
  function clock(seconds) {
    const s = Math.floor(seconds);
    return Math.floor(s / 60) + ":" + (s % 60 < 10 ? "0" : "") + (s % 60);
  }
  /** @param {Enji.Track} track */
  function readMark(track) {
    const value = storage.get("mark:" + track.trackKey);
    return typeof value === "number" && Number.isFinite(value) && value >= 0 ? value : null;
  }

  /** @type {string | null} */
  let queueRequest = null;
  /** @type {number | null} */
  let queueRevision = null;
  /** @type {{id: string, token: string, key: string, seconds: number, revision: number | null, line: Enji.LyricLine | null} | null} */
  let lookup = null;

  enji.on("settingChanged", function (change) {
    if (change.id === "leadSeconds") ui.toast("回听提前量：" + change.value + " 秒");
  });

  enji.on("action", function (action) {
    if (action.context === "player" && action.id === "queueMarks") {
      queueRequest = queue.requestPage(0, 3);
      if (queueRequest === null) ui.toast("未能请求队列，请稍后再试。");
      return;
    }

    const target = action.track;
    if (!target) return ui.toast("还没有歌曲。");
    const saved = readMark(target);
    const now = enji.song;
    const token = playback.playbackToken;
    if (!now || !token || target.trackKey !== now.trackKey) {
      // 歌曲菜单可能属于另一首歌；可以看它的书签，但不能动正在播放的歌。
      const mark = saved === null ? "没有书签" : "书签在 " + clock(saved);
      return ui.toast("《" + target.name + "》" + mark + "；不是当前歌曲，未跳转。");
    }
    if (action.context === "player" && action.id === "save") {
      lookup = null;
      storage.set("mark:" + target.trackKey, playback.position);
      ui.toast("已记下《" + target.name + "》" + clock(playback.position));
    } else if (saved === null) {
      ui.toast("《" + target.name + "》还没有书签。");
    } else if (action.context === "track" && action.id === "recall") {
      const lead = settings.get("leadSeconds");
      const to = Math.max(0, saved - (typeof lead === "number" ? lead : 0));
      // 立即发起，SDK 绑定本次事件的 token；换歌后宿主不会执行旧请求。
      ui.toast(playback.seekTo(to) ? "已请求跳至 " + clock(to) : "未能请求跳转。");
    } else if (action.context === "player" && action.id === "lyricAtMark") {
      const id = lyrics.requestTimeline(0, 50);
      lookup = id === null ? null : { id, token, key: target.trackKey, seconds: saved, revision: null, line: null };
      if (!lookup) ui.toast("未能请求歌词，请稍后再试。");
    }
  });

  enji.on("queueChanged", change => { queueRevision = change.revision; });
  enji.on("queuePage", function (reply) {
    if (reply.requestId !== queueRequest) return;
    queueRequest = null;
    if (!reply.ok) return ui.toast("读取队列失败：" + reply.error);
    const page = reply.page;
    if (queueRevision !== null && page.revision !== queueRevision) return ui.toast("队列已变，请重新查看。");
    if (!page.items.length) return ui.toast("队列为空。");
    const rows = page.items.map(function (track) {
      const seconds = readMark(track);
      return track.name + "：" + (seconds === null ? "无书签" : clock(seconds));
    });
    ui.toast("队首 " + rows.length + " 首（共 " + page.total + " 首）｜" + rows.join("；"));
  });

  enji.on("lyricsChanged", function (change) {
    if (lookup && (lookup.token !== change.playbackToken || (lookup.revision !== null && lookup.revision !== change.revision))) {
      lookup = null;
      ui.toast("歌曲或歌词已变，请重新查看书签歌词。");
    }
  });
  enji.on("lyricsTimeline", function (reply) {
    const read = lookup;
    if (!read || reply.requestId !== read.id) return;
    lookup = null;
    if (!reply.ok) return ui.toast("读取歌词失败：" + reply.error);
    const page = reply.page;
    if (!enji.song || enji.song.trackKey !== read.key || playback.playbackToken !== read.token ||
        page.playbackToken !== read.token || (read.revision !== null && read.revision !== page.revision)) {
      return ui.toast("歌曲或歌词已变，请重新查看书签歌词。");
    }
    read.revision = page.revision;
    // 找书签时刻之前最近的一行；只保留候选行，一次最多请求一页歌词。
    const next = page.offset + page.items.length;
    let passedMark = false;
    for (const line of page.items) {
      if (line.time > read.seconds) { passedMark = true; break; }
      read.line = line;
    }
    if (!passedMark && page.items.length && next < page.total) {
      const id = lyrics.requestTimeline(next, 50);
      if (id === null) return ui.toast("未能请求下一页歌词，请重试。");
      read.id = id;
      lookup = read;
      return;
    }
    if (!read.line) return ui.toast(page.total ? "书签在第一行歌词之前。" : "这首歌没有时间轴歌词。");
    const line = read.line;
    const chars = Array.from(line.text); // start/end 是 Unicode 标量下标，不是 UTF-16 偏移。
    const word = line.words.find(w => w.time <= read.seconds && read.seconds < w.time + w.duration);
    const ruby = line.ruby[0];
    let text = clock(read.seconds) + "｜" + line.text;
    if (word) text += "｜此刻的词：" + chars.slice(word.start, word.end).join("");
    if (ruby) text += "｜注音：" + chars.slice(ruby.start, ruby.end).join("") + "（" + ruby.reading + "）";
    ui.toast(text);
  });
})();
