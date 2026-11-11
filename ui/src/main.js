import * as THREE from 'three';
import {
  loadOfficialStudioAssets,
  makeMacBook5KScreenTexture,
  makeWindows300HzScreenTexture,
  makeMacStudioScreenTexture
} from './textures.js';
import {
  makeContactShadowTexture,
  makeBounceGlowTexture,
  buildMacOSStudioDesk,
  buildPhilips27E2N6900RW,
  buildWindowsWorkstationPC,
  buildMacStudioM5,
  buildMacBookPro14,
  buildTopologyBridgeAndCables
} from './hardware.js';

const hasTauri = Boolean(
  window.__TAURI_INTERNALS__ && typeof window.__TAURI_INTERNALS__.invoke === 'function'
);

if (
  (hasTauri && /Mac/i.test(navigator.userAgent || navigator.platform || '')) ||
  new URLSearchParams(window.location.search).get('macos') === '1'
) {
  document.documentElement.classList.add('is-macos-overlay');
}

const RISER_Y = 3.2;

const TOPOLOGY_CONFIGS = {
  dual: {
    hosts: ['macbook', 'windows'],
    camPos: new THREE.Vector3(-1.2, 43.5, 208.0),
    camLook: new THREE.Vector3(-1.2, 16.0, -3.5),
    devices: {
      windows: { x: -53.0, y: 0.0,     z: -2.4, rotY: 0, visible: true },
      studio:  { x: -35.5, y: 0.0,     z: 2.2,  rotY: 0, visible: false },
      monitor: { x: -1.2,  y: RISER_Y, z: -6.2, rotY: 0, visible: true },
      macbook: { x: 47.8,  y: 0.0,     z: 4.8,  rotY: 0, visible: true }
    }
  },
  triple: {
    hosts: ['macbook', 'windows', 'studio'],
    camPos: new THREE.Vector3(-3.2, 45.5, 232.0),
    camLook: new THREE.Vector3(-3.2, 16.2, -3.5),
    devices: {
      windows: { x: -66.5, y: 0.0,     z: -2.8, rotY: 0, visible: true },
      studio:  { x: -36.5, y: 0.0,     z: 2.2,  rotY: 0, visible: true },
      monitor: { x: 6.2,   y: RISER_Y, z: -6.2, rotY: 0, visible: true },
      macbook: { x: 54.8,  y: 0.0,     z: 4.8,  rotY: 0, visible: true }
    }
  },
  single: {
    hosts: ['macbook'],
    camPos: new THREE.Vector3(2.5, 41.5, 192.0),
    camLook: new THREE.Vector3(2.5, 16.0, -3.5),
    devices: {
      windows: { x: -55.0, y: 0.0,     z: -2.4, rotY: 0, visible: false },
      studio:  { x: -36.0, y: 0.0,     z: 2.2,  rotY: 0, visible: false },
      monitor: { x: -16.8, y: RISER_Y, z: -5.8, rotY: 0, visible: true },
      macbook: { x: 32.5,  y: 0.0,     z: 4.5,  rotY: 0, visible: true }
    }
  }
};

const STAGE_TO_BACKEND_ID = {
  macbook: 'macbook',
  windows: 'win-desktop',
  studio: 'mac-studio'
};

const BACKEND_TO_STAGE_ID = {
  macbook: 'macbook',
  'win-desktop': 'windows',
  'mac-studio': 'studio'
};

function portLabelForInputCode(code) {
  if (code === 21) return 'USB-C';
  if (code === 17) return 'HDMI 2.1';
  if (code === 15) return 'DP 1.4';
  return '视频信号';
}

const HOST_META = {
  macbook: {
    title: 'MacBook Pro 14',
    ambientRgb: '98, 168, 236',
    bounceHex: 0x6caee8,
    defaultInput: 21,
    defaultMode: '5120×2880 · 75Hz',
    gpuBrief: 'Apple M5'
  },
  windows: {
    title: 'Windows 台式机',
    ambientRgb: '56, 152, 255',
    bounceHex: 0x3b9eff,
    defaultInput: 17,
    defaultMode: '2560×1440 · 300Hz',
    gpuBrief: 'RTX 5060 Ti'
  },
  studio: {
    title: 'Mac Studio',
    ambientRgb: '216, 146, 118',
    bounceHex: 0xd89276,
    defaultInput: 15,
    defaultMode: '5120×2880 · 120Hz',
    gpuBrief: 'M5 Max'
  }
};

const params = new URLSearchParams(window.location.search);

function resolveInitialTheme() {
  const q = params.get('theme');
  if (q === 'light' || q === 'dark') return q;
  try {
    const saved = localStorage.getItem('kvmddc.theme');
    if (saved === 'light' || saved === 'dark') return saved;
  } catch (_) {}
  if (window.matchMedia && window.matchMedia('(prefers-color-scheme: light)').matches) {
    return 'light';
  }
  return 'dark';
}

const demoMode = params.get('demo') || 'normal';
const forcedTopo = ['dual', 'triple', 'single'].includes(params.get('topology'))
  ? params.get('topology')
  : null;

function buildMockDashboard(mode, topoHint) {
  const isSwap = mode === 'swap';
  const isTriple = topoHint === 'triple' || mode === 'triple' || isSwap;
  const isSingle = topoHint === 'single';
  const winInput = isSwap ? 15 : 17;
  const stuInput = isSwap ? 17 : 15;
  const activeInput =
    mode === 'win' ? winInput : mode === 'studio' || isSwap ? stuInput : 21;
  const colorCode = mode === 'win' ? 6 : 5;
  const colorLabelByCode = (c) =>
    c === 4 ? '5000K 暖光' : c === 6 ? '7500K 清冷' : c === 8 ? '9300K 冷白' : '6500K 标准';
  return {
    local_host_id: 'macbook',
    input_provider_host_id: isSingle ? 'macbook' : 'win-desktop',
    primary_ddc_controller_id: 'macbook',
    deskflow_enabled: mode !== 'df-off',
    ctrl_edge_switch_enabled: true,
    allow_plain_slide_to_internal: true,
    unify_cross_os_modifiers: true,
    observed_monitor_input: activeInput,
    preferred_brightness: mode === 'win' ? 92 : 85,
    current_brightness: mode === 'win' ? 92 : 85,
    preferred_volume: 75,
    current_volume: 75,
    audio_muted: false,
    current_color_preset: colorCode,
    color_preset_label: colorLabelByCode(colorCode),
    last_transfer_summary: null,
    deskflow_running: mode !== 'perm' && !isSingle,
    deskflow_connected: mode !== 'perm' && !isSingle,
    accessibility_trusted: mode !== 'perm',
    external_display_online: mode !== 'offline',
    monitor_model: '27E2N6900RW',
    edge_locked: mode === 'locked',
    secure_input_active: false,
    cooldown_remaining_ms: mode === 'settling' ? 3200 : 0,
    last_error:
      mode === 'error'
        ? 'DDC 输入源回读与目标不一致，请检查大屏连接'
        : mode === 'offline'
          ? '未检测到飞利浦外接屏，请开启显示器电源或检查视频线'
          : null,
    hosts: [
      {
        id: 'macbook',
        display_name: 'MacBook Pro 14',
        platform: 'macos',
        lan_ip: '192.168.1.10',
        monitor_input: 21,
        port_label: 'USB-C',
        detected_live: true,
        manual_port_override: false,
        hotkey_slot: 1,
        enabled: true,
        online: true,
        profile_summary: '2560×1440 · 75Hz',
        live_mode_summary: '2560×1440 · 75Hz',
        internal_height_percent: 81,
        display_modes: [
          { mode_id: 1, width: 2560, height: 1440, pixel_width: 5120, pixel_height: 2880, hz: 75, hidpi: true, active: true, label: '2560×1440 · 75Hz · HiDPI' },
          { mode_id: 2, width: 3200, height: 1800, pixel_width: 6400, pixel_height: 3600, hz: 75, hidpi: true, active: false, label: '3200×1800 · 75Hz · HiDPI' },
          { mode_id: 3, width: 3840, height: 2160, pixel_width: 7680, pixel_height: 4320, hz: 75, hidpi: true, active: false, label: '3840×2160 · 75Hz · HiDPI' },
          { mode_id: 4, width: 5120, height: 2880, pixel_width: 5120, pixel_height: 2880, hz: 75, hidpi: false, active: false, label: '5120×2880 · 75Hz' },
          { mode_id: 5, width: 1920, height: 1080, pixel_width: 3840, pixel_height: 2160, hz: 75, hidpi: true, active: false, label: '1920×1080 · 75Hz · HiDPI' }
        ]
      },
      {
        id: 'win-desktop',
        display_name: 'Windows 台式机',
        platform: 'windows',
        lan_ip: '192.168.1.20',
        monitor_input: winInput,
        port_label: portLabelForInputCode(winInput),
        detected_live: true,
        manual_port_override: false,
        hotkey_slot: 2,
        enabled: !isSingle,
        online: !isSingle,
        profile_summary: '5120×2880 · 75Hz',
        live_mode_summary: '5120×2880 · 75Hz',
        internal_height_percent: 81,
        display_modes: [
          { mode_id: 1, width: 5120, height: 2880, pixel_width: 5120, pixel_height: 2880, hz: 75, hidpi: false, active: true, label: '5120×2880 · 75Hz' },
          { mode_id: 2, width: 3840, height: 2160, pixel_width: 3840, pixel_height: 2160, hz: 75, hidpi: false, active: false, label: '3840×2160 · 75Hz' },
          { mode_id: 3, width: 2560, height: 1440, pixel_width: 2560, pixel_height: 1440, hz: 75, hidpi: false, active: false, label: '2560×1440 · 75Hz' },
          { mode_id: 4, width: 2560, height: 1440, pixel_width: 2560, pixel_height: 1440, hz: 300, hidpi: false, active: false, label: '2560×1440 · 300Hz' },
          { mode_id: 5, width: 1920, height: 1080, pixel_width: 1920, pixel_height: 1080, hz: 75, hidpi: false, active: false, label: '1920×1080 · 75Hz' }
        ]
      },
      {
        id: 'mac-studio',
        display_name: 'Mac Studio',
        platform: 'macos',
        lan_ip: '192.168.1.30',
        monitor_input: stuInput,
        port_label: portLabelForInputCode(stuInput),
        detected_live: isTriple,
        manual_port_override: false,
        hotkey_slot: 3,
        enabled: isTriple,
        online: isTriple,
        profile_summary: '5120×2880 · 120Hz',
        live_mode_summary: isTriple ? '5120×2880 · 120Hz' : null,
        internal_height_percent: 81,
        display_modes: [
          { mode_id: 1, width: 2560, height: 1440, pixel_width: 5120, pixel_height: 2880, hz: 120, hidpi: true, active: true, label: '2560×1440 · 120Hz · HiDPI' },
          { mode_id: 2, width: 3200, height: 1800, pixel_width: 6400, pixel_height: 3600, hz: 120, hidpi: true, active: false, label: '3200×1800 · 120Hz · HiDPI' },
          { mode_id: 3, width: 5120, height: 2880, pixel_width: 5120, pixel_height: 2880, hz: 120, hidpi: false, active: false, label: '5120×2880 · 120Hz' }
        ]
      }
    ],
    recent_requests: [
      {
        id: 1,
        to_host: 'macbook',
        target_input: 21,
        observed_input: mode === 'error' ? 17 : activeInput,
        source: 'menu',
        status: mode === 'error' ? 'failed' : 'done',
        attempts: 1,
        time_hms: '12:00:00'
      }
    ]
  };
}

let mockDb = buildMockDashboard(demoMode, forcedTopo);

async function invokeCmd(cmd, args = {}) {
  if (!hasTauri) {
    if (cmd === 'get_dashboard_state') return mockDb;
    if (cmd === 'switch_to_host' || cmd === 'wake_and_switch_host') {
      const target = mockDb.hosts.find((h) => h.id === args.hostId);
      if (target) {
        target.enabled = true;
        target.online = true;
        mockDb.observed_monitor_input = target.monitor_input;
        mockDb.last_error = null;
      }
      return mockDb;
    }
    if (cmd === 'leave_desk_to_windows') {
      mockDb.observed_monitor_input = 17;
      return mockDb;
    }
    if (cmd === 'set_brightness') {
      mockDb.preferred_brightness = args.brightness;
      mockDb.current_brightness = args.brightness;
      return mockDb;
    }
    if (cmd === 'set_volume') {
      mockDb.preferred_volume = args.volume;
      mockDb.current_volume = args.volume;
      if (args.volume > 0) mockDb.audio_muted = false;
      return mockDb;
    }
    if (cmd === 'toggle_audio_mute') {
      mockDb.audio_muted = Boolean(args.muted);
      return mockDb;
    }
    if (cmd === 'cycle_color_preset') {
      const order = [5, 4, 6, 8, 2];
      const labels = { 5: '6500K 标准', 4: '5000K 暖光', 6: '7500K 清冷', 8: '9300K 冷白', 2: '原生色温' };
      const idx = order.indexOf(mockDb.current_color_preset);
      const next = order[(idx + 1) % order.length];
      mockDb.current_color_preset = next;
      mockDb.color_preset_label = labels[next];
      return mockDb;
    }
    if (cmd === 'set_color_preset') {
      const labels = { 5: '6500K 标准', 4: '5000K 暖光', 6: '7500K 清冷', 8: '9300K 冷白', 2: '原生色温' };
      mockDb.current_color_preset = args.preset;
      mockDb.color_preset_label = labels[args.preset] || '6500K 标准';
      return mockDb;
    }
    if (cmd === 'set_input_provider') {
      mockDb.input_provider_host_id = args.hostId;
      return mockDb;
    }
    if (cmd === 'set_deskflow_enabled') {
      mockDb.deskflow_enabled = Boolean(args.enabled);
      mockDb.deskflow_running = Boolean(args.enabled);
      mockDb.deskflow_connected = Boolean(args.enabled);
      return mockDb;
    }
    if (cmd === 'toggle_ctrl_edge_switch') {
      mockDb.ctrl_edge_switch_enabled = !mockDb.ctrl_edge_switch_enabled;
      return mockDb;
    }
    if (cmd === 'toggle_cross_os_modifiers') {
      mockDb.unify_cross_os_modifiers = !mockDb.unify_cross_os_modifiers;
      return mockDb;
    }
    if (cmd === 'send_files_to_host') {
      const count = Array.isArray(args.paths) ? args.paths.length : 1;
      const t = mockDb.hosts.find((x) => x.id === args.targetHostId);
      mockDb.last_transfer_summary = `已传至 ${t ? t.display_name : '对端'} · ${count} 个文件`;
      return mockDb;
    }
    if (cmd === 'set_internal_alignment') {
      for (const h of mockDb.hosts) {
        if (h.id === 'macbook') h.internal_height_percent = args.percent;
      }
      return mockDb;
    }
    if (cmd === 'toggle_plain_slide') {
      mockDb.allow_plain_slide_to_internal = !mockDb.allow_plain_slide_to_internal;
      return mockDb;
    }
    if (cmd === 'swap_input_provider') {
      mockDb.input_provider_host_id =
        mockDb.input_provider_host_id === 'win-desktop' ? 'macbook' : 'win-desktop';
      return mockDb;
    }
    if (cmd === 'pair_peers_now') {
      mockDb.last_error = null;
      mockDb.deskflow_running = true;
      mockDb.deskflow_connected = true;
      return mockDb;
    }
    if (cmd === 'dismiss_alert') {
      mockDb.last_error = null;
      return mockDb;
    }
    if (cmd === 'sleep_displays') {
      mockDb.display_sleeping = true;
      return mockDb;
    }
    if (cmd === 'wake_displays') {
      mockDb.display_sleeping = false;
      return mockDb;
    }
    if (cmd === 'open_accessibility_settings') {
      return mockDb;
    }
    if (cmd === 'relaunch_app') {
      mockDb.accessibility_trusted = true;
      mockDb.deskflow_running = true;
      mockDb.deskflow_connected = true;
      return mockDb;
    }
    if (cmd === 'set_host_port_override') {
      const h = mockDb.hosts.find((x) => x.id === args.hostId);
      if (h) {
        if (args.monitorInput == null) {
          h.manual_port_override = false;
          h.detected_live = true;
        } else {
          h.monitor_input = args.monitorInput;
          h.port_label = portLabelForInputCode(args.monitorInput);
          h.manual_port_override = true;
          h.detected_live = false;
        }
      }
      return mockDb;
    }
    if (cmd === 'set_display_mode') {
      const targetId = args.hostId || 'macbook';
      const h = mockDb.hosts.find((x) => x.id === targetId);
      if (h) {
        const summary = `${args.width}×${args.height} · ${args.hz || 75}Hz`;
        h.profile_summary = summary;
        h.live_mode_summary = summary;
        if (Array.isArray(h.display_modes)) {
          for (const m of h.display_modes) {
            m.active =
              m.width === args.width &&
              m.height === args.height &&
              (!args.hz || m.hz === args.hz) &&
              Boolean(m.hidpi) === Boolean(args.hidpi);
          }
        }
      }
      return mockDb;
    }
    return mockDb;
  }
  return window.__TAURI_INTERNALS__.invoke(cmd, args);
}

const state = {
  localBackendId: 'macbook',
  topoMode: forcedTopo ? forcedTopo : 'auto',
  topology: forcedTopo || 'dual',
  theme: resolveInitialTheme(),
  host: 'macbook',
  brightness: 85,
  volume: 75,
  audioMuted: false,
  colorPreset: 5,
  colorPresetLabel: '6500K 标准',
  seamAlignPct: 81,
  deskflowEnabled: demoMode !== 'df-off',
  ctrlEdgeEnabled: true,
  plainSlide: true,
  unifyCrossOsModifiers: true,
  portalArmed: demoMode === 'portal',
  deskflowOk: true,
  deskflowRunning: true,
  accessibilityTrusted: demoMode !== 'perm',
  permDismissed: false,
  externalDisplayOnline: demoMode !== 'offline',
  monitorModel: '27E2N6900RW',
  edgeLocked: demoMode === 'locked',
  secureInput: false,
  cooldownMs: 0,
  lastError: null,
  lastTargetBackendId: 'macbook',
  displaySleep: demoMode === 'sleep',
  hidTarget: 'windows',
  hostOnline: { macbook: true, windows: true, studio: false },
  hostEnabled: { macbook: true, windows: true, studio: false },
  hostHw: {
    macbook: {
      input: 21,
      port: 'USB-C',
      mode: '2560×1440 · 75Hz · HiDPI',
      detectedLive: true,
      manualOverride: false,
      modes: []
    },
    windows: {
      input: 17,
      port: 'HDMI 2.1',
      mode: '5120×2880 · 75Hz',
      detectedLive: true,
      manualOverride: false,
      modes: []
    },
    studio: {
      input: 15,
      port: 'DP 1.4',
      mode: '5120×2880 · 120Hz',
      detectedLive: false,
      manualOverride: false,
      modes: []
    }
  },
  topDockOpen: params.get('open') === 'topdock',
  openZoneId: null,
  inspectId: null,
  dropTargetHost: ['windows', 'studio', 'macbook'].includes(params.get('drop'))
    ? params.get('drop')
    : null,
  transferBusyHost: null,
  lastTransferSummary: null,
  transientNotice: null
};

if (['macbook', 'windows', 'studio'].includes(params.get('host'))) {
  const reqHost = params.get('host');
  if (reqHost === 'studio') {
    state.topoMode = 'triple';
    state.topology = 'triple';
    mockDb = buildMockDashboard('studio', 'triple');
  } else if (reqHost === 'windows') {
    mockDb = buildMockDashboard('win', state.topology);
  }
  state.host = reqHost;
}

const openParamMap = {
  top: 'zoneLensTop',
  seam: 'zoneLensSeam',
  mbp: 'zoneLensMbp'
};
if (openParamMap[params.get('open')]) {
  state.openZoneId = openParamMap[params.get('open')];
}

if (['windows', 'studio', 'monitor', 'macbook'].includes(params.get('inspect'))) {
  const reqIns = params.get('inspect');
  if (reqIns === 'studio' && state.topology !== 'triple') {
    state.topoMode = 'triple';
    state.topology = 'triple';
    mockDb = buildMockDashboard(demoMode, 'triple');
  } else if (reqIns === 'windows' && state.topology === 'single') {
    state.topoMode = 'dual';
    state.topology = 'dual';
  }
  state.inspectId = reqIns;
}

let sweepAnim = { active: false, progress: 0 };
function triggerOpticalSweep() {
  sweepAnim.active = true;
  sweepAnim.progress = 0.01;
}

function resolveAutoTopology(dto) {
  const stu = dto.hosts?.find((h) => h.id === 'mac-studio');
  const win = dto.hosts?.find((h) => h.id === 'win-desktop');
  if (stu && (stu.enabled || stu.online)) return 'triple';
  if (win && (win.enabled || win.online)) return 'dual';
  return 'single';
}

function ensureValidHostForTopology() {
  const allowed = TOPOLOGY_CONFIGS[state.topology].hosts;
  if (!allowed.includes(state.host)) {
    state.host = 'macbook';
  }
  if (state.topology === 'single' && (state.inspectId === 'windows' || state.inspectId === 'studio')) {
    state.inspectId = null;
  } else if (state.topology === 'dual' && state.inspectId === 'studio') {
    state.inspectId = null;
  }
}

window.__kvmPerf = {
  isIdleSleeping: false,
  frameSampleCount: 0,
  get activeTopology() { return state.topology; },
  get activeHost() { return state.host; },
  get activeInspect() { return state.inspectId; }
};

const canvas = document.getElementById('stageCanvas');
const renderer = new THREE.WebGLRenderer({
  canvas,
  antialias: true,
  alpha: true,
  powerPreference: 'default'
});
renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 1.65));
renderer.setSize(window.innerWidth, window.innerHeight);
renderer.outputColorSpace = THREE.SRGBColorSpace;
renderer.toneMapping = THREE.NoToneMapping;

const scene = new THREE.Scene();
const camera = new THREE.PerspectiveCamera(27.5, window.innerWidth / window.innerHeight, 4, 520);

const contactTex = makeContactShadowTexture();
const studioDesk = buildMacOSStudioDesk(contactTex);
scene.add(studioDesk.group);

function createContactShadow(w, d, baseOpacity, renderOrder = 3) {
  const mat = new THREE.MeshBasicMaterial({
    map: contactTex,
    transparent: true,
    opacity: baseOpacity,
    depthWrite: false
  });
  const mesh = new THREE.Mesh(new THREE.PlaneGeometry(w, d).rotateX(-Math.PI / 2), mat);
  mesh.position.y = 0.03;
  mesh.renderOrder = renderOrder;
  mesh.userData.baseOpacity = baseOpacity;
  scene.add(mesh);
  return mesh;
}

const shadows = {
  windows: createContactShadow(32, 20, 0.62, 3),
  studio: createContactShadow(21, 14, 0.58, 3),
  monitor: createContactShadow(28, 12, 0.50, 4),
  macbook: createContactShadow(37, 19, 0.60, 3)
};

const bounceMat = new THREE.MeshBasicMaterial({
  map: makeBounceGlowTexture(),
  color: 0x6caee8,
  transparent: true,
  opacity: 0.16,
  depthWrite: false
});
const deskBounceMesh = new THREE.Mesh(
  new THREE.PlaneGeometry(82, 30).rotateX(-Math.PI / 2),
  bounceMat
);
deskBounceMesh.position.set(-1.2, 0.04, 6.0);
deskBounceMesh.renderOrder = 3;
scene.add(deskBounceMesh);

const dom = {
  html: document.documentElement,
  alertBanner: document.getElementById('alertBanner'),
  alertDot: document.getElementById('alertDot'),
  alertMessage: document.getElementById('alertMessage'),
  alertRetryBtn: document.getElementById('alertRetryBtn'),
  alertDismissBtn: document.getElementById('alertDismissBtn'),

  topDockZone: document.getElementById('topDockZone'),
  topEdgeLip: document.getElementById('topEdgeLip'),
  topDockDot: document.getElementById('topDockDot'),
  topDockBar: document.getElementById('topDockBar'),

  topologyBar: document.getElementById('topologyBar'),
  topologyBtns: Array.from(document.querySelectorAll('#topologyBar [data-topo-mode]')),
  btnPairNow: document.getElementById('btnPairNow'),
  btnLeaveDesk: document.getElementById('btnLeaveDesk'),
  btnGatherWin: document.getElementById('btnGatherWin'),
  btnSleepTop: document.getElementById('btnSleepTop'),
  themeSwitch: document.getElementById('themeSwitch'),
  themeBtns: Array.from(document.querySelectorAll('#themeSwitch [data-theme-val]')),

  hitWinMesh: document.getElementById('hitWinMesh'),
  hitStudioMesh: document.getElementById('hitStudioMesh'),
  hitExtScreen: document.getElementById('hitExtScreen'),
  hitMbpScreen: document.getElementById('hitMbpScreen'),

  zones: Array.from(document.querySelectorAll('.edge-zone')),
  zoneLensTop: document.getElementById('zoneLensTop'),
  zoneLensSeam: document.getElementById('zoneLensSeam'),
  zoneLensMbp: document.getElementById('zoneLensMbp'),

  lensTopDot: document.getElementById('lensTopDot'),
  lensSeamDot: document.getElementById('lensSeamDot'),
  lensHostSeg: document.getElementById('lensHostSeg'),
  lensHostBtns: Array.from(document.querySelectorAll('#lensHostSeg [data-host]')),
  lensSlider: document.getElementById('lensSlider'),
  lensBrightnessPct: document.getElementById('lensBrightnessPct'),
  lensMuteBtn: document.getElementById('lensMuteBtn'),
  lensMuteIcon: document.getElementById('lensMuteIcon'),
  lensVolSlider: document.getElementById('lensVolSlider'),
  lensVolumePct: document.getElementById('lensVolumePct'),
  lensColorBtn: document.getElementById('lensColorBtn'),
  lensColorLabel: document.getElementById('lensColorLabel'),
  lensSleepBtn: document.getElementById('lensSleepBtn'),
  lensSleepLabel: document.getElementById('lensSleepLabel'),

  lensAlignVal: document.getElementById('lensAlignVal'),
  alignUpBtn: document.getElementById('alignUpBtn'),
  alignDownBtn: document.getElementById('alignDownBtn'),
  lensSlideBtn: document.getElementById('lensSlideBtn'),
  lensPortalBtn: document.getElementById('lensPortalBtn'),
  lensDeskflowBtn: document.getElementById('lensDeskflowBtn'),
  lensModMapBtn: document.getElementById('lensModMapBtn'),

  lensKmBtn: document.getElementById('lensKmBtn'),
  lensKmLabel: document.getElementById('lensKmLabel'),

  mounts: {
    windows: document.getElementById('mountWindows'),
    studio: document.getElementById('mountStudio'),
    monitor: document.getElementById('mountMonitor'),
    macbook: document.getElementById('mountMacbook')
  },

  cardWindows: document.getElementById('cardWindows'),
  dotWindows: document.getElementById('dotWindows'),
  stateWindows: document.getElementById('stateWindows'),
  specWindows: document.getElementById('specWindows'),
  btnCycleWinPort: document.getElementById('btnCycleWinPort'),
  btnSelectWinMode: document.getElementById('btnSelectWinMode'),
  insWinPort: document.getElementById('insWinPort'),
  insWinMode: document.getElementById('insWinMode'),
  insWinHost: document.getElementById('insWinHost'),
  insWinKm: document.getElementById('insWinKm'),

  cardStudio: document.getElementById('cardStudio'),
  dotStudio: document.getElementById('dotStudio'),
  stateStudio: document.getElementById('stateStudio'),
  specStudio: document.getElementById('specStudio'),
  btnCycleStuPort: document.getElementById('btnCycleStuPort'),
  btnSelectStuMode: document.getElementById('btnSelectStuMode'),
  insStuPort: document.getElementById('insStuPort'),
  insStuMode: document.getElementById('insStuMode'),
  insStuHost: document.getElementById('insStuHost'),
  insStuKm: document.getElementById('insStuKm'),

  cardMonitor: document.getElementById('cardMonitor'),
  nameMonitor: document.getElementById('nameMonitor'),
  dotMonitor: document.getElementById('dotMonitor'),
  stateMonitor: document.getElementById('stateMonitor'),
  specMonitor: document.getElementById('specMonitor'),
  insMonInput: document.getElementById('insMonInput'),
  btnSelectMonMode: document.getElementById('btnSelectMonMode'),
  insMonMode: document.getElementById('insMonMode'),
  insMonBri: document.getElementById('insMonBri'),
  btnCycleColorPreset: document.getElementById('btnCycleColorPreset'),
  insMonColor: document.getElementById('insMonColor'),

  cardMacbook: document.getElementById('cardMacbook'),
  dotMacbook: document.getElementById('dotMacbook'),
  stateMacbook: document.getElementById('stateMacbook'),
  specMacbook: document.getElementById('specMacbook'),
  btnCycleMbpPort: document.getElementById('btnCycleMbpPort'),
  btnSelectMbpMode: document.getElementById('btnSelectMbpMode'),
  insMbpPort: document.getElementById('insMbpPort'),
  insMbpMode: document.getElementById('insMbpMode'),
  insMbpSync: document.getElementById('insMbpSync'),

  permOverlay: document.getElementById('permOverlay'),
  permDot: document.getElementById('permDot'),
  permStatusText: document.getElementById('permStatusText'),
  permOpenBtn: document.getElementById('permOpenBtn'),
  permRestartBtn: document.getElementById('permRestartBtn'),
  permLaterBtn: document.getElementById('permLaterBtn'),
  stageSelectMenu: document.getElementById('stageSelectMenu')
};

const CHICLET_COUNT = 16;
const chicletCells = [];
const volChicletCells = [];
{
  const track = document.createElement('span');
  track.className = 'chiclet-track';
  for (let i = 0; i < CHICLET_COUNT; i++) {
    const cell = document.createElement('i');
    track.appendChild(cell);
    chicletCells.push(cell);
  }
  dom.lensSlider.appendChild(track);
}
{
  const vTrack = document.createElement('span');
  vTrack.className = 'chiclet-track';
  for (let i = 0; i < CHICLET_COUNT; i++) {
    const cell = document.createElement('i');
    vTrack.appendChild(cell);
    volChicletCells.push(cell);
  }
  dom.lensVolSlider.appendChild(vTrack);
}

function renderChicletsInto(cells, pct) {
  const scaled = (THREE.MathUtils.clamp(pct, 0, 100) / 100) * CHICLET_COUNT;
  for (let i = 0; i < CHICLET_COUNT; i++) {
    const fill = THREE.MathUtils.clamp(scaled - i, 0, 1);
    cells[i].style.setProperty('--f', fill.toFixed(3));
  }
}

function renderChiclets(pct) {
  renderChicletsInto(chicletCells, pct);
}

function speakerSvgMarkup(muted) {
  if (muted) {
    return '<path d="M11 5L6.5 9H3.5v6h3L11 19V5Z" /><path d="M16 9.5l5 5M21 9.5l-5 5" />';
  }
  return '<path d="M11 5L6.5 9H3.5v6h3L11 19V5Z" /><path d="M15.5 8.8a4.8 4.8 0 0 1 0 6.4M18.6 6.2a8.5 8.5 0 0 1 0 11.6" />';
}

function shortColorTag(fullLabel) {
  if (!fullLabel) return '6500K';
  const token = String(fullLabel).split(/\s+/)[0];
  return token === '原生色温' ? '原生' : token;
}

function setSegmentedActive(container, btns, activeIdx) {
  container.style.setProperty('--active-idx', String(Math.max(0, activeIdx)));
  btns.forEach((b, idx) => {
    b.classList.toggle('is-active', idx === activeIdx);
  });
}

async function bootstrap() {
  const maxAniso = renderer.capabilities.getMaxAnisotropy();
  const assets = await loadOfficialStudioAssets(maxAniso);

  const texMac27 = makeMacBook5KScreenTexture(assets.wallpapers.wallMbpPro);
  const texWin27 = makeWindows300HzScreenTexture(assets.wallpapers.wallWinOfficial);
  const texStudio27 = makeMacStudioScreenTexture(assets.wallpapers.wallStudio);

  const studioGroup = new THREE.Group();
  scene.add(studioGroup);

  const windowsPC = buildWindowsWorkstationPC({
    texIdle: assets.plates.winIdle,
    texActive: assets.plates.winActive,
    meta: assets.meta.windows
  });
  const macStudio = buildMacStudioM5({
    texStudio: assets.plates.studio,
    meta: assets.meta.studio
  });
  const philipsDisplay = buildPhilips27E2N6900RW({
    texFrame: assets.plates.monitor,
    meta: assets.meta.monitor,
    texMac: texMac27,
    texWin: texWin27,
    texStudio: texStudio27
  });
  const macbookPro = buildMacBookPro14({
    texMbp: assets.plates.macbook,
    meta: assets.meta.macbook
  });
  const topologyBridge = buildTopologyBridgeAndCables();

  studioGroup.add(windowsPC, macStudio, philipsDisplay, macbookPro, topologyBridge);

  const deviceMeshes = {
    windows: windowsPC,
    studio: macStudio,
    monitor: philipsDisplay,
    macbook: macbookPro
  };

  const curCamPos = new THREE.Vector3();
  const curCamLook = new THREE.Vector3();
  const curBounceCol = new THREE.Color(0x6caee8);
  const targetBounceCol = new THREE.Color(0x6caee8);

  const animDevices = {
    windows: { x: -53.0, y: 0,       z: -2.4, rotY: 0, presence: 1 },
    studio:  { x: -35.5, y: 0,       z: 2.2,  rotY: 0, presence: 0 },
    monitor: { x: -1.2,  y: RISER_Y, z: -6.2, rotY: 0, presence: 1 },
    macbook: { x: 47.8,  y: 0,       z: 4.8,  rotY: 0, presence: 1 }
  };

  let isDraggingStage = false;
  let isDraggingSeam = false;
  let isDraggingSlider = false;
  let isDraggingVolSlider = false;
  let sliderSendTimer = null;
  let volSendTimer = null;
  let alignSendTimer = null;
  let noticeTimer = null;
  let dragYaw = 0;
  let dragPitch = 0;
  let lastDragX = 0;
  let lastDragY = 0;
  let mbpDownY = 0;
  let mbpMoved = false;
  let appliedThemeKey = null;
  let manualHostLockUntil = 0;

  const tmpVecA = new THREE.Vector3();
  const tmpVecB = new THREE.Vector3();
  const tmpGrommet = new THREE.Vector3();

  function showNotice(msg) {
    state.transientNotice = msg;
    syncUI();
    clearTimeout(noticeTimer);
    noticeTimer = setTimeout(() => {
      state.transientNotice = null;
      syncUI();
    }, 1800);
  }

  function applyDashboardDto(dto, { snap = false } = {}) {
    if (!dto) return;
    const prevHost = state.host;
    const prevTopo = state.topology;

    if (dto.local_host_id) {
      state.localBackendId = dto.local_host_id;
    }
    if (dto.monitor_model) {
      state.monitorModel = dto.monitor_model;
    }

    for (const h of dto.hosts || []) {
      const sid = BACKEND_TO_STAGE_ID[h.id];
      if (!sid) continue;
      state.hostOnline[sid] = Boolean(h.online);
      state.hostEnabled[sid] = Boolean(h.enabled);
      const rawModeStr = h.live_mode_summary || h.profile_summary || HOST_META[sid].defaultMode;
      const cleanModeStr = String(rawModeStr)
        .replace(/(\d+Hz)\s*·?\s*(HiDPI|原生)/i, '$1')
        .trim();
      state.hostHw[sid] = {
        input: Number(h.monitor_input || HOST_META[sid].defaultInput),
        port: h.port_label || portLabelForInputCode(h.monitor_input),
        mode: cleanModeStr,
        detectedLive: Boolean(h.detected_live),
        manualOverride: Boolean(h.manual_port_override),
        modes: Array.isArray(h.display_modes) ? h.display_modes : []
      };
      if (sid === 'macbook' && !isDraggingSeam && typeof h.internal_height_percent === 'number') {
        state.seamAlignPct = THREE.MathUtils.clamp(h.internal_height_percent, 60, 100);
      }
    }

    if (state.topoMode === 'auto') {
      state.topology = resolveAutoTopology(dto);
    } else if (state.topoMode === 'dual' || state.topoMode === 'triple' || state.topoMode === 'single') {
      state.topology = state.topoMode;
    }

    if (dto.observed_monitor_input && (snap || performance.now() >= manualHostLockUntil)) {
      const matchedHost = (dto.hosts || []).find(
        (h) => h.enabled && h.monitor_input === dto.observed_monitor_input
      );
      const observedStage = matchedHost ? BACKEND_TO_STAGE_ID[matchedHost.id] : null;
      if (observedStage) {
        if (observedStage === 'studio' && state.topoMode === 'auto') {
          state.topology = 'triple';
        }
        state.host = observedStage;
      }
    }

    if (!isDraggingSlider) {
      const bVal = dto.current_brightness ?? dto.preferred_brightness ?? 85;
      state.brightness = THREE.MathUtils.clamp(bVal, 0, 100);
    }
    if (!isDraggingVolSlider) {
      const vVal = dto.current_volume ?? dto.preferred_volume ?? 75;
      state.volume = THREE.MathUtils.clamp(vVal, 0, 100);
    }
    if (typeof dto.audio_muted === 'boolean') {
      state.audioMuted = dto.audio_muted;
    }
    if (typeof dto.current_color_preset === 'number') {
      state.colorPreset = dto.current_color_preset;
    }
    if (dto.color_preset_label) {
      state.colorPresetLabel = dto.color_preset_label;
    }
    if (typeof dto.unify_cross_os_modifiers === 'boolean') {
      state.unifyCrossOsModifiers = dto.unify_cross_os_modifiers;
    }
    if (dto.last_transfer_summary && dto.last_transfer_summary !== state.lastTransferSummary) {
      state.lastTransferSummary = dto.last_transfer_summary;
      if (!snap) {
        showNotice(dto.last_transfer_summary);
      }
    }

    const wasUntrusted = !state.accessibilityTrusted;
    state.deskflowEnabled = dto.deskflow_enabled !== false;
    state.ctrlEdgeEnabled = dto.ctrl_edge_switch_enabled !== false;
    state.plainSlide = Boolean(dto.allow_plain_slide_to_internal);
    state.clamshellClosed = Boolean(dto.clamshell_closed);
    state.plainSlideActive = Boolean(dto.plain_slide_active);
    state.deskflowRunning = Boolean(dto.deskflow_running);
    state.deskflowOk = Boolean(dto.deskflow_connected);
    if (typeof dto.accessibility_trusted === 'boolean') {
      state.accessibilityTrusted = dto.accessibility_trusted;
      if (wasUntrusted && state.accessibilityTrusted) {
        state.permDismissed = false;
        showNotice('辅助功能已生效');
      }
    }
    if (typeof dto.display_sleeping === 'boolean') {
      state.displaySleep = dto.display_sleeping;
    }
    if (typeof dto.external_display_online === 'boolean') {
      state.externalDisplayOnline = dto.external_display_online;
    }
    state.edgeLocked = Boolean(dto.edge_locked);
    state.secureInput = Boolean(dto.secure_input_active);
    state.cooldownMs = Number(dto.cooldown_remaining_ms || 0);
    state.lastError = dto.last_error || null;
    state.hidTarget =
      dto.input_provider_host_id === 'win-desktop'
        ? 'windows'
        : dto.input_provider_host_id === 'mac-studio'
          ? 'studio'
          : 'macbook';
    if (dto.recent_requests?.[0]?.to_host) {
      state.lastTargetBackendId = dto.recent_requests[0].to_host;
    }

    ensureValidHostForTopology();
    syncUI();

    if (!snap && (prevHost !== state.host || prevTopo !== state.topology)) {
      if (prevHost !== state.host) triggerOpticalSweep();
      requestWake(680);
    }
  }

  function toScreenPx(obj3d) {
    obj3d.getWorldPosition(tmpVecA);
    tmpVecA.project(camera);
    return {
      x: (tmpVecA.x * 0.5 + 0.5) * window.innerWidth,
      y: (-tmpVecA.y * 0.5 + 0.5) * window.innerHeight
    };
  }

  function updateAllPortCables() {
    scene.updateMatrixWorld(true);
    philipsDisplay.userData.anchorCableSlot.getWorldPosition(tmpVecB);
    const slotX = tmpVecB.x;
    const slotY = tmpVecB.y;
    const slotZ = tmpVecB.z;

    const rearZ = studioDesk.rearChannelZ;
    const rebuild = topologyBridge.userData.rebuildCableGeometry;

    const routeCfg = {
      windows: { laneZ: rearZ - 0.55, grommetDx: 4.2,  slotDx: -0.45 },
      studio:  { laneZ: rearZ,        grommetDx: 0.6,  slotDx: 0.0   },
      macbook: { laneZ: rearZ + 0.55, grommetDx: -3.6, slotDx: 0.45  }
    };

    for (const id of ['windows', 'studio', 'macbook']) {
      const mesh = deviceMeshes[id];
      mesh.userData.anchorCablePort.getWorldPosition(tmpVecA);
      const sx = tmpVecA.x;
      const sy = tmpVecA.y;
      const sz = tmpVecA.z;

      const rc = routeCfg[id];
      const gx = sx + rc.grommetDx;
      const laneZ = rc.laneZ;
      const targetBackX = slotX + rc.slotDx * 2.2;

      const pts = [
        new THREE.Vector3(sx, sy, sz),
        new THREE.Vector3(
          THREE.MathUtils.lerp(sx, gx, 0.52),
          Math.max(0.28, sy * 0.42),
          THREE.MathUtils.lerp(sz, laneZ, 0.58)
        ),
        new THREE.Vector3(gx, 0.14, laneZ),
        new THREE.Vector3(THREE.MathUtils.lerp(gx, targetBackX, 0.52), 0.14, laneZ),
        new THREE.Vector3(targetBackX, 0.24, laneZ + 1.2),
        new THREE.Vector3(slotX + rc.slotDx, slotY, slotZ)
      ];

      tmpGrommet.set(gx, 0.04, laneZ);
      rebuild(id, pts, tmpGrommet);
    }

    philipsDisplay.userData.anchorRightSeam.getWorldPosition(tmpVecA);
    const alignShift = ((state.seamAlignPct - 81) / 19) * 1.8;
    topologyBridge.userData.gateBar.position.set(tmpVecA.x + 0.12, tmpVecA.y + 1.2, tmpVecA.z + 0.08);
    topologyBridge.userData.ribbonMesh.position.set(
      tmpVecA.x + 3.8,
      tmpVecA.y - 4.4 + alignShift,
      tmpVecA.z + 1.2
    );
  }

  function resolveNonOverlappingCenters(items, minGap, vw, edgePad = 20) {
    if (!items.length) return {};
    const sorted = items.map((it) => {
      const hw = it.w * 0.5;
      return {
        id: it.id,
        w: it.w,
        x: THREE.MathUtils.clamp(it.x, edgePad + hw, Math.max(edgePad + hw, vw - edgePad - hw))
      };
    });
    sorted.sort((a, b) => a.x - b.x);

    for (let pass = 0; pass < 3; pass++) {
      for (let i = 1; i < sorted.length; i++) {
        const req = (sorted[i - 1].w + sorted[i].w) * 0.5 + minGap;
        if (sorted[i].x - sorted[i - 1].x < req) {
          sorted[i].x = sorted[i - 1].x + req;
        }
      }
      const last = sorted[sorted.length - 1];
      const maxRight = vw - edgePad - last.w * 0.5;
      if (last.x > maxRight) last.x = maxRight;

      for (let i = sorted.length - 2; i >= 0; i--) {
        const req = (sorted[i].w + sorted[i + 1].w) * 0.5 + minGap;
        if (sorted[i + 1].x - sorted[i].x < req) {
          sorted[i].x = sorted[i + 1].x - req;
        }
      }
      const first = sorted[0];
      const minLeft = edgePad + first.w * 0.5;
      if (first.x < minLeft) first.x = minLeft;
    }

    const out = {};
    for (const s of sorted) out[s.id] = s.x;
    return out;
  }

  function updateProjectedAnchors() {
    scene.updateMatrixWorld(true);
    camera.updateMatrixWorld(true);
    camera.matrixWorldInverse.copy(camera.matrixWorld).invert();

    const rootStyle = document.documentElement.style;
    const vw = window.innerWidth;
    const vh = window.innerHeight;

    const extTop = toScreenPx(philipsDisplay.userData.anchorTopCrown);
    const extSeam = toScreenPx(philipsDisplay.userData.anchorRightSeam);
    const mbpTop = toScreenPx(macbookPro.userData.anchorTopCrown);

    const crownPad = Math.min(420, Math.floor(vw * 0.36));
    rootStyle.setProperty('--ext-top-x', `${Math.round(THREE.MathUtils.clamp(extTop.x, crownPad, vw - crownPad))}px`);
    rootStyle.setProperty('--ext-top-y', `${Math.round(THREE.MathUtils.clamp(extTop.y, 68, vh - 250))}px`);

    rootStyle.setProperty('--seam-x', `${Math.round(THREE.MathUtils.clamp(extSeam.x + 6, 300, vw - 230))}px`);
    rootStyle.setProperty('--seam-y', `${Math.round(THREE.MathUtils.clamp(extSeam.y, 160, vh - 230))}px`);

    rootStyle.setProperty('--mbp-top-x', `${Math.round(THREE.MathUtils.clamp(mbpTop.x, 340, vw - 140))}px`);
    rootStyle.setProperty('--mbp-top-y', `${Math.round(THREE.MathUtils.clamp(mbpTop.y - 2, 180, vh - 210))}px`);

    const extCenter = toScreenPx(philipsDisplay.userData.activeScreen);
    const mbpCenter = toScreenPx(macbookPro.userData.screenMesh);
    const winCenter = toScreenPx(windowsPC.userData.hitAnchor);
    const stuCenter = toScreenPx(macStudio.userData.hitAnchor);

    rootStyle.setProperty('--ext-hit-x', `${Math.round(extCenter.x)}px`);
    rootStyle.setProperty('--ext-hit-y', `${Math.round(extCenter.y)}px`);
    rootStyle.setProperty('--mbp-hit-x', `${Math.round(mbpCenter.x)}px`);
    rootStyle.setProperty('--mbp-hit-y', `${Math.round(mbpCenter.y)}px`);
    rootStyle.setProperty('--win-hit-x', `${Math.round(winCenter.x)}px`);
    rootStyle.setProperty('--win-hit-y', `${Math.round(winCenter.y)}px`);
    rootStyle.setProperty('--studio-hit-x', `${Math.round(stuCenter.x)}px`);
    rootStyle.setProperty('--studio-hit-y', `${Math.round(stuCenter.y)}px`);

    const rawMounts = {
      windows: toScreenPx(windowsPC.userData.anchorMount),
      studio:  toScreenPx(macStudio.userData.anchorMount),
      monitor: toScreenPx(philipsDisplay.userData.anchorMount),
      macbook: toScreenPx(macbookPro.userData.anchorMount)
    };
    const deskFrontY = toScreenPx(studioDesk.anchorFrontEdge).y;

    const hostPillW = state.topology === 'triple' ? 234 : 248;
    const monPillW = state.topology === 'triple' ? 292 : 296;
    const visibleItems = [];
    if (state.topology !== 'single') visibleItems.push({ id: 'windows', x: rawMounts.windows.x, w: hostPillW });
    if (state.topology === 'triple') visibleItems.push({ id: 'studio',  x: rawMounts.studio.x,  w: hostPillW });
    visibleItems.push({ id: 'monitor', x: rawMounts.monitor.x, w: monPillW });
    visibleItems.push({ id: 'macbook', x: rawMounts.macbook.x, w: hostPillW });

    const solvedX = resolveNonOverlappingCenters(visibleItems, 14, vw, 20);
    const maxMountY = Math.max(480, vh - 216);
    const alignedMountY = THREE.MathUtils.clamp(deskFrontY + 10, 490, maxMountY);

    const varPrefix = {
      windows: 'win',
      studio: 'studio',
      monitor: 'mon',
      macbook: 'mbp'
    };

    for (const id of ['windows', 'studio', 'monitor', 'macbook']) {
      const pfx = varPrefix[id];
      const rx = rawMounts[id].x;
      const sx = solvedX[id] ?? rx;
      const sy = alignedMountY;

      rootStyle.setProperty(`--${pfx}-mount-x`, `${Math.round(sx)}px`);
      rootStyle.setProperty(`--${pfx}-mount-y`, `${Math.round(sy)}px`);

      const mountEl = dom.mounts[id];
      if (mountEl) {
        const tetherDx = THREE.MathUtils.clamp(rx - sx, -64, 64);
        const tetherH = THREE.MathUtils.clamp(sy - (rawMounts[id].y + 2), 12, 82);
        const insHalf = 124;
        let insDx = 0;
        if (sx - insHalf < 18) insDx = 18 - (sx - insHalf);
        else if (sx + insHalf > vw - 18) insDx = (vw - 18) - (sx + insHalf);

        mountEl.style.setProperty('--tether-dx', `${Math.round(tetherDx)}px`);
        mountEl.style.setProperty('--tether-h', `${Math.round(tetherH)}px`);
        mountEl.style.setProperty('--ins-dx', `${Math.round(insDx)}px`);
      }
    }
    syncTopDockAnchor();
  }

  function isMissingMonitorError(err) {
    return Boolean(
      err &&
        (err.includes('未检测到') || err.includes('DDC 通道') || err.includes('关闭电源'))
    );
  }

  let currentTopDockCx = 720;
  let currentTopLipCx = 720;
  function syncTopDockAnchor() {
    const vw = window.innerWidth || 1240;
    const dockW = dom.topDockBar.offsetWidth || 640;
    const halfW = dockW * 0.5 + 10;
    const leftSafe = dom.alertBanner.hidden
      ? dom.html.classList.contains('is-macos-overlay')
        ? 86
        : 20
      : dom.alertBanner.getBoundingClientRect().right + 14;
    const minCenter = Math.max(halfW + 16, leftSafe + halfW);
    const maxCenter = Math.max(minCenter, vw - 16 - halfW);
    const monAnchorX = parseFloat(dom.html.style.getPropertyValue('--ext-top-x')) || vw * 0.5;
    currentTopDockCx = Math.round(THREE.MathUtils.clamp(monAnchorX, minCenter, maxCenter));
    const lipTarget = THREE.MathUtils.clamp(
      monAnchorX,
      currentTopDockCx - (halfW - 44),
      currentTopDockCx + (halfW - 44)
    );
    currentTopLipCx = Math.round(lipTarget);
    dom.html.style.setProperty('--top-dock-cx', `${currentTopDockCx}px`);
    dom.html.style.setProperty('--top-lip-dx', `${currentTopLipCx - currentTopDockCx}px`);
  }

  function syncUI() {
    const topoCfg = TOPOLOGY_CONFIGS[state.topology];
    const hostMeta = HOST_META[state.host];
    const isLight = state.theme === 'light';
    const monitorMissing =
      !state.externalDisplayOnline || isMissingMonitorError(state.lastError);
    const genuineDdcError = Boolean(state.lastError && !monitorMissing);

    dom.html.setAttribute('data-theme', state.theme);
    dom.html.setAttribute('data-topology', state.topology);
    dom.html.style.setProperty(
      '--ambient-rgb',
      state.displaySleep || monitorMissing ? '90, 98, 118' : hostMeta.ambientRgb
    );

    if (appliedThemeKey !== state.theme) {
      studioDesk.setTheme(isLight);
      appliedThemeKey = state.theme;
    }
    for (const id of ['windows', 'studio', 'monitor', 'macbook']) {
      deviceMeshes[id].userData.plateUniforms.uThemeLight.value = isLight ? 1.0 : 0.0;
    }

    targetBounceCol.setHex(hostMeta.bounceHex);

    if (genuineDdcError) {
      dom.alertBanner.hidden = false;
      dom.alertBanner.setAttribute('data-tone', 'red');
      dom.alertDot.className = 'status-dot is-red';
      dom.alertMessage.textContent = '切源未响应';
      dom.alertRetryBtn.textContent = '重试';
      dom.alertRetryBtn.hidden = false;
      dom.alertDismissBtn.hidden = false;
    } else if (state.secureInput) {
      dom.alertBanner.hidden = false;
      dom.alertBanner.setAttribute('data-tone', 'amber');
      dom.alertDot.className = 'status-dot is-amber';
      dom.alertMessage.textContent = '安全输入保护中';
      dom.alertRetryBtn.hidden = true;
      dom.alertDismissBtn.hidden = true;
    } else {
      dom.alertBanner.hidden = true;
    }

    dom.topDockZone.setAttribute('data-open', state.topDockOpen ? 'true' : 'false');
    dom.topDockDot.className = genuineDdcError
      ? 'lip-dot is-red'
      : monitorMissing || state.displaySleep || state.cooldownMs > 0
        ? 'lip-dot is-amber'
        : state.portalArmed
          ? 'lip-dot is-blue'
          : 'lip-dot';
    syncTopDockAnchor();

    const modeOrder = ['auto', 'dual', 'triple'];
    const mIdx = modeOrder.indexOf(state.topoMode);
    setSegmentedActive(dom.topologyBar, dom.topologyBtns, mIdx >= 0 ? mIdx : 0);

    dom.btnSleepTop.setAttribute('aria-pressed', String(state.displaySleep));
    setSegmentedActive(dom.themeSwitch, dom.themeBtns, state.theme === 'dark' ? 1 : 0);

    dom.hitWinMesh.hidden = state.topology === 'single';
    dom.hitStudioMesh.hidden = state.topology !== 'triple';

    dom.zones.forEach((z) => {
      z.setAttribute('data-open', z.id === state.openZoneId ? 'true' : 'false');
    });

    const lipClass = genuineDdcError
      ? 'lip-dot is-red'
      : monitorMissing || state.displaySleep || state.cooldownMs > 0
        ? 'lip-dot is-amber'
        : state.portalArmed
          ? 'lip-dot is-blue'
          : 'lip-dot';
    dom.lensTopDot.className = lipClass;
    dom.lensSeamDot.className = state.portalArmed
      ? 'lip-dot is-blue'
      : genuineDdcError
        ? 'lip-dot is-red'
        : monitorMissing
          ? 'lip-dot is-amber'
          : 'lip-dot';

    const visibleHostBtns = [];
    dom.lensHostBtns.forEach((btn) => {
      const h = btn.dataset.host;
      const isConnected = topoCfg.hosts.includes(h);
      btn.hidden = !isConnected;
      if (isConnected) visibleHostBtns.push(btn);
      const dot = btn.querySelector('.host-dot');
      if (dot) dot.classList.toggle('is-on', h === state.host && !state.displaySleep);
    });
    dom.lensHostSeg.setAttribute('data-count', String(visibleHostBtns.length));
    const activeSegIdx = Math.max(0, visibleHostBtns.findIndex((b) => b.dataset.host === state.host));
    setSegmentedActive(dom.lensHostSeg, visibleHostBtns, activeSegIdx);

    renderChiclets(state.brightness);
    dom.lensSlider.setAttribute('aria-valuenow', String(Math.round(state.brightness)));
    dom.lensBrightnessPct.textContent = `${Math.round(state.brightness)}%`;

    renderChicletsInto(volChicletCells, state.audioMuted ? 0 : state.volume);
    dom.lensVolSlider.setAttribute('aria-valuenow', String(Math.round(state.volume)));
    dom.lensVolumePct.textContent = state.audioMuted ? '静音' : `${Math.round(state.volume)}%`;
    dom.lensMuteBtn.setAttribute('aria-pressed', String(state.audioMuted));
    dom.lensMuteIcon.innerHTML = speakerSvgMarkup(state.audioMuted);
    dom.lensColorLabel.textContent = shortColorTag(state.colorPresetLabel);

    dom.lensSleepBtn.setAttribute('aria-pressed', String(state.displaySleep));
    dom.lensSleepLabel.textContent = state.displaySleep ? '唤醒' : '熄屏';

    const dfActive = Boolean(state.deskflowEnabled && state.accessibilityTrusted);
    dom.lensDeskflowBtn.setAttribute('aria-checked', String(dfActive));

    dom.lensAlignVal.textContent = `${Math.round(state.seamAlignPct)}%`;
    dom.lensSlideBtn.disabled = !dfActive;
    dom.lensSlideBtn.classList.toggle('is-interlocked', !dfActive);
    dom.lensSlideBtn.setAttribute('aria-checked', String(dfActive && state.plainSlide));

    dom.lensPortalBtn.disabled = !dfActive;
    dom.lensPortalBtn.classList.toggle('is-interlocked', !dfActive);
    dom.lensPortalBtn.setAttribute('aria-checked', String(dfActive && state.ctrlEdgeEnabled));

    dom.lensModMapBtn.disabled = !dfActive;
    dom.lensModMapBtn.classList.toggle('is-interlocked', !dfActive);
    dom.lensModMapBtn.setAttribute('aria-checked', String(dfActive && state.unifyCrossOsModifiers));

    dom.lensKmLabel.textContent =
      state.hidTarget === 'windows'
        ? 'Windows 直连'
        : state.hidTarget === 'studio'
          ? 'Mac Studio 直连'
          : 'MacBook 直连';

    const winConnected = state.topology !== 'single';
    const stuConnected = state.topology === 'triple';
    const winActive = winConnected && state.host === 'windows' && !state.displaySleep;
    const stuActive = stuConnected && state.host === 'studio' && !state.displaySleep;
    const macActive = state.host === 'macbook' && !state.displaySleep;

    for (const [id, el] of Object.entries(dom.mounts)) {
      const isExp = state.inspectId === id;
      el.setAttribute('data-expanded', String(isExp));
      const btn = el.querySelector('.mount-pill');
      if (btn) btn.setAttribute('aria-expanded', String(isExp));
    }

    const activeHw = state.hostHw[state.host];
    const winHw = state.hostHw.windows;
    const stuHw = state.hostHw.studio;
    const mbpHw = state.hostHw.macbook;

    const formatPortTag = (hw) =>
      `${hw.port} · ${hw.manualOverride ? '手动指定' : hw.detectedLive ? '自动识别' : '待机认口'}`;

    const routineOnlineClass = genuineDdcError ? 'is-standby' : 'is-online';
    const winDrop = state.dropTargetHost === 'windows' || state.transferBusyHost === 'windows';
    dom.mounts.windows.classList.toggle('is-hidden', !winConnected);
    dom.mounts.windows.setAttribute('data-active', String(winActive));
    dom.mounts.windows.setAttribute('data-drop-target', String(winDrop));
    dom.dotWindows.className = `badge-dot ${
      winActive || winDrop ? 'is-active' : state.hostOnline.windows ? routineOnlineClass : 'is-sleep'
    }`;
    dom.stateWindows.textContent =
      state.transferBusyHost === 'windows'
        ? '局域网传输中'
        : state.dropTargetHost === 'windows'
          ? '松手传文件'
          : winActive
            ? '当前画面'
            : !state.hostOnline.windows
              ? '点击唤醒'
              : state.hidTarget === 'windows'
                ? '键鼠直连'
                : '在线就绪';
    dom.specWindows.textContent = `${winHw.port} · ${winHw.mode}`;
    dom.insWinPort.textContent = formatPortTag(winHw);
    dom.insWinMode.textContent = winHw.mode;
    dom.insWinHost.textContent = state.hostOnline.windows ? 'RTX 5060 Ti · 局域网在线' : '离线休眠 · 支持唤醒';
    dom.insWinKm.textContent = !state.deskflowEnabled
      ? '跨屏守护已暂停'
      : state.edgeLocked
        ? '本机被远程中 · 已自动避让'
        : state.hidTarget === 'windows'
          ? 'USB 接收器直连'
          : '跨屏加密接管';

    const stuDrop = state.dropTargetHost === 'studio' || state.transferBusyHost === 'studio';
    dom.mounts.studio.classList.toggle('is-hidden', !stuConnected);
    dom.mounts.studio.setAttribute('data-active', String(stuActive));
    dom.mounts.studio.setAttribute('data-drop-target', String(stuDrop));
    dom.dotStudio.className = `badge-dot ${
      stuActive || stuDrop ? 'is-active' : state.hostOnline.studio ? routineOnlineClass : 'is-standby'
    }`;
    dom.stateStudio.textContent =
      state.transferBusyHost === 'studio'
        ? '局域网传输中'
        : state.dropTargetHost === 'studio'
          ? '松手传文件'
          : stuActive
            ? '当前画面'
            : state.hostOnline.studio
              ? '待机在线'
              : '预留槽位';
    dom.specStudio.textContent = `${stuHw.port} · ${stuHw.mode}`;
    dom.insStuPort.textContent = formatPortTag(stuHw);
    dom.insStuMode.textContent = stuHw.mode;
    dom.insStuHost.textContent = state.hostEnabled.studio ? 'Apple M5 Max · 局域网在线' : '接入后自动上线';
    dom.insStuKm.textContent = !state.deskflowEnabled
      ? '跨屏守护已暂停'
      : state.hidTarget === 'studio'
        ? 'USB 接收器直连'
        : stuActive
          ? '键鼠跨屏已接管'
          : '切屏自动跟随';

    dom.nameMonitor.textContent = `飞利浦 ${state.monitorModel}`;
    dom.dotMonitor.className = `badge-dot ${
      genuineDdcError
        ? 'is-err'
        : state.transientNotice
          ? 'is-active'
          : monitorMissing || state.displaySleep || state.cooldownMs > 0
            ? 'is-sleep'
            : 'is-online'
    }`;
    dom.stateMonitor.textContent = state.transientNotice
      ? state.transientNotice
      : state.cooldownMs > 0
        ? `握手中 ${(state.cooldownMs / 1000).toFixed(1)}s`
        : monitorMissing
          ? '未连外接屏'
          : state.displaySleep
            ? '背光待机'
            : `${activeHw.port} · ${Math.round(state.brightness)}%`;
    dom.specMonitor.textContent = monitorMissing
      ? '27" 主屏 · 待开启电源或插线'
      : state.displaySleep
        ? '27" 主屏 · 面板熄屏 · 供电保持'
        : `27" 主屏 · ${activeHw.mode}`;
    dom.insMonInput.textContent = monitorMissing
      ? '未检测到外接视频链路'
      : state.displaySleep
        ? `${activeHw.port} · 背光休眠`
        : `${activeHw.port} · ${hostMeta.title}`;
    dom.insMonMode.textContent = activeHw.mode;
    const volDesc = state.audioMuted ? '已静音' : `音量 ${Math.round(state.volume)}%`;
    dom.insMonBri.textContent = monitorMissing
      ? '开启显示器电源后自动接管'
      : state.displaySleep
        ? '面板熄灭 · 唤醒恢复'
        : `亮度 ${Math.round(state.brightness)}% · ${volDesc}`;
    dom.insMonColor.textContent = state.colorPresetLabel;

    const mbpDrop = state.dropTargetHost === 'macbook' || state.transferBusyHost === 'macbook';
    dom.mounts.macbook.setAttribute('data-active', String(macActive));
    dom.mounts.macbook.setAttribute('data-drop-target', String(mbpDrop));
    dom.dotMacbook.className = `badge-dot ${macActive || mbpDrop ? 'is-active' : routineOnlineClass}`;
    dom.stateMacbook.textContent =
      state.transferBusyHost === 'macbook'
        ? '局域网传输中'
        : state.dropTargetHost === 'macbook'
          ? '松手传文件'
          : macActive
            ? '当前画面'
            : '14" 内屏在线';
    dom.specMacbook.textContent = macActive
      ? `${mbpHw.port} · ${mbpHw.mode}`
      : `${mbpHw.port} · 14" 内屏独立显示`;
    dom.insMbpPort.textContent = formatPortTag(mbpHw);
    dom.insMbpMode.textContent = mbpHw.mode;
    dom.insMbpSync.textContent = !state.deskflowEnabled
      ? '跨屏守护已暂停 · 右边缘锁止'
      : state.edgeLocked
        ? '本机被远程中 · 边缘已自动避让'
        : state.clamshellClosed
          ? '盒盖单屏中 · 边缘已自动锁止'
          : state.plainSlide && !state.plainSlideActive
            ? '当前单系统显示 · 边缘已自动锁止'
            : `高度 ${Math.round(state.seamAlignPct)}% · ${
                state.plainSlide ? '右滑进笔记本屏' : '右边缘锁止'
              }`;

    // Permission Guide Sheet visibility (auto-pops on startup when macOS Accessibility is untrusted)
    const shouldShowPerm = !state.accessibilityTrusted && !state.permDismissed;
    dom.permOverlay.hidden = !shouldShowPerm;
    if (shouldShowPerm) {
      dom.permDot.className = 'status-dot is-amber';
      dom.permStatusText.textContent = '待授权';
    }

    windowsPC.userData.setActivityState({ active: winActive, online: winConnected && state.hostOnline.windows });
    philipsDisplay.userData.powerLedMat.color.setHex(state.displaySleep ? 0xf59e0b : 0x60a5fa);
    macStudio.userData.ledMat.color.setHex(stuActive ? 0xffffff : 0x94a3b8);
  }

  let rafId = 0;
  let lastFrameMs = 0;
  let settleUntilMs = 0;
  let perfSampleCount = 0;

  function syncRiserAndShadows() {
    const mon = animDevices.monitor;
    studioDesk.riserGroup.position.set(mon.x, 0.0, mon.z + 0.6);

    for (const id of ['windows', 'studio', 'monitor', 'macbook']) {
      const a = animDevices[id];
      const sh = shadows[id];
      sh.visible = a.presence > 0.02;
      if (id === 'monitor') {
        sh.position.set(a.x, studioDesk.riserHeight + 0.04, a.z + 0.7);
      } else if (id === 'windows') {
        sh.position.set(a.x - 0.8, 0.03, a.z - 3.2);
      } else {
        sh.position.set(a.x, 0.03, a.z + 1.2);
      }
      sh.rotation.y = a.rotY;
      sh.material.opacity = sh.userData.baseOpacity * a.presence;
    }
  }

  function snapSceneImmediately() {
    const cfg = TOPOLOGY_CONFIGS[state.topology];
    curCamPos.copy(cfg.camPos);
    curCamLook.copy(cfg.camLook);
    const mbpYOffset = ((state.seamAlignPct - 81) / 19) * 1.1;

    for (const id of ['windows', 'studio', 'monitor', 'macbook']) {
      const t = cfg.devices[id];
      const a = animDevices[id];
      a.x = t.x;
      a.y = id === 'macbook' ? t.y + mbpYOffset : t.y;
      a.z = t.z;
      a.rotY = t.rotY;
      a.presence = t.visible ? 1 : 0;

      const mesh = deviceMeshes[id];
      mesh.visible = t.visible;
      mesh.position.set(a.x, a.y, a.z);
      mesh.rotation.y = a.rotY;
      mesh.scale.setScalar(t.visible ? 1 : 0.88);
      mesh.userData.plateUniforms.uOpacity.value = a.presence;
    }

    syncRiserAndShadows();

    windowsPC.userData.plateUniforms.uMix.value = windowsPC.userData.targetActiveMix;
    deskBounceMesh.position.x = cfg.devices.monitor.x + 1.5;

    const pu = philipsDisplay.userData.panelUniforms;
    pu.uMixWin.value = state.host === 'windows' ? 1 : 0;
    pu.uMixStudio.value = state.host === 'studio' ? 1 : 0;
    pu.uBrightness.value = state.brightness / 100;
    pu.uPower.value = state.displaySleep ? 0 : 1;
    pu.uPortalAura.value = state.portalArmed ? 1 : 0;

    const ru = topologyBridge.userData.ribbonUniforms;
    ru.uBridgeMode.value = state.plainSlide && !state.edgeLocked && !state.displaySleep ? 1 : 0;
    ru.uPortalGate.value = state.portalArmed ? 1 : 0;
    ru.uIsWin.value = state.host === 'windows' ? 1 : 0;
    topologyBridge.userData.gateUniforms.uActive.value = state.portalArmed && !state.displaySleep ? 1 : 0;

    for (const id of ['windows', 'studio', 'macbook']) {
      const entry = topologyBridge.userData.cables[id];
      const cu = entry.uniforms;
      const isAct = state.host === id && !state.displaySleep;
      cu.uConnected.value = animDevices[id].presence;
      cu.uActive.value = isAct ? 1 : 0;
      cu.uIsLight.value = state.theme === 'light' ? 1 : 0;
      const vis = animDevices[id].presence > 0.02;
      entry.mesh.visible = vis;
      entry.collar.visible = vis;
      entry.collarMat.color.setHex(isAct ? 0x38bdf8 : 0x64748b);
    }

    camera.position.copy(curCamPos);
    camera.lookAt(curCamLook);
    updateAllPortCables();
    updateProjectedAnchors();
    renderer.render(scene, camera);
  }

  function requestWake(holdMs = 520) {
    const now = performance.now();
    settleUntilMs = Math.max(settleUntilMs, now + holdMs);
    window.__kvmPerf.isIdleSleeping = false;
    if (!rafId) {
      lastFrameMs = now;
      rafId = requestAnimationFrame(tick);
    }
  }

  function tick(now) {
    rafId = 0;
    const dt = Math.min(0.05, Math.max(0.001, (now - lastFrameMs) / 1000));
    lastFrameMs = now;
    const k = 1 - Math.exp(-12 * dt);
    const kSlow = 1 - Math.exp(-8.5 * dt);

    let moving = false;
    const cfg = TOPOLOGY_CONFIGS[state.topology];

    if (!isDraggingStage && (Math.abs(dragYaw) > 0.0005 || Math.abs(dragPitch) > 0.0005)) {
      dragYaw *= Math.exp(-7 * dt);
      dragPitch *= Math.exp(-7 * dt);
      moving = true;
    }

    const desiredPos = cfg.camPos.clone();
    desiredPos.x += dragYaw * 18;
    desiredPos.y += dragPitch * 11;

    if (curCamPos.distanceToSquared(desiredPos) > 0.0004) {
      curCamPos.lerp(desiredPos, kSlow);
      moving = true;
    } else {
      curCamPos.copy(desiredPos);
    }

    if (curCamLook.distanceToSquared(cfg.camLook) > 0.0004) {
      curCamLook.lerp(cfg.camLook, kSlow);
      moving = true;
    } else {
      curCamLook.copy(cfg.camLook);
    }

    camera.position.copy(curCamPos);
    camera.lookAt(curCamLook);

    let cablesNeedRebuild = false;
    const mbpYOffset = ((state.seamAlignPct - 81) / 19) * 1.1;

    for (const id of ['windows', 'studio', 'monitor', 'macbook']) {
      const t = cfg.devices[id];
      const a = animDevices[id];
      const targetY = id === 'macbook' ? t.y + mbpYOffset : t.y;
      const targetPresence = t.visible ? 1 : 0;

      if (
        Math.abs(a.x - t.x) > 0.004 ||
        Math.abs(a.y - targetY) > 0.004 ||
        Math.abs(a.z - t.z) > 0.004 ||
        Math.abs(a.rotY - t.rotY) > 0.001 ||
        Math.abs(a.presence - targetPresence) > 0.004
      ) {
        a.x = THREE.MathUtils.lerp(a.x, t.x, kSlow);
        a.y = THREE.MathUtils.lerp(a.y, targetY, kSlow);
        a.z = THREE.MathUtils.lerp(a.z, t.z, kSlow);
        a.rotY = THREE.MathUtils.lerp(a.rotY, t.rotY, kSlow);
        a.presence = THREE.MathUtils.lerp(a.presence, targetPresence, k);
        moving = true;
        cablesNeedRebuild = true;
      } else {
        a.x = t.x;
        a.y = targetY;
        a.z = t.z;
        a.rotY = t.rotY;
        a.presence = targetPresence;
      }

      const mesh = deviceMeshes[id];
      mesh.visible = a.presence > 0.02;
      mesh.position.set(a.x, a.y, a.z);
      mesh.rotation.y = a.rotY;
      mesh.scale.setScalar(THREE.MathUtils.lerp(0.88, 1.0, a.presence));
      mesh.userData.plateUniforms.uOpacity.value = a.presence;
    }

    syncRiserAndShadows();

    const winMixCur = windowsPC.userData.plateUniforms.uMix.value;
    const winMixTgt = windowsPC.userData.targetActiveMix;
    if (Math.abs(winMixCur - winMixTgt) > 0.003) {
      windowsPC.userData.plateUniforms.uMix.value = THREE.MathUtils.lerp(winMixCur, winMixTgt, k);
      moving = true;
    } else {
      windowsPC.userData.plateUniforms.uMix.value = winMixTgt;
    }

    deskBounceMesh.position.x = animDevices.monitor.x + 1.5;

    const pu = philipsDisplay.userData.panelUniforms;
    const tgtWin = state.host === 'windows' ? 1 : 0;
    const tgtStu = state.host === 'studio' ? 1 : 0;
    const tgtBri = state.brightness / 100;
    const tgtPwr = state.displaySleep ? 0 : 1;
    const tgtAura = state.portalArmed && !state.displaySleep ? 1 : 0;

    pu.uMixWin.value = THREE.MathUtils.lerp(pu.uMixWin.value, tgtWin, k);
    pu.uMixStudio.value = THREE.MathUtils.lerp(pu.uMixStudio.value, tgtStu, k);
    pu.uBrightness.value = isDraggingSlider
      ? tgtBri
      : THREE.MathUtils.lerp(pu.uBrightness.value, tgtBri, k);
    pu.uPower.value = THREE.MathUtils.lerp(pu.uPower.value, tgtPwr, k);
    pu.uPortalAura.value = THREE.MathUtils.lerp(pu.uPortalAura.value, tgtAura, k);

    if (
      Math.abs(pu.uMixWin.value - tgtWin) > 0.003 ||
      Math.abs(pu.uMixStudio.value - tgtStu) > 0.003 ||
      Math.abs(pu.uBrightness.value - tgtBri) > 0.003 ||
      Math.abs(pu.uPower.value - tgtPwr) > 0.003
    ) {
      moving = true;
    }

    if (sweepAnim.active) {
      sweepAnim.progress += dt * 2.4;
      if (sweepAnim.progress >= 1) {
        sweepAnim.progress = 0;
        sweepAnim.active = false;
      } else {
        moving = true;
      }
      pu.uSwitchSweep.value = sweepAnim.progress;
    } else {
      pu.uSwitchSweep.value = 0;
    }

    curBounceCol.lerp(targetBounceCol, k);
    bounceMat.color.copy(curBounceCol);
    bounceMat.opacity = (state.theme === 'light' ? 0.10 : 0.18) * pu.uPower.value * (0.35 + 0.65 * pu.uBrightness.value);

    const ru = topologyBridge.userData.ribbonUniforms;
    const tgtBridge = state.plainSlide && !state.edgeLocked && !state.displaySleep ? 1 : 0;
    ru.uBridgeMode.value = THREE.MathUtils.lerp(ru.uBridgeMode.value, tgtBridge, k);
    ru.uPortalGate.value = THREE.MathUtils.lerp(ru.uPortalGate.value, tgtAura, k);
    ru.uIsWin.value = THREE.MathUtils.lerp(ru.uIsWin.value, tgtWin, k);
    topologyBridge.userData.gateUniforms.uActive.value = THREE.MathUtils.lerp(
      topologyBridge.userData.gateUniforms.uActive.value,
      tgtAura,
      k
    );

    for (const id of ['windows', 'studio', 'macbook']) {
      const entry = topologyBridge.userData.cables[id];
      const cu = entry.uniforms;
      const isAct = state.host === id && !state.displaySleep;
      const tgtAct = isAct ? 1 : 0;
      cu.uConnected.value = animDevices[id].presence;
      cu.uActive.value = THREE.MathUtils.lerp(cu.uActive.value, tgtAct, k);
      cu.uIsLight.value = state.theme === 'light' ? 1 : 0;
      cu.uPulse.value = sweepAnim.active ? sweepAnim.progress : 0.48;
      const vis = animDevices[id].presence > 0.02;
      entry.mesh.visible = vis;
      entry.collar.visible = vis;
      entry.collarMat.color.setHex(isAct ? 0x38bdf8 : 0x64748b);
      if (Math.abs(cu.uActive.value - tgtAct) > 0.003) moving = true;
    }

    if (cablesNeedRebuild || isDraggingSeam) {
      updateAllPortCables();
    }
    updateProjectedAnchors();
    renderer.render(scene, camera);

    perfSampleCount += 1;
    window.__kvmPerf.frameSampleCount = perfSampleCount;

    if (moving || isDraggingStage || isDraggingSeam || isDraggingSlider || now < settleUntilMs) {
      rafId = requestAnimationFrame(tick);
    } else {
      window.__kvmPerf.isIdleSleeping = true;
    }
  }

  async function switchHost(nextHost) {
    if (!TOPOLOGY_CONFIGS[state.topology].hosts.includes(nextHost)) return;
    const backendId = STAGE_TO_BACKEND_ID[nextHost];
    if (nextHost === 'studio' && hasTauri && !state.hostEnabled.studio) {
      showNotice('槽位未启用');
      return;
    }
    const changed = state.host !== nextHost || state.displaySleep;
    if (!changed && state.hostOnline[nextHost]) {
      openInspector(nextHost);
      try {
        const dto = await invokeCmd('switch_to_host', { hostId: backendId, source: 'menu' });
        applyDashboardDto(dto);
      } catch (_) {}
      return;
    }
    manualHostLockUntil = performance.now() + 6500;
    state.host = nextHost;
    state.displaySleep = false;
    if (changed) triggerOpticalSweep();
    syncUI();
    requestWake(620);

    try {
      const cmd = state.hostOnline[nextHost] ? 'switch_to_host' : 'wake_and_switch_host';
      const dto = await invokeCmd(cmd, { hostId: backendId, source: 'menu' });
      applyDashboardDto(dto);
    } catch (err) {
      state.lastError = String(err);
      syncUI();
    }
  }

  async function toggleSleepDisplays() {
    if (state.displaySleep) {
      state.displaySleep = false;
      triggerOpticalSweep();
      syncUI();
      requestWake(520);
      try {
        const dto = await invokeCmd('wake_displays');
        if (dto) applyDashboardDto(dto);
      } catch (_) {}
      return;
    }
    state.displaySleep = true;
    syncUI();
    requestWake(520);
    try {
      const dto = await invokeCmd('sleep_displays');
      if (dto) applyDashboardDto(dto);
    } catch (_) {}
  }

  function commitAlignment(pct) {
    const rounded = Math.round(THREE.MathUtils.clamp(pct, 60, 100));
    state.seamAlignPct = rounded;
    syncUI();
    updateAllPortCables();
    requestWake(420);
    clearTimeout(alignSendTimer);
    alignSendTimer = setTimeout(async () => {
      try {
        const dto = await invokeCmd('set_internal_alignment', { percent: rounded });
        applyDashboardDto(dto);
      } catch (_) {}
    }, 180);
  }

  dom.topologyBtns.forEach((btn) => {
    btn.addEventListener('click', () => {
      const nextMode = btn.dataset.topoMode;
      if (state.topoMode === nextMode) return;
      state.topoMode = nextMode;
      state.topology = nextMode === 'auto' ? resolveAutoTopology(mockDb) : nextMode;
      if (hasTauri && nextMode === 'auto') {
        invokeCmd('get_dashboard_state').then((dto) => applyDashboardDto(dto));
      }
      ensureValidHostForTopology();
      syncUI();
      requestWake(750);
    });
  });

  dom.btnPairNow.addEventListener('click', async () => {
    showNotice('配对校准中');
    try {
      const dto = await invokeCmd('pair_peers_now');
      applyDashboardDto(dto);
      showNotice('配对已校准');
    } catch (err) {
      state.lastError = String(err);
      syncUI();
    }
  });

  dom.btnLeaveDesk.addEventListener('click', async () => {
    state.displaySleep = false;
    if (state.topology !== 'single') {
      state.host = 'windows';
      triggerOpticalSweep();
    }
    syncUI();
    requestWake(620);
    try {
      const dto = await invokeCmd('leave_desk_to_windows');
      applyDashboardDto(dto);
      showNotice('已让屏切回');
    } catch (err) {
      state.lastError = String(err);
      syncUI();
    }
  });

  dom.btnGatherWin.addEventListener('click', async () => {
    try {
      await invokeCmd('gather_windows');
      showNotice('窗口已收拢');
    } catch (err) {
      state.lastError = String(err);
      syncUI();
    }
  });

  dom.btnSleepTop.addEventListener('click', toggleSleepDisplays);

  dom.alertRetryBtn.addEventListener('click', async () => {
    const targetStage = BACKEND_TO_STAGE_ID[state.lastTargetBackendId] || state.host;
    await switchHost(targetStage);
  });

  dom.alertDismissBtn.addEventListener('click', async () => {
    state.lastError = null;
    syncUI();
    try {
      const dto = await invokeCmd('dismiss_alert');
      applyDashboardDto(dto);
    } catch (_) {}
  });

  const TOP_DOCK_RETRACT_MS = 3000;
  let topDockRetractTimer = null;
  let isPointerInTopDock = false;
  let isFocusVisibleInTopDock = false;

  function openTopDock() {
    clearTimeout(topDockRetractTimer);
    topDockRetractTimer = null;
    if (!state.topDockOpen) {
      state.topDockOpen = true;
      dom.topDockZone.setAttribute('data-open', 'true');
      syncTopDockAnchor();
    }
  }

  function closeTopDockNow() {
    clearTimeout(topDockRetractTimer);
    topDockRetractTimer = null;
    isPointerInTopDock = false;
    isFocusVisibleInTopDock = false;
    if (state.topDockOpen) {
      state.topDockOpen = false;
      dom.topDockZone.setAttribute('data-open', 'false');
    }
  }

  function scheduleCloseTopDock(delayMs = TOP_DOCK_RETRACT_MS) {
    clearTimeout(topDockRetractTimer);
    topDockRetractTimer = setTimeout(() => {
      topDockRetractTimer = null;
      if (!isPointerInTopDock && !isFocusVisibleInTopDock) {
        state.topDockOpen = false;
        dom.topDockZone.setAttribute('data-open', 'false');
      }
    }, delayMs);
  }

  dom.topDockZone.addEventListener('pointerenter', () => {
    isPointerInTopDock = true;
    openTopDock();
  });

  dom.topDockZone.addEventListener('pointermove', () => {
    isPointerInTopDock = true;
    openTopDock();
  });

  dom.topDockZone.addEventListener('pointerleave', () => {
    isPointerInTopDock = false;
    if (!isFocusVisibleInTopDock) {
      scheduleCloseTopDock(TOP_DOCK_RETRACT_MS);
    }
  });

  dom.topEdgeLip.addEventListener('click', (e) => {
    e.stopPropagation();
    isPointerInTopDock = true;
    openTopDock();
  });

  dom.topDockZone.addEventListener('focusin', (e) => {
    if (e.target && e.target.matches && e.target.matches(':focus-visible')) {
      isFocusVisibleInTopDock = true;
      openTopDock();
    }
  });

  dom.topDockZone.addEventListener('focusout', (e) => {
    const next = e.relatedTarget;
    if (!next || !dom.topDockZone.contains(next)) {
      isFocusVisibleInTopDock = false;
      if (!isPointerInTopDock) {
        scheduleCloseTopDock(TOP_DOCK_RETRACT_MS);
      }
    }
  });

  window.addEventListener('pointermove', (e) => {
    if (!state.topDockOpen && e.clientY >= 0 && e.clientY <= 18) {
      if (Math.abs(e.clientX - currentTopLipCx) <= 145) {
        openTopDock();
      }
    }
  });

  window.addEventListener('pointerdown', (e) => {
    if (state.topDockOpen && !dom.topDockZone.contains(e.target)) {
      closeTopDockNow();
    }
  });

  dom.themeBtns.forEach((btn) => {
    btn.addEventListener('click', () => {
      state.theme = btn.dataset.themeVal;
      try {
        localStorage.setItem('kvmddc.theme', state.theme);
      } catch (_) {}
      if (hasTauri) {
        invokeCmd('set_app_theme', { theme: state.theme }).catch(() => {});
      }
      syncUI();
      requestWake(450);
    });
  });

  window.addEventListener('storage', (e) => {
    if (e.key === 'kvmddc.theme' && (e.newValue === 'light' || e.newValue === 'dark')) {
      state.theme = e.newValue;
      syncUI();
      requestWake(450);
    }
  });

  let activeSelectMenu = { anchor: null, holdZoneId: null, holdInspectId: null };

  const COLOR_PRESET_OPTIONS = [
    { value: 5, label: '6500K 标准' },
    { value: 4, label: '5000K 暖光' },
    { value: 6, label: '7500K 清冷' },
    { value: 8, label: '9300K 冷白' },
    { value: 2, label: '原生色温' }
  ];

  const PORT_OVERRIDE_OPTIONS = [
    { value: null, label: '自动识别（推荐）' },
    { value: 21, label: 'USB-C' },
    { value: 17, label: 'HDMI 2.1' },
    { value: 15, label: 'DP 1.4' }
  ];

  function closeStageSelectMenu({ restoreHoverClose = true } = {}) {
    const prevZone = activeSelectMenu.holdZoneId;
    const prevIns = activeSelectMenu.holdInspectId;
    if (activeSelectMenu.anchor) {
      activeSelectMenu.anchor.setAttribute('aria-expanded', 'false');
    }
    activeSelectMenu = { anchor: null, holdZoneId: null, holdInspectId: null };
    dom.stageSelectMenu.hidden = true;
    dom.stageSelectMenu.innerHTML = '';

    if (restoreHoverClose) {
      if (prevZone) {
        const zEl = document.getElementById(prevZone);
        if (!zEl || !zEl.matches(':hover')) {
          scheduleCloseZone(prevZone);
        }
      }
      if (prevIns) {
        const mEl = dom.mounts[prevIns];
        if (!mEl || !mEl.matches(':hover')) {
          scheduleCloseInspector(prevIns);
        }
      }
    }
  }

  function openStageSelectMenu({ anchor, options, currentValue, holdZoneId = null, holdInspectId = null, onSelect }) {
    if (activeSelectMenu.anchor === anchor && !dom.stageSelectMenu.hidden) {
      closeStageSelectMenu();
      return;
    }
    closeStageSelectMenu({ restoreHoverClose: false });
    activeSelectMenu = { anchor, holdZoneId, holdInspectId };
    anchor.setAttribute('aria-expanded', 'true');
    if (holdZoneId) openZone(holdZoneId);
    if (holdInspectId) openInspector(holdInspectId);

    dom.stageSelectMenu.innerHTML = '';
    for (const opt of options) {
      const btn = document.createElement('button');
      btn.type = 'button';
      btn.className = 'mac-select-item tabular-nums';
      btn.setAttribute('role', 'option');
      const isSel = opt.value === currentValue;
      btn.setAttribute('aria-selected', String(isSel));
      btn.disabled = Boolean(opt.disabled);
      btn.innerHTML =
        '<span class="mac-select-check" aria-hidden="true"><svg class="icon-13" viewBox="0 0 24 24"><path d="M4.5 12.5l4.2 4.2L19.5 6"/></svg></span>' +
        `<span>${opt.label}</span>`;
      if (!opt.disabled) {
        btn.addEventListener('click', async (ev) => {
          ev.stopPropagation();
          closeStageSelectMenu();
          await onSelect(opt.value);
        });
      }
      dom.stageSelectMenu.appendChild(btn);
    }

    dom.stageSelectMenu.hidden = false;
    const alignEl = anchor.querySelector('.ins-select-right') || anchor;
    const ar = alignEl.getBoundingClientRect();
    const mw = Math.max(dom.stageSelectMenu.offsetWidth || 152, Math.min(ar.width, 180));
    const mh = dom.stageSelectMenu.offsetHeight || options.length * 28 + 8;
    const vw = window.innerWidth || 1240;
    const vh = window.innerHeight || 800;
    const left = Math.max(12, Math.min(vw - mw - 12, ar.right - mw));
    const top = ar.bottom + 4 + mh <= vh - 12 ? ar.bottom + 4 : Math.max(12, ar.top - 4 - mh);
    dom.stageSelectMenu.style.minWidth = `${Math.round(mw)}px`;
    dom.stageSelectMenu.style.left = `${Math.round(left)}px`;
    dom.stageSelectMenu.style.top = `${Math.round(top)}px`;
  }

  function closeAllStageTransientMenus() {
    closeStageSelectMenu({ restoreHoverClose: false });
    let changed = false;
    if (!params.get('zone') && state.openZoneId !== null) {
      state.openZoneId = null;
      changed = true;
    }
    if (!params.get('inspect') && state.inspectId !== null) {
      state.inspectId = null;
      changed = true;
    }
    if (state.topDockOpen) {
      closeTopDockNow();
    }
    if (changed) {
      syncUI();
    }
  }

  window.__closeStageTransientMenus = () => {
    if (!params.get('menu') && !params.get('zone') && !params.get('inspect')) {
      if (document.hidden) {
        closeAllStageTransientMenus();
      } else {
        closeStageSelectMenu();
      }
    }
  };

  window.addEventListener('blur', () => {
    if (!params.get('menu') && !params.get('zone') && !params.get('inspect')) {
      closeStageSelectMenu();
    }
  });

  window.addEventListener('pointerdown', (e) => {
    if (!dom.stageSelectMenu.hidden) {
      if (
        !dom.stageSelectMenu.contains(e.target) &&
        (!activeSelectMenu.anchor || !activeSelectMenu.anchor.contains(e.target))
      ) {
        closeStageSelectMenu();
      }
    }
    const targetEl =
      e.target instanceof Element
        ? e.target
        : e.target && e.target.parentElement
          ? e.target.parentElement
          : null;
    if (
      targetEl &&
      !targetEl.closest(
        '.edge-zone, .device-mount, .top-dock-zone, .mac-select-menu, .perm-overlay, .alert-inline-pill, .mesh-hit'
      )
    ) {
      let changed = false;
      if (state.openZoneId !== null) {
        state.openZoneId = null;
        changed = true;
      }
      if (state.inspectId !== null) {
        state.inspectId = null;
        changed = true;
      }
      if (changed) {
        syncUI();
      }
    }
  });

  window.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') {
      if (!dom.stageSelectMenu.hidden) {
        closeStageSelectMenu();
        return;
      }
      closeAllStageTransientMenus();
      return;
    }
    if (e.key === 'Control' && state.ctrlEdgeEnabled && !state.portalArmed) {
      state.portalArmed = true;
      syncUI();
      requestWake(360);
    }
  });

  window.addEventListener('keyup', (e) => {
    if (e.key === 'Control' && state.portalArmed && demoMode !== 'portal') {
      state.portalArmed = false;
      syncUI();
      requestWake(360);
    }
  });

  const closeTimers = new Map();
  function openZone(id) {
    for (const [zid, t] of closeTimers.entries()) {
      clearTimeout(t);
      closeTimers.delete(zid);
    }
    if (activeSelectMenu.holdZoneId && activeSelectMenu.holdZoneId !== id) {
      closeStageSelectMenu({ restoreHoverClose: false });
    }
    if (state.openZoneId !== id) {
      state.openZoneId = id;
      syncUI();
    }
  }

  function scheduleCloseZone(id) {
    if ((isDraggingSlider || isDraggingVolSlider) && id === 'zoneLensTop') return;
    if (activeSelectMenu.holdZoneId === id) return;
    clearTimeout(closeTimers.get(id));
    closeTimers.set(
      id,
      setTimeout(() => {
        if ((isDraggingSlider || isDraggingVolSlider) && id === 'zoneLensTop') return;
        if (activeSelectMenu.holdZoneId === id) return;
        if (state.openZoneId === id) {
          state.openZoneId = null;
          syncUI();
        }
      }, 170)
    );
  }

  dom.zones.forEach((zone) => {
    zone.addEventListener('pointerenter', () => {
      if (!dom.stageSelectMenu.hidden && activeSelectMenu.holdZoneId !== zone.id) return;
      openZone(zone.id);
    });
    zone.addEventListener('pointerleave', () => scheduleCloseZone(zone.id));
    const lip = zone.querySelector('.edge-lip');
    if (lip) {
      lip.addEventListener('click', (e) => {
        e.stopPropagation();
        openZone(zone.id);
      });
    }
  });

  let inspectTimer = null;
  function openInspector(id) {
    if (id === 'windows' && state.topology === 'single') return;
    if (id === 'studio' && state.topology !== 'triple') return;
    clearTimeout(inspectTimer);
    if (activeSelectMenu.holdInspectId && activeSelectMenu.holdInspectId !== id) {
      closeStageSelectMenu({ restoreHoverClose: false });
    }
    if (state.inspectId !== id) {
      state.inspectId = id;
      syncUI();
    }
  }

  function scheduleCloseInspector(id) {
    if (activeSelectMenu.holdInspectId === id) return;
    clearTimeout(inspectTimer);
    inspectTimer = setTimeout(() => {
      if (activeSelectMenu.holdInspectId === id) return;
      if (state.inspectId === id) {
        state.inspectId = null;
        syncUI();
      }
    }, 140);
  }

  dom.stageSelectMenu.addEventListener('pointerenter', () => {
    for (const [zid, t] of closeTimers.entries()) {
      clearTimeout(t);
      closeTimers.delete(zid);
    }
    clearTimeout(inspectTimer);
  });

  dom.stageSelectMenu.addEventListener('pointerleave', () => {
    if (dom.stageSelectMenu.hidden) {
      if (state.openZoneId) scheduleCloseZone(state.openZoneId);
      if (state.inspectId) scheduleCloseInspector(state.inspectId);
    }
  });

  const hitToDevice = [
    ['windows', dom.mounts.windows, dom.hitWinMesh],
    ['studio',  dom.mounts.studio,  dom.hitStudioMesh],
    ['monitor', dom.mounts.monitor, dom.hitExtScreen],
    ['macbook', dom.mounts.macbook, dom.hitMbpScreen]
  ];

  for (const [id, mountEl, hitEl] of hitToDevice) {
    mountEl.addEventListener('pointerenter', () => {
      if (!dom.stageSelectMenu.hidden && activeSelectMenu.holdInspectId !== id) return;
      openInspector(id);
    });
    mountEl.addEventListener('pointerleave', () => scheduleCloseInspector(id));
    mountEl.addEventListener('focusin', () => {
      if (!dom.stageSelectMenu.hidden && activeSelectMenu.holdInspectId !== id) return;
      openInspector(id);
    });
    mountEl.addEventListener('focusout', () => {
      if (!dom.stageSelectMenu.hidden || mountEl.matches(':hover')) return;
      scheduleCloseInspector(id);
    });
    if (hitEl) {
      hitEl.addEventListener('pointerenter', () => {
        if (!dom.stageSelectMenu.hidden && activeSelectMenu.holdInspectId !== id) return;
        openInspector(id);
      });
      hitEl.addEventListener('pointerleave', () => scheduleCloseInspector(id));
    }
  }

  dom.lensHostBtns.forEach((btn) => {
    btn.addEventListener('click', () => switchHost(btn.dataset.host));
  });

  function updateBrightnessFromClientX(clientX, commitNow = false) {
    const rect = dom.lensSlider.getBoundingClientRect();
    if (rect.width <= 0) return;
    const pct = THREE.MathUtils.clamp(((clientX - rect.left) / rect.width) * 100, 0, 100);
    state.brightness = pct;
    if (state.displaySleep && pct > 2) {
      state.displaySleep = false;
    }
    renderChiclets(pct);
    dom.lensSlider.setAttribute('aria-valuenow', String(Math.round(pct)));
    dom.lensBrightnessPct.textContent = `${Math.round(pct)}%`;
    if (!state.displaySleep) {
      dom.stateMonitor.textContent = `${state.hostHw[state.host].port} · ${Math.round(pct)}%`;
      const volDesc = state.audioMuted ? '已静音' : `音量 ${Math.round(state.volume)}%`;
      dom.insMonBri.textContent = `亮度 ${Math.round(pct)}% · ${volDesc}`;
    }
    philipsDisplay.userData.panelUniforms.uBrightness.value = pct / 100;
    requestWake(260);

    const rounded = Math.round(pct);
    if (commitNow) {
      clearTimeout(sliderSendTimer);
      sliderSendTimer = null;
      invokeCmd('set_brightness', { brightness: rounded }).then((dto) => {
        if (!isDraggingSlider) applyDashboardDto(dto);
      });
    } else if (!sliderSendTimer) {
      sliderSendTimer = setTimeout(() => {
        sliderSendTimer = null;
        invokeCmd('set_brightness', { brightness: Math.round(state.brightness) });
      }, 140);
    }
  }

  dom.lensSlider.addEventListener('pointerdown', (e) => {
    e.preventDefault();
    isDraggingSlider = true;
    dom.lensSlider.dataset.dragging = 'true';
    openZone('zoneLensTop');
    try {
      dom.lensSlider.setPointerCapture(e.pointerId);
    } catch (_) {}
    updateBrightnessFromClientX(e.clientX, false);
  });

  dom.lensSlider.addEventListener('pointermove', (e) => {
    if (!isDraggingSlider) return;
    updateBrightnessFromClientX(e.clientX, false);
  });

  function endSliderDrag(e) {
    if (!isDraggingSlider) return;
    isDraggingSlider = false;
    delete dom.lensSlider.dataset.dragging;
    try {
      dom.lensSlider.releasePointerCapture(e.pointerId);
    } catch (_) {}
    updateBrightnessFromClientX(e.clientX, true);
    syncUI();
    if (!dom.zoneLensTop.matches(':hover')) {
      scheduleCloseZone('zoneLensTop');
    }
  }

  dom.lensSlider.addEventListener('pointerup', endSliderDrag);
  dom.lensSlider.addEventListener('pointercancel', endSliderDrag);
  dom.lensSlider.addEventListener('keydown', async (e) => {
    if (e.key === 'ArrowLeft' || e.key === 'ArrowDown') {
      e.preventDefault();
      state.brightness = Math.max(0, Math.round(state.brightness - 6));
      syncUI();
      requestWake(320);
      const dto = await invokeCmd('set_brightness', { brightness: state.brightness });
      applyDashboardDto(dto);
    } else if (e.key === 'ArrowRight' || e.key === 'ArrowUp') {
      e.preventDefault();
      state.brightness = Math.min(100, Math.round(state.brightness + 6));
      syncUI();
      requestWake(320);
      const dto = await invokeCmd('set_brightness', { brightness: state.brightness });
      applyDashboardDto(dto);
    }
  });

  dom.lensSleepBtn.addEventListener('click', toggleSleepDisplays);

  function updateVolumeFromClientX(clientX, commitNow = false) {
    const rect = dom.lensVolSlider.getBoundingClientRect();
    if (rect.width <= 0) return;
    const pct = THREE.MathUtils.clamp(((clientX - rect.left) / rect.width) * 100, 0, 100);
    state.volume = pct;
    if (pct > 0 && state.audioMuted) {
      state.audioMuted = false;
    }
    renderChicletsInto(volChicletCells, state.audioMuted ? 0 : pct);
    dom.lensVolSlider.setAttribute('aria-valuenow', String(Math.round(pct)));
    dom.lensVolumePct.textContent = state.audioMuted ? '静音' : `${Math.round(pct)}%`;
    dom.lensMuteBtn.setAttribute('aria-pressed', String(state.audioMuted));
    dom.lensMuteIcon.innerHTML = speakerSvgMarkup(state.audioMuted);

    const rounded = Math.round(pct);
    if (commitNow) {
      clearTimeout(volSendTimer);
      volSendTimer = null;
      invokeCmd('set_volume', { volume: rounded }).then((dto) => {
        if (!isDraggingVolSlider) applyDashboardDto(dto);
      });
    } else if (!volSendTimer) {
      volSendTimer = setTimeout(() => {
        volSendTimer = null;
        invokeCmd('set_volume', { volume: Math.round(state.volume) });
      }, 140);
    }
  }

  dom.lensVolSlider.addEventListener('pointerdown', (e) => {
    e.preventDefault();
    isDraggingVolSlider = true;
    dom.lensVolSlider.dataset.dragging = 'true';
    openZone('zoneLensTop');
    try {
      dom.lensVolSlider.setPointerCapture(e.pointerId);
    } catch (_) {}
    updateVolumeFromClientX(e.clientX, false);
  });

  dom.lensVolSlider.addEventListener('pointermove', (e) => {
    if (!isDraggingVolSlider) return;
    updateVolumeFromClientX(e.clientX, false);
  });

  function endVolSliderDrag(e) {
    if (!isDraggingVolSlider) return;
    isDraggingVolSlider = false;
    delete dom.lensVolSlider.dataset.dragging;
    try {
      dom.lensVolSlider.releasePointerCapture(e.pointerId);
    } catch (_) {}
    updateVolumeFromClientX(e.clientX, true);
    syncUI();
    if (!dom.zoneLensTop.matches(':hover')) {
      scheduleCloseZone('zoneLensTop');
    }
  }

  dom.lensVolSlider.addEventListener('pointerup', endVolSliderDrag);
  dom.lensVolSlider.addEventListener('pointercancel', endVolSliderDrag);
  dom.lensVolSlider.addEventListener('keydown', async (e) => {
    if (e.key === 'ArrowLeft' || e.key === 'ArrowDown') {
      e.preventDefault();
      state.volume = Math.max(0, Math.round(state.volume - 6));
      state.audioMuted = false;
      syncUI();
      const dto = await invokeCmd('set_volume', { volume: state.volume });
      applyDashboardDto(dto);
    } else if (e.key === 'ArrowRight' || e.key === 'ArrowUp') {
      e.preventDefault();
      state.volume = Math.min(100, Math.round(state.volume + 6));
      state.audioMuted = false;
      syncUI();
      const dto = await invokeCmd('set_volume', { volume: state.volume });
      applyDashboardDto(dto);
    }
  });

  dom.lensMuteBtn.addEventListener('click', async () => {
    state.audioMuted = !state.audioMuted;
    syncUI();
    const dto = await invokeCmd('toggle_audio_mute', { muted: state.audioMuted });
    applyDashboardDto(dto);
  });

  function openColorPresetMenu(anchor, { holdZoneId = null, holdInspectId = null } = {}) {
    openStageSelectMenu({
      anchor,
      options: COLOR_PRESET_OPTIONS,
      currentValue: state.colorPreset || 5,
      holdZoneId,
      holdInspectId,
      onSelect: async (preset) => {
        const dto = await invokeCmd('set_color_preset', { preset });
        applyDashboardDto(dto);
        if (dto && dto.color_preset_label) {
          showNotice(`色温 · ${dto.color_preset_label}`);
        }
      }
    });
  }

  dom.lensColorBtn.addEventListener('click', (e) => {
    e.stopPropagation();
    openColorPresetMenu(dom.lensColorBtn, { holdZoneId: 'zoneLensTop' });
  });

  dom.btnCycleColorPreset.addEventListener('click', (e) => {
    e.stopPropagation();
    openColorPresetMenu(dom.btnCycleColorPreset, { holdInspectId: 'monitor' });
  });

  dom.lensModMapBtn.addEventListener('click', async () => {
    if (!state.deskflowEnabled) return;
    state.unifyCrossOsModifiers = !state.unifyCrossOsModifiers;
    syncUI();
    const dto = await invokeCmd('toggle_cross_os_modifiers');
    applyDashboardDto(dto);
    showNotice(state.unifyCrossOsModifiers ? '⌘/Ctrl 对齐已开' : '⌘/Ctrl 对齐已关');
  });

  function hitRectContains(el, x, y, pad = 16) {
    if (!el || el.hidden || el.classList.contains('is-hidden')) return false;
    const r = el.getBoundingClientRect();
    if (r.width <= 0 || r.height <= 0) return false;
    return x >= r.left - pad && x <= r.right + pad && y >= r.top - pad && y <= r.bottom + pad;
  }

  function resolveDropStageHost(clientX, clientY) {
    const candidates = [
      ['windows', dom.mounts.windows, dom.hitWinMesh],
      ['studio', dom.mounts.studio, dom.hitStudioMesh],
      ['macbook', dom.mounts.macbook, dom.hitMbpScreen]
    ];
    for (const [sid, mountEl, hitEl] of candidates) {
      if (sid === 'windows' && state.topology === 'single') continue;
      if (sid === 'studio' && state.topology !== 'triple') continue;
      if (hitRectContains(mountEl, clientX, clientY, 16) || hitRectContains(hitEl, clientX, clientY, 12)) {
        return sid;
      }
    }
    return null;
  }

  window.__onNativeFileDragOver = (lx, ly) => {
    const hovered = resolveDropStageHost(lx, ly);
    if (state.dropTargetHost !== hovered) {
      state.dropTargetHost = hovered;
      syncUI();
    }
  };

  window.__onNativeFileDragLeave = () => {
    if (state.dropTargetHost !== null) {
      state.dropTargetHost = null;
      syncUI();
    }
  };

  window.__onNativeFileDrop = async (lx, ly, paths) => {
    const targetStage = resolveDropStageHost(lx, ly) || state.dropTargetHost;
    state.dropTargetHost = null;
    syncUI();
    if (!targetStage || !Array.isArray(paths) || !paths.length) return;

    const targetBackendId = STAGE_TO_BACKEND_ID[targetStage];
    if (targetBackendId === state.localBackendId) {
      showNotice('文件已在本机');
      return;
    }

    state.transferBusyHost = targetStage;
    syncUI();
    try {
      const dto = await invokeCmd('send_files_to_host', {
        targetHostId: targetBackendId,
        paths
      });
      state.transferBusyHost = null;
      applyDashboardDto(dto);
    } catch (err) {
      state.transferBusyHost = null;
      state.lastError = String(err);
      syncUI();
    }
  };

  window.addEventListener('dragover', (e) => {
    e.preventDefault();
  });
  window.addEventListener('drop', (e) => {
    e.preventDefault();
  });

  dom.alignUpBtn.addEventListener('click', () => {
    commitAlignment(state.seamAlignPct + 4);
  });

  dom.alignDownBtn.addEventListener('click', () => {
    commitAlignment(state.seamAlignPct - 4);
  });

  dom.lensSlideBtn.addEventListener('click', async () => {
    if (!state.deskflowEnabled) return;
    state.plainSlide = !state.plainSlide;
    syncUI();
    requestWake(420);
    const dto = await invokeCmd('toggle_plain_slide');
    applyDashboardDto(dto);
  });

  dom.lensPortalBtn.addEventListener('click', async () => {
    if (!state.deskflowEnabled) return;
    state.ctrlEdgeEnabled = !state.ctrlEdgeEnabled;
    if (!state.ctrlEdgeEnabled) {
      state.portalArmed = false;
    }
    syncUI();
    requestWake(360);
    const dto = await invokeCmd('toggle_ctrl_edge_switch');
    applyDashboardDto(dto);
    showNotice(state.ctrlEdgeEnabled ? 'Ctrl 推边切屏已开启' : 'Ctrl 推边切屏已暂停');
  });

  dom.lensDeskflowBtn.addEventListener('click', async () => {
    if (!state.accessibilityTrusted) {
      state.permDismissed = false;
      syncUI();
      return;
    }
    const next = !state.deskflowEnabled;
    state.deskflowEnabled = next;
    if (!next) {
      state.deskflowRunning = false;
      state.deskflowOk = false;
      state.portalArmed = false;
    }
    syncUI();
    requestWake(360);
    const dto = await invokeCmd('set_deskflow_enabled', { enabled: next });
    applyDashboardDto(dto);
    showNotice(next ? '键鼠跨屏守护已开启' : '键鼠跨屏守护已暂停');
  });

  function openHostPortMenu(stageId, anchor) {
    const hw = state.hostHw[stageId];
    const backendId = STAGE_TO_BACKEND_ID[stageId];
    const curVal = hw.manualOverride ? hw.input : null;
    openStageSelectMenu({
      anchor,
      options: PORT_OVERRIDE_OPTIONS,
      currentValue: curVal,
      holdInspectId: stageId,
      onSelect: async (nextInput) => {
        const dto = await invokeCmd('set_host_port_override', {
          hostId: backendId,
          monitorInput: nextInput
        });
        applyDashboardDto(dto);
        showNotice(nextInput == null ? '已恢复自动认口' : `已指定 ${portLabelForInputCode(nextInput)}`);
      }
    });
  }

  dom.btnCycleWinPort.addEventListener('click', (e) => {
    e.stopPropagation();
    openHostPortMenu('windows', dom.btnCycleWinPort);
  });
  dom.btnCycleStuPort.addEventListener('click', (e) => {
    e.stopPropagation();
    openHostPortMenu('studio', dom.btnCycleStuPort);
  });
  dom.btnCycleMbpPort.addEventListener('click', (e) => {
    e.stopPropagation();
    openHostPortMenu('macbook', dom.btnCycleMbpPort);
  });

  function openHostDisplayModeMenu(stageId, anchor, holdInspectId = stageId) {
    const hw = state.hostHw[stageId];
    if (!hw) return;
    const backendId = STAGE_TO_BACKEND_ID[stageId];
    const isMacStage = stageId === 'macbook' || stageId === 'studio';
    const rawModes =
      Array.isArray(hw.modes) && hw.modes.length > 0
        ? hw.modes
        : [
            {
              width: 2560,
              height: 1440,
              hz: 75,
              hidpi: isMacStage,
              active: hw.mode.includes('2560×1440'),
              label: isMacStage ? '2560×1440 · 75Hz · HiDPI' : '2560×1440 · 75Hz'
            },
            {
              width: 3200,
              height: 1800,
              hz: 75,
              hidpi: isMacStage,
              active: hw.mode.includes('3200×1800'),
              label: isMacStage ? '3200×1800 · 75Hz · HiDPI' : '3200×1800 · 75Hz'
            },
            {
              width: 3840,
              height: 2160,
              hz: 75,
              hidpi: isMacStage,
              active: hw.mode.includes('3840×2160'),
              label: isMacStage ? '3840×2160 · 75Hz · HiDPI' : '3840×2160 · 75Hz'
            },
            {
              width: 5120,
              height: 2880,
              hz: 75,
              hidpi: false,
              active: hw.mode.includes('5120×2880'),
              label: '5120×2880 · 75Hz · 原生'
            }
          ];

    let currentVal = null;
    const modeMap = new Map();
    const options = rawModes.map((m) => {
      const key = `${m.width}x${m.height}@${m.hz}:${m.hidpi ? 1 : 0}`;
      const label = m.label || `${m.width}×${m.height} · ${m.hz}Hz${m.hidpi ? ' · HiDPI' : ''}`;
      modeMap.set(key, m);
      if (m.active || (currentVal == null && hw.mode.includes(`${m.width}×${m.height}`))) {
        currentVal = key;
      }
      return { value: key, label };
    });

    openStageSelectMenu({
      anchor,
      options,
      currentValue: currentVal,
      holdInspectId,
      onSelect: async (selectedKey) => {
        const chosen = modeMap.get(selectedKey);
        if (!chosen) return;
        const dto = await invokeCmd('set_display_mode', {
          hostId: backendId,
          modeId: typeof chosen.mode_id === 'number' ? chosen.mode_id : -1,
          width: chosen.width,
          height: chosen.height,
          hz: chosen.hz,
          hidpi: Boolean(chosen.hidpi)
        });
        applyDashboardDto(dto);
        showNotice(`已切 ${chosen.width}×${chosen.height}`);
      }
    });
  }

  dom.btnSelectWinMode.addEventListener('click', (e) => {
    e.stopPropagation();
    openHostDisplayModeMenu('windows', dom.btnSelectWinMode, 'windows');
  });
  dom.btnSelectStuMode.addEventListener('click', (e) => {
    e.stopPropagation();
    openHostDisplayModeMenu('studio', dom.btnSelectStuMode, 'studio');
  });
  dom.btnSelectMonMode.addEventListener('click', (e) => {
    e.stopPropagation();
    const targetStage =
      state.extMonitorOnline !== false && state.hostHw[state.host] && state.hostHw[state.host].online
        ? state.host
        : BACKEND_TO_STAGE_ID[state.localBackendId] || 'macbook';
    openHostDisplayModeMenu(targetStage, dom.btnSelectMonMode, 'monitor');
  });
  dom.btnSelectMbpMode.addEventListener('click', (e) => {
    e.stopPropagation();
    openHostDisplayModeMenu('macbook', dom.btnSelectMbpMode, 'macbook');
  });

  dom.permOpenBtn.addEventListener('click', async () => {
    const dto = await invokeCmd('open_accessibility_settings');
    applyDashboardDto(dto);
  });

  dom.permRestartBtn.addEventListener('click', async () => {
    const dto = await invokeCmd('relaunch_app');
    if (dto) applyDashboardDto(dto);
  });

  dom.permLaterBtn.addEventListener('click', () => {
    state.permDismissed = true;
    syncUI();
  });

  dom.lensKmBtn.addEventListener('click', (e) => {
    e.stopPropagation();
    const opts = [
      { value: 'win-desktop', stage: 'windows', label: 'Windows 直连', disabled: state.topology === 'single' },
      { value: 'macbook', stage: 'macbook', label: 'MacBook 直连', disabled: false },
      { value: 'mac-studio', stage: 'studio', label: 'Mac Studio 直连', disabled: state.topology !== 'triple' }
    ].filter((o) => !o.disabled);
    const curBackend = STAGE_TO_BACKEND_ID[state.hidTarget] || 'win-desktop';
    openStageSelectMenu({
      anchor: dom.lensKmBtn,
      options: opts,
      currentValue: curBackend,
      holdZoneId: 'zoneLensMbp',
      onSelect: async (hostId) => {
        state.hidTarget = BACKEND_TO_STAGE_ID[hostId] || 'windows';
        syncUI();
        requestWake(320);
        const dto = await invokeCmd('set_input_provider', { hostId });
        applyDashboardDto(dto);
        showNotice(`键鼠源 · ${dom.lensKmLabel.textContent}`);
      }
    });
  });

  dom.cardWindows.addEventListener('click', () => switchHost('windows'));
  dom.cardStudio.addEventListener('click', () => switchHost('studio'));
  dom.cardMacbook.addEventListener('click', () => switchHost('macbook'));
  dom.cardMonitor.addEventListener('click', () => {
    if (state.displaySleep) {
      toggleSleepDisplays();
      return;
    }
    openInspector('monitor');
  });

  dom.hitWinMesh.addEventListener('click', () => switchHost('windows'));
  dom.hitStudioMesh.addEventListener('click', () => switchHost('studio'));
  dom.hitExtScreen.addEventListener('click', () => {
    if (state.displaySleep) {
      toggleSleepDisplays();
      return;
    }
    openInspector('monitor');
  });

  dom.hitMbpScreen.addEventListener('pointerdown', (e) => {
    isDraggingSeam = true;
    mbpDownY = e.clientY;
    mbpMoved = false;
    try {
      dom.hitMbpScreen.setPointerCapture(e.pointerId);
    } catch (_) {}
  });

  dom.hitMbpScreen.addEventListener('pointermove', (e) => {
    if (!isDraggingSeam) return;
    const dy = e.clientY - mbpDownY;
    if (Math.abs(dy) > 4) mbpMoved = true;
    if (mbpMoved) {
      state.seamAlignPct = THREE.MathUtils.clamp(state.seamAlignPct - dy * 0.16, 60, 100);
      mbpDownY = e.clientY;
      syncUI();
      requestWake(300);
    }
  });

  dom.hitMbpScreen.addEventListener('pointerup', (e) => {
    if (!isDraggingSeam) return;
    isDraggingSeam = false;
    try {
      dom.hitMbpScreen.releasePointerCapture(e.pointerId);
    } catch (_) {}
    if (mbpMoved) {
      commitAlignment(state.seamAlignPct);
    } else {
      switchHost('macbook');
    }
  });

  canvas.addEventListener('pointerdown', (e) => {
    isDraggingStage = true;
    lastDragX = e.clientX;
    lastDragY = e.clientY;
    requestWake(400);
  });

  window.addEventListener('pointermove', (e) => {
    if (!isDraggingStage) return;
    const dx = e.clientX - lastDragX;
    const dy = e.clientY - lastDragY;
    lastDragX = e.clientX;
    lastDragY = e.clientY;
    dragYaw = THREE.MathUtils.clamp(dragYaw - dx * 0.0022, -0.32, 0.32);
    dragPitch = THREE.MathUtils.clamp(dragPitch + dy * 0.0018, -0.20, 0.22);
    requestWake(400);
  });

  window.addEventListener('pointerup', () => {
    if (!isDraggingStage) return;
    isDraggingStage = false;
    requestWake(650);
  });

  window.addEventListener('resize', () => {
    camera.aspect = window.innerWidth / window.innerHeight;
    camera.updateProjectionMatrix();
    renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 1.65));
    renderer.setSize(window.innerWidth, window.innerHeight);
    updateProjectedAnchors();
    syncTopDockAnchor();
    requestWake(350);
  });

  try {
    const initialDto = await invokeCmd('get_dashboard_state');
    applyDashboardDto(initialDto, { snap: true });
  } catch (_) {
    syncUI();
  }

  if (hasTauri) {
    invokeCmd('set_app_theme', { theme: state.theme }).catch(() => {});
  }
  snapSceneImmediately();
  if (params.get('menu') === 'color') {
    if (state.openZoneId === 'zoneLensTop') {
      openColorPresetMenu(dom.lensColorBtn, { holdZoneId: 'zoneLensTop' });
    } else if (state.inspectId === 'monitor') {
      openColorPresetMenu(dom.btnCycleColorPreset, { holdInspectId: 'monitor' });
    }
  } else if (params.get('menu') === 'port' && state.inspectId) {
    const mapBtn = {
      windows: dom.btnCycleWinPort,
      studio: dom.btnCycleStuPort,
      macbook: dom.btnCycleMbpPort
    }[state.inspectId];
    if (mapBtn) openHostPortMenu(state.inspectId, mapBtn);
  } else if (params.get('menu') === 'mode' && state.inspectId) {
    const modeBtn = {
      windows: dom.btnSelectWinMode,
      studio: dom.btnSelectStuMode,
      monitor: dom.btnSelectMonMode,
      macbook: dom.btnSelectMbpMode
    }[state.inspectId];
    const targetStage = state.inspectId === 'monitor' ? state.host : state.inspectId;
    if (modeBtn) openHostDisplayModeMenu(targetStage, modeBtn, state.inspectId);
  }
  window.__kvmReady = true;
  requestWake(280);

  if (hasTauri) {
    setInterval(async () => {
      if (document.hidden && state.accessibilityTrusted) return;
      try {
        const dto = await invokeCmd('get_dashboard_state');
        applyDashboardDto(dto);
      } catch (_) {}
    }, 1000);

    document.addEventListener('visibilitychange', () => {
      if (document.hidden) {
        if (!params.get('menu') && !params.get('zone') && !params.get('inspect')) {
          closeAllStageTransientMenus();
        }
      } else {
        invokeCmd('get_dashboard_state').then((dto) => applyDashboardDto(dto));
      }
    });
  }
}

bootstrap();
