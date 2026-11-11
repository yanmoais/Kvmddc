import * as THREE from 'three';
import { OFFICIAL_DATA_URLS, PLATES_META } from './official-assets.js';

function ctx2d(w, h) {
  const c = document.createElement('canvas');
  c.width = Math.max(2, Math.round(w));
  c.height = Math.max(2, Math.round(h));
  return c.getContext('2d');
}

export function texFrom(canvas, { srgb = true, aniso = 8, mipmaps = true } = {}) {
  const t = new THREE.CanvasTexture(canvas);
  if (srgb) t.colorSpace = THREE.SRGBColorSpace;
  t.generateMipmaps = mipmaps;
  t.minFilter = mipmaps ? THREE.LinearMipmapLinearFilter : THREE.LinearFilter;
  t.magFilter = THREE.LinearFilter;
  t.anisotropy = aniso;
  t.needsUpdate = true;
  return t;
}

function texFromImage(img, { srgb = true, aniso = 8, mipmaps = true } = {}) {
  const t = new THREE.Texture(img);
  if (srgb) t.colorSpace = THREE.SRGBColorSpace;
  t.generateMipmaps = mipmaps;
  t.minFilter = mipmaps ? THREE.LinearMipmapLinearFilter : THREE.LinearFilter;
  t.magFilter = THREE.LinearFilter;
  t.anisotropy = aniso;
  t.needsUpdate = true;
  return t;
}

function decodeDataUrl(dataUrl) {
  return new Promise((resolve, reject) => {
    const img = new Image();
    img.decoding = 'async';
    img.onload = () => resolve(img);
    img.onerror = reject;
    img.src = dataUrl;
  });
}

/**
 * Decodes all official Apple / NZXT / Philips studio plates & wallpapers from embedded WebP data URLs.
 * Works identically on file:// and http:// with zero CORS errors.
 */
export async function loadOfficialStudioAssets(maxAniso = 8) {
  const entries = await Promise.all(
    Object.entries(OFFICIAL_DATA_URLS).map(async ([k, url]) => [k, await decodeDataUrl(url)])
  );
  const imgs = Object.fromEntries(entries);
  const aniso = Math.min(8, maxAniso || 8);

  return {
    meta: PLATES_META,
    plates: {
      winIdle: texFromImage(imgs.winIdle, { aniso }),
      winActive: texFromImage(imgs.winActive, { aniso }),
      studio: texFromImage(imgs.studio, { aniso }),
      macbook: texFromImage(imgs.macbook, { aniso }),
      monitor: texFromImage(imgs.monitor, { aniso })
    },
    wallpapers: {
      wallMbpPro: imgs.wallMbpPro,
      wallStudio: imgs.wallStudio,
      wallWinOfficial: imgs.wallWinOfficial
    }
  };
}

export function rr(x, cx, cy, w, h, r) {
  const rad = Math.min(r, w / 2, h / 2);
  x.beginPath();
  if (x.roundRect) {
    x.roundRect(cx, cy, w, h, rad);
    return;
  }
  x.moveTo(cx + rad, cy);
  x.arcTo(cx + w, cy, cx + w, cy + h, rad);
  x.arcTo(cx + w, cy + h, cx, cy + h, rad);
  x.arcTo(cx, cy + h, cx, cy, rad);
  x.arcTo(cx, cy, cx + w, cy, rad);
  x.closePath();
}

function squircle(x, cx, cy, s, k = 0.285) {
  rr(x, cx, cy, s, s, s * k);
}

const FONT = (px, w = 500) =>
  `${w} ${Math.round(px)}px -apple-system, BlinkMacSystemFont, "SF Pro Display", "SF Pro Text", "PingFang SC", "Helvetica Neue", sans-serif`;

const APPLE_D =
  'M788.1 340.9c-5.8 4.5-108.2 62.2-108.2 190.5 0 148.4 130.3 200.9 134.2 202.2-.6 3.2-20.7 71.9-68.7 141.9-42.8 61.6-87.5 123.1-155.5 123.1s-85.5-39.5-164-39.5c-76.5 0-103.7 40.8-165.9 40.8s-105.6-57-155.5-127C46.7 790.7 0 663 0 541.8c0-194.4 126.4-297.5 250.8-297.5 66.1 0 121.2 43.4 162.7 43.4 39.5 0 101.1-46 176.3-46 28.5 0 130.9 2.6 198.3 99.2zm-234-181.5c31.1-36.9 53.1-88.1 53.1-139.3 0-7.1-.6-14.3-1.9-20.1-50.6 1.9-110.8 33.7-147.1 75.8-28.5 32.4-55.1 83.6-55.1 135.5 0 7.8 1.3 15.6 1.9 18.1 3.2.6 8.4 1.3 13.6 1.3 45.4 0 102.5-30.4 135.5-71.3z';

function drawAppleGlyph(x, cx, cy, h, fill) {
  const s = h / 1000;
  x.save();
  x.translate(cx - (814 * s) / 2, cy - h / 2);
  x.scale(s, s);
  x.fillStyle = fill;
  x.fill(new Path2D(APPLE_D));
  x.restore();
}

function drawDockIcon(x, ix, iy, S, id) {
  const cx = ix + S / 2;
  const cy = iy + S / 2;
  const grad = (a, b) => {
    const g = x.createLinearGradient(ix, iy, ix, iy + S);
    g.addColorStop(0, a);
    g.addColorStop(1, b);
    return g;
  };
  const plate = (a, b) => {
    squircle(x, ix, iy, S);
    x.fillStyle = grad(a, b);
    x.fill();
  };
  x.save();
  x.lineCap = 'round';
  x.lineJoin = 'round';

  switch (id) {
    case 'sep':
      x.fillStyle = 'rgba(255,255,255,0.28)';
      x.fillRect(cx - 0.6, iy + 6, 1.2, S - 12);
      break;
    case 'finder':
      plate('#5cc0ff', '#1c6ef2');
      x.save();
      squircle(x, ix, iy, S);
      x.clip();
      x.fillStyle = 'rgba(255,255,255,0.93)';
      x.fillRect(cx, iy, S / 2, S);
      x.restore();
      x.strokeStyle = '#192a46';
      x.lineWidth = 2.3;
      x.beginPath();
      x.moveTo(cx - 10, cy - 9);
      x.lineTo(cx - 10, cy - 3);
      x.moveTo(cx + 10, cy - 9);
      x.lineTo(cx + 10, cy - 3);
      x.stroke();
      x.beginPath();
      x.arc(cx, cy + 2, 11, 0.22 * Math.PI, 0.78 * Math.PI);
      x.stroke();
      break;
    case 'safari':
      plate('#f2f8ff', '#c5e2ff');
      x.beginPath();
      x.arc(cx, cy, S * 0.37, 0, Math.PI * 2);
      x.fillStyle = '#1e86f5';
      x.fill();
      x.beginPath();
      x.moveTo(cx + 11, cy - 11);
      x.lineTo(cx - 2.2, cy - 2.2);
      x.lineTo(cx + 2.2, cy + 2.2);
      x.closePath();
      x.fillStyle = '#ff4b4b';
      x.fill();
      x.beginPath();
      x.moveTo(cx - 11, cy + 11);
      x.lineTo(cx + 2.2, cy + 2.2);
      x.lineTo(cx - 2.2, cy - 2.2);
      x.closePath();
      x.fillStyle = '#ffffff';
      x.fill();
      break;
    case 'messages':
      plate('#6ef58c', '#14ba3c');
      x.fillStyle = '#ffffff';
      x.beginPath();
      x.ellipse(cx, cy - 2, 16.5, 12.5, 0, 0, Math.PI * 2);
      x.fill();
      break;
    case 'mail':
      plate('#74c6ff', '#187cf0');
      x.fillStyle = '#ffffff';
      rr(x, cx - 18, cy - 12, 36, 24, 4);
      x.fill();
      x.strokeStyle = '#3c93e6';
      x.lineWidth = 2.1;
      x.beginPath();
      x.moveTo(cx - 16, cy - 10);
      x.lineTo(cx, cy + 3);
      x.lineTo(cx + 16, cy - 10);
      x.stroke();
      break;
    case 'photos': {
      plate('#ffffff', '#edf0f5');
      const cols = ['#f8c03a', '#f0663f', '#e34a8c', '#9a4ad6', '#3f7ee0', '#31b7d8', '#4fc16a', '#a9d13c'];
      for (let i = 0; i < 8; i++) {
        const a = (i / 8) * Math.PI * 2 - Math.PI / 2;
        x.save();
        x.globalAlpha = 0.9;
        x.translate(cx, cy);
        x.rotate(a);
        x.fillStyle = cols[i];
        x.beginPath();
        x.ellipse(0, -10, 5.2, 10.6, 0, 0, Math.PI * 2);
        x.fill();
        x.restore();
      }
      break;
    }
    case 'calendar':
      plate('#ffffff', '#eaedf2');
      x.save();
      squircle(x, ix, iy, S);
      x.clip();
      x.fillStyle = '#f23844';
      x.fillRect(ix, iy, S, S * 0.30);
      x.restore();
      x.fillStyle = '#ffffff';
      x.font = FONT(S * 0.18, 600);
      x.textAlign = 'center';
      x.textBaseline = 'middle';
      x.fillText('NOV', cx, iy + S * 0.16);
      x.fillStyle = '#1c1c1e';
      x.font = FONT(S * 0.44, 400);
      x.fillText('9', cx, iy + S * 0.66);
      break;
    case 'notes':
      plate('#fff2ae', '#f5c731');
      x.save();
      squircle(x, ix, iy, S);
      x.clip();
      x.fillStyle = '#e79e1c';
      x.fillRect(ix, iy, S, S * 0.24);
      x.restore();
      x.strokeStyle = 'rgba(110,85,15,0.38)';
      x.lineWidth = 2;
      for (let i = 0; i < 3; i++) {
        x.beginPath();
        x.moveTo(ix + 11, cy - 2 + i * 8);
        x.lineTo(ix + S - 11, cy - 2 + i * 8);
        x.stroke();
      }
      break;
    case 'music':
      plate('#ff7b8b', '#ef2246');
      x.fillStyle = '#ffffff';
      x.beginPath();
      x.ellipse(cx - 8, cy + 9, 6.5, 5, -0.3, 0, Math.PI * 2);
      x.ellipse(cx + 9, cy + 5, 6.5, 5, -0.3, 0, Math.PI * 2);
      x.fill();
      x.fillRect(cx - 3, cy - 13, 3, 22);
      x.fillRect(cx + 14, cy - 17, 3, 22);
      x.beginPath();
      x.moveTo(cx - 3, cy - 13);
      x.lineTo(cx + 17, cy - 17);
      x.lineTo(cx + 17, cy - 10);
      x.lineTo(cx - 3, cy - 6);
      x.closePath();
      x.fill();
      break;
    case 'terminal':
      plate('#3f4149', '#141519');
      x.fillStyle = '#f2f3f5';
      x.font = FONT(S * 0.32, 600);
      x.textAlign = 'left';
      x.textBaseline = 'middle';
      x.fillText('>_', ix + S * 0.2, cy);
      break;
    case 'settings':
      plate('#a6a9b2', '#595c66');
      x.strokeStyle = 'rgba(255,255,255,0.92)';
      x.lineWidth = 5.4;
      x.beginPath();
      x.arc(cx, cy, 11.5, 0, Math.PI * 2);
      x.stroke();
      for (let i = 0; i < 8; i++) {
        const a = (i / 8) * Math.PI * 2;
        x.beginPath();
        x.moveTo(cx + Math.cos(a) * 14, cy + Math.sin(a) * 14);
        x.lineTo(cx + Math.cos(a) * 20, cy + Math.sin(a) * 20);
        x.stroke();
      }
      break;
    case 'kvmddc':
      plate('#2c303a', '#111319');
      x.strokeStyle = '#7cc4ff';
      x.lineWidth = 2.5;
      rr(x, cx - 17, cy - 11, 20, 14, 3);
      x.stroke();
      x.strokeStyle = '#f5d7b5';
      rr(x, cx + 1, cy - 5, 15, 11, 2.5);
      x.stroke();
      break;
    case 'trash':
      plate('rgba(255,255,255,0.22)', 'rgba(210,215,225,0.14)');
      x.strokeStyle = 'rgba(255,255,255,0.85)';
      x.lineWidth = 2.3;
      x.beginPath();
      x.moveTo(cx - 12, cy - 11);
      x.lineTo(cx + 12, cy - 11);
      x.moveTo(cx - 9.5, cy - 8);
      x.lineTo(cx - 7.5, cy + 15);
      x.lineTo(cx + 7.5, cy + 15);
      x.lineTo(cx + 9.5, cy - 8);
      x.stroke();
      break;
    default:
      plate('#6aa9ff', '#2864ec');
      break;
  }
  x.restore();
}

function drawMacMenuBarAndDock(x, W, H, K, { barAlpha = 0.42 } = {}) {
  const MB = Math.round(H * 0.035);
  x.fillStyle = `rgba(12, 14, 20, ${barAlpha})`;
  x.fillRect(0, 0, W, MB);
  x.fillStyle = 'rgba(255, 255, 255, 0.14)';
  x.fillRect(0, MB - 1, W, 1);

  drawAppleGlyph(x, 22 * K, MB * 0.5, MB * 0.48, 'rgba(255,255,255,0.96)');

  x.textBaseline = 'middle';
  x.fillStyle = 'rgba(255,255,255,0.96)';
  let mx = 44 * K;
  const menus = [
    ['Finder', 600],
    ['File', 400],
    ['Edit', 400],
    ['View', 400],
    ['Go', 400],
    ['Window', 400],
    ['Help', 400]
  ];
  for (const [label, wt] of menus) {
    x.font = FONT(MB * 0.41, wt);
    const tw = x.measureText(label).width;
    x.fillText(label, mx, MB * 0.53);
    mx += tw + 18 * K;
  }

  x.font = FONT(MB * 0.40, 500);
  x.textAlign = 'right';
  const clockStr = 'Mon Nov 9  9:41 AM';
  x.fillText(clockStr, W - 18 * K, MB * 0.53);
  let rx = W - 30 * K - x.measureText(clockStr).width;
  x.textAlign = 'left';

  rx -= 24 * K;
  x.strokeStyle = 'rgba(255,255,255,0.94)';
  x.lineWidth = 1.6 * K;
  for (let i = 0; i < 2; i++) {
    const yy = MB * 0.5 - 3.6 * K + i * 7.2 * K;
    rr(x, rx, yy - 2.6 * K, 15 * K, 5.2 * K, 2.6 * K);
    x.stroke();
    x.fillStyle = 'rgba(255,255,255,0.96)';
    x.beginPath();
    x.arc(rx + (i ? 4.5 : 10.5) * K, yy, 2.2 * K, 0, Math.PI * 2);
    x.fill();
  }

  rx -= 28 * K;
  rr(x, rx, MB * 0.5 - 5 * K, 12 * K, 8.5 * K, 2 * K);
  x.stroke();
  rr(x, rx + 9 * K, MB * 0.5 - 2 * K, 9 * K, 6.5 * K, 1.6 * K);
  x.stroke();

  const ICONS = [
    'finder',
    'safari',
    'messages',
    'mail',
    'photos',
    'calendar',
    'notes',
    'music',
    'terminal',
    'kvmddc',
    'settings',
    'sep',
    'trash'
  ];
  const S = 48 * K;
  const PAD = 8.5 * K;
  const GP = 7.5 * K;
  const dw = ICONS.length * S + (ICONS.length - 1) * GP + PAD * 2;
  const dh = S + PAD * 2;
  const dx = (W - dw) / 2;
  const dy = H - dh - 10 * K;

  x.save();
  rr(x, dx, dy, dw, dh, 20 * K);
  x.fillStyle = 'rgba(24, 28, 38, 0.42)';
  x.fill();
  x.strokeStyle = 'rgba(255, 255, 255, 0.28)';
  x.lineWidth = 1.2 * K;
  x.stroke();
  x.restore();

  ICONS.forEach((id, i) => {
    x.save();
    x.translate(dx + PAD + i * (S + GP), dy + PAD);
    x.scale(S / 58, S / 58);
    drawDockIcon(x, 0, 0, 58, id);
    x.restore();
  });
}

function drawCoverImage(x, img, W, H) {
  const srcA = img.naturalWidth / img.naturalHeight;
  const dstA = W / H;
  let sx = 0;
  let sy = 0;
  let sw = img.naturalWidth;
  let sh = img.naturalHeight;
  if (srcA > dstA) {
    sw = Math.round(img.naturalHeight * dstA);
    sx = Math.round((img.naturalWidth - sw) / 2);
  } else {
    sh = Math.round(img.naturalWidth / dstA);
    sy = Math.round((img.naturalHeight - sh) / 2);
  }
  x.drawImage(img, sx, sy, sw, sh, 0, 0, W, H);
}

/**
 * 1. MacBook Pro 5K External Screen — Official Apple 6K macOS Sonoma Silk-Wave Wallpaper + macOS Menu Bar & Dock
 */
export function makeMacBook5KScreenTexture(wallMbpProImg) {
  const W = 1440;
  const H = 810;
  const K = W / 1512;
  const x = ctx2d(W, H);

  drawCoverImage(x, wallMbpProImg, W, H);
  drawMacMenuBarAndDock(x, W, H, K, { barAlpha: 0.52 });

  return texFrom(x.canvas);
}

/**
 * 2. Mac Studio 5K External Screen — Apple Official 2026 Studio Display XDR Wallpaper + macOS Menu Bar & Dock
 */
export function makeMacStudioScreenTexture(wallStudioImg) {
  const W = 1440;
  const H = 810;
  const K = W / 1512;
  const x = ctx2d(W, H);

  drawCoverImage(x, wallStudioImg, W, H);
  drawMacMenuBarAndDock(x, W, H, K, { barAlpha: 0.52 });

  return texFrom(x.canvas);
}

/**
 * 3. Windows 300Hz External Screen — Official Microsoft 4K Windows Glass-Beam Wallpaper + Centered Taskbar
 */
export function makeWindows300HzScreenTexture(wallWinImg) {
  const W = 1440;
  const H = 810;
  const K = W / 1512;
  const x = ctx2d(W, H);

  drawCoverImage(x, wallWinImg, W, H);

  // Subtle dark-studio sapphire grading so left cyan beam stays rich and high-contrast
  x.save();
  x.globalCompositeOperation = 'multiply';
  const vig = x.createLinearGradient(0, 0, W, H);
  vig.addColorStop(0.0, '#7cb8f8');
  vig.addColorStop(0.55, '#b8dcff');
  vig.addColorStop(1.0, '#dceeff');
  x.fillStyle = vig;
  x.fillRect(0, 0, W, H);
  x.restore();

  // Centered Windows 11 Acrylic Taskbar
  const tbH = Math.round(46 * K);
  x.fillStyle = 'rgba(10, 15, 28, 0.76)';
  x.fillRect(0, H - tbH, W, tbH);
  x.fillStyle = 'rgba(255, 255, 255, 0.12)';
  x.fillRect(0, H - tbH, W, 1);

  const count = 7;
  const step = 38 * K;
  const startX = W / 2 - ((count - 1) * step) / 2;
  const cy = H - tbH / 2;

  for (let i = 0; i < count; i++) {
    const ix = startX + i * step;
    if (i === 0) {
      // Official Windows 11 4-square cyan Start emblem
      const s = 9.5 * K;
      const gap = 2.0 * K;
      x.fillStyle = '#38bdf8';
      x.fillRect(ix - s - gap / 2, cy - s - gap / 2, s, s);
      x.fillRect(ix + gap / 2, cy - s - gap / 2, s, s);
      x.fillStyle = '#0ea5e9';
      x.fillRect(ix - s - gap / 2, cy + gap / 2, s, s);
      x.fillRect(ix + gap / 2, cy + gap / 2, s, s);
    } else if (i === 1) {
      // Search pill
      rr(x, ix - 12 * K, cy - 9 * K, 24 * K, 18 * K, 9 * K);
      x.fillStyle = 'rgba(255,255,255,0.14)';
      x.fill();
      x.strokeStyle = 'rgba(255,255,255,0.75)';
      x.lineWidth = 1.6 * K;
      x.beginPath();
      x.arc(ix - 1 * K, cy - 1 * K, 4.2 * K, 0, Math.PI * 2);
      x.stroke();
    } else {
      const cols = [
        ['#facc15', '#eab308'],
        ['#38bdf8', '#2563eb'],
        ['#34d399', '#059669'],
        ['#a78bfa', '#6d28d9'],
        ['#94a3b8', '#475569']
      ][i - 2];
      const g = x.createLinearGradient(ix - 11 * K, cy - 11 * K, ix + 11 * K, cy + 11 * K);
      g.addColorStop(0, cols[0]);
      g.addColorStop(1, cols[1]);
      rr(x, ix - 11 * K, cy - 11 * K, 22 * K, 22 * K, 5.5 * K);
      x.fillStyle = g;
      x.fill();
    }
  }

  x.fillStyle = 'rgba(241, 245, 249, 0.88)';
  x.font = FONT(11.5 * K, 500);
  x.textAlign = 'right';
  x.textBaseline = 'middle';
  x.fillText('9:41 AM   2026/11/9', W - 20 * K, cy);

  return texFrom(x.canvas);
}
