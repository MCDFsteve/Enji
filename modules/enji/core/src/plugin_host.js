// Each context owns one SDK. The engine sends JSON data and polls the returned call batches;
// in particular a handler never awaits an engine reply on this evaluation's call stack.
var plugin, enji, overlay, cover, ui, storage, console, playback, queue, lyrics, audio, settings;
var __enji = (function (config) {
  var stringify = JSON.stringify, parse = JSON.parse;
  var permissions = config.permissions.slice();
  var features = ['playback.state', 'playback.control', 'queue.page', 'lyrics.timeline',
    'audio.levels', 'audio.loudness', 'audio.trackGain', 'plugin.settings', 'plugin.actions'];
  var handlers = Object.create(null), pending = Object.create(null);
  var pendingCount = 0, sequence = 0, outbox = [], state = {};
  var instance = stringify(config.requestPrefix);

  function dictionary(input) {
    var dict = Object.create(null);
    Object.keys(input || {}).forEach(function (key) { dict[key] = input[key]; });
    return dict;
  }
  var stored = dictionary(config.storage), preferences = dictionary(config.settings);
  function has(name) { return permissions.indexOf(name) !== -1; }
  function finite(n) { return typeof n === 'number' && Number.isFinite(n); }
  function token() { return typeof state.playbackToken === 'string' && state.playbackToken ? state.playbackToken : null; }
  function call(name, args) {
    // Freeze a call's arguments at enqueue time. A circular overlay spec cannot break unrelated page requests.
    try { outbox.push(stringify([name, args])); return true; } catch (_) { return false; }
  }
  function flush() {
    var batch = outbox;
    outbox = [];
    return '{"instance":' + instance + ',"calls":[' + batch.join(',') + ']}';
  }

  plugin = { id: config.id, name: config.name, version: config.version,
    permissions: permissions.slice(), hasPermission: has };

  function control(operation, seconds) {
    var t = token();
    if (!has('playback_control') || !t) return false;
    if (operation === 'seek') {
      if (!finite(seconds) || seconds < 0) return false;
      return call('playbackControl', [operation, t, seconds]);
    }
    return call('playbackControl', [operation, t]);
  }
  playback = {
    get playing() { return !!state.playing; },
    get position() { return finite(state.position) ? state.position : 0; },
    get duration() { return finite(state.duration) ? state.duration : 0; },
    get mode() { return typeof state.mode === 'string' ? state.mode : null; },
    get playbackToken() { return token(); },
    resume: function () { return control('resume'); },
    pause: function () { return control('pause'); },
    next: function () { return control('next'); },
    previous: function () { return control('previous'); },
    seekTo: function (seconds) { return control('seek', seconds); },
  };
  playback.play = playback.resume;
  playback.seek = playback.seekTo;

  function request(kind, permission, offset, limit) {
    if (!has(permission) || pendingCount >= config.pagesInFlight) return null;
    if (offset === undefined) offset = 0;
    if (limit === undefined) limit = 50;
    if (!Number.isSafeInteger(offset) || offset < 0 || offset > config.maxOffset ||
        !Number.isSafeInteger(limit) || limit < 1 || limit > config.pageLimit) return null;
    var t = token();
    if (kind === 'lyricsTimeline' && !t) return null;
    var id = config.requestPrefix + (++sequence);
    if (!call(kind, kind === 'queuePage' ? [id, offset, limit] : [id, t, offset, limit])) return null;
    pending[id] = kind;
    pendingCount++;
    return id;
  }
  queue = { requestPage: function (offset, limit) { return request('queuePage', 'queue', offset, limit); } };
  lyrics = { requestTimeline: function (offset, limit) { return request('lyricsTimeline', 'lyrics', offset, limit); } };
  audio = {
    // true means queued, not applied. The worker enforces source identity, authorization and next-play-only policy.
    suggestTrackGain: function (sourceKey, gainDb) {
      if (!has('audio_gain') || typeof sourceKey !== 'string' || !sourceKey.trim() || sourceKey.length > 4096 ||
          !finite(gainDb) || gainDb < -24 || gainDb > 12) return false;
      return call('suggestTrackGain', [sourceKey, gainDb]);
    },
  };
  settings = { get: function (id) { return Object.prototype.hasOwnProperty.call(preferences, id) ? preferences[id] : null; } };
  enji = {
    platform: config.platform, hostVersion: config.hostVersion, apiVersion: config.apiVersion,
    supports: function (feature) { return features.indexOf(feature) !== -1; },
    on: function (event, handler) {
      if (typeof event === 'string' && typeof handler === 'function') (handlers[event] || (handlers[event] = [])).push(handler);
    },
    get song() { return state.song || null; },
    get playing() { return !!state.playing; },
    get loudness() { return has('loudness') || has('audio_gain') ? state.loudness || null : null; },
    playback: playback, queue: queue, lyrics: lyrics, audio: audio, settings: settings,
  };

  overlay = {
    show: function (id, spec) { return has('overlay') && call('overlayShow', [String(id), spec || {}]); },
    hide: function (id) { return has('overlay') && call('overlayHide', [String(id)]); },
  };
  cover = { pulse: function (strength) { return has('cover') && call('coverPulse', [strength === undefined ? 0.5 : Number(strength)]); } };
  ui = { toast: function (message) { call('toast', [String(message)]); } };
  storage = {
    get: function (key) {
      var text = stored[String(key)];
      if (text == null) return null;
      try { return parse(text); } catch (_) { return text; }
    },
    set: function (key, value) {
      var text = stringify(value === undefined ? null : value);
      stored[String(key)] = text;
      call('storageSet', [String(key), text]);
    },
  };
  console = { log: function () { call('log', [Array.prototype.slice.call(arguments).join(' ')]); } };
  console.warn = console.error = console.log;

  function dispatch(event) {
    if (event.snapshot) state = event.snapshot;
    var data = event.data, type = event.type;
    if (type === 'settingChanged') preferences[data.id] = data.value;
    if (type === 'queuePage' || type === 'lyricsTimeline') {
      if (pending[data.requestId] !== type) return flush();
      delete pending[data.requestId];
      pendingCount--;
    }
    // Listeners added during delivery begin on the next event, never this one.
    var listeners = (handlers[type] || []).slice();
    for (var i = 0; i < listeners.length; i++) {
      try { listeners[i](data); } catch (error) { call('log', [String(error && error.stack ? error.stack : error)]); }
    }
    return flush();
  }
  return { dispatch: dispatch, flush: flush };
})(__enjiConfig);
