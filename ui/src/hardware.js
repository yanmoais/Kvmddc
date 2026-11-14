import * as THREE from 'three';
import { texFrom, rr } from './textures.js';

/* ----------------------------------------------------------- Baked Soft Decals */

export function makeContactShadowTexture() {
  const S = 256;
  const c = document.createElement('canvas');
  c.width = S;
  c.height = S;
  const x = c.getContext('2d');
  const g = x.createRadialGradient(S / 2, S / 2, S * 0.04, S / 2, S / 2, S * 0.49);
  g.addColorStop(0.0, 'rgba(4, 7, 14, 0.72)');
  g.addColorStop(0.36, 'rgba(4, 7, 14, 0.34)');
  g.addColorStop(0.72, 'rgba(4, 7, 14, 0.08)');
  g.addColorStop(1.0, 'rgba(4, 7, 14, 0)');
  x.fillStyle = g;
  x.fillRect(0, 0, S, S);
  return texFrom(c, { srgb: false, aniso: 2 });
}

export function makeBounceGlowTexture() {
  const S = 256;
  const c = document.createElement('canvas');
  c.width = S;
  c.height = S;
  const x = c.getContext('2d');
  const g = x.createRadialGradient(S / 2, S / 2, S * 0.02, S / 2, S / 2, S * 0.48);
  g.addColorStop(0.0, 'rgba(255, 255, 255, 0.92)');
  g.addColorStop(0.34, 'rgba(255, 255, 255, 0.42)');
  g.addColorStop(0.70, 'rgba(255, 255, 255, 0.09)');
  g.addColorStop(1.0, 'rgba(255, 255, 255, 0)');
  x.fillStyle = g;
  x.fillRect(0, 0, S, S);
  return texFrom(c, { srgb: true, aniso: 2 });
}

/* ----------------------------------------------------------- Studio Plate Shader */

function makeStudioPlateMaterial({ texA, texB = null, lightContrast = 0.0, boostDiff = 1.0 }) {
  const uniforms = {
    uTexA: { value: texA },
    uTexB: { value: texB || texA },
    uMix: { value: 0.0 },
    uBoostDiff: { value: boostDiff },
    uThemeLight: { value: 0.0 },
    uLightContrast: { value: lightContrast },
    uOpacity: { value: 1.0 }
  };

  return new THREE.ShaderMaterial({
    uniforms,
    transparent: true,
    depthWrite: false,
    toneMapped: false,
    vertexShader: /* glsl */ `
      varying vec2 vUv;
      void main() {
        vUv = uv;
        gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
      }
    `,
    fragmentShader: /* glsl */ `
      uniform sampler2D uTexA;
      uniform sampler2D uTexB;
      uniform float uMix;
      uniform float uBoostDiff;
      uniform float uThemeLight;
      uniform float uLightContrast;
      uniform float uOpacity;
      varying vec2 vUv;

      void main() {
        vec4 a = texture2D(uTexA, vUv);
        vec4 b = texture2D(uTexB, vUv);
        float m = clamp(uMix, 0.0, 1.0);
        vec3 mixedRgb = clamp(a.rgb + (b.rgb - a.rgb) * m * uBoostDiff, 0.0, 1.0);
        float mixedA = mix(a.a, b.a, m);

        float k = uThemeLight * uLightContrast;
        vec3 rgb = pow(max(mixedRgb, vec3(0.0)), vec3(1.0 + k)) * (1.0 - 0.12 * k);

        gl_FragColor = vec4(rgb, mixedA * uOpacity);
        #include <colorspace_fragment>
      }
    `
  });
}

/* ============================================================================
   0. 建筑感 macOS 虚拟工作室桌面 + 中央磁吸桌垫 + 后沿隐藏理线暗槽 + 铝合金增高架
   ========================================================================== */

function createRoundedRectShape(w, d, r) {
  const s = new THREE.Shape();
  const hw = w * 0.5;
  const hd = d * 0.5;
  const rad = Math.min(r, hw, hd);
  s.moveTo(-hw + rad, -hd);
  s.lineTo(hw - rad, -hd);
  s.quadraticCurveTo(hw, -hd, hw, -hd + rad);
  s.lineTo(hw, hd - rad);
  s.quadraticCurveTo(hw, hd, hw - rad, hd);
  s.lineTo(-hw + rad, hd);
  s.quadraticCurveTo(-hw, hd, -hw, hd - rad);
  s.lineTo(-hw, -hd + rad);
  s.quadraticCurveTo(-hw, -hd, -hw + rad, -hd);
  return s;
}

function makeMacOSDeskTopTexture(isLight) {
  const W = 1536;
  const H = 512;
  const c = document.createElement('canvas');
  c.width = W;
  c.height = H;
  const x = c.getContext('2d');

  // 1. Base macOS Studio tabletop (Dark: Smoked American Walnut / Light: Cupertino Natural Birch-Oak)
  const bg = x.createLinearGradient(0, 0, 0, H);
  if (isLight) {
    bg.addColorStop(0.0, '#d2c0a9');
    bg.addColorStop(0.32, '#deceba');
    bg.addColorStop(0.76, '#e7d9c7');
    bg.addColorStop(1.0, '#d8c7b1');
  } else {
    bg.addColorStop(0.0, '#231e1c');
    bg.addColorStop(0.32, '#2d2623');
    bg.addColorStop(0.76, '#352d29');
    bg.addColorStop(1.0, '#2a2320');
  }
  rr(x, 4, 4, W - 8, H - 8, 44);
  x.fillStyle = bg;
  x.fill();

  x.save();
  rr(x, 4, 4, W - 8, H - 8, 44);
  x.clip();

  // Warm studio overhead keylight sheen
  const sheen = x.createRadialGradient(W * 0.50, H * 0.44, H * 0.06, W * 0.50, H * 0.44, W * 0.54);
  if (isLight) {
    sheen.addColorStop(0.0, 'rgba(255, 255, 255, 0.52)');
    sheen.addColorStop(0.65, 'rgba(255, 255, 255, 0.14)');
    sheen.addColorStop(1.0, 'rgba(186, 174, 158, 0.10)');
  } else {
    sheen.addColorStop(0.0, 'rgba(255, 228, 206, 0.08)');
    sheen.addColorStop(0.62, 'rgba(180, 156, 140, 0.03)');
    sheen.addColorStop(1.0, 'rgba(8, 9, 14, 0.34)');
  }
  x.fillStyle = sheen;
  x.fillRect(0, 0, W, H);

  // Subtle organic architectural wood-grain lines
  for (let y = 10; y < H - 10; y += 3) {
    const wave = Math.sin(y * 0.19) * 0.5 + 0.5;
    const alpha = isLight ? 0.012 + wave * 0.014 : 0.016 + wave * 0.018;
    x.strokeStyle = isLight ? `rgba(112, 88, 62, ${alpha})` : `rgba(14, 10, 8, ${alpha * 1.6})`;
    x.lineWidth = 1.2;
    x.beginPath();
    x.moveTo(0, y);
    x.bezierCurveTo(W * 0.33, y + Math.sin(y * 0.07) * 1.8, W * 0.66, y - Math.cos(y * 0.06) * 1.8, W, y);
    x.stroke();
  }

  // 2. Recessed Rear Anodized Cable Management Channel (cy: 48..88 -> center cy = 68)
  const chX = W * 0.065;
  const chW = W * 0.87;
  const chY = 48;
  const chH = 40;
  rr(x, chX, chY, chW, chH, 13);
  const chGrad = x.createLinearGradient(0, chY, 0, chY + chH);
  if (isLight) {
    chGrad.addColorStop(0.0, '#c2c8d4');
    chGrad.addColorStop(0.48, '#cfd5e1');
    chGrad.addColorStop(1.0, '#dce1ec');
  } else {
    chGrad.addColorStop(0.0, '#0b0d13');
    chGrad.addColorStop(0.55, '#11151f');
    chGrad.addColorStop(1.0, '#181d29');
  }
  x.fillStyle = chGrad;
  x.fill();

  x.strokeStyle = isLight ? 'rgba(255, 255, 255, 0.76)' : 'rgba(255, 255, 255, 0.12)';
  x.lineWidth = 1.4;
  rr(x, chX, chY, chW, chH, 13);
  x.stroke();

  // Precision machined cable brush slit inside the rear channel (center cy = 68)
  rr(x, chX + 24, chY + 13, chW - 48, 14, 7);
  x.fillStyle = isLight ? 'rgba(30, 41, 59, 0.24)' : 'rgba(4, 6, 11, 0.85)';
  x.fill();

  // 3. Central Matte Studio Leather/Felt Desk Mat (under Monitor Riser + Mac Studio + MacBook Pro)
  const padX = W * 0.205;
  const padW = W * 0.735;
  const padY = 108;
  const padH = H - 156;
  rr(x, padX, padY, padW, padH, 22);
  const padGrad = x.createLinearGradient(0, padY, 0, padY + padH);
  if (isLight) {
    padGrad.addColorStop(0.0, 'rgba(176, 185, 201, 0.90)');
    padGrad.addColorStop(1.0, 'rgba(186, 195, 210, 0.94)');
  } else {
    padGrad.addColorStop(0.0, 'rgba(20, 23, 31, 0.90)');
    padGrad.addColorStop(1.0, 'rgba(16, 19, 27, 0.94)');
  }
  x.fillStyle = padGrad;
  x.fill();

  x.strokeStyle = isLight ? 'rgba(15, 23, 42, 0.08)' : 'rgba(255, 255, 255, 0.07)';
  x.lineWidth = 1.2;
  rr(x, padX, padY, padW, padH, 22);
  x.stroke();

  // 4. Subtle Rounded Bullnose Perimeter Highlight (soft & organic, no harsh laser stripe)
  rr(x, 5, 5, W - 10, H - 10, 43);
  x.strokeStyle = isLight ? 'rgba(255, 255, 255, 0.72)' : 'rgba(255, 238, 224, 0.12)';
  x.lineWidth = 1.8;
  x.stroke();

  x.restore();
  return texFrom(c, { srgb: true, aniso: 8 });
}

function makeRiserTopTexture(isLight) {
  const W = 512;
  const H = 192;
  const c = document.createElement('canvas');
  c.width = W;
  c.height = H;
  const x = c.getContext('2d');

  const g = x.createLinearGradient(0, 0, W, H);
  if (isLight) {
    g.addColorStop(0.0, '#f7f9fd');
    g.addColorStop(0.5, '#edf1f8');
    g.addColorStop(1.0, '#e1e7f2');
  } else {
    g.addColorStop(0.0, '#323a4b');
    g.addColorStop(0.5, '#293040');
    g.addColorStop(1.0, '#222836');
  }
  rr(x, 3, 3, W - 6, H - 6, 24);
  x.fillStyle = g;
  x.fill();

  rr(x, 4, 4, W - 8, H - 8, 23);
  x.strokeStyle = isLight ? 'rgba(255, 255, 255, 0.95)' : 'rgba(255, 255, 255, 0.24)';
  x.lineWidth = 2;
  x.stroke();

  return texFrom(c, { srgb: true, aniso: 4 });
}

export function buildMacOSStudioDesk(contactShadowTex) {
  const group = new THREE.Group();

  const DESK_W = 190.0;
  const DESK_D = 48.0;
  const DESK_R = 5.8;
  const DESK_THICK = 2.2;
  const DESK_Z = -4.5;

  const texTopDark = makeMacOSDeskTopTexture(false);
  const texTopLight = makeMacOSDeskTopTexture(true);

  // Soft studio floor drop-shadow under the floating architectural desk slab
  const underShadowMat = new THREE.MeshBasicMaterial({
    map: contactShadowTex,
    transparent: true,
    opacity: 0.72,
    depthWrite: false
  });
  const underShadow = new THREE.Mesh(
    new THREE.PlaneGeometry(DESK_W * 1.12, DESK_D * 1.34).rotateX(-Math.PI / 2),
    underShadowMat
  );
  underShadow.position.set(0, -DESK_THICK - 1.4, DESK_Z + 2.8);
  underShadow.renderOrder = 0;
  group.add(underShadow);

  // 3D Desk Edge / Front Bullnose Apron Extrusion
  const slabShape = createRoundedRectShape(DESK_W - 0.5, DESK_D - 0.5, DESK_R);
  const sideGeo = new THREE.ExtrudeGeometry(slabShape, {
    depth: DESK_THICK,
    bevelEnabled: true,
    bevelThickness: 0.26,
    bevelSize: 0.24,
    bevelSegments: 2,
    curveSegments: 18
  });
  sideGeo.rotateX(Math.PI / 2);

  const sideMat = new THREE.MeshBasicMaterial({
    color: 0x1e1917,
    transparent: true,
    opacity: 0.98,
    depthWrite: false
  });
  const slabBody = new THREE.Mesh(sideGeo, sideMat);
  slabBody.position.set(0, -0.26, DESK_Z);
  slabBody.renderOrder = 0;
  group.add(slabBody);

  // Subtle warm bullnose catchlight on the desk front edge
  const frontChamferMat = new THREE.MeshBasicMaterial({
    color: 0x9c8878,
    transparent: true,
    opacity: 0.20,
    depthWrite: false
  });
  const frontChamfer = new THREE.Mesh(
    new THREE.PlaneGeometry(DESK_W - DESK_R * 2.2, 0.22),
    frontChamferMat
  );
  frontChamfer.position.set(0, -0.14, DESK_Z + DESK_D * 0.5 + 0.03);
  frontChamfer.renderOrder = 1;
  group.add(frontChamfer);

  // Textured Top Surface Plane (mapped 0..1 cleanly across DESK_W x DESK_D)
  const topMat = new THREE.MeshBasicMaterial({
    map: texTopDark,
    transparent: true,
    depthWrite: false
  });
  const topMesh = new THREE.Mesh(
    new THREE.PlaneGeometry(DESK_W, DESK_D).rotateX(-Math.PI / 2),
    topMat
  );
  topMesh.position.set(0, 0.0, DESK_Z);
  topMesh.renderOrder = 1;
  group.add(topMesh);

  // ---------------- Elevated macOS Studio Display Riser Shelf ----------------
  const riserGroup = new THREE.Group();
  const RISER_W = 54.0;
  const RISER_D = 15.2;
  const RISER_H = 3.2;
  const RISER_THICK = 0.62;

  const texRiserDark = makeRiserTopTexture(false);
  const texRiserLight = makeRiserTopTexture(true);

  const riserShadowMat = new THREE.MeshBasicMaterial({
    map: contactShadowTex,
    transparent: true,
    opacity: 0.64,
    depthWrite: false
  });
  const riserShadow = new THREE.Mesh(
    new THREE.PlaneGeometry(RISER_W * 1.08, RISER_D * 1.32).rotateX(-Math.PI / 2),
    riserShadowMat
  );
  riserShadow.position.set(0, 0.02, 0.7);
  riserShadow.renderOrder = 3;
  riserGroup.add(riserShadow);

  const legMat = new THREE.MeshBasicMaterial({
    color: 0x7a879f,
    transparent: true,
    opacity: 0.96,
    depthWrite: false
  });
  const legGeo = new THREE.BoxGeometry(1.05, RISER_H - RISER_THICK * 0.5, RISER_D * 0.76);
  const legLeft = new THREE.Mesh(legGeo, legMat);
  legLeft.position.set(-RISER_W * 0.39, (RISER_H - RISER_THICK * 0.5) * 0.5, 0);
  legLeft.renderOrder = 3;
  const legRight = new THREE.Mesh(legGeo, legMat);
  legRight.position.set(RISER_W * 0.39, (RISER_H - RISER_THICK * 0.5) * 0.5, 0);
  legRight.renderOrder = 3;
  riserGroup.add(legLeft, legRight);

  const riserShape = createRoundedRectShape(RISER_W - 0.3, RISER_D - 0.3, 2.3);
  const riserEdgeGeo = new THREE.ExtrudeGeometry(riserShape, {
    depth: RISER_THICK,
    bevelEnabled: true,
    bevelThickness: 0.12,
    bevelSize: 0.12,
    bevelSegments: 2,
    curveSegments: 14
  });
  riserEdgeGeo.rotateX(Math.PI / 2);
  const riserEdgeMat = new THREE.MeshBasicMaterial({
    color: 0x455066,
    transparent: true,
    opacity: 0.98,
    depthWrite: false
  });
  const riserEdgeMesh = new THREE.Mesh(riserEdgeGeo, riserEdgeMat);
  riserEdgeMesh.position.set(0, RISER_H - 0.12, 0);
  riserEdgeMesh.renderOrder = 3;
  riserGroup.add(riserEdgeMesh);

  const riserChamferMat = new THREE.MeshBasicMaterial({
    color: 0xb6c8e6,
    transparent: true,
    opacity: 0.44,
    depthWrite: false
  });
  const riserChamfer = new THREE.Mesh(
    new THREE.PlaneGeometry(RISER_W - 4.8, 0.20),
    riserChamferMat
  );
  riserChamfer.position.set(0, RISER_H - 0.08, RISER_D * 0.5 + 0.04);
  riserChamfer.renderOrder = 3;
  riserGroup.add(riserChamfer);

  const riserTopMat = new THREE.MeshBasicMaterial({
    map: texRiserDark,
    transparent: true,
    depthWrite: false
  });
  const riserTopMesh = new THREE.Mesh(
    new THREE.PlaneGeometry(RISER_W, RISER_D).rotateX(-Math.PI / 2),
    riserTopMat
  );
  riserTopMesh.position.set(0, RISER_H + 0.02, 0);
  riserTopMesh.renderOrder = 3;
  riserGroup.add(riserTopMesh);

  group.add(riserGroup);

  // Rear cable slit world Z = -4.5 - 24.0 + (68 / 512) * 48.0 = -22.125
  const rearChannelZ = DESK_Z - DESK_D * 0.5 + (68 / 512) * DESK_D;
  const frontEdgeZ = DESK_Z + DESK_D * 0.5;

  const anchorFrontEdge = new THREE.Object3D();
  anchorFrontEdge.position.set(0, -DESK_THICK, frontEdgeZ);
  group.add(anchorFrontEdge);

  function setTheme(isLight) {
    topMat.map = isLight ? texTopLight : texTopDark;
    topMat.needsUpdate = true;
    sideMat.color.setHex(isLight ? 0xd4c9ba : 0x1e1917);
    frontChamferMat.color.setHex(isLight ? 0xffffff : 0x9c8878);
    frontChamferMat.opacity = isLight ? 0.65 : 0.20;
    underShadowMat.opacity = isLight ? 0.22 : 0.72;

    riserTopMat.map = isLight ? texRiserLight : texRiserDark;
    riserTopMat.needsUpdate = true;
    riserEdgeMat.color.setHex(isLight ? 0xc6cfdf : 0x455066);
    riserChamferMat.color.setHex(isLight ? 0xffffff : 0xb6c8e6);
    riserChamferMat.opacity = isLight ? 0.88 : 0.44;
    legMat.color.setHex(isLight ? 0x9aa6bc : 0x7a879f);
    riserShadowMat.opacity = isLight ? 0.26 : 0.64;
  }

  return {
    group,
    riserGroup,
    riserHeight: RISER_H,
    rearChannelZ,
    frontEdgeZ,
    anchorFrontEdge,
    setTheme
  };
}

/* ============================================================================
   1. 飞利浦 27E2N6900RW (27" 5K/300Hz Dual-Mode Pearl-White Monitor)
   ========================================================================== */

export function buildPhilips27E2N6900RW({ texFrame, meta, texMac, texWin, texStudio }) {
  const root = new THREE.Group();

  const W = 58.6;
  const H = W / meta.aspect;
  const [u0, v0, u1, v1] = meta.screenUv;

  const sw = (u1 - u0) * W;
  const sh = (v1 - v0) * H;
  const sx = ((u0 + u1) * 0.5 - 0.5) * W;
  const sy = (v0 + v1) * 0.5 * H;

  const panelUniforms = {
    uTexMac: { value: texMac },
    uTexWin: { value: texWin },
    uTexStudio: { value: texStudio },
    uMixWin: { value: 0.0 },
    uMixStudio: { value: 0.0 },
    uBrightness: { value: 0.85 },
    uPower: { value: 1.0 },
    uSwitchSweep: { value: 0.0 },
    uPortalAura: { value: 0.0 }
  };

  const panelMat = new THREE.ShaderMaterial({
    uniforms: panelUniforms,
    transparent: false,
    depthWrite: false,
    toneMapped: false,
    vertexShader: /* glsl */ `
      varying vec2 vUv;
      void main() {
        vUv = uv;
        gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
      }
    `,
    fragmentShader: /* glsl */ `
      uniform sampler2D uTexMac;
      uniform sampler2D uTexWin;
      uniform sampler2D uTexStudio;
      uniform float uMixWin;
      uniform float uMixStudio;
      uniform float uBrightness;
      uniform float uPower;
      uniform float uSwitchSweep;
      uniform float uPortalAura;
      varying vec2 vUv;

      void main() {
        vec3 colMac = texture2D(uTexMac, vUv).rgb;
        vec3 colWin = texture2D(uTexWin, vUv).rgb;
        vec3 colStu = texture2D(uTexStudio, vUv).rgb;

        vec3 activeCol = mix(colMac, colWin, clamp(uMixWin, 0.0, 1.0));
        activeCol = mix(activeCol, colStu, clamp(uMixStudio, 0.0, 1.0));

        float diag = vUv.x * 0.82 + vUv.y * 0.18;
        float sweepBand = exp(-42.0 * (diag - uSwitchSweep) * (diag - uSwitchSweep));
        float sweepMask = sin(clamp(uSwitchSweep, 0.0, 1.0) * 3.14159265);
        activeCol += vec3(0.95, 0.97, 1.0) * sweepBand * sweepMask * 0.18;

        float rightDist = 1.0 - vUv.x;
        float edgeGlow = exp(-rightDist * 34.0) * uPortalAura;
        float vGate = smoothstep(0.06, 0.18, vUv.y) * smoothstep(0.94, 0.82, vUv.y);
        activeCol = mix(activeCol, vec3(0.55, 0.82, 1.0), edgeGlow * vGate * 0.62);

        float gain = mix(0.12, 1.05, clamp(uBrightness, 0.0, 1.0));
        vec3 lit = activeCol * gain;
        vec3 offGlass = vec3(0.004, 0.005, 0.008);
        vec3 finalRgb = mix(offGlass, lit, clamp(uPower, 0.0, 1.0));

        gl_FragColor = vec4(finalRgb, 1.0);
        #include <colorspace_fragment>
      }
    `
  });

  const activeScreen = new THREE.Mesh(
    new THREE.PlaneGeometry(sw + 0.34, sh + 0.34),
    panelMat
  );
  activeScreen.position.set(sx, sy, -0.04);
  activeScreen.renderOrder = 6;
  root.add(activeScreen);

  const frameMat = makeStudioPlateMaterial({ texA: texFrame, lightContrast: 0.24 });
  const frameMesh = new THREE.Mesh(new THREE.PlaneGeometry(W, H), frameMat);
  frameMesh.position.set(0, H * 0.5, 0.0);
  frameMesh.renderOrder = 7;
  root.add(frameMesh);

  const powerLedMat = new THREE.MeshBasicMaterial({
    color: 0xffffff,
    transparent: true,
    opacity: 0.9,
    depthWrite: false,
    toneMapped: false
  });
  const powerLed = new THREE.Mesh(new THREE.CircleGeometry(0.11, 14), powerLedMat);
  const [lu, lv] = meta.ledUv || [0.938, 0.296];
  powerLed.position.set((lu - 0.5) * W, lv * H, 0.03);
  powerLed.renderOrder = 8;
  root.add(powerLed);

  const anchorTopCrown = new THREE.Object3D();
  anchorTopCrown.position.set(0, (v1 + 0.009) * H, 0.06);
  root.add(anchorTopCrown);

  const anchorRightSeam = new THREE.Object3D();
  anchorRightSeam.position.set((u1 + 0.010 - 0.5) * W, sy - 6.6, 0.06);
  root.add(anchorRightSeam);

  const anchorCableSlot = new THREE.Object3D();
  anchorCableSlot.position.set(
    (meta.cableSlotUv[0] - 0.5) * W,
    meta.cableSlotUv[1] * H,
    -0.8
  );
  root.add(anchorCableSlot);

  // Mount point directly at front base of monitor riser shelf
  const anchorMount = new THREE.Object3D();
  anchorMount.position.set(0, -3.2, 8.2);
  root.add(anchorMount);

  root.userData = {
    activeScreen,
    hitBox: {
      minX: sx - sw * 0.5,
      maxX: sx + sw * 0.5,
      minY: sy - sh * 0.5,
      maxY: sy + sh * 0.5,
      z: 0
    },
    panelUniforms,
    powerLedMat,
    plateUniforms: frameMat.uniforms,
    anchorTopCrown,
    anchorRightSeam,
    anchorCableSlot,
    anchorMount
  };
  return root;
}

/* ============================================================================
   2. Windows 台式机 · RTX 5060 Ti (Official NZXT H9 Panoramic Build Studio Render)
   ========================================================================== */

export function buildWindowsWorkstationPC({ texIdle, texActive, meta }) {
  const root = new THREE.Group();

  const H = 35.4;
  const W = H * meta.aspect;

  const plateMat = makeStudioPlateMaterial({
    texA: texIdle,
    texB: texActive,
    lightContrast: 0.0,
    boostDiff: 1.6
  });
  const mesh = new THREE.Mesh(new THREE.PlaneGeometry(W, H), plateMat);
  mesh.position.set(0, H * 0.5 - 3.1, 0);
  mesh.renderOrder = 4;
  root.add(mesh);

  const hitAnchor = new THREE.Object3D();
  hitAnchor.position.set(0, H * 0.46, 0);
  root.add(hitAnchor);

  // Rear GPU HDMI 2.1 exit port (routes rearward into the desk cable raceway)
  const anchorCablePort = new THREE.Object3D();
  anchorCablePort.position.set(W * 0.22, H * 0.16, -2.2);
  root.add(anchorCablePort);

  // Mount point directly beneath the Windows PC chassis footprint
  const anchorMount = new THREE.Object3D();
  anchorMount.position.set(0.4, -1.6, 4.2);
  root.add(anchorMount);

  function setActivityState({ active = false, online = true } = {}) {
    root.userData.targetActiveMix = online && active ? 1.0 : 0.0;
  }

  root.userData = {
    plateUniforms: plateMat.uniforms,
    targetActiveMix: 0.0,
    hitAnchor,
    hitBox: {
      minX: -W * 0.44,
      maxX: W * 0.44,
      minY: 0.04 * H - 3.1,
      maxY: 0.96 * H - 3.1,
      z: 0
    },
    anchorCablePort,
    anchorMount,
    setActivityState
  };
  return root;
}

/* ============================================================================
   3. Apple Mac Studio · M5 Max (Cycles Path-Traced from Official Apple 2026 .usdz)
   ========================================================================== */

export function buildMacStudioM5({ texStudio, meta }) {
  const root = new THREE.Group();

  const W = 18.2;
  const H = W / meta.aspect;

  const plateMat = makeStudioPlateMaterial({ texA: texStudio, lightContrast: 0.34 });
  const mesh = new THREE.Mesh(new THREE.PlaneGeometry(W, H), plateMat);
  mesh.position.set(0, H * 0.5 - 0.04, 0);
  mesh.renderOrder = 5;
  root.add(mesh);

  const ledMat = new THREE.MeshBasicMaterial({
    color: 0xffffff,
    transparent: true,
    opacity: 0.92,
    depthWrite: false,
    toneMapped: false
  });
  const statusLed = new THREE.Mesh(new THREE.CircleGeometry(0.11, 14), ledMat);
  statusLed.position.set((0.785 - 0.5) * W, 0.175 * H - 0.04, 0.03);
  statusLed.renderOrder = 6;
  root.add(statusLed);

  const hitAnchor = new THREE.Object3D();
  hitAnchor.position.set(0, H * 0.5, 0);
  root.add(hitAnchor);

  // Rear Thunderbolt 5 port (routes straight back into the rear cable raceway)
  const anchorCablePort = new THREE.Object3D();
  anchorCablePort.position.set(0.0, H * 0.26, -2.0);
  root.add(anchorCablePort);

  // Mount point directly beneath Mac Studio footprint
  const anchorMount = new THREE.Object3D();
  anchorMount.position.set(0.0, 0.0, 1.5);
  root.add(anchorMount);

  root.userData = {
    ledMat,
    plateUniforms: plateMat.uniforms,
    hitAnchor,
    hitBox: {
      minX: -W * 0.48,
      maxX: W * 0.48,
      minY: 0.04 * H - 0.04,
      maxY: 0.96 * H - 0.04,
      z: 0
    },
    anchorCablePort,
    anchorMount
  };
  return root;
}

/* ============================================================================
   4. Apple MacBook Pro 14" Space Black (Cycles Path-Traced with Official Sonoma 6K)
   ========================================================================== */

export function buildMacBookPro14({ texMbp, meta }) {
  const root = new THREE.Group();

  const W = 34.4;
  const H = W / meta.aspect;

  const plateMat = makeStudioPlateMaterial({ texA: texMbp, lightContrast: 0.04 });
  const mesh = new THREE.Mesh(new THREE.PlaneGeometry(W, H), plateMat);
  mesh.position.set(0, H * 0.5 - 0.12, 0);
  mesh.renderOrder = 9;
  root.add(mesh);

  const screenMesh = new THREE.Object3D();
  screenMesh.position.set((0.40 - 0.5) * W, 0.62 * H - 0.12, 0.05);
  root.add(screenMesh);

  const anchorTopCrown = new THREE.Object3D();
  anchorTopCrown.position.set(
    (meta.notchUvX - 0.5) * W,
    meta.notchUvYFromBottom * H - 0.12,
    0.06
  );
  root.add(anchorTopCrown);

  // Left-rear Thunderbolt 5 USB-C 96W port (sweeps cleanly rearward into cable raceway!)
  const anchorCablePort = new THREE.Object3D();
  anchorCablePort.position.set(-0.465 * W, 0.22 * H - 0.12, -0.6);
  root.add(anchorCablePort);

  // Mount point directly beneath MacBook Pro front lip
  const anchorMount = new THREE.Object3D();
  anchorMount.position.set(0.0, 0.0, 1.5);
  root.add(anchorMount);

  root.userData = {
    screenMesh,
    hitBox: {
      minX: -0.46 * W,
      maxX: 0.44 * W,
      minY: 0.07 * H - 0.12,
      maxY: 0.97 * H - 0.12,
      z: 0
    },
    plateUniforms: plateMat.uniforms,
    anchorTopCrown,
    anchorCablePort,
    anchorMount
  };
  return root;
}

/* ============================================================================
   5. Precision Rear-Raceway Optical Bus Cables + Cross-Screen Continuity Bridge
      - Routes cleanly into the rear desk cable channel (zero messy wires in front!)
      - Dual-layer braided sheath + sapphire-ice optical fiber core for active host
   ========================================================================== */

function makePortCableMaterial(isLight = false) {
  const uniforms = {
    uConnected: { value: 1.0 },
    uActive: { value: 0.0 },
    uPulse: { value: 0.45 },
    uIsLight: { value: isLight ? 1.0 : 0.0 }
  };
  const mat = new THREE.ShaderMaterial({
    uniforms,
    transparent: true,
    depthWrite: false,
    toneMapped: false,
    vertexShader: /* glsl */ `
      varying vec2 vUv;
      void main() {
        vUv = uv;
        gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
      }
    `,
    fragmentShader: /* glsl */ `
      varying vec2 vUv;
      uniform float uConnected;
      uniform float uActive;
      uniform float uPulse;
      uniform float uIsLight;

      void main() {
        float endFade = smoothstep(0.0, 0.035, vUv.x) * smoothstep(1.0, 0.955, vUv.x);
        float radial = abs(sin(vUv.y * 3.14159265));
        float coreMask = smoothstep(0.32, 0.96, radial);

        vec3 sheathDark  = mix(vec3(0.24, 0.29, 0.39), vec3(0.46, 0.53, 0.66), coreMask * 0.65);
        vec3 sheathLight = mix(vec3(0.48, 0.54, 0.64), vec3(0.66, 0.72, 0.82), coreMask * 0.55);
        vec3 idleCol     = mix(sheathDark, sheathLight, uIsLight);

        vec3 sapphireBase = mix(vec3(0.08, 0.48, 0.98), vec3(0.02, 0.44, 0.94), uIsLight);
        vec3 sapphireCore = mix(sapphireBase, vec3(0.84, 0.95, 1.0), coreMask * 0.78);

        float packet = exp(-28.0 * (vUv.x - uPulse) * (vUv.x - uPulse));
        vec3 activeRgb = mix(sapphireCore, vec3(0.97, 0.99, 1.0), packet * 0.55);
        vec3 rgb = mix(idleCol, activeRgb, uActive);

        float baseAlpha = mix(0.56, 0.95, uActive);
        float alpha = baseAlpha * endFade * uConnected;

        gl_FragColor = vec4(rgb, clamp(alpha, 0.0, 0.96));
      }
    `
  });
  return { mat, uniforms };
}

export function buildTopologyBridgeAndCables() {
  const group = new THREE.Group();

  // Cross-Screen Continuity Ribbon (between 27" right bezel and 14" MacBook)
  const ribGeo = new THREE.PlaneGeometry(8.2, 14.6, 22, 10);
  const pos = ribGeo.attributes.position;
  for (let i = 0; i < pos.count; i++) {
    const px = pos.getX(i);
    const py = pos.getY(i);
    const u = px / 8.2 + 0.5;
    const heightScale = THREE.MathUtils.lerp(1.0, 0.74, u);
    const zCurve = Math.sin(u * Math.PI) * 0.9 + u * 2.8;
    const yOffset = THREE.MathUtils.lerp(2.0, -4.2, u);
    pos.setY(i, py * heightScale + yOffset);
    pos.setZ(i, zCurve);
  }
  ribGeo.computeVertexNormals();

  const ribbonUniforms = {
    uTime: { value: 0.35 },
    uBridgeMode: { value: 1.0 },
    uPortalGate: { value: 0.0 },
    uIsWin: { value: 0.0 }
  };

  const ribbonMat = new THREE.ShaderMaterial({
    uniforms: ribbonUniforms,
    transparent: true,
    depthWrite: false,
    toneMapped: false,
    side: THREE.DoubleSide,
    vertexShader: /* glsl */ `
      varying vec2 vUv;
      void main() {
        vUv = uv;
        gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
      }
    `,
    fragmentShader: /* glsl */ `
      uniform float uTime;
      uniform float uBridgeMode;
      uniform float uPortalGate;
      uniform float uIsWin;
      varying vec2 vUv;

      void main() {
        float edgeY = smoothstep(0.0, 0.18, vUv.y) * smoothstep(1.0, 0.82, vUv.y);
        float edgeX = smoothstep(0.0, 0.14, vUv.x) * smoothstep(1.0, 0.86, vUv.x);

        vec3 warmGlass = mix(vec3(0.76, 0.88, 1.0), vec3(0.66, 0.82, 1.0), vUv.x);
        vec3 winGlass  = mix(vec3(0.42, 0.74, 1.0), vec3(0.82, 0.88, 0.96), vUv.x);
        vec3 baseTint  = mix(warmGlass, winGlass, uIsWin);

        float stream = sin((vUv.y * 12.0) - uTime * 1.2 + vUv.x * 3.0) * 0.5 + 0.5;
        float silkLine = smoothstep(0.68, 0.98, stream);

        float rimLine = exp(-180.0 * (vUv.y - 0.10) * (vUv.y - 0.10)) +
                        exp(-180.0 * (vUv.y - 0.90) * (vUv.y - 0.90));

        vec3 rgb = baseTint * (0.58 + 0.42 * silkLine) + vec3(1.0) * (rimLine * 0.48);
        float alpha = (0.11 + silkLine * 0.13 + rimLine * 0.22) * edgeY * edgeX * uBridgeMode;
        alpha = mix(alpha, alpha * 1.25, uPortalGate * 0.5);

        gl_FragColor = vec4(rgb, clamp(alpha, 0.0, 0.62));
      }
    `
  });

  const ribbonMesh = new THREE.Mesh(ribGeo, ribbonMat);
  ribbonMesh.position.set(32.0, 18.0, 1.2);
  ribbonMesh.renderOrder = 10;
  group.add(ribbonMesh);

  const gateBarGeo = new THREE.PlaneGeometry(2.4, 18.2);
  const gateUniforms = {
    uActive: { value: 0.0 },
    uTime: { value: 0.0 }
  };
  const gateBarMat = new THREE.ShaderMaterial({
    uniforms: gateUniforms,
    transparent: true,
    depthWrite: false,
    toneMapped: false,
    side: THREE.DoubleSide,
    vertexShader: `
      varying vec2 vUv;
      void main() {
        vUv = uv;
        gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
      }
    `,
    fragmentShader: `
      varying vec2 vUv;
      uniform float uActive;
      uniform float uTime;

      void main() {
        float fadeY = smoothstep(0.0, 0.16, vUv.y) * smoothstep(1.0, 0.84, vUv.y);
        float dx = abs(vUv.x - 0.5) * 2.0;
        float core = exp(-26.0 * dx * dx);
        float halo = exp(-4.8 * dx * dx);
        vec3 sapphire = vec3(0.16, 0.60, 1.0);
        vec3 rgb = mix(sapphire, vec3(0.96, 0.99, 1.0), core * 0.82);
        float alpha = (halo * 0.48 + core * 0.46) * fadeY * uActive;
        gl_FragColor = vec4(rgb, clamp(alpha, 0.0, 0.92));
      }
    `
  });
  const gateBar = new THREE.Mesh(gateBarGeo, gateBarMat);
  gateBar.renderOrder = 10;
  group.add(gateBar);

  // Precision Braided Optical Cables + Rear Channel Grommet Nodes
  const cables = {};
  for (const id of ['windows', 'studio', 'macbook']) {
    const { mat, uniforms } = makePortCableMaterial(false);
    const dummyCurve = new THREE.CatmullRomCurve3([
      new THREE.Vector3(0, 0.2, -2),
      new THREE.Vector3(0, 0.2, -10),
      new THREE.Vector3(0, 5.0, -8)
    ]);
    const mesh = new THREE.Mesh(new THREE.TubeGeometry(dummyCurve, 44, 0.16, 8, false), mat);
    mesh.renderOrder = 2;
    group.add(mesh);

    // Small machined port collar node where the cable enters the rear desk raceway
    const collarMat = new THREE.MeshBasicMaterial({
      color: 0x64748b,
      transparent: true,
      opacity: 0.90,
      depthWrite: false
    });
    const collar = new THREE.Mesh(
      new THREE.CircleGeometry(0.42, 16).rotateX(-Math.PI / 2),
      collarMat
    );
    collar.renderOrder = 2;
    group.add(collar);

    cables[id] = { mesh, uniforms, collar, collarMat };
  }

  function rebuildCableGeometry(id, pts, grommetPos) {
    const entry = cables[id];
    if (!entry) return;
    const curve = new THREE.CatmullRomCurve3(pts, false, 'centripetal', 0.35);
    const nextGeo = new THREE.TubeGeometry(curve, 52, 0.16, 8, false);
    entry.mesh.geometry.dispose();
    entry.mesh.geometry = nextGeo;
    if (grommetPos) {
      entry.collar.position.copy(grommetPos);
    }
  }

  group.userData = {
    ribbonMesh,
    ribbonUniforms,
    gateBar,
    gateUniforms,
    cables,
    rebuildCableGeometry
  };
  return group;
}
