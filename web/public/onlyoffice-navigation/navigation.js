/* global Asc, Api */
(function () {
  "use strict";
  var channel = "kb.word-navigation.v1";
  var origin = window.location.origin;
  var binding = null;
  var initialized = false;
  var navigating = false;
  var queued = null;
  var helloTimer = null;
  function send(message) {
    window.top.postMessage(Object.assign({ channel: channel }, binding || {}, message), origin);
  }
  function hello() {
    send({ type: "hello" });
    if (helloTimer === null) helloTimer = window.setInterval(function () {
      if (!binding) send({ type: "hello" });
    }, 500);
  }
  function current(request) {
    return binding && request.nonce === binding.nonce && request.editorKey === binding.editorKey && request.versionId === binding.versionId;
  }
  function navigate(request) {
    navigating = true;
    Asc.scope.kbBookmark = request.bookmark;
    Asc.plugin.callCommand(function () {
      var bookmark = Api.GetDocument().GetBookmark(Asc.scope.kbBookmark);
      if (!bookmark) return false;
      bookmark.GoTo();
      return true;
    }, false, false, function (found) {
      navigating = false;
      send({ id: request.id, nonce: request.nonce, editorKey: request.editorKey,
        versionId: request.versionId, type: "result", ok: found === true });
      var next = queued;
      queued = null;
      if (next && current(next)) navigate(next);
    });
  }
  Asc.plugin.init = function () {
    initialized = true;
    if (binding) send({ type: "ready" });
    else hello();
  };
  Asc.plugin.button = function () {};
  window.addEventListener("message", function (event) {
    if (event.source !== window.top || event.origin !== origin) return;
    var data = event.data;
    if (!data || data.channel !== channel) return;
    if (binding && data.type === "unbind" && data.nonce === binding.nonce) {
      binding = null;
      queued = null;
      hello();
      return;
    }
    if (data.type === "bind" && typeof data.nonce === "string" && typeof data.editorKey === "string" && typeof data.versionId === "string") {
      binding = { nonce: data.nonce, editorKey: data.editorKey, versionId: data.versionId };
      queued = null;
      window.clearInterval(helloTimer);
      helloTimer = null;
      if (initialized) send({ type: "ready" });
      return;
    }
    if (!binding || !initialized || data.type !== "navigate" || !current(data) || typeof data.id !== "string" || !/^kb_[ct]_[a-f0-9]{28}$/.test(data.bookmark)) return;
    var request = { id: data.id, nonce: binding.nonce, editorKey: binding.editorKey, versionId: binding.versionId, bookmark: data.bookmark };
    if (navigating) queued = request;
    else navigate(request);
  });
  // No document data, commands or authentication tokens cross this handshake.
  hello();
}());
