import React, { useState, useEffect, useRef, useMemo, useCallback } from 'react';
import {
  Wind,
  Gauge,
  Sliders,
  Layers,
  Volume2,
  VolumeX,
  Download,
  Upload,
  RotateCcw,
  Compass,
  Activity,
  FileText,
  HelpCircle,
  ChevronDown,
  Sparkles,
  Plane,
  Eye,
  Zap,
} from 'lucide-react';

import {
  AirfoilGeometry,
  AirfoilType,
  AeroResults,
  AtmosphereData,
  FlightPerformance,
  PlanetType,
  FlapType,
  EngineType,
  SimState,
} from './types';
import { generateAirfoilCoordinates, parseAirfoilFile } from './physics/airfoilGenerator';
import { calculateAtmosphere } from './physics/atmosphere';
import { solveAerodynamics } from './physics/aeroSolver';
import { calculateFlightPerformance } from './physics/flightMechanics';
import { WindTunnelAudioEngine } from './audio/windTunnelAudio';
import { exportOBJ, exportSTL, exportCSV, exportStateJSON, exportDAT } from './utils/exportUtils';

import { WindTunnel3D } from './components/WindTunnel3D';
import { WindTunnel2D } from './components/WindTunnel2D';
import { AeroTelemetry } from './components/AeroTelemetry';
import { AeroCharts } from './components/AeroCharts';

const STORAGE_KEY = 'glenn_extended_sim_state_v2';

const DEFAULT_STATE: SimState = {
  // Atmosphere & Flow
  planet: 'earth',
  customDensity: 1.225,
  customSpeedOfSound: 340.29,
  altitude: 1000,
  velocity: 70,
  alpha: 4.0,

  // Airfoil
  profileType: 'naca4',
  profileName: 'NACA 2412',
  camber: 2.0,
  camberPos: 40,
  thickness: 12.0,
  cylinderRpm: 0,

  // Mechanization
  flapAngle: 0,
  flapType: 'plain',
  slatAngle: 0,

  // 3D Planform
  chord: 1.6,
  span: 9.6,
  sweep: 12.0,
  taper: 0.6,
  twist: -2.0,
  dihedral: 2.5,
  hasWinglets: true,

  // Propulsion & Aircraft
  engineType: 'turbofan',
  maxThrust: 45000,
  aircraftMass: 4800,
  fuelMass: 1500,
  tsfc: 0.65,

  // Visuals
  show3DStreamlines: true,
  show3DVortices: true,
  show3DForceVectors: true,
  show3DPressureMap: true,
  show3DWireframe: false,
  show3DPitotProbe: false,
  showShockWaves: true,
  colormap: 'rainbow',
  smokeDensity: 3,

  // Interactive probe
  probePosition: { x: -0.4, y: 0.35, z: 0.5 },

  // Audio
  audioEnabled: false,
  audioVolume: 0.35,

  // Custom Raw Airfoil
  customUpper: null,
  customLower: null,
};

interface PresetConfig {
  id: string;
  name: string;
  badge: string;
  state: Partial<SimState>;
}

const PRESETS: PresetConfig[] = [
  {
    id: 'naca2412',
    name: 'NACA 2412 — Крейсерский (Cessna 172)',
    badge: 'GA / Subsonic',
    state: {
      profileType: 'naca4',
      profileName: 'NACA 2412',
      camber: 2.0,
      camberPos: 40,
      thickness: 12.0,
      chord: 1.6,
      span: 11.0,
      sweep: 0,
      taper: 0.68,
      twist: -1.8,
      dihedral: 2.0,
      hasWinglets: false,
      velocity: 65,
      altitude: 1200,
      alpha: 3.5,
      aircraftMass: 1150,
      maxThrust: 2400,
      engineType: 'piston_prop',
      planet: 'earth',
      cylinderRpm: 0,
      flapAngle: 0,
      slatAngle: 0,
      customUpper: null,
      customLower: null,
    },
  },
  {
    id: 'naca0012',
    name: 'NACA 0012 — Симметричный (Пилотажный / Вертолетный)',
    badge: 'Aerobatic / Rotor',
    state: {
      profileType: 'naca4',
      profileName: 'NACA 0012',
      camber: 0.0,
      camberPos: 30,
      thickness: 12.0,
      chord: 1.4,
      span: 8.4,
      sweep: 0,
      taper: 1.0,
      twist: 0,
      dihedral: 0,
      hasWinglets: false,
      velocity: 75,
      altitude: 800,
      alpha: 4.0,
      aircraftMass: 850,
      maxThrust: 3800,
      engineType: 'piston_prop',
      planet: 'earth',
      cylinderRpm: 0,
      flapAngle: 0,
      slatAngle: 0,
      customUpper: null,
      customLower: null,
    },
  },
  {
    id: 'naca4415',
    name: 'NACA 4415 — Высоконесущий БПЛА (STOL / Cargo)',
    badge: 'UAV / STOL',
    state: {
      profileType: 'naca4',
      profileName: 'NACA 4415',
      camber: 4.0,
      camberPos: 40,
      thickness: 15.0,
      chord: 1.8,
      span: 14.5,
      sweep: 3,
      taper: 0.75,
      twist: -2.5,
      dihedral: 3.0,
      hasWinglets: true,
      velocity: 42,
      altitude: 1500,
      alpha: 5.5,
      aircraftMass: 1600,
      maxThrust: 3400,
      engineType: 'electric_prop',
      planet: 'earth',
      cylinderRpm: 0,
      flapAngle: 0,
      slatAngle: 0,
      customUpper: null,
      customLower: null,
    },
  },
  {
    id: 'naca6409',
    name: 'NACA 6409 — Планерный ламинарный (Высокое L/D)',
    badge: 'Sailplane',
    state: {
      profileType: 'naca4',
      profileName: 'NACA 6409',
      camber: 6.0,
      camberPos: 40,
      thickness: 9.0,
      chord: 0.9,
      span: 18.0,
      sweep: 1,
      taper: 0.45,
      twist: -2.0,
      dihedral: 2.5,
      hasWinglets: true,
      velocity: 32,
      altitude: 1800,
      alpha: 4.5,
      aircraftMass: 420,
      maxThrust: 450,
      engineType: 'electric_prop',
      planet: 'earth',
      cylinderRpm: 0,
      flapAngle: 0,
      slatAngle: 0,
      customUpper: null,
      customLower: null,
    },
  },
  {
    id: 'supercritical',
    name: 'NASA SC(2)-0714 — Трансзвуковой лайнер (Boeing/Airbus)',
    badge: 'Mach 0.85',
    state: {
      profileType: 'supercritical',
      profileName: 'NASA SC(2)-0714',
      camber: 2.8,
      camberPos: 50,
      thickness: 14.0,
      chord: 4.5,
      span: 35.0,
      sweep: 30.0,
      taper: 0.26,
      twist: -3.5,
      dihedral: 5.0,
      hasWinglets: true,
      velocity: 245,
      altitude: 10800,
      alpha: 2.2,
      aircraftMass: 75000,
      maxThrust: 180000,
      engineType: 'turbofan',
      planet: 'earth',
      cylinderRpm: 0,
      flapAngle: 0,
      slatAngle: 0,
      customUpper: null,
      customLower: null,
    },
  },
  {
    id: 'biconvex',
    name: 'Двояковыпуклый сверхзвуковой (F-104 Starfighter)',
    badge: 'Supersonic Mach 1.8',
    state: {
      profileType: 'biconvex',
      profileName: 'Biconvex Supersonic',
      camber: 0.0,
      camberPos: 50,
      thickness: 4.5,
      chord: 2.3,
      span: 6.7,
      sweep: 26.0,
      taper: 0.38,
      twist: 0,
      dihedral: -10.0,
      hasWinglets: false,
      velocity: 520,
      altitude: 12000,
      alpha: 3.0,
      aircraftMass: 9200,
      maxThrust: 72000,
      engineType: 'turbojet',
      planet: 'earth',
      cylinderRpm: 0,
      flapAngle: 0,
      slatAngle: 0,
      customUpper: null,
      customLower: null,
    },
  },
  {
    id: 'double_wedge',
    name: 'Двойной клин — Гиперзвуковой (Mach 3+ ракета / X-15)',
    badge: 'Mach 3.0+',
    state: {
      profileType: 'double_wedge',
      profileName: 'Supersonic Double-Wedge',
      camber: 0.0,
      camberPos: 50,
      thickness: 3.5,
      chord: 1.6,
      span: 4.2,
      sweep: 45.0,
      taper: 0.25,
      twist: 0,
      dihedral: 0,
      hasWinglets: false,
      velocity: 920,
      altitude: 19000,
      alpha: 2.5,
      aircraftMass: 4200,
      maxThrust: 110000,
      engineType: 'rocket',
      planet: 'earth',
      cylinderRpm: 0,
      flapAngle: 0,
      slatAngle: 0,
      customUpper: null,
      customLower: null,
    },
  },
  {
    id: 'cylinder',
    name: 'Цилиндр Флеттнера — Эффект Магнуса (Ротор)',
    badge: 'Magnus Rotor',
    state: {
      profileType: 'cylinder',
      profileName: 'Flettner Rotor Cylinder',
      camber: 0,
      camberPos: 50,
      thickness: 25,
      cylinderRpm: 1400,
      chord: 1.2,
      span: 8.0,
      sweep: 0,
      taper: 1.0,
      twist: 0,
      dihedral: 0,
      hasWinglets: false,
      velocity: 35,
      altitude: 400,
      alpha: 0,
      aircraftMass: 1600,
      maxThrust: 4500,
      engineType: 'electric_prop',
      planet: 'earth',
      flapAngle: 0,
      slatAngle: 0,
      customUpper: null,
      customLower: null,
    },
  },
  {
    id: 'mars_drone',
    name: 'Марсианский БПЛА (Ingenuity / Разреженная CO2)',
    badge: 'Mars Scout',
    state: {
      planet: 'mars',
      profileType: 'naca4',
      profileName: 'NACA 6412 (Mars)',
      camber: 6.0,
      camberPos: 40,
      thickness: 12.0,
      chord: 0.32,
      span: 2.2,
      sweep: 0,
      taper: 0.55,
      twist: -3.0,
      dihedral: 1.0,
      hasWinglets: true,
      velocity: 34,
      altitude: 120,
      alpha: 6.0,
      aircraftMass: 5.2,
      maxThrust: 52,
      engineType: 'electric_prop',
      cylinderRpm: 0,
      flapAngle: 0,
      slatAngle: 0,
      customUpper: null,
      customLower: null,
    },
  },
  {
    id: 'hydrofoil',
    name: 'Подводное крыло (Гидродинамика / Плотность 998 кг/м³)',
    badge: 'Hydrodynamics',
    state: {
      planet: 'water',
      profileType: 'naca4',
      profileName: 'NACA 4412 (Hydro)',
      camber: 4.0,
      camberPos: 40,
      thickness: 12.0,
      chord: 0.8,
      span: 3.5,
      sweep: 15,
      taper: 0.7,
      twist: -1.0,
      dihedral: 5.0,
      hasWinglets: false,
      velocity: 18,
      altitude: 5,
      alpha: 2.5,
      aircraftMass: 3200,
      maxThrust: 16000,
      engineType: 'electric_prop',
      cylinderRpm: 0,
      flapAngle: 0,
      slatAngle: 0,
      customUpper: null,
      customLower: null,
    },
  },
];

export const App: React.FC = () => {
  // Load state from localStorage or fallback
  const [state, setState] = useState<SimState>(() => {
    try {
      const saved = localStorage.getItem(STORAGE_KEY);
      if (saved) {
        return { ...DEFAULT_STATE, ...JSON.parse(saved) };
      }
    } catch {}
    return DEFAULT_STATE;
  });

  // Active Control Tab: flow, profile, wing3d, propulsion, visuals
  const [activeTab, setActiveTab] = useState<'flow' | 'profile' | 'wing3d' | 'propulsion' | 'visuals'>('flow');

  // Mesh cache for OBJ/STL export
  const meshExportData = useRef<{ vertices: Float32Array; indices: Uint16Array | Uint32Array } | null>(null);

  // File input ref for DAT/DXF
  const fileInputRef = useRef<HTMLInputElement>(null);
  const jsonImportRef = useRef<HTMLInputElement>(null);

  // Drag and drop state
  const [isDraggingFile, setIsDraggingFile] = useState(false);

  // Audio Engine ref
  const audioEngineRef = useRef<WindTunnelAudioEngine | null>(null);

  // Auto-save state
  useEffect(() => {
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(state));
    } catch {}
  }, [state]);

  // Atmosphere Calculation
  const atmo: AtmosphereData = useMemo(() => {
    return calculateAtmosphere(
      state.planet,
      state.altitude,
      state.velocity,
      state.customDensity,
      state.customSpeedOfSound
    );
  }, [state.planet, state.altitude, state.velocity, state.customDensity, state.customSpeedOfSound]);

  // Airfoil Geometry Generation
  const geom: AirfoilGeometry = useMemo(() => {
    return generateAirfoilCoordinates({
      type: state.profileType,
      camber: state.camber,
      camberPos: state.camberPos,
      thickness: state.thickness,
      flapAngle: state.flapAngle,
      flapType: state.flapType,
      slatAngle: state.slatAngle,
      customUpper: state.customUpper,
      customLower: state.customLower,
    });
  }, [
    state.profileType,
    state.camber,
    state.camberPos,
    state.thickness,
    state.flapAngle,
    state.flapType,
    state.slatAngle,
    state.customUpper,
    state.customLower,
  ]);

  // Aerodynamic Analysis
  const aero: AeroResults = useMemo(() => {
    return solveAerodynamics(geom, state, atmo);
  }, [geom, state, atmo]);

  // Flight Performance Calculation
  const perf: FlightPerformance = useMemo(() => {
    return calculateFlightPerformance(state, atmo, aero.CL, aero.CD0, aero.oswaldEfficiency);
  }, [state, atmo, aero.CL, aero.CD0, aero.oswaldEfficiency]);

  // Single point polar solver for charts & CSV export
  const solvePolarPoint = useCallback(
    (alphaTest: number): AeroResults => {
      const testState: SimState = { ...state, alpha: alphaTest };
      return solveAerodynamics(geom, testState, atmo);
    },
    [geom, state, atmo]
  );

  // Audio Engine Lifecycle & Realtime Update
  useEffect(() => {
    if (state.audioEnabled) {
      if (!audioEngineRef.current) {
        audioEngineRef.current = new WindTunnelAudioEngine();
        audioEngineRef.current.init();
      }
      audioEngineRef.current.update(
        state.velocity,
        aero.dynamicPressure,
        aero.stallFactor,
        aero.Mach,
        state.audioVolume
      );
    } else {
      if (audioEngineRef.current) {
        audioEngineRef.current.setMute(true);
      }
    }
  }, [state.audioEnabled, state.audioVolume, state.velocity, aero.dynamicPressure, aero.stallFactor, aero.Mach]);

  useEffect(() => {
    return () => {
      if (audioEngineRef.current) {
        audioEngineRef.current.destroy();
      }
    };
  }, []);

  // Update handlers
  const updateState = (patch: Partial<SimState>) => {
    setState((prev) => ({ ...prev, ...patch }));
  };

  const handleApplyPreset = (presetId: string) => {
    const p = PRESETS.find((item) => item.id === presetId);
    if (!p) return;
    setState((prev) => ({
      ...prev,
      ...p.state,
    }));
  };

  const handleResetDefaults = () => {
    if (confirm('Сбросить все параметры к заводским настройкам NASA Glenn?')) {
      setState(DEFAULT_STATE);
    }
  };

  // Mesh export receiver from WindTunnel3D
  const handleMeshExport = useCallback((vertices: Float32Array, indices: Uint16Array | Uint32Array) => {
    meshExportData.current = { vertices, indices };
  }, []);

  // File import (DAT / DXF)
  const processAirfoilText = (content: string, filename: string) => {
    const parsed = parseAirfoilFile(content, filename);
    if (parsed) {
      setState((prev) => ({
        ...prev,
        profileType: 'custom',
        profileName: parsed.name,
        customUpper: parsed.upper,
        customLower: parsed.lower,
      }));
    } else {
      alert('Не удалось распознать формат файла. Поддерживаются .DAT (Selig/Lednicer) и .DXF');
    }
  };

  const handleFileUpload = (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file) return;
    const reader = new FileReader();
    reader.onload = (ev) => {
      const text = ev.target?.result as string;
      if (text) processAirfoilText(text, file.name);
    };
    reader.readAsText(file);
    e.target.value = '';
  };

  // Drag and Drop handling
  const handleDragOver = (e: React.DragEvent) => {
    e.preventDefault();
    setIsDraggingFile(true);
  };

  const handleDragLeave = (e: React.DragEvent) => {
    e.preventDefault();
    setIsDraggingFile(false);
  };

  const handleDrop = (e: React.DragEvent) => {
    e.preventDefault();
    setIsDraggingFile(false);
    const file = e.dataTransfer.files?.[0];
    if (!file) return;
    const reader = new FileReader();
    reader.onload = (ev) => {
      const text = ev.target?.result as string;
      if (text) processAirfoilText(text, file.name);
    };
    reader.readAsText(file);
  };

  // JSON state import
  const handleJsonImport = (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file) return;
    const reader = new FileReader();
    reader.onload = (ev) => {
      try {
        const text = ev.target?.result as string;
        const loaded = JSON.parse(text);
        setState((prev) => ({ ...prev, ...loaded }));
      } catch {
        alert('Ошибка при чтении JSON конфигурации');
      }
    };
    reader.readAsText(file);
    e.target.value = '';
  };

  return (
    <div
      className="min-h-screen flex flex-col bg-[#050813] text-slate-100 font-sans select-none"
      onDragOver={handleDragOver}
      onDragLeave={handleDragLeave}
      onDrop={handleDrop}
    >
      {/* Drag & Drop Visual Overlay */}
      {isDraggingFile && (
        <div className="fixed inset-0 z-50 bg-cyan-950/80 backdrop-blur-md flex flex-col items-center justify-center border-4 border-dashed border-cyan-400 p-8 pointer-events-none">
          <Upload className="w-16 h-16 text-cyan-300 animate-bounce mb-4" />
          <h2 className="text-2xl font-bold text-cyan-200">Перетащите файл профиля (.DAT или .DXF)</h2>
          <p className="text-slate-300 mt-2 font-mono">Автоматический парсинг координат и построение 3D геометрии</p>
        </div>
      )}

      {/* Top Main Navigation Bar */}
      <header className="glass-panel border-b border-cyan-500/20 px-4 py-2.5 flex flex-wrap items-center justify-between gap-3 sticky top-0 z-40">
        {/* Brand & Title */}
        <div className="flex items-center gap-3">
          <div className="w-9 h-9 rounded-xl bg-gradient-to-tr from-cyan-600 to-blue-500 flex items-center justify-center shadow-lg shadow-cyan-500/25 ring-1 ring-white/20">
            <Plane className="w-5 h-5 text-white" />
          </div>
          <div>
            <div className="flex items-center gap-2">
              <h1 className="font-bold text-base tracking-wide bg-gradient-to-r from-cyan-300 via-sky-200 to-indigo-300 bg-clip-text text-transparent">
                NASA Glenn FoilSim 3D PRO
              </h1>
              <span className="text-[10px] font-mono px-2 py-0.5 rounded-full bg-cyan-500/15 text-cyan-300 border border-cyan-500/30 font-semibold">
                v2.0 NextGen
              </span>
            </div>
            <p className="text-[10px] text-slate-400 font-mono hidden sm:block">
              Вычислительный аэрогазодинамический комплекс крыла и силовой установки
            </p>
          </div>
        </div>

        {/* Quick Presets & Environment Selection */}
        <div className="flex flex-wrap items-center gap-2">
          {/* Preset Selector */}
          <div className="relative">
            <select
              value={state.profileName}
              onChange={(e) => handleApplyPreset(e.target.value)}
              className="bg-slate-900/90 text-cyan-300 text-xs font-mono rounded-lg px-3 py-1.5 border border-cyan-500/30 focus:outline-none focus:ring-1 focus:ring-cyan-400 cursor-pointer"
            >
              <option value="" disabled>
                -- Выберите пресет --
              </option>
              {PRESETS.map((p) => (
                <option key={p.id} value={p.id} className="bg-slate-900 text-slate-100">
                  {p.name}
                </option>
              ))}
            </select>
          </div>

          {/* Planet Selector */}
          <select
            value={state.planet}
            onChange={(e) => updateState({ planet: e.target.value as PlanetType })}
            className="bg-slate-900/90 text-slate-200 text-xs font-mono rounded-lg px-2.5 py-1.5 border border-slate-700 focus:outline-none focus:border-cyan-400 cursor-pointer"
          >
            <option value="earth">Земля (ISA 0-80 км)</option>
            <option value="mars">Марс (Тонкая CO2)</option>
            <option value="venus">Венера (Плотная 93 атм)</option>
            <option value="titan">Титан (Криогенный метан)</option>
            <option value="water">Вода (Гидродинамика)</option>
          </select>

          {/* Audio Synthesizer Button */}
          <button
            onClick={() => updateState({ audioEnabled: !state.audioEnabled })}
            className={`p-1.5 rounded-lg border text-xs flex items-center gap-1.5 transition ${
              state.audioEnabled
                ? 'bg-cyan-500/20 text-cyan-300 border-cyan-400/50 shadow-md shadow-cyan-500/20'
                : 'bg-slate-900/80 text-slate-400 border-slate-700 hover:text-slate-200'
            }`}
            title="Синтезатор аэродинамического шума трубы и срыва потока (Web Audio API)"
          >
            {state.audioEnabled ? <Volume2 className="w-4 h-4 text-cyan-400" /> : <VolumeX className="w-4 h-4" />}
            <span className="hidden md:inline font-mono text-[11px]">Звук</span>
          </button>

          {/* Import Button */}
          <button
            onClick={() => fileInputRef.current?.click()}
            className="px-2.5 py-1.5 rounded-lg bg-slate-800 hover:bg-slate-700 text-slate-200 text-xs border border-slate-700 flex items-center gap-1.5 transition"
            title="Загрузить профиль (.DAT / .DXF)"
          >
            <Upload className="w-3.5 h-3.5 text-cyan-400" />
            <span className="hidden sm:inline font-mono text-[11px]">Импорт CAD</span>
          </button>
          <input
            ref={fileInputRef}
            type="file"
            accept=".dat,.dxf,.txt"
            onChange={handleFileUpload}
            className="hidden"
          />

          {/* Export Dropdown Menu */}
          <div className="relative group">
            <button className="px-2.5 py-1.5 rounded-lg bg-cyan-600/90 hover:bg-cyan-500 text-white font-medium text-xs flex items-center gap-1.5 transition shadow-lg shadow-cyan-600/20">
              <Download className="w-3.5 h-3.5" />
              <span className="font-mono text-[11px]">Экспорт</span>
              <ChevronDown className="w-3 h-3 opacity-70" />
            </button>
            <div className="absolute right-0 mt-1 w-48 rounded-xl glass-panel border border-cyan-500/30 py-1.5 shadow-2xl hidden group-hover:block z-50 font-mono text-xs">
              <button
                onClick={() => {
                  if (meshExportData.current) {
                    exportOBJ(
                      meshExportData.current.vertices,
                      meshExportData.current.indices,
                      state.profileName
                    );
                  }
                }}
                className="w-full text-left px-3 py-1.5 hover:bg-cyan-500/20 text-slate-200 hover:text-cyan-200 flex items-center justify-between"
              >
                <span>3D Крыло (.OBJ)</span>
                <span className="text-[10px] text-slate-400">CAD/Mesh</span>
              </button>
              <button
                onClick={() => {
                  if (meshExportData.current) {
                    exportSTL(
                      meshExportData.current.vertices,
                      meshExportData.current.indices,
                      state.profileName
                    );
                  }
                }}
                className="w-full text-left px-3 py-1.5 hover:bg-cyan-500/20 text-slate-200 hover:text-cyan-200 flex items-center justify-between"
              >
                <span>3D Печать (.STL)</span>
                <span className="text-[10px] text-slate-400">3D Print</span>
              </button>
              <button
                onClick={() => exportCSV(state, solvePolarPoint)}
                className="w-full text-left px-3 py-1.5 hover:bg-cyan-500/20 text-slate-200 hover:text-cyan-200 flex items-center justify-between"
              >
                <span>Поляра (.CSV)</span>
                <span className="text-[10px] text-slate-400">Таблица</span>
              </button>
              <button
                onClick={() => exportDAT(geom, state.profileName)}
                className="w-full text-left px-3 py-1.5 hover:bg-cyan-500/20 text-slate-200 hover:text-cyan-200 flex items-center justify-between"
              >
                <span>Профиль (.DAT)</span>
                <span className="text-[10px] text-slate-400">Selig</span>
              </button>
              <div className="border-t border-slate-700/50 my-1" />
              <button
                onClick={() => exportStateJSON(state)}
                className="w-full text-left px-3 py-1.5 hover:bg-cyan-500/20 text-slate-200 hover:text-cyan-200 flex items-center justify-between"
              >
                <span>Проект (.JSON)</span>
                <span className="text-[10px] text-slate-400">Конфиг</span>
              </button>
              <button
                onClick={() => jsonImportRef.current?.click()}
                className="w-full text-left px-3 py-1.5 hover:bg-cyan-500/20 text-slate-200 hover:text-cyan-200 flex items-center justify-between"
              >
                <span>Загрузить (.JSON)</span>
                <span className="text-[10px] text-slate-400">Восстановить</span>
              </button>
            </div>
            <input
              ref={jsonImportRef}
              type="file"
              accept=".json"
              onChange={handleJsonImport}
              className="hidden"
            />
          </div>

          {/* Reset State Button */}
          <button
            onClick={handleResetDefaults}
            className="p-1.5 rounded-lg bg-rose-950/40 hover:bg-rose-900/60 text-rose-300 border border-rose-800/40 transition"
            title="Сбросить все параметры"
          >
            <RotateCcw className="w-4 h-4" />
          </button>
        </div>
      </header>

      {/* Real-time KPI Telemetry Bar */}
      <section className="px-4 py-2 bg-[#080d1a] border-b border-cyan-500/10">
        <AeroTelemetry state={state} aero={aero} atmo={atmo} perf={perf} />
      </section>

      {/* Main Multi-Column Interactive Workspace */}
      <main className="flex-1 grid grid-cols-1 lg:grid-cols-12 gap-3 p-3">
        {/* ======================================================== */}
        {/* LEFT COLUMN: Configuration Controls & Inputs (Tabs)       */}
        {/* ======================================================== */}
        <aside className="lg:col-span-3 flex flex-col gap-2.5">
          {/* Controls Tab Switcher */}
          <div className="grid grid-cols-4 gap-1 p-1 rounded-xl glass-panel text-[11px] font-mono">
            <button
              onClick={() => setActiveTab('flow')}
              className={`py-1 px-1.5 rounded-lg transition text-center ${
                activeTab === 'flow'
                  ? 'bg-cyan-500/20 text-cyan-300 font-semibold border border-cyan-500/40'
                  : 'text-slate-400 hover:text-slate-200'
              }`}
            >
              Поток
            </button>
            <button
              onClick={() => setActiveTab('profile')}
              className={`py-1 px-1.5 rounded-lg transition text-center ${
                activeTab === 'profile'
                  ? 'bg-cyan-500/20 text-cyan-300 font-semibold border border-cyan-500/40'
                  : 'text-slate-400 hover:text-slate-200'
              }`}
            >
              Профиль
            </button>
            <button
              onClick={() => setActiveTab('wing3d')}
              className={`py-1 px-1.5 rounded-lg transition text-center ${
                activeTab === 'wing3d'
                  ? 'bg-cyan-500/20 text-cyan-300 font-semibold border border-cyan-500/40'
                  : 'text-slate-400 hover:text-slate-200'
              }`}
            >
              3D Крыло
            </button>
            <button
              onClick={() => setActiveTab('propulsion')}
              className={`py-1 px-1.5 rounded-lg transition text-center ${
                activeTab === 'propulsion'
                  ? 'bg-cyan-500/20 text-cyan-300 font-semibold border border-cyan-500/40'
                  : 'text-slate-400 hover:text-slate-200'
              }`}
            >
              Двигатель
            </button>
          </div>

          {/* TAB 1: FLOW & ATMOSPHERE */}
          {activeTab === 'flow' && (
            <div className="glass-panel rounded-xl p-3 space-y-3.5 text-xs">
              <div className="flex items-center justify-between border-b border-slate-800 pb-2">
                <span className="font-bold text-cyan-400 uppercase tracking-wider flex items-center gap-1.5">
                  <Wind className="w-4 h-4 text-cyan-400" />
                  Поток и Атмосфера
                </span>
                <span
                  className={`text-[10px] font-mono px-2 py-0.5 rounded font-semibold ${
                    aero.stallState === 'stalled'
                      ? 'bg-rose-500/20 text-rose-300 border border-rose-500/30 animate-pulse'
                      : aero.stallState === 'incipient'
                      ? 'bg-amber-500/20 text-amber-300 border border-amber-500/30'
                      : 'bg-emerald-500/20 text-emerald-300 border border-emerald-500/30'
                  }`}
                >
                  {aero.stallState === 'stalled' ? 'Срыв потока!' : aero.stallState === 'incipient' ? 'Предсрывной' : 'Безотрывный'}
                </span>
              </div>

              {/* Angle of attack */}
              <div>
                <div className="flex justify-between mb-1">
                  <span className="text-slate-300">Угол атаки (α)</span>
                  <span className="font-mono text-cyan-300 font-bold">{state.alpha.toFixed(1)}°</span>
                </div>
                <input
                  type="range"
                  min="-15"
                  max="25"
                  step="0.2"
                  value={state.alpha}
                  onChange={(e) => updateState({ alpha: parseFloat(e.target.value) })}
                  className="w-full"
                />
                <div className="flex justify-between text-[10px] text-slate-500 font-mono mt-1">
                  <span>-15°</span>
                  <span
                    onClick={() => updateState({ alpha: 0 })}
                    className="cursor-pointer hover:text-cyan-400"
                  >
                    0°
                  </span>
                  <span
                    onClick={() => updateState({ alpha: 4 })}
                    className="cursor-pointer hover:text-cyan-400 text-cyan-500"
                  >
                    4° (opt)
                  </span>
                  <span
                    onClick={() => updateState({ alpha: 12 })}
                    className="cursor-pointer hover:text-cyan-400"
                  >
                    12°
                  </span>
                  <span>+25°</span>
                </div>
              </div>

              {/* True Airspeed */}
              <div>
                <div className="flex justify-between mb-1">
                  <span className="text-slate-300">Скорость потока (V∞)</span>
                  <span className="font-mono text-cyan-300 font-bold">
                    {state.velocity.toFixed(0)} м/с ({(state.velocity * 3.6).toFixed(0)} км/ч)
                  </span>
                </div>
                <input
                  type="range"
                  min="5"
                  max="650"
                  step="2"
                  value={state.velocity}
                  onChange={(e) => updateState({ velocity: parseFloat(e.target.value) })}
                  className="w-full"
                />
                <div className="flex justify-between text-[10px] text-slate-500 font-mono mt-1">
                  <span>5 м/с</span>
                  <span className="text-slate-400">Mach {aero.Mach.toFixed(2)}</span>
                  <span>650 м/с (M1.9)</span>
                </div>
              </div>

              {/* Flight Altitude */}
              <div>
                <div className="flex justify-between mb-1">
                  <span className="text-slate-300">Высота полета (h)</span>
                  <span className="font-mono text-cyan-300 font-bold">{state.altitude.toFixed(0)} м</span>
                </div>
                <input
                  type="range"
                  min="0"
                  max="25000"
                  step="250"
                  value={state.altitude}
                  onChange={(e) => updateState({ altitude: parseFloat(e.target.value) })}
                  className="w-full"
                />
                <div className="flex justify-between text-[10px] text-slate-500 font-mono mt-1">
                  <span>0 (Уровень моря)</span>
                  <span>11 000 м (Тропопауза)</span>
                  <span>25 км</span>
                </div>
              </div>

              {/* Atmosphere breakdown */}
              <div className="pt-2 border-t border-slate-800 space-y-1 font-mono text-[11px] text-slate-400">
                <div className="flex justify-between">
                  <span>Среда:</span>
                  <span className="text-slate-200 font-semibold">{atmo.name}</span>
                </div>
                <div className="flex justify-between">
                  <span>Плотность ρ:</span>
                  <span className="text-slate-200">{atmo.density.toFixed(4)} кг/м³</span>
                </div>
                <div className="flex justify-between">
                  <span>Давление P:</span>
                  <span className="text-slate-200">{(atmo.pressure / 1000).toFixed(2)} кПа</span>
                </div>
                <div className="flex justify-between">
                  <span>Температура T:</span>
                  <span className="text-slate-200">
                    {atmo.temperature.toFixed(1)} K ({(atmo.temperature - 273.15).toFixed(1)} °C)
                  </span>
                </div>
                <div className="flex justify-between">
                  <span>Скорость звука a:</span>
                  <span className="text-slate-200">{atmo.speedOfSound.toFixed(1)} м/с</span>
                </div>
              </div>
            </div>
          )}

          {/* TAB 2: PROFILE & MECHANIZATION */}
          {activeTab === 'profile' && (
            <div className="glass-panel rounded-xl p-3 space-y-3.5 text-xs">
              <div className="flex items-center justify-between border-b border-slate-800 pb-2">
                <span className="font-bold text-indigo-400 uppercase tracking-wider flex items-center gap-1.5">
                  <Sliders className="w-4 h-4 text-indigo-400" />
                  Профиль и Механизация
                </span>
                <span className="text-[10px] font-mono px-2 py-0.5 rounded bg-indigo-500/15 text-indigo-300 border border-indigo-500/30">
                  {state.profileName}
                </span>
              </div>

              {/* Profile Type Selector */}
              <div>
                <label className="text-slate-300 mb-1 block">Тип семейства профилей</label>
                <select
                  value={state.profileType}
                  onChange={(e) => {
                    const t = e.target.value as AirfoilType;
                    updateState({
                      profileType: t,
                      customUpper: null,
                      customLower: null,
                      profileName: t === 'naca4' ? `NACA ${Math.round(state.camber)}${Math.round(state.camberPos / 10)}${Math.round(state.thickness).toString().padStart(2, '0')}` : t.toUpperCase(),
                    });
                  }}
                  className="w-full bg-slate-900/90 text-slate-200 rounded-lg px-2.5 py-1.5 border border-slate-700 text-xs font-mono"
                >
                  <option value="naca4">NACA 4-значный (Параметрический)</option>
                  <option value="supercritical">Сверхкритический (NASA SC-2)</option>
                  <option value="biconvex">Двояковыпуклый (Сверхзвуковой)</option>
                  <option value="double_wedge">Двойной клин (Сверхзвук/Гиперзвук)</option>
                  <option value="cylinder">Цилиндр Магнуса (Вращающийся ротор)</option>
                  <option value="flat_plate">Тонкая плоская пластина</option>
                  {state.customUpper && <option value="custom">Пользовательский файл (.DAT/.DXF)</option>}
                </select>
              </div>

              {/* Parametric Airfoil controls (when applicable) */}
              {(state.profileType === 'naca4' || state.profileType === 'supercritical') && (
                <div className="space-y-3 pt-1">
                  <div>
                    <div className="flex justify-between mb-1">
                      <span className="text-slate-300">Вогнутость / прогиб (m)</span>
                      <span className="font-mono text-indigo-300 font-bold">{state.camber.toFixed(1)}%</span>
                    </div>
                    <input
                      type="range"
                      min="0"
                      max="9.5"
                      step="0.5"
                      value={state.camber}
                      onChange={(e) => updateState({ camber: parseFloat(e.target.value) })}
                      className="w-full"
                    />
                  </div>

                  <div>
                    <div className="flex justify-between mb-1">
                      <span className="text-slate-300">Позиция макс. вогнутости (p)</span>
                      <span className="font-mono text-indigo-300 font-bold">{state.camberPos.toFixed(0)}%</span>
                    </div>
                    <input
                      type="range"
                      min="10"
                      max="80"
                      step="5"
                      value={state.camberPos}
                      onChange={(e) => updateState({ camberPos: parseFloat(e.target.value) })}
                      className="w-full"
                    />
                  </div>

                  <div>
                    <div className="flex justify-between mb-1">
                      <span className="text-slate-300">Относительная толщина (t/c)</span>
                      <span className="font-mono text-indigo-300 font-bold">{state.thickness.toFixed(1)}%</span>
                    </div>
                    <input
                      type="range"
                      min="3"
                      max="28"
                      step="0.5"
                      value={state.thickness}
                      onChange={(e) => updateState({ thickness: parseFloat(e.target.value) })}
                      className="w-full"
                    />
                  </div>
                </div>
              )}

              {/* Cylinder RPM for Magnus effect */}
              {state.profileType === 'cylinder' && (
                <div>
                  <div className="flex justify-between mb-1">
                    <span className="text-slate-300">Обороты вращения ротора</span>
                    <span className="font-mono text-amber-300 font-bold">{state.cylinderRpm} об/мин</span>
                  </div>
                  <input
                    type="range"
                    min="-3000"
                    max="3000"
                    step="50"
                    value={state.cylinderRpm}
                    onChange={(e) => updateState({ cylinderRpm: parseInt(e.target.value, 10) })}
                    className="w-full"
                  />
                  <div className="flex justify-between text-[10px] text-slate-500 font-mono mt-1">
                    <span>-3000 (вниз)</span>
                    <span>0</span>
                    <span>+3000 (вверх)</span>
                  </div>
                </div>
              )}

              {/* High-lift mechanization (Flaps & Slats) */}
              <div className="pt-2 border-t border-slate-800 space-y-2.5">
                <span className="font-semibold text-amber-400 block text-[11px] uppercase tracking-wider">
                  Механизация крыла
                </span>

                <div className="grid grid-cols-2 gap-2">
                  <div>
                    <label className="text-slate-400 text-[10px] block mb-0.5">Тип закрылка</label>
                    <select
                      value={state.flapType}
                      onChange={(e) => updateState({ flapType: e.target.value as FlapType })}
                      className="w-full bg-slate-900 text-slate-200 rounded px-2 py-1 border border-slate-700 text-xs font-mono"
                    >
                      <option value="plain">Простой</option>
                      <option value="split">Щитковый</option>
                      <option value="slotted">Щелевой</option>
                      <option value="fowler">Фаулера</option>
                    </select>
                  </div>
                  <div>
                    <div className="flex justify-between mb-0.5">
                      <span className="text-slate-400 text-[10px]">Угол δf</span>
                      <span className="font-mono text-amber-300 font-bold">{state.flapAngle.toFixed(0)}°</span>
                    </div>
                    <input
                      type="range"
                      min="-15"
                      max="45"
                      step="1"
                      value={state.flapAngle}
                      onChange={(e) => updateState({ flapAngle: parseFloat(e.target.value) })}
                      className="w-full"
                    />
                  </div>
                </div>

                <div>
                  <div className="flex justify-between mb-0.5">
                    <span className="text-slate-400 text-[10px]">Предкрылок (отклонение δs)</span>
                    <span className="font-mono text-amber-300 font-bold">{state.slatAngle.toFixed(0)}°</span>
                  </div>
                  <input
                    type="range"
                    min="0"
                    max="25"
                    step="1"
                    value={state.slatAngle}
                    onChange={(e) => updateState({ slatAngle: parseFloat(e.target.value) })}
                    className="w-full"
                  />
                </div>
              </div>
            </div>
          )}

          {/* TAB 3: 3D WING PLANFORM */}
          {activeTab === 'wing3d' && (
            <div className="glass-panel rounded-xl p-3 space-y-3 text-xs">
              <div className="flex items-center justify-between border-b border-slate-800 pb-2">
                <span className="font-bold text-emerald-400 uppercase tracking-wider flex items-center gap-1.5">
                  <Layers className="w-4 h-4 text-emerald-400" />
                  3D Геометрия крыла
                </span>
                <span className="text-[10px] font-mono px-2 py-0.5 rounded bg-emerald-500/15 text-emerald-300 border border-emerald-500/30">
                  AR = {aero.aspectRatio.toFixed(2)}
                </span>
              </div>

              {/* Chord and Span */}
              <div className="grid grid-cols-2 gap-2">
                <div>
                  <div className="flex justify-between mb-1">
                    <span className="text-slate-300">Корневая хорда (c)</span>
                    <span className="font-mono text-emerald-300 font-bold">{state.chord.toFixed(1)} м</span>
                  </div>
                  <input
                    type="range"
                    min="0.3"
                    max="6.0"
                    step="0.1"
                    value={state.chord}
                    onChange={(e) => updateState({ chord: parseFloat(e.target.value) })}
                    className="w-full"
                  />
                </div>
                <div>
                  <div className="flex justify-between mb-1">
                    <span className="text-slate-300">Размах крыла (b)</span>
                    <span className="font-mono text-emerald-300 font-bold">{state.span.toFixed(1)} м</span>
                  </div>
                  <input
                    type="range"
                    min="1.5"
                    max="45.0"
                    step="0.5"
                    value={state.span}
                    onChange={(e) => updateState({ span: parseFloat(e.target.value) })}
                    className="w-full"
                  />
                </div>
              </div>

              {/* Sweep and Taper */}
              <div className="grid grid-cols-2 gap-2">
                <div>
                  <div className="flex justify-between mb-1">
                    <span className="text-slate-300">Стреловидность (Λ)</span>
                    <span className="font-mono text-emerald-300 font-bold">{state.sweep.toFixed(0)}°</span>
                  </div>
                  <input
                    type="range"
                    min="0"
                    max="55"
                    step="1"
                    value={state.sweep}
                    onChange={(e) => updateState({ sweep: parseFloat(e.target.value) })}
                    className="w-full"
                  />
                </div>
                <div>
                  <div className="flex justify-between mb-1">
                    <span className="text-slate-300">Сужение крыла (λ)</span>
                    <span className="font-mono text-emerald-300 font-bold">{state.taper.toFixed(2)}</span>
                  </div>
                  <input
                    type="range"
                    min="0.15"
                    max="1.0"
                    step="0.05"
                    value={state.taper}
                    onChange={(e) => updateState({ taper: parseFloat(e.target.value) })}
                    className="w-full"
                  />
                </div>
              </div>

              {/* Twist and Dihedral */}
              <div className="grid grid-cols-2 gap-2">
                <div>
                  <div className="flex justify-between mb-1">
                    <span className="text-slate-300">Крутка / washout (ε)</span>
                    <span className="font-mono text-emerald-300 font-bold">{state.twist.toFixed(1)}°</span>
                  </div>
                  <input
                    type="range"
                    min="-8"
                    max="4"
                    step="0.5"
                    value={state.twist}
                    onChange={(e) => updateState({ twist: parseFloat(e.target.value) })}
                    className="w-full"
                  />
                </div>
                <div>
                  <div className="flex justify-between mb-1">
                    <span className="text-slate-300">V-образность (Γ)</span>
                    <span className="font-mono text-emerald-300 font-bold">{state.dihedral.toFixed(1)}°</span>
                  </div>
                  <input
                    type="range"
                    min="-12"
                    max="12"
                    step="0.5"
                    value={state.dihedral}
                    onChange={(e) => updateState({ dihedral: parseFloat(e.target.value) })}
                    className="w-full"
                  />
                </div>
              </div>

              {/* Winglets Checkbox */}
              <div className="pt-2 border-t border-slate-800 flex items-center justify-between">
                <label className="flex items-center gap-2 cursor-pointer text-slate-300">
                  <input
                    type="checkbox"
                    checked={state.hasWinglets}
                    onChange={(e) => updateState({ hasWinglets: e.target.checked })}
                    className="rounded bg-slate-900 border-slate-700 text-cyan-500 focus:ring-0"
                  />
                  <span>Аэродинамические законцовки (Уинглы)</span>
                </label>
                <span className="text-[10px] font-mono text-slate-400">
                  {state.hasWinglets ? '+5% L/D' : 'Стандарт'}
                </span>
              </div>

              {/* Planform calculations */}
              <div className="p-2 rounded-lg bg-slate-950/70 border border-slate-800 space-y-1 font-mono text-[11px] text-slate-400">
                <div className="flex justify-between">
                  <span>Площадь крыла S:</span>
                  <span className="text-slate-200">{aero.wingArea.toFixed(2)} м²</span>
                </div>
                <div className="flex justify-between">
                  <span>Удлинение AR:</span>
                  <span className="text-slate-200">{aero.aspectRatio.toFixed(2)}</span>
                </div>
                <div className="flex justify-between">
                  <span>Коэфф. Освальда e:</span>
                  <span className="text-slate-200">{aero.oswaldEfficiency.toFixed(3)}</span>
                </div>
              </div>
            </div>
          )}

          {/* TAB 4: PROPULSION & FLIGHT ENVELOPE */}
          {activeTab === 'propulsion' && (
            <div className="glass-panel rounded-xl p-3 space-y-3 text-xs">
              <div className="flex items-center justify-between border-b border-slate-800 pb-2">
                <span className="font-bold text-rose-400 uppercase tracking-wider flex items-center gap-1.5">
                  <Activity className="w-4 h-4 text-rose-400" />
                  Силовая установка и Масса
                </span>
                <span className="text-[10px] font-mono px-2 py-0.5 rounded bg-rose-500/15 text-rose-300 border border-rose-500/30">
                  T/W = {(perf.thrustAvailable / (state.aircraftMass * atmo.gravity)).toFixed(2)}
                </span>
              </div>

              {/* Engine type */}
              <div>
                <label className="text-slate-300 mb-1 block">Тип двигателя</label>
                <select
                  value={state.engineType}
                  onChange={(e) => updateState({ engineType: e.target.value as EngineType })}
                  className="w-full bg-slate-900/90 text-slate-200 rounded-lg px-2.5 py-1.5 border border-slate-700 text-xs font-mono"
                >
                  <option value="turbofan">ТРДД (Турбовентиляторный - Пассажирские лайнеры)</option>
                  <option value="turbojet">ТРД (Турбореактивный - Сверхзвук / Истребители)</option>
                  <option value="piston_prop">Поршневой винтовой (Авиация общего назначения)</option>
                  <option value="electric_prop">Электрический привод (БПЛА / e-Aviation)</option>
                  <option value="rocket">Жидкостный ракетный (ЖРД / Космоплан)</option>
                </select>
              </div>

              {/* Max Thrust */}
              <div>
                <div className="flex justify-between mb-1">
                  <span className="text-slate-300">Статическая тяга (T_max)</span>
                  <span className="font-mono text-rose-300 font-bold">
                    {(state.maxThrust / 1000).toFixed(1)} кН ({state.maxThrust.toFixed(0)} Н)
                  </span>
                </div>
                <input
                  type="range"
                  min="50"
                  max="250000"
                  step="250"
                  value={state.maxThrust}
                  onChange={(e) => updateState({ maxThrust: parseFloat(e.target.value) })}
                  className="w-full"
                />
              </div>

              {/* Aircraft Gross Mass */}
              <div>
                <div className="flex justify-between mb-1">
                  <span className="text-slate-300">Взлетная масса самолета (M)</span>
                  <span className="font-mono text-rose-300 font-bold">
                    {state.aircraftMass >= 1000 ? (state.aircraftMass / 1000).toFixed(2) + ' т' : state.aircraftMass.toFixed(0) + ' кг'}
                  </span>
                </div>
                <input
                  type="range"
                  min="20"
                  max="150000"
                  step="50"
                  value={state.aircraftMass}
                  onChange={(e) => updateState({ aircraftMass: parseFloat(e.target.value) })}
                  className="w-full"
                />
              </div>

              {/* Flight envelope indicators */}
              <div className="p-2.5 rounded-lg bg-slate-950/70 border border-slate-800 space-y-1 font-mono text-[11px] text-slate-300">
                <div className="flex justify-between">
                  <span className="text-slate-400">Скорость сваливания V_stall:</span>
                  <span className="text-amber-400 font-bold">
                    {perf.stallSpeed.toFixed(1)} м/с ({(perf.stallSpeed * 3.6).toFixed(0)} км/ч)
                  </span>
                </div>
                <div className="flex justify-between">
                  <span className="text-slate-400">Наивыгоднейшая V_glide:</span>
                  <span className="text-cyan-400">
                    {perf.minDragSpeed.toFixed(1)} м/с ({(perf.minDragSpeed * 3.6).toFixed(0)} км/ч)
                  </span>
                </div>
                <div className="flex justify-between">
                  <span className="text-slate-400">Макс. качество (L/D)_max:</span>
                  <span className="text-emerald-400 font-bold">{perf.maxGlideRatio.toFixed(1)}</span>
                </div>
                <div className="flex justify-between">
                  <span className="text-slate-400">Скороподъемность V_y:</span>
                  <span className="text-emerald-300">{perf.maxClimbRate.toFixed(1)} м/с</span>
                </div>
                <div className="flex justify-between">
                  <span className="text-slate-400">Практический потолок:</span>
                  <span className="text-slate-200">~{perf.serviceCeiling.toFixed(0)} м</span>
                </div>
                <div className="flex justify-between">
                  <span className="text-slate-400">Тяговооруженность T/W:</span>
                  <span className="text-slate-200">
                    {(perf.thrustAvailable / (state.aircraftMass * atmo.gravity)).toFixed(2)}
                  </span>
                </div>
              </div>
            </div>
          )}

          {/* Visual Toggles Mini-Toolbar */}
          <div className="glass-panel rounded-xl p-2.5 flex flex-wrap gap-2 text-[11px] font-mono">
            <label className="flex items-center gap-1.5 cursor-pointer text-slate-300">
              <input
                type="checkbox"
                checked={state.show3DStreamlines}
                onChange={(e) => updateState({ show3DStreamlines: e.target.checked })}
                className="accent-cyan-400"
              />
              <span>Линии тока</span>
            </label>
            <label className="flex items-center gap-1.5 cursor-pointer text-slate-300">
              <input
                type="checkbox"
                checked={state.show3DVortices}
                onChange={(e) => updateState({ show3DVortices: e.target.checked })}
                className="accent-cyan-400"
              />
              <span>Вихри Прандтля</span>
            </label>
            <label className="flex items-center gap-1.5 cursor-pointer text-slate-300">
              <input
                type="checkbox"
                checked={state.show3DForceVectors}
                onChange={(e) => updateState({ show3DForceVectors: e.target.checked })}
                className="accent-cyan-400"
              />
              <span>Векторы сил</span>
            </label>
            <label className="flex items-center gap-1.5 cursor-pointer text-slate-300">
              <input
                type="checkbox"
                checked={state.show3DPressureMap}
                onChange={(e) => updateState({ show3DPressureMap: e.target.checked })}
                className="accent-cyan-400"
              />
              <span>Эпюра Cp</span>
            </label>
            <label className="flex items-center gap-1.5 cursor-pointer text-slate-300">
              <input
                type="checkbox"
                checked={state.show3DWireframe}
                onChange={(e) => updateState({ show3DWireframe: e.target.checked })}
                className="accent-cyan-400"
              />
              <span>Сетка</span>
            </label>
            <label className="flex items-center gap-1.5 cursor-pointer text-slate-300">
              <input
                type="checkbox"
                checked={state.show3DPitotProbe}
                onChange={(e) => updateState({ show3DPitotProbe: e.target.checked })}
                className="accent-cyan-400"
              />
              <span>Трубка Пито</span>
            </label>
          </div>
        </aside>

        {/* ======================================================== */}
        {/* CENTER COLUMN: 3D Aerodynamic Wind Tunnel               */}
        {/* ======================================================== */}
        <section className="lg:col-span-5 flex flex-col gap-2.5">
          {/* Main 3D Viewport */}
          <div className="flex-1 min-h-[460px] relative rounded-xl overflow-hidden">
            <WindTunnel3D
              state={state}
              geom={geom}
              aero={aero}
              atmo={atmo}
              onProbeMove={(pos) => updateState({ probePosition: pos })}
              onMeshExport={handleMeshExport}
            />
          </div>

          {/* Quick Real-Time Flow Diagnostics Strip */}
          <div className="glass-panel rounded-xl px-3.5 py-2 flex flex-wrap items-center justify-between gap-2 text-[11px] font-mono text-slate-400">
            <span>
              Xcp: <strong className="text-cyan-300 font-bold">{(aero.xcp * 100).toFixed(1)}% c</strong>
            </span>
            <span>
              M_crit: <strong className="text-amber-300 font-bold">{aero.MachCrit.toFixed(2)}</strong>
            </span>
            <span>
              CD0 (профиль): <strong className="text-slate-200">{aero.CD0.toFixed(4)}</strong>
            </span>
            <span>
              CDi (индукт.): <strong className="text-slate-200">{aero.CDi.toFixed(4)}</strong>
            </span>
            <span>
              Волновое CDw: <strong className="text-slate-200">{aero.CDwave.toFixed(4)}</strong>
            </span>
          </div>
        </section>

        {/* ======================================================== */}
        {/* RIGHT COLUMN: 2D Section & Interactive Aero Charts       */}
        {/* ======================================================== */}
        <section className="lg:col-span-4 flex flex-col gap-2.5">
          {/* Top: 2D Airfoil Drawing & Pressure Vectors */}
          <div className="glass-panel rounded-xl p-2.5 h-[210px] flex flex-col">
            <div className="flex items-center justify-between pb-1 mb-1 border-b border-slate-800 text-[11px] font-mono">
              <span className="font-bold text-slate-200 uppercase flex items-center gap-1.5">
                <Eye className="w-3.5 h-3.5 text-cyan-400" />
                2D Сечение и Эпюра Cp
              </span>
              <span className="text-[10px] text-slate-400">
                {geom.upper.length + geom.lower.length} расчетных точек
              </span>
            </div>
            <div className="flex-1 w-full overflow-hidden">
              <WindTunnel2D state={state} geom={geom} aero={aero} />
            </div>
          </div>

          {/* Bottom: Comprehensive Interactive Charts */}
          <div className="flex-1 min-h-[350px]">
            <AeroCharts state={state} aero={aero} solvePolarPoint={solvePolarPoint} />
          </div>
        </section>
      </main>

      {/* Footer / Status bar */}
      <footer className="glass-panel border-t border-cyan-500/10 px-4 py-1.5 flex flex-wrap items-center justify-between text-[11px] text-slate-400 font-mono">
        <div className="flex items-center gap-3">
          <span className="flex items-center gap-1 text-cyan-400">
            <Sparkles className="w-3.5 h-3.5" /> FoilSim Modern Architecture
          </span>
          <span>•</span>
          <span>Модель: Панельный метод Вихревого слоя + Теория Прандтля + Несущая линия</span>
        </div>
        <div className="flex items-center gap-4">
          <span>Re: {(aero.Reynolds / 1e6).toFixed(2)}M</span>
          <span>Mach: {aero.Mach.toFixed(2)}</span>
          <span>q: {aero.dynamicPressure.toFixed(0)} Па</span>
        </div>
      </footer>
    </div>
  );
};

export default App;
