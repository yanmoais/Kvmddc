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
  if (window.matchMedia && !params.get("theme")) {
    window.matchMedia("(prefers-color-scheme: light)").addEventListener("change", (ev) => {
      try {
        if (!localStorage.getItem("kvmddc.theme")) {
          applyTheme(ev.matches ? "light" : "dark");
        }
      } catch (_) {}
    });
  }

  const demoMode = params.get("demo") || "normal";

  function buildDemoState(mode) {
    const isTriple = mode === "triple" || mode === "swap";
    const winIn = mode === "swap" ? 15 : 17;
    const stuIn = mode === "swap" ? 17 : 15;
    const activeInput = mode === "win" ? winIn : isTriple ? stuIn : 21;
    return {
      local_host_id: "macbook",
      input_provider_host_id: "win-desktop",
      primary_ddc_controller_id: "macbook",
      deskflow_enabled: mode !== "df-off",
      ctrl_edge_switch_enabled: true,
      allow_plain_slide_to_internal: true,
      unify_cross_os_modifiers: true,
      observed_monitor_input: activeInput,
      preferred_brightness: mode === "win" ? 92 : 85,
      current_brightness: mode === "win" ? 92 : 85,
      preferred_volume: 75,
      current_volume: 75,
      audio_muted: false,
      current_color_preset: mode === "win" ? 6 : 5,
      color_preset_label: mode === "win" ? "7500K 清冷" : "6500K 标准",
      last_transfer_summary: null,
      deskflow_running: mode !== "perm",
      deskflow_connected: mode !== "perm",
      accessibility_trusted: mode !== "perm",
      external_display_online: true,
      monitor_model: "27E2N6900RW",
      edge_locked: mode === "locked",
      secure_input_active: false,
      cooldown_remaining_ms: mode === "settling" ? 3200 : 0,
      last_error: mode === "error" ? "DDC 输入源回读不一致，点击底部立即重试" : null,
      hosts: [
        {
          id: "macbook",
          display_name: "MacBook Pro 14",
          platform: "macos",
          lan_ip: "192.168.1.10",
          monitor_input: 21,
          port_label: "USB-C",
          detected_live: true,
          hotkey_slot: 1,
          enabled: true,
          online: true,
          profile_summary: "2560×1440 · 75Hz · HiDPI",
          live_mode_summary: "2560×1440 · 75Hz · HiDPI",
          display_modes: [
            { width: 2560, height: 1440, hz: 75, hidpi: true, active: true, label: "2560×1440 · 75Hz · HiDPI" },
            { width: 3200, height: 1800, hz: 75, hidpi: true, active: false, label: "3200×1800 · 75Hz · HiDPI" },
            { width: 3840, height: 2160, hz: 75, hidpi: true, active: false, label: "3840×2160 · 75Hz · HiDPI" },
            { width: 5120, height: 2880, hz: 75, hidpi: false, active: false, label: "5120×2880 · 75Hz · 原生" }
          ],
          internal_height_percent: 81
        },
        {
          id: "win-desktop",
          display_name: "Windows 台式机",
          platform: "windows",
          lan_ip: "192.168.1.20",
          monitor_input: winIn,
          port_label: winIn === 15 ? "DP 1.4" : "HDMI 2.1",
          detected_live: true,
          hotkey_slot: 2,
          enabled: true,
          online: true,
          profile_summary: "5120×2880 · 75Hz",
          live_mode_summary: "5120×2880 · 75Hz",
          display_modes: [
            { width: 5120, height: 2880, hz: 75, hidpi: false, active: true, label: "5120×2880 · 75Hz · 原生" },
            { width: 3840, height: 2160, hz: 75, hidpi: false, active: false, label: "3840×2160 · 75Hz" },
            { width: 2560, height: 1440, hz: 75, hidpi: false, active: false, label: "2560×1440 · 75Hz" }
          ],
          internal_height_percent: 81
        },
        {
          id: "mac-studio",
          display_name: "Mac Studio",
          platform: "macos",
          lan_ip: "192.168.1.30",
          monitor_input: stuIn,
          port_label: stuIn === 17 ? "HDMI 2.1" : "DP 1.4",
          detected_live: isTriple,
          hotkey_slot: 3,
          enabled: isTriple,
          online: isTriple,
          profile_summary: "5120×2880 · 120Hz",
          live_mode_summary: "5120×2880 · 120Hz",
          display_modes: [
            { width: 2560, height: 1440, hz: 120, hidpi: true, active: false, label: "2560×1440 · 120Hz · HiDPI" },
            { width: 5120, height: 2880, hz: 120, hidpi: false, active: true, label: "5120×2880 · 120Hz · 原生" }
          ],
          internal_height_percent: 81
        }
      ],
      recent_requests: [
        {
          id: 1,
          to_host: "macbook",
          target_input: 21,
          observed_input: mode === "error" ? 17 : activeInput,
          source: "menu",
          status: mode === "error" ? "failed" : "done",
          attempts: 1,
          time_hms: "12:00:00"
        }
      ]
    };
  }

  let currentState = buildDemoState(demoMode);
  let isDraggingSlider = false;
  let isDraggingVol = false;
  let sliderSendTimer = null;
  let volSendTimer = null;

  async function invokeCmd(cmd, args) {
    if (!hasTauri) {
      return mockInvoke(cmd, args || {});
    }
    return window.__TAURI_INTERNALS__.invoke(cmd, args || {});
  }

  async function mockInvoke(cmd, args) {
    if (cmd === "get_dashboard_state") return currentState;
    if (cmd === "switch_to_host" || cmd === "wake_and_switch_host") {
      const target = currentState.hosts.find((h) => h.id === args.hostId);
      if (target) {
        currentState.observed_monitor_input = target.monitor_input;
        currentState.last_error = null;
      }
      return currentState;
    }
    if (cmd === "leave_desk_to_windows") {
      currentState.observed_monitor_input = 17;
      return currentState;
    }
    if (cmd === "set_brightness") {
      currentState.preferred_brightness = args.brightness;
      currentState.current_brightness = args.brightness;
      return currentState;
    }
    if (cmd === "set_volume") {
      currentState.preferred_volume = args.volume;
      currentState.current_volume = args.volume;
      if (args.volume > 0) currentState.audio_muted = false;
      return currentState;
    }
    if (cmd === "toggle_audio_mute") {
      currentState.audio_muted = Boolean(args.muted);
      return currentState;
    }
    if (cmd === "cycle_color_preset") {
      const order = [5, 4, 6, 8, 2];
      const labels = { 5: "6500K 标准", 4: "5000K 暖光", 6: "7500K 清冷", 8: "9300K 冷白", 2: "原生色温" };
      const idx = order.indexOf(currentState.current_color_preset);
      const next = order[(idx + 1) % order.length];
      currentState.current_color_preset = next;
      currentState.color_preset_label = labels[next];
      return currentState;
    }
    if (cmd === "set_color_preset") {
      const labels = { 5: "6500K 标准", 4: "5000K 暖光", 6: "7500K 清冷", 8: "9300K 冷白", 2: "原生色温" };
      currentState.current_color_preset = args.preset;
      currentState.color_preset_label = labels[args.preset] || "6500K 标准";
      return currentState;
    }
    if (cmd === "set_display_mode") {
      const target = (currentState.hosts || []).find((h) => h.id === args.hostId);
      if (target) {
        const summary = `${args.width}×${args.height} · ${args.hz || 75}Hz${args.hidpi ? " · HiDPI" : ""}`;
        target.profile_summary = summary;
        target.live_mode_summary = summary;
        if (Array.isArray(target.display_modes)) {
          for (const m of target.display_modes) {
            m.active =
              m.width === args.width &&
              m.height === args.height &&
              (!args.hz || m.hz === args.hz) &&
              Boolean(m.hidpi) === Boolean(args.hidpi);
          }
        }
      }
      return currentState;
    }
    if (cmd === "set_input_provider") {
      currentState.input_provider_host_id = args.hostId;
      return currentState;
    }
    if (cmd === "set_deskflow_enabled") {
      currentState.deskflow_enabled = Boolean(args.enabled);
      currentState.deskflow_running = Boolean(args.enabled);
      currentState.deskflow_connected = Boolean(args.enabled);
      return currentState;
    }
    if (cmd === "toggle_cross_os_modifiers") {
      currentState.unify_cross_os_modifiers = !currentState.unify_cross_os_modifiers;
      return currentState;
    }
    if (cmd === "toggle_plain_slide") {
      if (currentState.deskflow_enabled !== false) {
        currentState.allow_plain_slide_to_internal = !currentState.allow_plain_slide_to_internal;
      }
      return currentState;
    }
    if (cmd === "toggle_ctrl_edge_switch") {
      if (currentState.deskflow_enabled !== false) {
        currentState.ctrl_edge_switch_enabled = !currentState.ctrl_edge_switch_enabled;
      }
      return currentState;
    }
    if (cmd === "swap_input_provider") {
      currentState.input_provider_host_id =
        currentState.input_provider_host_id === "win-desktop" ? "macbook" : "win-desktop";
      return currentState;
    }
    if (cmd === "pair_peers_now") {
      currentState.last_error = null;
      currentState.deskflow_connected = true;
      return currentState;
    }
    return currentState;
  }

  function portLabelByCode(code) {
    if (code === 21) return "USB-C";
    if (code === 17) return "HDMI 2.1";
    if (code === 15) return "DP 1.4";
    return "待读取";
  }

  function compactModeSummary(raw) {
    if (!raw) return "";
    return String(raw)
      .replace(/\s*·?\s*(HiDPI|原生)/gi, "")
      .replace("5120×2880", "5K")
      .replace("5120×3200", "5K")
      .replace("3840×2160", "4K")
      .replace("3840×2400", "4K")
      .replace("2560×1440", "2K")
      .replace("2560×1600", "2.5K")
      .replace("1920×1080", "1080P")
      .replace(/\s*·\s*/g, " ")
      .trim();
  }

  function hostSvgMarkup(hostId) {
    if (hostId === "win-desktop") {
      return '<svg class="icon-17" viewBox="0 0 24 24" aria-hidden="true"><rect x="5" y="2.5" width="14" height="19" rx="2.2"/><line x1="8.5" y1="6.5" x2="15.5" y2="6.5"/><rect x="8.5" y="9.8" width="7" height="5.2" rx="1"/><circle cx="15" cy="18" r="0.9"/></svg>';
    }
    if (hostId === "mac-studio") {
      return '<svg class="icon-17" viewBox="0 0 24 24" aria-hidden="true"><rect x="3.5" y="7" width="17" height="9.8" rx="2.6"/><line x1="7.5" y1="12.6" x2="7.5" y2="14.2"/><line x1="10" y1="12.6" x2="10" y2="14.2"/><line x1="12.6" y1="13.4" x2="15.8" y2="13.4"/></svg>';
    }
    return '<svg class="icon-17" viewBox="0 0 24 24" aria-hidden="true"><rect x="3.8" y="4.5" width="16.4" height="11.5" rx="1.8"/><line x1="10.5" y1="4.5" x2="13.5" y2="4.5"/><path d="M2 18.5h20"/></svg>';
  }

  // Initialize 16-chiclet sliders (brightness + volume)
  const CHICLET_COUNT = 16;
  const chicletCells = [];
  const volCells = [];
  const sliderEl = document.getElementById("traySlider");
  const pctEl = document.getElementById("trayBrightnessPct");
  const volSliderEl = document.getElementById("trayVolSlider");
  const volPctEl = document.getElementById("trayVolumePct");
  const muteBtnEl = document.getElementById("trayMuteBtn");
  const muteIconEl = document.getElementById("trayMuteIcon");
  {
    const track = document.createElement("span");
    track.className = "chiclet-track";
    for (let i = 0; i < CHICLET_COUNT; i++) {
      const cell = document.createElement("i");
      track.appendChild(cell);
      chicletCells.push(cell);
    }
    sliderEl.appendChild(track);
  }
  {
    const vTrack = document.createElement("span");
    vTrack.className = "chiclet-track";
    for (let i = 0; i < CHICLET_COUNT; i++) {
      const cell = document.createElement("i");
      vTrack.appendChild(cell);
      volCells.push(cell);
    }
    volSliderEl.appendChild(vTrack);
  }

  function renderChiclets(pct) {
    const clamped = Math.max(0, Math.min(100, pct));
    const scaled = (clamped / 100) * CHICLET_COUNT;
    for (let i = 0; i < CHICLET_COUNT; i++) {
      const fill = Math.max(0, Math.min(1, scaled - i));
      chicletCells[i].style.setProperty("--f", fill.toFixed(3));
    }
    sliderEl.setAttribute("aria-valuenow", String(Math.round(clamped)));
    pctEl.textContent = `${Math.round(clamped)}%`;
  }

  function renderVolChiclets(pct, muted) {
    const clamped = Math.max(0, Math.min(100, pct));
    const eff = muted ? 0 : clamped;
    const scaled = (eff / 100) * CHICLET_COUNT;
    for (let i = 0; i < CHICLET_COUNT; i++) {
      const fill = Math.max(0, Math.min(1, scaled - i));
      volCells[i].style.setProperty("--f", fill.toFixed(3));
    }
    volSliderEl.setAttribute("aria-valuenow", String(Math.round(clamped)));
    volPctEl.textContent = muted ? "静音" : `${Math.round(clamped)}%`;
    muteBtnEl.setAttribute("aria-pressed", String(Boolean(muted)));
    muteIconEl.innerHTML = muted
      ? '<path d="M11 5L6.5 9H3.5v6h3L11 19V5Z"/><path d="M16 9.5l5 5M21 9.5l-5 5"/>'
      : '<path d="M11 5L6.5 9H3.5v6h3L11 19V5Z"/><path d="M15.5 8.8a4.8 4.8 0 0 1 0 6.4M18.6 6.2a8.5 8.5 0 0 1 0 11.6"/>';
  }

  function render(st) {
    currentState = st;
    const statusDot = document.getElementById("trayStatusDot");
    const statusText = document.getElementById("trayStatusText");
    const titleEl = document.getElementById("trayMonitorTitle");
    if (titleEl && st.monitor_model) {
      titleEl.textContent = `飞利浦 ${st.monitor_model}`;
    }

    const permBar = document.getElementById("trayPermBar");
    if (permBar) {
      permBar.hidden = st.accessibility_trusted !== false;
    }

    const activeHostObj = (st.hosts || []).find(
      (x) => x.enabled && x.monitor_input === st.observed_monitor_input
    );
    const activePortShort = activeHostObj
      ? activeHostObj.port_label || portLabelByCode(activeHostObj.monitor_input)
      : portLabelByCode(st.observed_monitor_input);
    const activeModeStr = activeHostObj
      ? compactModeSummary(activeHostObj.profile_summary || "")
      : "";

    const monitorMissing =
      st.external_display_online === false ||
      Boolean(st.last_error && (st.last_error.includes("未检测到") || st.last_error.includes("DDC 通道")));

    if (st.cooldown_remaining_ms > 0) {
      statusDot.className = "status-dot is-amber";
      statusText.textContent = `握手中 · ${(st.cooldown_remaining_ms / 1000).toFixed(1)}s`;
    } else if (monitorMissing) {
      statusDot.className = "status-dot is-amber";
      statusText.textContent = "外接屏未开机或未连线";
    } else if (st.last_error) {
      statusDot.className = "status-dot is-red";
      statusText.textContent = `回读未过 · ${activePortShort}`;
    } else if (st.edge_locked) {
      statusDot.className = "status-dot is-amber";
      statusText.textContent = `本机被远程中 · ${activePortShort}`;
    } else {
      statusDot.className = "status-dot is-green";
      statusText.textContent = activeModeStr
        ? `已同步 · ${activePortShort} · ${activeModeStr}`
        : `已同步 · ${activePortShort}`;
    }

    const isMacLocal =
      st.hosts.find((h) => h.id === st.local_host_id)?.platform !== "windows";
    const keyPrefix = isMacLocal ? "⌃⌥" : "^Alt ";

    const hostListEl = document.getElementById("trayHostList");
    hostListEl.innerHTML = "";

    for (const h of st.hosts) {
      const isActive = h.enabled && st.observed_monitor_input === h.monitor_input;
      const pLabel = h.port_label || portLabelByCode(h.monitor_input);
      const shortMode = compactModeSummary(h.profile_summary);
      const pSpec = shortMode ? `${pLabel} · ${shortMode}` : pLabel;
      const btn = document.createElement("button");
      btn.type = "button";
      btn.className = `tray-host-row${isActive ? " is-active" : ""}${!h.enabled ? " is-muted" : ""}`;
      btn.disabled = !h.enabled;

      let roleSuffix = "在线就绪";
      if (!h.enabled) {
        roleSuffix = "待接入";
      } else if (isActive) {
        roleSuffix = "当前画面";
      } else if (!h.online) {
        roleSuffix = "点击唤醒";
      } else if (h.id === st.input_provider_host_id) {
        roleSuffix = "键鼠直连";
      }

      const left = document.createElement("span");
      left.className = "tray-host-left";

      const iconWrap = document.createElement("span");
      iconWrap.className = "tray-host-icon";
      iconWrap.innerHTML = hostSvgMarkup(h.id);

      const txtWrap = document.createElement("span");
      txtWrap.className = "tray-host-text";

      const nameEl = document.createElement("span");
      nameEl.className = "tray-host-name";
      nameEl.textContent = h.display_name.replace(" M5 Max", "");

      const metaEl = document.createElement("span");
      metaEl.className = "tray-host-meta tabular-nums";
      const dotEl = document.createElement("span");
      const onlineClass = st.last_error ? "is-standby" : "is-online";
      dotEl.className = `badge-dot ${
        !h.enabled ? "is-standby" : isActive ? "is-active" : h.online ? onlineClass : "is-sleep"
      }`;
      const specTxt = document.createElement("span");
      specTxt.textContent = `${pSpec} · ${roleSuffix}`;
      metaEl.appendChild(dotEl);
      metaEl.appendChild(specTxt);

      txtWrap.appendChild(nameEl);
      txtWrap.appendChild(metaEl);
      left.appendChild(iconWrap);
      left.appendChild(txtWrap);

      const kbd = document.createElement("kbd");
      kbd.className = "tray-kbd tabular-nums";
      kbd.textContent = `${keyPrefix}${h.hotkey_slot}`;

      btn.appendChild(left);
      btn.appendChild(kbd);

      if (h.enabled) {
        btn.addEventListener("click", async () => {
          if (isActive && !st.display_sleeping && h.online) {
            const next = await invokeCmd("switch_to_host", { hostId: h.id, source: "menu" });
            if (next) render(next);
            return;
          }
          currentState.observed_monitor_input = h.monitor_input;
          render(currentState);
          const cmd = h.online ? "switch_to_host" : "wake_and_switch_host";
          const next = await invokeCmd(cmd, { hostId: h.id, source: "menu" });
          if (next) render(next);
        });
      }

      hostListEl.appendChild(btn);
    }

    if (!isDraggingSlider) {
      const bVal = st.current_brightness ?? st.preferred_brightness ?? 85;
      renderChiclets(bVal);
    }
    if (!isDraggingVol) {
      const vVal = st.current_volume ?? st.preferred_volume ?? 75;
      renderVolChiclets(vVal, Boolean(st.audio_muted));
    }

    const dfBtn = document.getElementById("trayDeskflowSwitch");
    const dfEnabled = st.deskflow_enabled !== false && st.accessibility_trusted !== false;
    dfBtn.setAttribute("aria-checked", String(dfEnabled));

    const slideBtn = document.getElementById("traySlideSwitch");
    slideBtn.disabled = !dfEnabled;
    slideBtn.classList.toggle("is-interlocked", !dfEnabled);
    slideBtn.setAttribute(
      "aria-checked",
      String(dfEnabled && Boolean(st.allow_plain_slide_to_internal))
    );

    const ctrlEdgeBtn = document.getElementById("trayCtrlEdgeSwitch");
    if (ctrlEdgeBtn) {
      ctrlEdgeBtn.disabled = !dfEnabled;
      ctrlEdgeBtn.classList.toggle("is-interlocked", !dfEnabled);
      ctrlEdgeBtn.setAttribute(
        "aria-checked",
        String(dfEnabled && st.ctrl_edge_switch_enabled !== false)
      );
    }

    const modBtn = document.getElementById("trayModSwitch");
    modBtn.disabled = !dfEnabled;
    modBtn.classList.toggle("is-interlocked", !dfEnabled);
    modBtn.setAttribute(
      "aria-checked",
      String(dfEnabled && st.unify_cross_os_modifiers !== false)
    );

    const modeLabel = document.getElementById("trayModeLabel");
    if (modeLabel) {
      const activeOnlineHost =
        st.external_display_online !== false && activeHostObj && activeHostObj.online
          ? activeHostObj
          : null;
      const modeHost =
        activeOnlineHost ||
        (st.hosts || []).find((x) => x.id === st.local_host_id) ||
        activeHostObj ||
        (st.hosts || [])[0];
      const activeModeItem =
        modeHost && Array.isArray(modeHost.display_modes)
          ? modeHost.display_modes.find((m) => m.active)
          : null;
      if (activeModeItem) {
        modeLabel.textContent = `${activeModeItem.width}×${activeModeItem.height} · ${
          activeModeItem.hidpi ? "HiDPI" : `${activeModeItem.hz}Hz`
        }`;
      } else {
        const rawMode =
          (modeHost && (modeHost.live_mode_summary || modeHost.profile_summary)) || "2560×1440 · 75Hz";
        modeLabel.textContent = rawMode.replace(/·\s*\d+Hz\s*·\s*HiDPI/i, "· HiDPI");
      }
    }

    const colorLabel = document.getElementById("trayColorLabel");
    colorLabel.textContent = st.color_preset_label || "6500K 标准";

    const providerLabel = document.getElementById("trayProviderLabel");
    providerLabel.textContent =
      st.input_provider_host_id === "win-desktop"
        ? "Windows 直连"
        : st.input_provider_host_id === "mac-studio"
          ? "Mac Studio 直连"
          : "MacBook 直连";

    const footerDot = document.getElementById("trayFooterDot");
    const footerText = document.getElementById("trayFooterText");
    const footerBtn = document.getElementById("trayFooterBtn");

    if (st.last_error) {
      footerDot.className = "status-dot is-red";
      footerText.textContent = "DDC 回读未过 · 点击重试";
      footerBtn.classList.add("is-clickable");
    } else if (st.secure_input_active) {
      footerDot.className = "status-dot is-amber";
      footerText.textContent = "密码框安全输入保护中";
      footerBtn.classList.remove("is-clickable");
    } else if (st.deskflow_enabled === false) {
      footerDot.className = "status-dot is-standby";
      footerText.textContent = "键鼠跨屏守护已暂停 · 各机独立键鼠";
      footerBtn.classList.remove("is-clickable");
    } else if (st.edge_locked) {
      footerDot.className = "status-dot is-amber";
      footerText.textContent = "检测到本机被远程控制 · 跨屏已自动避让";
      footerBtn.classList.remove("is-clickable");
    } else if (st.deskflow_connected) {
      footerDot.className = "status-dot is-green";
      const edgeHint = st.plain_slide_active
        ? "右滑进笔记本屏"
        : st.clamshell_closed
          ? "盒盖边缘已自动锁止"
          : st.ctrl_edge_switch_enabled !== false
            ? "按 Ctrl 切主屏"
            : "单屏边缘已自动锁止";
      footerText.textContent = `键鼠跨屏已加密 · ${edgeHint}`;
      footerBtn.classList.add("is-clickable");
    } else if (st.deskflow_running) {
      footerDot.className = "status-dot is-amber";
      footerText.textContent = "跨屏守护待连接 · 点击校准";
      footerBtn.classList.add("is-clickable");
    } else {
      footerDot.className = "status-dot is-standby";
      footerText.textContent = "单机直连守护中";
      footerBtn.classList.add("is-clickable");
    }
  }

  function updateSliderFromX(clientX, commitNow) {
    const rect = sliderEl.getBoundingClientRect();
    if (rect.width <= 0) return;
    const pct = Math.round(Math.max(0, Math.min(100, ((clientX - rect.left) / rect.width) * 100)));
    currentState.current_brightness = pct;
    currentState.preferred_brightness = pct;
    renderChiclets(pct);

    if (commitNow) {
      clearTimeout(sliderSendTimer);
      sliderSendTimer = null;
      invokeCmd("set_brightness", { brightness: pct }).then((st) => {
        if (st && !isDraggingSlider) render(st);
      });
    } else if (!sliderSendTimer) {
      sliderSendTimer = setTimeout(() => {
        sliderSendTimer = null;
        invokeCmd("set_brightness", { brightness: currentState.preferred_brightness });
      }, 140);
    }
  }

  sliderEl.addEventListener("pointerdown", (e) => {
    e.preventDefault();
    isDraggingSlider = true;
    sliderEl.dataset.dragging = "true";
    try {
      sliderEl.setPointerCapture(e.pointerId);
    } catch (_) {}
    updateSliderFromX(e.clientX, false);
  });

  sliderEl.addEventListener("pointermove", (e) => {
    if (!isDraggingSlider) return;
    updateSliderFromX(e.clientX, false);
  });

  function finishSliderDrag(e) {
    if (!isDraggingSlider) return;
    isDraggingSlider = false;
    delete sliderEl.dataset.dragging;
    try {
      sliderEl.releasePointerCapture(e.pointerId);
    } catch (_) {}
    updateSliderFromX(e.clientX, true);
  }

  sliderEl.addEventListener("pointerup", finishSliderDrag);
  sliderEl.addEventListener("pointercancel", finishSliderDrag);
  sliderEl.addEventListener("keydown", async (e) => {
    const cur = currentState.current_brightness ?? currentState.preferred_brightness ?? 85;
    if (e.key === "ArrowLeft" || e.key === "ArrowDown") {
      e.preventDefault();
      const next = Math.max(0, cur - 6);
      renderChiclets(next);
      const st = await invokeCmd("set_brightness", { brightness: next });
      if (st) render(st);
    } else if (e.key === "ArrowRight" || e.key === "ArrowUp") {
      e.preventDefault();
      const next = Math.min(100, cur + 6);
      renderChiclets(next);
      const st = await invokeCmd("set_brightness", { brightness: next });
      if (st) render(st);
    }
  });

  document.getElementById("btnOpenStage").addEventListener("click", () => {
    closeSelectMenu();
    invokeCmd("open_main_window");
  });

  const selectMenuEl = document.getElementById("traySelectMenu");
  let activeMenuAnchor = null;

  const COLOR_OPTIONS = [
    { value: 5, label: "6500K 标准" },
    { value: 4, label: "5000K 暖光" },
    { value: 6, label: "7500K 清冷" },
    { value: 8, label: "9300K 冷白" },
    { value: 2, label: "原生色温" }
  ];

  function closeSelectMenu() {
    if (activeMenuAnchor) {
      activeMenuAnchor.setAttribute("aria-expanded", "false");
      activeMenuAnchor = null;
    }
    selectMenuEl.hidden = true;
    selectMenuEl.innerHTML = "";
  }

  function openSelectMenu(anchorBtn, options, currentVal, onPick) {
    if (activeMenuAnchor === anchorBtn && !selectMenuEl.hidden) {
      closeSelectMenu();
      return;
    }
    closeSelectMenu();
    activeMenuAnchor = anchorBtn;
    anchorBtn.setAttribute("aria-expanded", "true");
    selectMenuEl.innerHTML = "";

    for (const opt of options) {
      const item = document.createElement("button");
      item.type = "button";
      item.className = "mac-select-item tabular-nums";
      item.setAttribute("role", "option");
      const selected = opt.value === currentVal;
      item.setAttribute("aria-selected", String(selected));
      item.disabled = Boolean(opt.disabled);
      item.innerHTML =
        '<span class="mac-select-check" aria-hidden="true"><svg class="icon-13" viewBox="0 0 24 24"><path d="M4.5 12.5l4.2 4.2L19.5 6"/></svg></span>' +
        `<span>${opt.label}</span>`;
      if (!opt.disabled) {
        item.addEventListener("click", async (ev) => {
          ev.stopPropagation();
          closeSelectMenu();
          await onPick(opt.value);
        });
      }
      selectMenuEl.appendChild(item);
    }

    selectMenuEl.hidden = false;
    const ar = anchorBtn.getBoundingClientRect();
    const mw = Math.max(selectMenuEl.offsetWidth || 152, ar.width);
    const mh = selectMenuEl.offsetHeight || options.length * 28 + 8;
    const vw = window.innerWidth || 332;
    const vh = window.innerHeight || 572;
    const left = Math.max(8, Math.min(vw - mw - 8, ar.right - mw));
    const top = ar.bottom + 4 + mh <= vh - 8 ? ar.bottom + 4 : Math.max(8, ar.top - 4 - mh);
    selectMenuEl.style.minWidth = `${Math.round(mw)}px`;
    selectMenuEl.style.left = `${Math.round(left)}px`;
    selectMenuEl.style.top = `${Math.round(top)}px`;
  }

  window.addEventListener("pointerdown", (e) => {
    if (!selectMenuEl.hidden) {
      if (!selectMenuEl.contains(e.target) && (!activeMenuAnchor || !activeMenuAnchor.contains(e.target))) {
        closeSelectMenu();
      }
    }
  });

  document.getElementById("trayDeskflowSwitch").addEventListener("click", async () => {
    closeSelectMenu();
    if (currentState.accessibility_trusted === false) {
      const st = await invokeCmd("open_accessibility_settings");
      if (st) render(st);
      return;
    }
    const next = currentState.deskflow_enabled === false;
    currentState.deskflow_enabled = next;
    if (!next) {
      currentState.deskflow_running = false;
      currentState.deskflow_connected = false;
    }
    render(currentState);
    const st = await invokeCmd("set_deskflow_enabled", { enabled: next });
    if (st) render(st);
  });

  document.getElementById("traySlideSwitch").addEventListener("click", async () => {
    closeSelectMenu();
    if (currentState.deskflow_enabled === false) return;
    const st = await invokeCmd("toggle_plain_slide");
    if (st) render(st);
  });

  document.getElementById("trayCtrlEdgeSwitch").addEventListener("click", async () => {
    closeSelectMenu();
    if (currentState.deskflow_enabled === false) return;
    const st = await invokeCmd("toggle_ctrl_edge_switch");
    if (st) render(st);
  });

  document.getElementById("trayModSwitch").addEventListener("click", async () => {
    closeSelectMenu();
    if (currentState.deskflow_enabled === false) return;
    const st = await invokeCmd("toggle_cross_os_modifiers");
    if (st) render(st);
  });

  const trayModeBtn = document.getElementById("trayModeBtn");
  if (trayModeBtn) {
    trayModeBtn.addEventListener("click", () => {
      const activeOnlineHost =
        currentState.external_display_online !== false
          ? (currentState.hosts || []).find(
              (x) =>
                x.enabled && x.online && x.monitor_input === currentState.observed_monitor_input
            )
          : null;
      const modeHost =
        activeOnlineHost ||
        (currentState.hosts || []).find((x) => x.id === currentState.local_host_id) ||
        (currentState.hosts || []).find(
          (x) => x.enabled && x.monitor_input === currentState.observed_monitor_input
        ) ||
        (currentState.hosts || [])[0];
      if (!modeHost) return;
      const isMac = modeHost.platform !== "windows";
      const rawModes =
        Array.isArray(modeHost.display_modes) && modeHost.display_modes.length > 0
          ? modeHost.display_modes
          : [
              {
                width: 2560,
                height: 1440,
                hz: 75,
                hidpi: isMac,
                active: (modeHost.profile_summary || "").includes("2560×1440"),
                label: isMac ? "2560×1440 · 75Hz · HiDPI" : "2560×1440 · 75Hz"
              },
              {
                width: 3200,
                height: 1800,
                hz: 75,
                hidpi: isMac,
                active: (modeHost.profile_summary || "").includes("3200×1800"),
                label: isMac ? "3200×1800 · 75Hz · HiDPI" : "3200×1800 · 75Hz"
              },
              {
                width: 3840,
                height: 2160,
                hz: 75,
                hidpi: isMac,
                active: (modeHost.profile_summary || "").includes("3840×2160"),
                label: isMac ? "3840×2160 · 75Hz · HiDPI" : "3840×2160 · 75Hz"
              },
              {
                width: 5120,
                height: 2880,
                hz: 75,
                hidpi: false,
                active: (modeHost.profile_summary || "").includes("5120×2880"),
                label: "5120×2880 · 75Hz · 原生"
              }
            ];

      let curKey = null;
      const modeMap = new Map();
      const opts = rawModes.map((m) => {
        const key = `${m.width}x${m.height}@${m.hz}:${m.hidpi ? 1 : 0}`;
        const label = m.label || `${m.width}×${m.height} · ${m.hz}Hz${m.hidpi ? " · HiDPI" : ""}`;
        modeMap.set(key, m);
        if (
          m.active ||
          (curKey == null &&
            (modeHost.live_mode_summary || modeHost.profile_summary || "").includes(
              `${m.width}×${m.height}`
            ))
        ) {
          curKey = key;
        }
        return { value: key, label };
      });

      openSelectMenu(trayModeBtn, opts, curKey, async (pickedKey) => {
        const chosen = modeMap.get(pickedKey);
        if (!chosen) return;
        const st = await invokeCmd("set_display_mode", {
          hostId: modeHost.id,
          modeId: typeof chosen.mode_id === "number" ? chosen.mode_id : -1,
          width: chosen.width,
          height: chosen.height,
          hz: chosen.hz,
          hidpi: Boolean(chosen.hidpi)
        });
        if (st) render(st);
      });
    });
  }

  const trayColorBtn = document.getElementById("trayColorBtn");
  trayColorBtn.addEventListener("click", () => {
    const cur = currentState.current_color_preset || 5;
    openSelectMenu(trayColorBtn, COLOR_OPTIONS, cur, async (preset) => {
      const st = await invokeCmd("set_color_preset", { preset });
      if (st) render(st);
    });
  });

  function updateVolFromX(clientX, commitNow) {
    const rect = volSliderEl.getBoundingClientRect();
    if (rect.width <= 0) return;
    const pct = Math.round(Math.max(0, Math.min(100, ((clientX - rect.left) / rect.width) * 100)));
    currentState.current_volume = pct;
    currentState.preferred_volume = pct;
    if (pct > 0) currentState.audio_muted = false;
    renderVolChiclets(pct, Boolean(currentState.audio_muted));

    if (commitNow) {
      clearTimeout(volSendTimer);
      volSendTimer = null;
      invokeCmd("set_volume", { volume: pct }).then((st) => {
        if (st && !isDraggingVol) render(st);
      });
    } else if (!volSendTimer) {
      volSendTimer = setTimeout(() => {
        volSendTimer = null;
        invokeCmd("set_volume", { volume: currentState.preferred_volume });
      }, 140);
    }
  }

  volSliderEl.addEventListener("pointerdown", (e) => {
    e.preventDefault();
    isDraggingVol = true;
    volSliderEl.dataset.dragging = "true";
    try {
      volSliderEl.setPointerCapture(e.pointerId);
    } catch (_) {}
    updateVolFromX(e.clientX, false);
  });

  volSliderEl.addEventListener("pointermove", (e) => {
    if (!isDraggingVol) return;
    updateVolFromX(e.clientX, false);
  });

  function finishVolDrag(e) {
    if (!isDraggingVol) return;
    isDraggingVol = false;
    delete volSliderEl.dataset.dragging;
    try {
      volSliderEl.releasePointerCapture(e.pointerId);
    } catch (_) {}
    updateVolFromX(e.clientX, true);
  }

  volSliderEl.addEventListener("pointerup", finishVolDrag);
  volSliderEl.addEventListener("pointercancel", finishVolDrag);
  volSliderEl.addEventListener("keydown", async (e) => {
    const cur = currentState.current_volume ?? currentState.preferred_volume ?? 75;
    if (e.key === "ArrowLeft" || e.key === "ArrowDown") {
      e.preventDefault();
      const next = Math.max(0, cur - 6);
      renderVolChiclets(next, false);
      const st = await invokeCmd("set_volume", { volume: next });
      if (st) render(st);
    } else if (e.key === "ArrowRight" || e.key === "ArrowUp") {
      e.preventDefault();
      const next = Math.min(100, cur + 6);
      renderVolChiclets(next, false);
      const st = await invokeCmd("set_volume", { volume: next });
      if (st) render(st);
    }
  });

  muteBtnEl.addEventListener("click", async () => {
    const nextMuted = !currentState.audio_muted;
    const st = await invokeCmd("toggle_audio_mute", { muted: nextMuted });
    if (st) render(st);
  });

  const trayProviderBtn = document.getElementById("trayProviderBtn");
  trayProviderBtn.addEventListener("click", () => {
    const opts = (currentState.hosts || []).map((h) => ({
      value: h.id,
      label:
        h.id === "win-desktop"
          ? "Windows 直连"
          : h.id === "mac-studio"
            ? "Mac Studio 直连"
            : "MacBook 直连",
      disabled: !h.enabled
    }));
    openSelectMenu(trayProviderBtn, opts, currentState.input_provider_host_id, async (hostId) => {
      currentState.input_provider_host_id = hostId;
      render(currentState);
      const st = await invokeCmd("set_input_provider", { hostId });
      if (st) render(st);
    });
  });

  document.getElementById("traySleepBtn").addEventListener("click", async () => {
    closeSelectMenu();
    invokeCmd("close_tray_popover");
    if (currentState && currentState.display_sleeping) {
      await invokeCmd("wake_displays");
    } else {
      await invokeCmd("sleep_displays");
    }
  });

  document.getElementById("trayLeaveBtn").addEventListener("click", async () => {
    closeSelectMenu();
    const st = await invokeCmd("leave_desk_to_windows");
    if (st) render(st);
  });

  document.getElementById("trayGatherBtn").addEventListener("click", async () => {
    closeSelectMenu();
    await invokeCmd("gather_windows");
    invokeCmd("close_tray_popover");
  });

  document.getElementById("trayFooterBtn").addEventListener("click", async () => {
    if (currentState.last_error && currentState.recent_requests?.[0]?.to_host) {
      const st = await invokeCmd("switch_to_host", {
        hostId: currentState.recent_requests[0].to_host,
        source: "menu"
      });
      if (st) render(st);
      return;
    }
    const st = await invokeCmd("pair_peers_now");
    if (st) render(st);
  });

  document.getElementById("trayQuitBtn").addEventListener("click", () => {
    invokeCmd("quit_app");
  });

  document.getElementById("trayPermOpenBtn").addEventListener("click", async () => {
    const st = await invokeCmd("open_accessibility_settings");
    if (st) render(st);
  });

  document.getElementById("trayPermRestartBtn").addEventListener("click", async () => {
    const st = await invokeCmd("relaunch_app");
    if (st) render(st);
  });

  window.addEventListener("keydown", (e) => {
    if (e.key === "Escape") {
      if (!selectMenuEl.hidden) {
        closeSelectMenu();
        return;
      }
      invokeCmd("close_tray_popover");
    }
  });

  window.addEventListener("blur", () => {
    if (!params.get("menu")) {
      closeSelectMenu();
    }
  });

  document.addEventListener("visibilitychange", () => {
    if (document.hidden && !params.get("menu")) {
      closeSelectMenu();
    }
  });

  async function refresh() {
    try {
      const st = await invokeCmd("get_dashboard_state");
      if (st) render(st);
    } catch (_) {
      render(currentState);
    }
    const qMenu = params.get("menu");
    if (qMenu === "color" && selectMenuEl.hidden) {
      trayColorBtn.click();
    } else if (qMenu === "mode" && selectMenuEl.hidden && trayModeBtn) {
      trayModeBtn.click();
    } else if (qMenu === "provider" && selectMenuEl.hidden) {
      trayProviderBtn.click();
    }
  }

  window.__closeTrayTransientMenus = closeSelectMenu;
  window.__refreshTrayPopover = () => {
    if (!params.get("menu")) {
      closeSelectMenu();
    }
    return refresh();
  };
  refresh();

  if (hasTauri) {
    setInterval(() => {
      if (!document.hidden) {
        refresh();
      }
    }, 1200);
  }
})();
