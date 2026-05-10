// PWA glue. Loaded with `<script defer>` from index.html, runs before
// the WASM bundle finishes booting so we never miss `beforeinstallprompt`
// (Chrome fires it once, very early — Leptos is too late).
//
// This file deliberately knows nothing about Leptos or Rust: it talks
// to the page via CustomEvent + a small `window.__pwa` namespace, and
// the Leptos `pwa.rs` component subscribes from there. That keeps SW
// registration off the WASM critical path.

(function () {
  "use strict";

  if (typeof window === "undefined") return;

  // Resolve "./sw.js" against the document URL — works under both
  // domain-root deploys (chess-net --static-dir) and subpath deploys
  // (GitHub Pages /Chinese-Chess_Xiangqi/). The trailing-segment hack
  // strips the current document filename if any.
  function resolveBase() {
    var path = window.location.pathname;
    var lastSlash = path.lastIndexOf("/");
    return lastSlash >= 0 ? path.slice(0, lastSlash + 1) : "/";
  }

  var BASE = resolveBase();
  // The `<base href>` injected by Trunk via --public-url tells us the
  // canonical app root; prefer it when present (more reliable for
  // sub-routes like /play/<room>).
  var baseEl = document.querySelector("base[href]");
  if (baseEl) {
    try {
      var u = new URL(baseEl.getAttribute("href"), window.location.origin);
      BASE = u.pathname.endsWith("/") ? u.pathname : u.pathname + "/";
    } catch (_e) {
      // ignore — fall through to resolveBase()
    }
  }

  var deferredPrompt = null;
  var pendingUpdate = null; // ServiceWorkerRegistration with .waiting

  function dispatch(name, detail) {
    window.dispatchEvent(new CustomEvent(name, { detail: detail || {} }));
  }

  function isStandalone() {
    return (
      (window.matchMedia &&
        window.matchMedia("(display-mode: standalone)").matches) ||
      window.navigator.standalone === true
    );
  }

  function isIos() {
    var ua = window.navigator.userAgent || "";
    var iosUa = /iPad|iPhone|iPod/.test(ua);
    // iPadOS 13+ reports as Mac with touch — sniff that too.
    var iPadOs =
      window.navigator.platform === "MacIntel" &&
      typeof window.navigator.maxTouchPoints === "number" &&
      window.navigator.maxTouchPoints > 1;
    return iosUa || iPadOs;
  }

  function isMobile() {
    var ua = window.navigator.userAgent || "";
    return /Mobi|Android|iPad|iPhone|iPod/i.test(ua) || isIos();
  }

  // ---- Service worker registration ------------------------------------

  function registerSw() {
    if (!("serviceWorker" in navigator)) return;
    var swUrl = BASE + "sw.js";

    navigator.serviceWorker
      .register(swUrl, {
        scope: BASE,
        // Don't let HTTP cache hold a stale sw.js for days — force a
        // fresh check on every registration.
        updateViaCache: "none",
      })
      .then(function (reg) {
        // Already-waiting worker on a hard reload.
        if (reg.waiting && navigator.serviceWorker.controller) {
          pendingUpdate = reg;
          dispatch("pwa:update-ready");
        }

        reg.addEventListener("updatefound", function () {
          var nw = reg.installing;
          if (!nw) return;
          nw.addEventListener("statechange", function () {
            if (
              nw.state === "installed" &&
              navigator.serviceWorker.controller
            ) {
              // We have an old controlling SW + a new installed one —
              // that's exactly the "new version available" condition.
              pendingUpdate = reg;
              dispatch("pwa:update-ready");
            }
          });
        });

        // Foregrounding the tab — opportunistically check for new SW.
        window.addEventListener("focus", function () {
          reg.update().catch(function () {});
        });
      })
      .catch(function (err) {
        console.warn("[pwa] service worker registration failed:", err);
      });

    var refreshing = false;
    navigator.serviceWorker.addEventListener("controllerchange", function () {
      if (refreshing) return;
      refreshing = true;
      window.location.reload();
    });
  }

  // ---- Install prompt -------------------------------------------------

  window.addEventListener("beforeinstallprompt", function (event) {
    event.preventDefault();
    deferredPrompt = event;
    dispatch("pwa:install-available");
  });

  window.addEventListener("appinstalled", function () {
    deferredPrompt = null;
    dispatch("pwa:installed");
  });

  // ---- Public API for Leptos -----------------------------------------

  window.__pwa = {
    base: BASE,
    canInstall: function () {
      return !!deferredPrompt;
    },
    isStandalone: isStandalone,
    isIos: isIos,
    isMobile: isMobile,
    install: function () {
      if (!deferredPrompt) return Promise.resolve("unavailable");
      var p = deferredPrompt;
      deferredPrompt = null;
      return p
        .prompt()
        .then(function () {
          return p.userChoice;
        })
        .then(function (choice) {
          return choice && choice.outcome ? choice.outcome : "dismissed";
        });
    },
    applyUpdate: function () {
      if (pendingUpdate && pendingUpdate.waiting) {
        pendingUpdate.waiting.postMessage({ type: "SKIP_WAITING" });
        return true;
      }
      // No waiting worker — just reload as a best-effort fallback.
      window.location.reload();
      return false;
    },
  };

  // Replay current state on subscribe — Leptos mounts after we run, so
  // late subscribers can still ask `window.__pwa.canInstall()` and we
  // re-fire `pwa:installed` if the user is already in standalone.
  if (isStandalone()) {
    // Defer so listeners attached during `TrunkApplicationStarted` see it.
    setTimeout(function () {
      dispatch("pwa:installed");
    }, 0);
  }

  if (document.readyState === "loading") {
    window.addEventListener("DOMContentLoaded", registerSw);
  } else {
    registerSw();
  }
})();
