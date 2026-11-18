/// <reference path="../../enji.d.ts" />
// @ts-check
(function () {
  if (typeof enji.supports !== "function" || !enji.supports("audio.loudness") ||
      !enji.supports("audio.trackGain") || !plugin.hasPermission("audio_gain")) return;

  enji.on("loudness", function (info) {
    if (!info || !info.enabled || info.provider !== plugin.id || info.status !== "ready" ||
        !info.sourceKey || info.bitPerfectBypass || info.recommendedGainDb === null) return;

    // 已知的衰减照用；若宿主建议放大，则建议 0 dB（原音量）。未知值不建议。
    // 建议交给宿主在起播时采用；这里没有、也不需要实时音量控制。
    audio.suggestTrackGain(info.sourceKey, Math.max(-24, Math.min(0, info.recommendedGainDb)));
  });
})();
