// The 8.3.3 SDK accepts plugin_init code from its editor. Bind the editor origin
// through our own host first, before allowing that initialization to reach it.
(function () {
  "use strict";
  var editorOrigin = null;
  var waiting = null;
  window.addEventListener("message", function (event) {
    var data = event.data;
    if (event.source === window.top && event.origin === window.location.origin && data && data.channel === "kb.word-navigation.v1" && data.type === "bind") {
      try {
        var url = new URL(data.editorOrigin);
        if (!/^https?:$/.test(url.protocol) || url.origin !== data.editorOrigin) return;
        editorOrigin = url.origin;
        if (waiting) {
          var saved = waiting;
          waiting = null;
          window.dispatchEvent(new MessageEvent("message", { data: saved.data, source: saved.source, origin: saved.origin }));
        }
      } catch { /* Invalid editor origin cannot initialize the SDK. */ }
      return;
    }
    if (typeof data !== "string") return;
    if (event.source !== window.parent || event.origin !== editorOrigin) {
      if (!editorOrigin && event.source === window.parent) waiting = event;
      event.stopImmediatePropagation();
    }
  }, true);
}());
