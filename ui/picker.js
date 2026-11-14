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

  const initialSelectedParam = params.get("selected");
  let selectedId = initialSelectedParam || null;
  let settled = false;
  let lastPointerX = null;
  let lastPointerY = null;
  let wheelLockUntil = 0;
  let wheelAccum = 0;
  let lastWheelEventAt = 0;
  let lastWheelStepAt = 0;

  let candidates = [
    { id: "win-desktop", display_name: "Windows 台式机", monitor_input: 15, hotkey_slot: 2, online: true },
    { id: "mac-studio", display_name: "Mac Studio", monitor_input: 17, hotkey_slot: 3, online: true }
  ];

  async function invokeCmd(cmd, args) {
    if (!hasTauri) return null;
    return window.__TAURI_INTERNALS__.invoke(cmd, args || {});
  }

  function syncSelectionClasses() {
    const listEl = document.getElementById("picker-list");
    if (!listEl) return;
    const buttons = listEl.querySelectorAll(".picker-item");
    for (const btn of buttons) {
      const isSelected = btn.getAttribute("data-host-id") === selectedId;
      btn.className = isSelected ? "picker-item picker-item-selected" : "picker-item";
      btn.setAttribute("aria-selected", isSelected ? "true" : "false");
    }
  }

  function setSelected(hostId, notifyBackend) {
    if (!hostId || !candidates.some((c) => c.id === hostId)) return;
    if (selectedId === hostId) {
      syncSelectionClasses();
      return;
    }
    selectedId = hostId;
    syncSelectionClasses();
    if (notifyBackend) {
      invokeCmd("hover_edge_candidate", { hostId });
    }
  }

  function stepSelection(deltaRows) {
    if (!candidates.length || settled) return;
    const curIdx = Math.max(
      0,
      candidates.findIndex((c) => c.id === selectedId)
    );
    const nextIdx = Math.max(0, Math.min(candidates.length - 1, curIdx + deltaRows));
    if (nextIdx !== curIdx) {
      setSelected(candidates[nextIdx].id, true);
    }
  }

  function confirmPick(hostId) {
    if (settled || !hostId) return;
    settled = true;
    setSelected(hostId, false);
    invokeCmd("confirm_edge_pick", { hostId });
  }

  function cancelPick() {
    if (settled) return;
    settled = true;
    invokeCmd("cancel_edge_pick");
  }

  function renderCandidates() {
    if (!selectedId || !candidates.some((c) => c.id === selectedId)) {
      selectedId = candidates.length > 0 ? candidates[0].id : null;
    }
    const listEl = document.getElementById("picker-list");
    listEl.innerHTML = "";

    for (const c of candidates) {
      const btn = document.createElement("button");
      btn.type = "button";
      btn.setAttribute("data-host-id", c.id);
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

      btn.addEventListener("mousemove", (e) => {
        if (settled) return;
        if (lastPointerX === e.clientX && lastPointerY === e.clientY) return;
        lastPointerX = e.clientX;
        lastPointerY = e.clientY;
        if (performance.now() < wheelLockUntil) return;
        setSelected(c.id, true);
      });

      btn.addEventListener("mouseenter", (e) => {
        if (settled || performance.now() < wheelLockUntil) return;
        if (lastPointerX !== null && lastPointerX === e.clientX && lastPointerY === e.clientY) {
          return;
        }
        lastPointerX = e.clientX;
        lastPointerY = e.clientY;
        setSelected(c.id, true);
      });

      // Handle both standard primary click and macOS Ctrl+LeftClick (which WebKit reports
      // with ctrlKey=true / button=2 without emitting a standard button `click` event).
      btn.addEventListener("pointerdown", (e) => {
        if (e.button === 0 || (e.button === 2 && e.ctrlKey)) {
          e.preventDefault();
          e.stopPropagation();
          confirmPick(c.id);
        }
      });

      btn.addEventListener("click", (e) => {
        e.preventDefault();
        e.stopPropagation();
        confirmPick(c.id);
      });

      listEl.appendChild(btn);
    }
  }

  async function loadCandidates() {
    settled = false;
    wheelAccum = 0;
    wheelLockUntil = 0;
    lastPointerX = null;
    lastPointerY = null;

    if (!hasTauri) {
      if (initialSelectedParam && candidates.some((c) => c.id === initialSelectedParam)) {
        selectedId = initialSelectedParam;
      } else if (candidates.length > 0) {
        selectedId = candidates[0].id;
      }
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

  window.addEventListener("contextmenu", (e) => {
    e.preventDefault();
  });

  // Clicking blank space outside `.picker-card` or clicking the `Esc` pill cancels switching.
  document.addEventListener("pointerdown", (e) => {
    const target = e.target;
    if (!(target instanceof Element)) return;
    if (target.closest("#picker-esc") || !target.closest(".picker-card")) {
      e.preventDefault();
      e.stopPropagation();
      cancelPick();
    }
  });

  // Smooth mouse wheel & trackpad scrolling across candidate hosts without clobbering hover.
  window.addEventListener(
    "wheel",
    (e) => {
      if (settled || candidates.length <= 1) return;
      e.preventDefault();
      const now = performance.now();
      if (now - lastWheelEventAt > 180) {
        wheelAccum = 0;
      }
      lastWheelEventAt = now;

      const dy = e.deltaMode === 1 ? e.deltaY * 28 : e.deltaY;
      if (Math.abs(dy) < 0.5) return;
      wheelAccum += dy;

      if (Math.abs(wheelAccum) >= 20 && now - lastWheelStepAt >= 55) {
        const dir = wheelAccum > 0 ? 1 : -1;
        wheelAccum = 0;
        lastWheelStepAt = now;
        wheelLockUntil = now + 150;
        stepSelection(dir);
      }
    },
    { passive: false }
  );

  window.addEventListener("keydown", (e) => {
    if (e.key === "Escape") {
      e.preventDefault();
      cancelPick();
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      wheelLockUntil = performance.now() + 150;
      stepSelection(1);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      wheelLockUntil = performance.now() + 150;
      stepSelection(-1);
    } else if ((e.key === "Enter" || e.key === " ") && selectedId) {
      e.preventDefault();
      confirmPick(selectedId);
    }
  });

  window.addEventListener("keyup", (e) => {
    if (e.key === "Control" && !settled && selectedId) {
      confirmPick(selectedId);
    }
  });

  window.__reloadPickerCandidates = loadCandidates;
  loadCandidates();
})();
