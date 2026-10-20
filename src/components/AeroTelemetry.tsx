import React from 'react';
import { AlertTriangle, CheckCircle2, Flame, Gauge, Wind } from 'lucide-react';
import { AeroResults, AtmosphereData, FlightPerformance, SimState } from '../types';

interface AeroTelemetryProps {
  state: SimState;
  aero: AeroResults;
  atmo: AtmosphereData;
  perf: FlightPerformance;
}

export const AeroTelemetry: React.FC<AeroTelemetryProps> = ({ state, aero, atmo, perf }) => {
  const isSupersonic = aero.Mach >= 1.0;
  const isTransonic = aero.Mach >= 0.75 && aero.Mach < 1.0;

  return (
    <div className="w-full grid grid-cols-2 sm:grid-cols-3 lg:grid-cols-6 gap-2 text-xs font-mono select-none">
      {/* 1. Lift Force & CL */}
      <div className="metric-card p-2.5 rounded-xl border border-cyan-500/20 relative overflow-hidden group">
        <div className="text-[10px] text-slate-400 uppercase tracking-wider flex items-center justify-between">
          <span>Подъемная сила (L)</span>
          <span className="text-cyan-400 font-bold">CL {aero.CL.toFixed(3)}</span>
        </div>
        <div className="mt-1 flex items-baseline gap-1.5">
          <span className="text-lg font-bold text-cyan-300">
            {aero.Lift >= 10000 ? (aero.Lift / 1000).toFixed(1) + ' кН' : aero.Lift.toFixed(0) + ' Н'}
          </span>
          <span className="text-[10px] text-slate-400">({(aero.Lift / 9.80665).toFixed(0)} кгс)</span>
        </div>
        <div className="text-[10px] text-slate-400 mt-0.5">
          q: <span className="text-slate-300">{aero.dynamicPressure.toFixed(0)} Па</span>
        </div>
      </div>

      {/* 2. Drag Force & CD Breakdown */}
      <div className="metric-card p-2.5 rounded-xl border border-rose-500/20 relative overflow-hidden group">
        <div className="text-[10px] text-slate-400 uppercase tracking-wider flex items-center justify-between">
          <span>Сопротивление (D)</span>
          <span className="text-rose-400 font-bold">CD {aero.CD.toFixed(4)}</span>
        </div>
        <div className="mt-1 flex items-baseline gap-1.5">
          <span className="text-lg font-bold text-rose-300">
            {aero.Drag >= 10000 ? (aero.Drag / 1000).toFixed(1) + ' кН' : aero.Drag.toFixed(1) + ' Н'}
          </span>
        </div>
        <div className="text-[10px] text-slate-400 mt-0.5 flex gap-2">
          <span>CDi: {aero.CDi.toFixed(4)}</span>
          <span>CD0: {aero.CD0.toFixed(4)}</span>
        </div>
      </div>

      {/* 3. Aerodynamic Quality L/D */}
      <div className="metric-card p-2.5 rounded-xl border border-emerald-500/20 relative overflow-hidden group">
        <div className="text-[10px] text-slate-400 uppercase tracking-wider flex items-center justify-between">
          <span>Аэродин. качество</span>
          <span className="text-emerald-400 font-bold">L/D</span>
        </div>
        <div className="mt-1 flex items-baseline gap-1.5">
          <span className={`text-xl font-bold ${aero.LD > 14 ? 'text-emerald-300' : aero.LD > 7 ? 'text-sky-300' : 'text-amber-300'}`}>
            {aero.LD.toFixed(1)}
          </span>
          <span className="text-[10px] text-slate-400">Критич: {perf.maxGlideRatio.toFixed(1)}</span>
        </div>
        <div className="text-[10px] text-slate-400 mt-0.5">
          Скольжение: 1 : {aero.LD.toFixed(1)}
        </div>
      </div>

      {/* 4. Velocity & Mach Number */}
      <div className="metric-card p-2.5 rounded-xl border border-cyan-500/20 relative overflow-hidden group">
        <div className="text-[10px] text-slate-400 uppercase tracking-wider flex items-center justify-between">
          <span>Скорость & Число М</span>
          <span className={`${isSupersonic ? 'text-purple-400 font-bold' : isTransonic ? 'text-amber-400' : 'text-slate-300'}`}>
            M {aero.Mach.toFixed(2)}
          </span>
        </div>
        <div className="mt-1 flex items-baseline gap-1.5">
          <span className="text-lg font-bold text-slate-100">{state.velocity.toFixed(0)} м/с</span>
          <span className="text-[10px] text-slate-400">({(state.velocity * 3.6).toFixed(0)} км/ч)</span>
        </div>
        <div className="text-[10px] text-slate-400 mt-0.5">
          Звук a: <span className="text-slate-300">{atmo.speedOfSound.toFixed(0)} м/с</span>
        </div>
      </div>

      {/* 5. Center of Pressure & Pitching Moment */}
      <div className="metric-card p-2.5 rounded-xl border border-amber-500/20 relative overflow-hidden group">
        <div className="text-[10px] text-slate-400 uppercase tracking-wider flex items-center justify-between">
          <span>Фокус & Момент</span>
          <span className="text-amber-300">Cm {aero.Cm.toFixed(3)}</span>
        </div>
        <div className="mt-1 flex items-baseline gap-1.5">
          <span className="text-lg font-bold text-amber-300">Xcp {(aero.xcp * 100).toFixed(0)}%</span>
          <span className="text-[10px] text-slate-400">хорды</span>
        </div>
        <div className="text-[10px] text-slate-400 mt-0.5">
          Re: <span className="text-slate-300">{aero.Reynolds.toExponential(2)}</span>
        </div>
      </div>

      {/* 6. Flow Regime & Stall Warning */}
      <div
        className={`metric-card p-2.5 rounded-xl border relative overflow-hidden transition-all duration-300 ${
          aero.stallState === 'stalled'
            ? 'border-rose-500 bg-rose-950/40 shadow-[0_0_20px_rgba(244,63,94,0.3)] animate-pulse'
            : aero.stallState === 'incipient'
            ? 'border-amber-500/60 bg-amber-950/30'
            : 'border-emerald-500/30 bg-slate-900/40'
        }`}
      >
        <div className="text-[10px] text-slate-400 uppercase tracking-wider flex items-center justify-between">
          <span>Режим обтекания</span>
          {aero.stallState === 'stalled' ? (
            <AlertTriangle className="w-3.5 h-3.5 text-rose-400" />
          ) : (
            <CheckCircle2 className="w-3.5 h-3.5 text-emerald-400" />
          )}
        </div>
        <div className="mt-1">
          <span
            className={`text-sm font-bold block ${
              aero.stallState === 'stalled'
                ? 'text-rose-400 tracking-wide'
                : aero.stallState === 'incipient'
                ? 'text-amber-300'
                : 'text-emerald-300'
            }`}
          >
            {aero.stallState === 'stalled'
              ? 'СРЫВ ПОТОКА (STALL)'
              : aero.stallState === 'incipient'
              ? 'Предсрывной режим'
              : 'Безотрывное ламинар.'}
          </span>
        </div>
        <div className="text-[10px] text-slate-400 mt-0.5 flex justify-between">
          <span>Срыв: {(aero.stallFactor * 100).toFixed(0)}%</span>
          <span>AR = {aero.aspectRatio.toFixed(1)}</span>
        </div>
      </div>
    </div>
  );
};
