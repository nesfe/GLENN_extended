import * as THREE from 'three';

export function getPressureColor(cp: number, scheme: 'rainbow' | 'jet' | 'plasma' | 'thermal' = 'rainbow'): THREE.Color {
  // Normalize Cp from roughly -3.5 (high suction) to +1.0 (stagnation) -> 0 to 1
  const t = Math.max(0, Math.min(1, (cp + 3.2) / 4.2));
  const color = new THREE.Color();

  switch (scheme) {
    case 'jet': {
      // Classic Jet: Blue -> Cyan -> Green -> Yellow -> Red
      let r = 0, g = 0, b = 0;
      if (t < 0.125) {
        b = 0.5 + 4 * t;
      } else if (t < 0.375) {
        b = 1;
        g = 4 * (t - 0.125);
      } else if (t < 0.625) {
        b = 1 - 4 * (t - 0.375);
        g = 1;
        r = 4 * (t - 0.375);
      } else if (t < 0.875) {
        g = 1 - 4 * (t - 0.625);
        r = 1;
      } else {
        r = 1 - 2 * (t - 0.875);
      }
      color.setRGB(Math.max(0, Math.min(1, r)), Math.max(0, Math.min(1, g)), Math.max(0, Math.min(1, b)));
      break;
    }

    case 'plasma': {
      // Perceptually uniform Plasma: Purple -> Pink -> Orange -> Yellow
      const r = 0.05 + 0.95 * Math.sin(t * 1.5);
      const g = Math.pow(t, 1.8) * 0.9;
      const b = 0.5 + 0.5 * Math.cos(t * Math.PI * 1.2);
      color.setRGB(Math.max(0, Math.min(1, r)), Math.max(0, Math.min(1, g)), Math.max(0, Math.min(1, b)));
      break;
    }

    case 'thermal': {
      // Blackbody / Thermal: Dark Blue -> Red -> Orange -> White
      const r = Math.min(1, Math.max(0, 1.5 * t));
      const g = Math.min(1, Math.max(0, 2.0 * (t - 0.5)));
      const b = Math.min(1, Math.max(0, 4.0 * (0.35 - t)));
      color.setRGB(r, g, b);
      break;
    }

    case 'rainbow':
    default: {
      // NASA Rainbow: Blue (suction) -> Cyan -> Green -> Yellow -> Red (pressure)
      // t = 0 (max suction) -> HSL hue ~ 0.72 (Deep Blue/Violet)
      // t = 1 (max pressure) -> HSL hue ~ 0.0 (Red)
      const hue = (1.0 - t) * 0.68;
      color.setHSL(hue, 0.95, 0.48);
      break;
    }
  }

  return color;
}
