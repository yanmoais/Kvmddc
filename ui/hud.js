(function () {
  const params = new URLSearchParams(window.location.search);
  const hasTauri = Boolean(
    window.__TAURI_INTERNALS__ && typeof window.__TAURI_INTERNALS__.invoke === "function"
  );

  if (hasTauri) {
    document.documentElement.classList.add("is-tauri-frameless");
  }

  function applyTheme(theme) {
    document.documentElement.setAttribute("data-theme", theme === "light" ? "light" : "dark");
  }
  window.__applyThemeFromHost = applyTheme;

  const qTheme = params.get("theme");
  if (qTheme === "light" || qTheme === "dark") {
    applyTheme(qTheme);
  } else {
    try {
      const saved = localStorage.getItem("kvmddc.theme");
      if (saved === "light" || saved === "dark") {
        applyTheme(saved);
      } else if (window.matchMedia && window.matchMedia("(prefers-color-scheme: light)").matches) {
        applyTheme("light");
      }
    } catch (_) {}
  }

  window.addEventListener("storage", (e) => {
    if (e.key === "kvmddc.theme" && (e.newValue === "light" || e.newValue === "dark")) {
      applyTheme(e.newValue);
    }
  });

  const CHICLET_COUNT = 16;
  const cells = [];
  const sliderEl = document.getElementById("hudSlider");
  const iconEl = document.getElementById("hudIcon");
  const labelEl = document.getElementById("hudLabel");
  const readoutEl = document.getElementById("hudReadout");

  {
    const track = document.createElement("span");
    track.className = "chiclet-track";
    for (let i = 0; i < CHICLET_COUNT; i++) {
      const c = document.createElement("i");
      track.appendChild(c);
      cells.push(c);
    }
    sliderEl.appendChild(track);
  }

  function iconSvg(kind, muted) {
    if (kind === "brightness") {
      return '<svg class="icon-14" viewBox="0 0 24 24"><circle cx="12" cy="12" r="4.2"/><path d="M12 2.5v2.4M12 19.1v2.4M4.9 4.9l1.7 1.7M17.4 17.4l1.7 1.7M2.5 12h2.4M19.1 12h2.4M4.9 19.1l1.7-1.7M17.4 6.6l1.7-1.7"/></svg>';
    }
    if (kind === "volume") {
      if (muted) {
        return '<svg class="icon-14" viewBox="0 0 24 24"><path d="M11 5L6.5 9H3.5v6h3L11 19V5Z"/><path d="M16 9.5l5 5M21 9.5l-5 5"/></svg>';
      }
      return '<svg class="icon-14" viewBox="0 0 24 24"><path d="M11 5L6.5 9H3.5v6h3L11 19V5Z"/><path d="M15.5 8.8a4.8 4.8 0 0 1 0 6.4M18.6 6.2a8.5 8.5 0 0 1 0 11.6"/></svg>';
    }
    if (kind === "color") {
      return '<svg class="icon-14" viewBox="0 0 24 24"><circle cx="12" cy="12" r="8"/><path d="M12 4v16"/></svg>';
    }
    if (kind === "transfer") {
      return '<svg class="icon-14" viewBox="0 0 24 24"><path d="M12 3v12M8 11l4 4 4-4"/><path d="M4 17v2a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-2"/></svg>';
    }
    return '<svg class="icon-14" viewBox="0 0 24 24"><rect x="3" y="4.5" width="18" height="13" rx="2"/><path d="M9 20.5h6"/></svg>';
  }

  function renderSignal(sig) {
    if (!sig) return;
    const kind = sig.kind || "volume";
    const val = Math.max(0, Math.min(100, Number(sig.value ?? 75)));
    const muted = Boolean(sig.muted);

    iconEl.innerHTML = iconSvg(kind, muted);

    if (kind === "brightness" || kind === "volume") {
      labelEl.hidden = true;
      sliderEl.hidden = false;
      readoutEl.hidden = false;
      const eff = muted ? 0 : val;
      const scaled = (eff / 100) * CHICLET_COUNT;
      for (let i = 0; i < CHICLET_COUNT; i++) {
        const fill = Math.max(0, Math.min(1, scaled - i));
        cells[i].style.setProperty("--f", fill.toFixed(3));
      }
      readoutEl.textContent = muted ? "静音" : `${Math.round(val)}%`;
    } else {
      sliderEl.hidden = true;
      readoutEl.hidden = true;
      labelEl.hidden = false;
      labelEl.textContent = sig.label || "";
    }
  }

  window.__renderHudSignal = renderSignal;

  renderSignal({
    kind: params.get("kind") || "volume",
    label: params.get("label") || "显示器音量",
    value: Number(params.get("value") ?? 75),
    muted: params.get("muted") === "1"
  });
})();
