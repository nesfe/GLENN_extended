import React, { useEffect, useRef, useState } from 'react';
import { AeroResults, AirfoilGeometry, SimState } from '../types';

interface WindTunnel2DProps {
  state: SimState;
  geom: AirfoilGeometry;
  aero: AeroResults;
}

export const WindTunnel2D: React.FC<WindTunnel2DProps> = ({ state, geom, aero }) => {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const [showCpVectors, setShowCpVectors] = useState(true);
  const [showCamberLine, setShowCamberLine] = useState(true);
  const [showBoundaryLayer, setShowBoundaryLayer] = useState(true);
  const [showStreamlines2D, setShowStreamlines2D] = useState(true);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;

    let animId: number;
    let t0 = performance.now();

    const render = (time: number) => {
      const elapsed = (time - t0) * 0.001;

      // Handle High-DPI canvas
      const rect = canvas.getBoundingClientRect();
      const dpr = Math.min(window.devicePixelRatio || 1, 2);
      if (canvas.width !== rect.width * dpr || canvas.height !== rect.height * dpr) {
        canvas.width = rect.width * dpr;
        canvas.height = rect.height * dpr;
      }

      ctx.save();
      ctx.scale(dpr, dpr);
      const w = rect.width;
      const h = rect.height;

      // Dark aerospace background
      ctx.fillStyle = '#060a14';
      ctx.fillRect(0, 0, w, h);

      // Fine grid background
      ctx.strokeStyle = '#0e1726';
      ctx.lineWidth = 1;
      const gridSize = 32;
      for (let x = 0; x < w; x += gridSize) {
        ctx.beginPath();
        ctx.moveTo(x, 0);
        ctx.lineTo(x, h);
        ctx.stroke();
      }
      for (let y = 0; y < h; y += gridSize) {
        ctx.beginPath();
        ctx.moveTo(0, y);
        ctx.lineTo(w, y);
        ctx.stroke();
      }

      // Airfoil transformation setup
      const padX = 70;
      const chordPixelLen = w - padX * 2;
      const originX = padX;
      const originY = h * 0.52;

      ctx.save();
      // Translate to quarter chord
      ctx.translate(originX + chordPixelLen * 0.25, originY);
      // Pitch rotation around quarter-chord by angle of attack alpha
      ctx.rotate(-(state.alpha * Math.PI) / 180);
      ctx.translate(-chordPixelLen * 0.25, 0);

      const upper = geom.upper;
      const lower = geom.lower;
      const camber = geom.camberLine;

      // Draw Boundary Layer Displacement Thickness delta*
      if (showBoundaryLayer && upper.length > 5) {
        ctx.fillStyle = 'rgba(56, 189, 248, 0.08)';
        ctx.strokeStyle = 'rgba(56, 189, 248, 0.25)';
        ctx.lineWidth = 1.2;
        ctx.setLineDash([3, 3]);

        ctx.beginPath();
        for (let i = 0; i < upper.length; i++) {
          const x = upper[i].x * chordPixelLen;
          // Blasius laminar / turbulent boundary layer growth delta approx 0.038 * sqrt(x)
          const deltaStar = Math.min(30, 16 * Math.sqrt(Math.max(0.001, upper[i].x)) * (1 + aero.stallFactor * 1.5));
          const y = -(upper[i].y * chordPixelLen + deltaStar);
          if (i === 0) ctx.moveTo(x, y);
          else ctx.lineTo(x, y);
        }
        ctx.stroke();
        ctx.setLineDash([]);
      }

      // Draw 2D Airfoil Skin
      if (upper.length > 2 && lower.length > 2) {
        ctx.beginPath();
        // Upper contour from TE to LE
        for (let i = upper.length - 1; i >= 0; i--) {
          const px = upper[i].x * chordPixelLen;
          const py = -upper[i].y * chordPixelLen;
          if (i === upper.length - 1) ctx.moveTo(px, py);
          else ctx.lineTo(px, py);
        }
        // Lower contour from LE to TE
        for (let i = 1; i < lower.length; i++) {
          const px = lower[i].x * chordPixelLen;
          const py = -lower[i].y * chordPixelLen;
          ctx.lineTo(px, py);
        }
        ctx.closePath();

        // Airfoil body fill & stroke
        const grad = ctx.createLinearGradient(0, -50, 0, 50);
        grad.addColorStop(0, 'rgba(30, 58, 138, 0.35)');
        grad.addColorStop(1, 'rgba(15, 23, 42, 0.7)');
        ctx.fillStyle = grad;
        ctx.fill();

        ctx.strokeStyle = '#38bdf8';
        ctx.lineWidth = 2.2;
        ctx.shadowColor = 'rgba(56, 189, 248, 0.5)';
        ctx.shadowBlur = 8;
        ctx.stroke();
        ctx.shadowBlur = 0;
      }

      // Mean Camber Line
      if (showCamberLine && camber.length > 2) {
        ctx.strokeStyle = '#fbbf24';
        ctx.lineWidth = 1.6;
        ctx.beginPath();
        for (let i = 0; i < camber.length; i++) {
          const px = camber[i].x * chordPixelLen;
          const py = -camber[i].y * chordPixelLen;
          if (i === 0) ctx.moveTo(px, py);
          else ctx.lineTo(px, py);
        }
        ctx.stroke();
      }

      // Chord Reference Line
      ctx.strokeStyle = 'rgba(148, 163, 184, 0.3)';
      ctx.lineWidth = 1;
      ctx.setLineDash([4, 4]);
      ctx.beginPath();
      ctx.moveTo(0, 0);
      ctx.lineTo(chordPixelLen, 0);
      ctx.stroke();
      ctx.setLineDash([]);

      // Surface Normal Pressure Vectors (Cp vectors)
      if (showCpVectors && aero.cpUpper.length > 0) {
        const cpScale = 36;
        // Upper surface suction vectors
        ctx.lineWidth = 1.4;
        for (let i = 2; i < upper.length - 1; i += 3) {
          const px = upper[i].x * chordPixelLen;
          const py = -upper[i].y * chordPixelLen;
          const cpVal = aero.cpUpper[i]?.cp ?? 0;
          // Normal outward vector
          const vLen = Math.max(-120, Math.min(80, cpVal * cpScale));

          ctx.strokeStyle = cpVal < 0 ? 'rgba(56, 189, 248, 0.8)' : 'rgba(244, 63, 94, 0.8)';
          ctx.beginPath();
          ctx.moveTo(px, py);
          ctx.lineTo(px, py + vLen);
          ctx.stroke();

          // Arrowhead
          ctx.fillStyle = cpVal < 0 ? '#38bdf8' : '#f43f5e';
          ctx.beginPath();
          ctx.arc(px, py + vLen, 2, 0, Math.PI * 2);
          ctx.fill();
        }

        // Lower surface pressure vectors
        for (let i = 2; i < lower.length - 1; i += 3) {
          const px = lower[i].x * chordPixelLen;
          const py = -lower[i].y * chordPixelLen;
          const cpVal = aero.cpLower[i]?.cp ?? 0;
          const vLen = Math.max(-80, Math.min(80, cpVal * cpScale));

          ctx.strokeStyle = cpVal < 0 ? 'rgba(56, 189, 248, 0.7)' : 'rgba(244, 63, 94, 0.7)';
          ctx.beginPath();
          ctx.moveTo(px, py);
          ctx.lineTo(px, py - vLen);
          ctx.stroke();
        }
      }

      // Center of Pressure marker Xcp
      const xcpPixel = aero.xcp * chordPixelLen;
      ctx.fillStyle = '#f43f5e';
      ctx.shadowColor = '#f43f5e';
      ctx.shadowBlur = 10;
      ctx.beginPath();
      ctx.arc(xcpPixel, 0, 5, 0, Math.PI * 2);
      ctx.fill();
      ctx.shadowBlur = 0;

      // Xcp label
      ctx.fillStyle = '#fecdd3';
      ctx.font = '10px JetBrains Mono, monospace';
      ctx.fillText(`Xcp ${(aero.xcp * 100).toFixed(0)}%`, xcpPixel - 16, 18);

      ctx.restore(); // Restore airfoil transformation

      // 2D Streamlines in Wind Tunnel Frame
      if (showStreamlines2D) {
        ctx.strokeStyle = 'rgba(56, 189, 248, 0.22)';
        ctx.lineWidth = 1.2;
        const numLines2D = 12;
        const gamma2D = aero.CL * 35;

        for (let l = 0; l < numLines2D; l++) {
          const y0 = 35 + (l / (numLines2D - 1)) * (h - 70);
          ctx.beginPath();

          for (let px = 0; px < w; px += 8) {
            const rx = (px - originX) / chordPixelLen;
            const ry = (y0 - originY) / chordPixelLen;

            // Flow deflection calculation
            const dist = Math.hypot(rx - 0.25, ry);
            let dy = 0;
            if (dist > 0.08) {
              dy = (gamma2D * Math.exp(-Math.abs(rx - 0.25) * 1.5)) / (dist * 4 + 0.5);
              if (rx > 0.4) dy -= gamma2D * 0.45 * (rx - 0.4);
            }

            const py = y0 + dy;
            if (px === 0) ctx.moveTo(px, py);
            else ctx.lineTo(px, py);
          }
          ctx.stroke();

          // Animated particle on each streamline
          const speed2D = 0.5 + state.velocity / 100;
          const partFrac = (elapsed * speed2D + l * 0.12) % 1;
          const partX = partFrac * w;
          const rxP = (partX - originX) / chordPixelLen;
          const ryP = (y0 - originY) / chordPixelLen;
          const distP = Math.hypot(rxP - 0.25, ryP);
          let dyP = 0;
          if (distP > 0.08) {
            dyP = (gamma2D * Math.exp(-Math.abs(rxP - 0.25) * 1.5)) / (distP * 4 + 0.5);
            if (rxP > 0.4) dyP -= gamma2D * 0.45 * (rxP - 0.4);
          }

          ctx.fillStyle = '#7dd3fc';
          ctx.beginPath();
          ctx.arc(partX, y0 + dyP, 2.2, 0, Math.PI * 2);
          ctx.fill();
        }
      }

      ctx.restore();
      animId = requestAnimationFrame(render);
    };

    animId = requestAnimationFrame(render);
    return () => cancelAnimationFrame(animId);
  }, [state, geom, aero, showCpVectors, showCamberLine, showBoundaryLayer, showStreamlines2D]);

  return (
    <div className="relative w-full h-full min-h-[360px] bg-[#060a14] rounded-xl overflow-hidden border border-cyan-500/20 shadow-xl flex flex-col">
      {/* 2D Canvas */}
      <canvas ref={canvasRef} className="w-full flex-1 block" />

      {/* Floating Toggle Controls */}
      <div className="absolute top-3 left-3 flex flex-wrap gap-1.5 p-1.5 rounded-lg glass-panel z-10 text-[11px] font-mono">
        <button
          onClick={() => setShowCpVectors(!showCpVectors)}
          className={`px-2.5 py-1 rounded transition border ${
            showCpVectors
              ? 'bg-cyan-500/20 text-cyan-300 border-cyan-500/40 shadow-sm'
              : 'bg-slate-800/60 text-slate-400 border-slate-700/40'
          }`}
        >
          Эпюра Cp
        </button>
        <button
          onClick={() => setShowCamberLine(!showCamberLine)}
          className={`px-2.5 py-1 rounded transition border ${
            showCamberLine
              ? 'bg-amber-500/20 text-amber-300 border-amber-500/40'
              : 'bg-slate-800/60 text-slate-400 border-slate-700/40'
          }`}
        >
          Средняя линия
        </button>
        <button
          onClick={() => setShowBoundaryLayer(!showBoundaryLayer)}
          className={`px-2.5 py-1 rounded transition border ${
            showBoundaryLayer
              ? 'bg-sky-500/20 text-sky-300 border-sky-500/40'
              : 'bg-slate-800/60 text-slate-400 border-slate-700/40'
          }`}
        >
          Погранслой δ*
        </button>
        <button
          onClick={() => setShowStreamlines2D(!showStreamlines2D)}
          className={`px-2.5 py-1 rounded transition border ${
            showStreamlines2D
              ? 'bg-cyan-500/20 text-cyan-300 border-cyan-500/40'
              : 'bg-slate-800/60 text-slate-400 border-slate-700/40'
          }`}
        >
          Линии тока
        </button>
      </div>

      {/* Profile Info Overlay */}
      <div className="absolute bottom-3 right-3 p-2.5 rounded-lg glass-panel z-10 font-mono text-[11px] space-y-0.5 border border-cyan-500/20 text-right">
        <div className="text-cyan-400 font-semibold">{state.profileName}</div>
        <div className="text-slate-400">
          Кривизна: <span className="text-slate-200">{state.camber}% @ {state.camberPos}%</span>
        </div>
        <div className="text-slate-400">
          Толщина: <span className="text-slate-200">{state.thickness}%</span> | Хорда: <span className="text-slate-200">{state.chord.toFixed(1)} м</span>
        </div>
      </div>
    </div>
  );
};
