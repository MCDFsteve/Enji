// Enji's side of a plugin's context: `__enjiConfig` comes first, then this,
// then the plugin. The engine cannot be called from here: it hands each
// event over with what the plugin may read, and takes back the calls the
// plugin queued, which `__enji.dispatch` answers as JSON.
var __enji = {
  config: __enjiConfig,
  snapshot: { storage: __enjiConfig.storage || {} },
  outbox: [],
  handlers: {},
};

function __enjiHas(permission) {
  return __enji.config.permissions.indexOf(permission) >= 0;
}

function __enjiCall(name, args) {
  __enji.outbox.push([name, args || []]);
}

var plugin = {
  id: __enji.config.id,
  name: __enji.config.name,
  version: __enji.config.version,
  permissions: __enji.config.permissions.slice(),
  hasPermission: __enjiHas,
};

var enji = {
  // macos, ios, windows, linux or android.
  platform: __enji.config.platform,
  hostVersion: __enji.config.hostVersion,
  // `song`, `lyric`, `playback` and, with the `audio` permission, `audio`.
  on: function (event, handler) {
    (__enji.handlers[event] = __enji.handlers[event] || []).push(handler);
  },
  // What plays now, as the last event left it.
  get song() {
    return __enji.snapshot.song || null;
  },
  get playing() {
    return !!__enji.snapshot.playing;
  },
};

var overlay = {
  // Text over the other windows: `{ lines: [{ text, size, bold, color }],
  // place: "free" | "taskbar", locked }`; colours as `#rrggbb` or
  // `#rrggbbaa`.
  show: function (id, spec) {
    if (!__enjiHas('overlay')) return false;
    __enjiCall('overlayShow', [String(id), spec || {}]);
    return true;
  },
  hide: function (id) {
    if (!__enjiHas('overlay')) return false;
    __enjiCall('overlayHide', [String(id)]);
    return true;
  },
};

var cover = {
  // The playing page's cover gives a little pulse.
  pulse: function () {
    if (!__enjiHas('cover')) return false;
    __enjiCall('coverPulse', []);
    return true;
  },
};

var ui = {
  toast: function (message) {
    __enjiCall('toast', [String(message)]);
  },
};

var storage = {
  get: function (key) {
    var text = __enji.snapshot.storage[String(key)];
    if (text == null) return null;
    try {
      return JSON.parse(text);
    } catch (error) {
      return text;
    }
  },
  set: function (key, value) {
    var text = JSON.stringify(value === undefined ? null : value);
    __enji.snapshot.storage[String(key)] = text;
    __enjiCall('storageSet', [String(key), text]);
  },
};

var console = {
  log: function () {
    __enjiCall('log', [Array.prototype.slice.call(arguments).join(' ')]);
  },
};
console.warn = console.error = console.log;

__enji.dispatch = function (event) {
  var snapshot = event.snapshot || {};
  for (var key in snapshot) __enji.snapshot[key] = snapshot[key];
  var handlers = __enji.handlers[event.type] || [];
  for (var i = 0; i < handlers.length; i++) {
    try {
      handlers[i](event.data);
    } catch (error) {
      __enjiCall('log', [String(error && error.stack ? error.stack : error)]);
    }
  }
  var calls = __enji.outbox;
  __enji.outbox = [];
  return JSON.stringify(calls);
};
