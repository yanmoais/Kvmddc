(function () {
  const params = new URLSearchParams(window.location.search);
  const hasTauri = Boolean(
    window.__TAURI_INTERNALS__ && typeof window.__TAURI_INTERNALS__.invoke === "function"
  );

  if (hasTauri) {
    document.documentElement.classList.add("is-tauri-frameless");
  }

  function resolveInitialTheme() {
    const q = params.get("theme");
    if (q === "light" || q === "dark") return q;
    try {
      const saved = localStorage.getItem("kvmddc.theme");
      if (saved === "light" || saved === "dark") return saved;
    } catch (_) {}
    if (window.matchMedia && window.matchMedia("(prefers-color-scheme: light)").matches) {
      return "light";
    }
    return "dark";
  }

  function applyTheme(theme) {
    document.documentElement.setAttribute("data-theme", theme === "light" ? "light" : "dark");
  }
  window.__applyThemeFromHost = applyTheme;

  applyTheme(resolveInitialTheme());
  window.addEventListener("storage", (e) => {
    if (e.key === "kvmddc.theme" && (e.newValue === "light" || e.newValue === "dark")) {
      applyTheme(e.newValue);
    }
  });

  let selectedId = null;
  let candidates = [
    { id: "macbook", display_name: "MacBook Pro 14", monitor_input: 21, hotkey_slot: 1, online: true },
    { id: "mac-studio", display_name: "Mac Studio", monitor_input: 15, hotkey_slot: 3, online: true }
  ];

  async function invokeCmd(cmd, args) {
    if (!hasTauri) return null;
    return window.__TAURI_INTERNALS__.invoke(cmd, args || {});
  }

  function renderCandidates() {
    if (!selectedId && candidates.length > 0) {
      selectedId = candidates[0].id;
    }
    const listEl = document.getElementById("picker-list");
    listEl.innerHTML = "";

    for (const c of candidates) {
      const btn = document.createElement("button");
      btn.type = "button";
      btn.className = c.id === selectedId ? "picker-item picker-item-selected" : "picker-item";
      btn.setAttribute("role", "option");
      btn.setAttribute("aria-selected", c.id === selectedId ? "true" : "false");

      const left = document.createElement("span");
      left.className = "picker-item-left";

      const dot = document.createElement("span");
      dot.className = c.online ? "status-dot is-green" : "status-dot is-amber";
      left.appendChild(dot);

      const label = document.createElement("span");
      label.textContent = c.display_name.replace(" M5 Max", "");
      left.appendChild(label);

      const keycap = document.createElement("kbd");
      keycap.className = "tray-kbd tabular-nums";
      keycap.textContent = String(c.hotkey_slot);

      btn.appendChild(left);
      btn.appendChild(keycap);

      btn.addEventListener("mouseenter", () => {
        selectedId = c.id;
        renderCandidates();
        invokeCmd("hover_edge_candidate", { hostId: c.id });
      });

      btn.addEventListener("click", () => {
        invokeCmd("confirm_edge_pick", { hostId: c.id });
      });

      listEl.appendChild(btn);
    }
  }

  async function loadCandidates() {
    if (!hasTauri) {
      renderCandidates();
      return;
    }
    try {
      const list = await invokeCmd("get_edge_candidates");
      if (Array.isArray(list) && list.length > 0) {
        candidates = list;
        selectedId = list[0].id;
      }
    } catch (_) {}
    renderCandidates();
  }

  window.addEventListener("keydown", (e) => {
    if (e.key === "Escape") {
      invokeCmd("cancel_edge_pick");
    }
  });

  window.addEventListener("keyup", (e) => {
    if (e.key === "Control" && selectedId) {
      invokeCmd("confirm_edge_pick", { hostId: selectedId });
    }
  });

  window.__reloadPickerCandidates = loadCandidates;
  loadCandidates();
})();
