import React, { useEffect, useRef, useState } from 'react';
import {
  Chart as ChartJS,
  CategoryScale,
  LinearScale,
  PointElement,
  LineElement,
  Title,
  Tooltip,
  Legend,
  Filler,
  ChartData,
  ChartOptions,
} from 'chart.js';
import { Line } from 'react-chartjs-2';
import { AeroResults, SimState } from '../types';

ChartJS.register(CategoryScale, LinearScale, PointElement, LineElement, Title, Tooltip, Legend, Filler);

interface AeroChartsProps {
  state: SimState;
  aero: AeroResults;
  solvePolarPoint: (alpha: number) => AeroResults;
}

type ChartTab = 'cp' | 'polar' | 'alpha' | 'spanwise' | 'power';

export const AeroCharts: React.FC<AeroChartsProps> = ({ state, aero, solvePolarPoint }) => {
  const [activeTab, setActiveTab] = useState<ChartTab>('cp');

  // Chart 1: Cp vs x/c Distribution
  const cpData: ChartData<'line'> = {
    datasets: [
      {
        label: 'Верхняя поверхность (Разрежение)',
        data: aero.cpUpper.map((p) => ({ x: p.x, y: p.cp })),
        borderColor: '#38bdf8',
        backgroundColor: 'rgba(56, 189, 248, 0.12)',
        borderWidth: 2,
        pointRadius: 0,
        fill: true,
        tension: 0.1,
      },
      {
        label: 'Нижняя поверхность (Давление)',
        data: aero.cpLower.map((p) => ({ x: p.x, y: p.cp })),
        borderColor: '#f43f5e',
        backgroundColor: 'rgba(244, 63, 94, 0.1)',
        borderWidth: 2,
        pointRadius: 0,
        fill: true,
        tension: 0.1,
      },
    ],
  };

  const cpOptions: ChartOptions<'line'> = {
    responsive: true,
    maintainAspectRatio: false,
    animation: false,
    scales: {
      x: {
        type: 'linear',
        min: 0,
        max: 1.0,
        title: { display: true, text: 'Относительная хорда x / c', color: '#94a3b8' },
        grid: { color: 'rgba(51, 65, 85, 0.4)' },
        ticks: { color: '#94a3b8' },
      },
      y: {
        type: 'linear',
        reverse: true, // Aerospace convention: negative Cp (suction) upwards
        min: -3.8,
        max: 1.2,
        title: { display: true, text: 'Коэффициент давления Cp (↑ разряжение)', color: '#94a3b8' },
        grid: { color: 'rgba(51, 65, 85, 0.4)' },
        ticks: { color: '#94a3b8' },
      },
    },
    plugins: {
      legend: { labels: { color: '#cbd5e1', boxWidth: 12, font: { size: 11 } } },
      tooltip: {
        callbacks: {
          label: (ctx) => `Cp: ${(ctx.raw as { y: number }).y.toFixed(3)} at x/c: ${(ctx.raw as { x: number }).x.toFixed(2)}`,
        },
      },
    },
  };

  // Chart 2: Lilienthal Drag Polar CL vs CD
  const [dragPolarPoints, setDragPolarPoints] = useState<Array<{ x: number; y: number }>>([]);
  const [tangentPoints, setTangentPoints] = useState<Array<{ x: number; y: number }>>([]);

  useEffect(() => {
    const pts: Array<{ x: number; y: number }> = [];
    let maxLD = 0;
    let optPt = { x: 0.02, y: 0.5 };

    for (let a = -12; a <= 22; a += 1) {
      const r = solvePolarPoint(a);
      pts.push({ x: r.CD, y: r.CL });
      if (r.LD > maxLD) {
        maxLD = r.LD;
        optPt = { x: r.CD, y: r.CL };
      }
    }
    setDragPolarPoints(pts);

    // Tangent through origin representing (L/D)max
    setTangentPoints([
      { x: 0, y: 0 },
      { x: optPt.x * 2.2, y: optPt.y * 2.2 },
    ]);
  }, [state, solvePolarPoint]);

  const lilienthalData: ChartData<'line'> = {
    datasets: [
      {
        label: 'Поляра Лилиенталя CL(CD)',
        data: dragPolarPoints,
        borderColor: '#10b981',
        borderWidth: 2.2,
        pointRadius: 0,
        tension: 0.2,
      },
      {
        label: `Касательная (L/D)max = ${aero.LD.toFixed(1)}`,
        data: tangentPoints,
        borderColor: 'rgba(251, 191, 36, 0.6)',
        borderDash: [5, 5],
        borderWidth: 1.5,
        pointRadius: 0,
      },
      {
        label: 'Текущий режим полета',
        data: [{ x: aero.CD, y: aero.CL }],
        backgroundColor: '#facc15',
        borderColor: '#ffffff',
        borderWidth: 2,
        pointRadius: 6,
        showLine: false,
      },
    ],
  };

  const lilienthalOptions: ChartOptions<'line'> = {
    responsive: true,
    maintainAspectRatio: false,
    animation: false,
    scales: {
      x: {
        type: 'linear',
        min: 0,
        title: { display: true, text: 'Коэффициент сопротивления CD', color: '#94a3b8' },
        grid: { color: 'rgba(51, 65, 85, 0.4)' },
        ticks: { color: '#94a3b8' },
      },
      y: {
        type: 'linear',
        title: { display: true, text: 'Коэффициент подъемной силы CL', color: '#94a3b8' },
        grid: { color: 'rgba(51, 65, 85, 0.4)' },
        ticks: { color: '#94a3b8' },
      },
    },
    plugins: {
      legend: { labels: { color: '#cbd5e1', boxWidth: 12, font: { size: 11 } } },
    },
  };

  // Chart 3: CL(alpha) & 5*CD(alpha)
  const [alphaClPoints, setAlphaClPoints] = useState<Array<{ x: number; y: number }>>([]);
  const [alphaCdPoints, setAlphaCdPoints] = useState<Array<{ x: number; y: number }>>([]);

  useEffect(() => {
    const clPts: Array<{ x: number; y: number }> = [];
    const cdPts: Array<{ x: number; y: number }> = [];
    for (let a = -15; a <= 25; a += 1) {
      const r = solvePolarPoint(a);
      clPts.push({ x: a, y: r.CL });
      cdPts.push({ x: a, y: r.CD * 5 });
    }
    setAlphaClPoints(clPts);
    setAlphaCdPoints(cdPts);
  }, [state, solvePolarPoint]);

  const alphaData: ChartData<'line'> = {
    datasets: [
      {
        label: 'CL (α)',
        data: alphaClPoints,
        borderColor: '#38bdf8',
        borderWidth: 2.2,
        pointRadius: 0,
        tension: 0.15,
      },
      {
        label: '5 × CD (α)',
        data: alphaCdPoints,
        borderColor: '#f43f5e',
        borderDash: [4, 4],
        borderWidth: 1.8,
        pointRadius: 0,
        tension: 0.15,
      },
      {
        label: `Текущий α = ${state.alpha.toFixed(1)}°`,
        data: [{ x: state.alpha, y: aero.CL }],
        backgroundColor: '#fbbf24',
        borderColor: '#ffffff',
        borderWidth: 2,
        pointRadius: 6,
        showLine: false,
      },
    ],
  };

  const alphaOptions: ChartOptions<'line'> = {
    responsive: true,
    maintainAspectRatio: false,
    animation: false,
    scales: {
      x: {
        type: 'linear',
        min: -15,
        max: 25,
        title: { display: true, text: 'Угол атаки α (°)', color: '#94a3b8' },
        grid: { color: 'rgba(51, 65, 85, 0.4)' },
        ticks: { color: '#94a3b8' },
      },
      y: {
        type: 'linear',
        min: -1.4,
        max: 2.6,
        title: { display: true, text: 'Коэффициенты CL / 5·CD', color: '#94a3b8' },
        grid: { color: 'rgba(51, 65, 85, 0.4)' },
        ticks: { color: '#94a3b8' },
      },
    },
    plugins: {
      legend: { labels: { color: '#cbd5e1', boxWidth: 12, font: { size: 11 } } },
    },
  };

  // Chart 4: Spanwise Lift & Circulation Distribution
  const spanwiseData: ChartData<'line'> = {
    datasets: [
      {
        label: 'Распределение циркуляции Γ(y)',
        data: aero.spanwiseY.map((y, i) => ({ x: y, y: aero.spanwiseCirculation[i] })),
        borderColor: '#a855f7',
        backgroundColor: 'rgba(168, 85, 247, 0.15)',
        borderWidth: 2.2,
        pointRadius: 0,
        fill: true,
        tension: 0.2,
      },
      {
        label: 'Эллиптический идеал',
        data: aero.spanwiseY.map((y) => {
          const halfB = state.span * 0.5;
          const eta = Math.abs(y / halfB);
          const circ = aero.CL * 0.9 * Math.sqrt(Math.max(0, 1 - eta * eta));
          return { x: y, y: circ };
        }),
        borderColor: 'rgba(148, 163, 184, 0.5)',
        borderDash: [4, 4],
        borderWidth: 1.5,
        pointRadius: 0,
      },
    ],
  };

  const spanwiseOptions: ChartOptions<'line'> = {
    responsive: true,
    maintainAspectRatio: false,
    animation: false,
    scales: {
      x: {
        type: 'linear',
        title: { display: true, text: 'Размах крыла y (м)', color: '#94a3b8' },
        grid: { color: 'rgba(51, 65, 85, 0.4)' },
        ticks: { color: '#94a3b8' },
      },
      y: {
        type: 'linear',
        title: { display: true, text: 'Циркуляция Γ (м²/с)', color: '#94a3b8' },
        grid: { color: 'rgba(51, 65, 85, 0.4)' },
        ticks: { color: '#94a3b8' },
      },
    },
    plugins: {
      legend: { labels: { color: '#cbd5e1', boxWidth: 12, font: { size: 11 } } },
    },
  };

  return (
    <div className="w-full h-full min-h-[340px] flex flex-col bg-[#060a14] rounded-xl p-3 border border-cyan-500/20 shadow-xl">
      {/* Tabs Header */}
      <div className="flex items-center justify-between pb-2 mb-2 border-b border-slate-800">
        <div className="flex items-center gap-1">
          <button
            onClick={() => setActiveTab('cp')}
            className={`px-3 py-1 rounded text-[11px] font-mono transition border ${
              activeTab === 'cp' ? 'tab-active font-semibold' : 'text-slate-400 border-transparent hover:text-slate-200'
            }`}
          >
            Эпюра давления Cp(x/c)
          </button>
          <button
            onClick={() => setActiveTab('polar')}
            className={`px-3 py-1 rounded text-[11px] font-mono transition border ${
              activeTab === 'polar' ? 'tab-active font-semibold' : 'text-slate-400 border-transparent hover:text-slate-200'
            }`}
          >
            Поляра Лилиенталя CL-CD
          </button>
          <button
            onClick={() => setActiveTab('alpha')}
            className={`px-3 py-1 rounded text-[11px] font-mono transition border ${
              activeTab === 'alpha' ? 'tab-active font-semibold' : 'text-slate-400 border-transparent hover:text-slate-200'
            }`}
          >
            Кривые CL(α) и CD(α)
          </button>
          <button
            onClick={() => setActiveTab('spanwise')}
            className={`px-3 py-1 rounded text-[11px] font-mono transition border ${
              activeTab === 'spanwise' ? 'tab-active font-semibold' : 'text-slate-400 border-transparent hover:text-slate-200'
            }`}
          >
            Нагрузка по размаху Γ(y)
          </button>
        </div>
      </div>

      {/* Chart Canvas Area */}
      <div className="flex-1 w-full relative min-h-[260px]">
        {activeTab === 'cp' && <Line data={cpData} options={cpOptions} />}
        {activeTab === 'polar' && <Line data={lilienthalData} options={lilienthalOptions} />}
        {activeTab === 'alpha' && <Line data={alphaData} options={alphaOptions} />}
        {activeTab === 'spanwise' && <Line data={spanwiseData} options={spanwiseOptions} />}
      </div>
    </div>
  );
};
