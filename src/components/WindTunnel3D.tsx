import React, { useEffect, useRef, useState } from 'react';
import * as THREE from 'three';
import { OrbitControls } from 'three/addons/controls/OrbitControls.js';
import { Camera, Compass, Eye, Maximize2, Move, RefreshCw, Zap } from 'lucide-react';
import { AeroResults, AirfoilGeometry, AtmosphereData, PitotProbeData, SimState } from '../types';
import { getPressureColor } from '../utils/colormap';
import { samplePitotProbe } from '../physics/pitotProbe';

interface WindTunnel3DProps {
  state: SimState;
  geom: AirfoilGeometry;
  aero: AeroResults;
  atmo: AtmosphereData;
  onProbeMove?: (pos: { x: number; y: number; z: number }) => void;
  onMeshExport?: (vertices: Float32Array, indices: Uint16Array | Uint32Array) => void;
}

export const WindTunnel3D: React.FC<WindTunnel3DProps> = ({
  state,
  geom,
  aero,
  atmo,
  onProbeMove,
  onMeshExport,
}) => {
  const containerRef = useRef<HTMLDivElement>(null);
  const rendererRef = useRef<THREE.WebGLRenderer | null>(null);
  const sceneRef = useRef<THREE.Scene | null>(null);
  const cameraRef = useRef<THREE.PerspectiveCamera | null>(null);
  const controlsRef = useRef<OrbitControls | null>(null);

  // Scene objects
  const wingGroupRef = useRef<THREE.Group | null>(null);
  const wingMeshRef = useRef<THREE.Mesh | null>(null);
  const wireMeshRef = useRef<THREE.LineSegments | null>(null);
  const vectorsGroupRef = useRef<THREE.Group | null>(null);
  const liftArrowRef = useRef<THREE.ArrowHelper | null>(null);
  const dragArrowRef = useRef<THREE.ArrowHelper | null>(null);
  const resultArrowRef = useRef<THREE.ArrowHelper | null>(null);
  const streamGroupRef = useRef<THREE.Group | null>(null);
  const vortexGroupRef = useRef<THREE.Group | null>(null);
  const probeMeshRef = useRef<THREE.Group | null>(null);
  const shockConeRef = useRef<THREE.Mesh | null>(null);

  // Dynamic particle & streamline data
  const particlesGeoRef = useRef<THREE.BufferGeometry | null>(null);
  const streamLinesDataRef = useRef<Array<{ line: THREE.Line; y0: number; z0: number }>>([]);
  const vortexMeshesRef = useRef<Array<{ line: THREE.Line; side: number }>>([]);

  // Telemetry of local Pitot probe
  const [probeData, setProbeData] = useState<PitotProbeData | null>(null);
  const [isProbeActive, setIsProbeActive] = useState<boolean>(state.show3DPitotProbe);

  // Initialize Three.js Scene
  useEffect(() => {
    if (!containerRef.current) return;
    const width = containerRef.current.clientWidth;
    const height = containerRef.current.clientHeight;

    const scene = new THREE.Scene();
    scene.background = new THREE.Color(0x050813);
    sceneRef.current = scene;

    const camera = new THREE.PerspectiveCamera(40, width / height, 0.1, 150);
    camera.position.set(3.2, 2.0, 4.6);
    cameraRef.current = camera;

    const renderer = new THREE.WebGLRenderer({ antialias: true, alpha: false, powerPreference: 'high-performance' });
    renderer.setSize(width, height);
    renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
    renderer.shadowMap.enabled = true;
    containerRef.current.innerHTML = '';
    containerRef.current.appendChild(renderer.domElement);
    rendererRef.current = renderer;

    const controls = new OrbitControls(camera, renderer.domElement);
    controls.enableDamping = true;
    controls.dampingFactor = 0.06;
    controls.maxDistance = 30;
    controls.minDistance = 0.5;
    controlsRef.current = controls;

    // Wind Tunnel Lighting
    const ambLight = new THREE.AmbientLight(0xffffff, 0.85);
    scene.add(ambLight);

    const dirLight1 = new THREE.DirectionalLight(0xffffff, 1.4);
    dirLight1.position.set(6, 10, 7);
    scene.add(dirLight1);

    const dirLight2 = new THREE.DirectionalLight(0x38bdf8, 0.8);
    dirLight2.position.set(-6, -4, -6);
    scene.add(dirLight2);

    const topCyanLight = new THREE.DirectionalLight(0x06b6d4, 0.6);
    topCyanLight.position.set(0, 8, 0);
    scene.add(topCyanLight);

    // Wind tunnel floor test grid
    const grid = new THREE.GridHelper(20, 40, 0x1e293b, 0x090f1d);
    grid.position.y = -1.6;
    scene.add(grid);

    // Tunnel boundary outline box (transparent acrylic wind tunnel walls)
    const boxGeo = new THREE.BoxGeometry(7.5, 3.2, 5.8);
    const boxEdges = new THREE.LineSegments(
      new THREE.EdgesGeometry(boxGeo),
      new THREE.LineBasicMaterial({ color: 0x1e293b, transparent: true, opacity: 0.35 })
    );
    boxEdges.position.y = 0;
    scene.add(boxEdges);

    // Groups
    const wingGroup = new THREE.Group();
    scene.add(wingGroup);
    wingGroupRef.current = wingGroup;

    const vecGroup = new THREE.Group();
    scene.add(vecGroup);
    vectorsGroupRef.current = vecGroup;

    // Lift & Drag arrows
    const liftArrow = new THREE.ArrowHelper(new THREE.Vector3(0, 1, 0), new THREE.Vector3(0, 0, 0), 1, 0x38bdf8, 0.22, 0.12);
    const dragArrow = new THREE.ArrowHelper(new THREE.Vector3(1, 0, 0), new THREE.Vector3(0, 0, 0), 0.6, 0xf43f5e, 0.18, 0.1);
    const resultArrow = new THREE.ArrowHelper(new THREE.Vector3(0.5, 1, 0), new THREE.Vector3(0, 0, 0), 1.2, 0xfbbf24, 0.2, 0.11);
    vecGroup.add(liftArrow, dragArrow, resultArrow);
    liftArrowRef.current = liftArrow;
    dragArrowRef.current = dragArrow;
    resultArrowRef.current = resultArrow;

    // Streamlines & Particles
    const streamGroup = new THREE.Group();
    scene.add(streamGroup);
    streamGroupRef.current = streamGroup;

    const numStreamers = 24;
    const ptsPerStreamer = 70;
    const linesData: Array<{ line: THREE.Line; y0: number; z0: number }> = [];

    for (let s = 0; s < numStreamers; s++) {
      const g = new THREE.BufferGeometry();
      g.setAttribute('position', new THREE.BufferAttribute(new Float32Array(ptsPerStreamer * 3), 3));
      const mat = new THREE.LineBasicMaterial({
        color: 0x38bdf8,
        transparent: true,
        opacity: 0.38,
      });
      const line = new THREE.Line(g, mat);
      streamGroup.add(line);

      // Distribute streamers across span and vertical height
      const zFrac = ((s % 6) / 5 - 0.5) * 2.8;
      const yFrac = (Math.floor(s / 6) / 3 - 0.5) * 1.4;
      linesData.push({ line, y0: yFrac, z0: zFrac });
    }
    streamLinesDataRef.current = linesData;

    // Smoke particles flowing on streamers
    const totalParticles = numStreamers * 4;
    const partPositions = new Float32Array(totalParticles * 3);
    const pGeo = new THREE.BufferGeometry();
    pGeo.setAttribute('position', new THREE.BufferAttribute(partPositions, 3));
    particlesGeoRef.current = pGeo;

    const pMat = new THREE.PointsMaterial({
      color: 0x7dd3fc,
      size: 0.055,
      transparent: true,
      opacity: 0.85,
    });
    const pPoints = new THREE.Points(pGeo, pMat);
    streamGroup.add(pPoints);

    // Prandtl Wingtip Vortices
    const vortexGroup = new THREE.Group();
    scene.add(vortexGroup);
    vortexGroupRef.current = vortexGroup;

    const vMeshes = [-1, 1].map((side) => {
      const g = new THREE.BufferGeometry();
      g.setAttribute('position', new THREE.BufferAttribute(new Float32Array(100 * 3), 3));
      const l = new THREE.Line(
        g,
        new THREE.LineBasicMaterial({ color: 0xa855f7, transparent: true, opacity: 0.72, linewidth: 2 })
      );
      vortexGroup.add(l);
      return { line: l, side };
    });
    vortexMeshesRef.current = vMeshes;

    // Pitot-Static Probe Model
    const probeGrp = new THREE.Group();
    const probeStem = new THREE.Mesh(
      new THREE.CylinderGeometry(0.015, 0.015, 0.45, 12),
      new THREE.MeshStandardMaterial({ color: 0xe2e8f0, metalness: 0.8, roughness: 0.2 })
    );
    probeStem.rotation.z = Math.PI / 2;
    const probeTip = new THREE.Mesh(
      new THREE.ConeGeometry(0.02, 0.08, 12),
      new THREE.MeshStandardMaterial({ color: 0x38bdf8, emissive: 0x0284c7, emissiveIntensity: 0.5 })
    );
    probeTip.rotation.z = -Math.PI / 2;
    probeTip.position.x = -0.26;
    probeGrp.add(probeStem, probeTip);
    scene.add(probeGrp);
    probeMeshRef.current = probeGrp;

    // Supersonic Schlieren Shock Cone
    const coneGeo = new THREE.ConeGeometry(1.4, 3.2, 32, 1, true);
    const coneMat = new THREE.MeshBasicMaterial({
      color: 0x38bdf8,
      transparent: true,
      opacity: 0.18,
      side: THREE.DoubleSide,
      wireframe: true,
    });
    const shockMesh = new THREE.Mesh(coneGeo, coneMat);
    shockMesh.rotation.z = -Math.PI / 2;
    shockMesh.position.x = 0.5;
    shockMesh.visible = false;
    scene.add(shockMesh);
    shockConeRef.current = shockMesh;

    // Resize handler
    const handleResize = () => {
      if (!containerRef.current || !rendererRef.current || !cameraRef.current) return;
      const w = containerRef.current.clientWidth;
      const h = containerRef.current.clientHeight;
      cameraRef.current.aspect = w / h;
      cameraRef.current.updateProjectionMatrix();
      rendererRef.current.setSize(w, h);
    };
    window.addEventListener('resize', handleResize);

    // Animation Loop
    let animId: number;
    const t0 = performance.now();

    const animate = (time: number) => {
      animId = requestAnimationFrame(animate);
      const elapsedSec = (time - t0) * 0.001;

      // Update flow streamlines and particles
      updateFlowAnimation(elapsedSec);

      controls.update();
      renderer.render(scene, camera);
    };
    animId = requestAnimationFrame(animate);

    return () => {
      window.removeEventListener('resize', handleResize);
      cancelAnimationFrame(animId);
      renderer.dispose();
    };
  }, []);

  // Rebuild 3D Wing Geometry when parameters change
  useEffect(() => {
    if (!wingGroupRef.current || !sceneRef.current) return;

    // Remove old meshes
    if (wingMeshRef.current) {
      wingGroupRef.current.remove(wingMeshRef.current);
      wingMeshRef.current.geometry.dispose();
    }
    if (wireMeshRef.current) {
      wingGroupRef.current.remove(wireMeshRef.current);
      wireMeshRef.current.geometry.dispose();
    }

    const upper = geom.upper;
    const lower = geom.lower;
    const n = upper.length;
    if (n < 5) return;

    // Build closed 2D contour from TE over upper to LE and back along lower
    const contour: Array<{ x: number; y: number; cp: number }> = [];
    for (let i = n - 1; i >= 0; i--) {
      const cpVal = aero.cpUpper[i]?.cp ?? 0;
      contour.push({ x: upper[i].x - 0.25, y: upper[i].y, cp: cpVal });
    }
    for (let i = 1; i < lower.length; i++) {
      const cpVal = aero.cpLower[i]?.cp ?? 0;
      contour.push({ x: lower[i].x - 0.25, y: lower[i].y, cp: cpVal });
    }

    const cLen = contour.length;
    const spanSegments = 32;

    const vRootChord = 1.7;
    const vSpan = Math.min(5.2, Math.max(1.8, (state.span / state.chord) * 0.5));
    const sweepTan = Math.tan((state.sweep * Math.PI) / 180);
    const twistRad = (state.twist * Math.PI) / 180;
    const dihedralTan = Math.tan((state.dihedral * Math.PI) / 180);

    const positions: number[] = [];
    const colors: number[] = [];
    const indices: number[] = [];

    for (let s = 0; s <= spanSegments; s++) {
      const zFrac = s / spanSegments - 0.5; // -0.5 to +0.5
      const absFrac = Math.abs(zFrac) * 2.0; // 0 at root, 1.0 at tip

      // Planform scaling and offsets
      const localChordScale = vRootChord * (1 - absFrac * (1 - state.taper));
      const sweepOffset = absFrac * (vSpan * 0.5) * sweepTan;
      const dihedralOffset = absFrac * (vSpan * 0.5) * dihedralTan;

      // Washout twist around quarter chord (0, 0)
      const localTwist = -absFrac * twistRad;
      const cosT = Math.cos(localTwist);
      const sinT = Math.sin(localTwist);

      // Winglet tip curve
      let wingletY = 0;
      let wingletZ = zFrac * vSpan;
      if (state.hasWinglets && absFrac > 0.88) {
        const wFrac = (absFrac - 0.88) / 0.12;
        wingletY = Math.pow(wFrac, 1.8) * 0.28;
      }

      // Spanwise circulation elliptical dropoff factor for Cp
      const tipLoad = Math.sqrt(Math.max(0.12, 1 - 0.88 * absFrac * absFrac));

      for (let c = 0; c < cLen; c++) {
        const lx = contour[c].x * localChordScale;
        const ly = contour[c].y * localChordScale;

        // Apply twist rotation
        const rx = lx * cosT - ly * sinT + sweepOffset;
        const ry = lx * sinT + ly * cosT + dihedralOffset + wingletY;
        const rz = wingletZ;

        positions.push(rx, ry, rz);

        // Pressure false color
        if (state.show3DPressureMap) {
          const cpMod = contour[c].cp * tipLoad;
          const col = getPressureColor(cpMod, state.colormap);
          colors.push(col.r, col.g, col.b);
        } else {
          // Sleek aerospace titanium metallic tone
          colors.push(0.75, 0.82, 0.92);
        }
      }
    }

    // Connect quads into triangles
    for (let s = 0; s < spanSegments; s++) {
      for (let c = 0; c < cLen - 1; c++) {
        const a = s * cLen + c;
        const b = (s + 1) * cLen + c;
        const d = (s + 1) * cLen + c + 1;
        const e = s * cLen + c + 1;
        indices.push(a, b, d, a, d, e);
      }
    }

    const posArray = new Float32Array(positions);
    const colArray = new Float32Array(colors);
    const idxArray = new Uint16Array(indices);

    const geo3D = new THREE.BufferGeometry();
    geo3D.setAttribute('position', new THREE.BufferAttribute(posArray, 3));
    geo3D.setAttribute('color', new THREE.BufferAttribute(colArray, 3));
    geo3D.setIndex(new THREE.BufferAttribute(idxArray, 1));
    geo3D.computeVertexNormals();

    const wingMesh = new THREE.Mesh(
      geo3D,
      new THREE.MeshStandardMaterial({
        vertexColors: true,
        roughness: 0.28,
        metalness: 0.35,
        side: THREE.DoubleSide,
      })
    );
    wingGroupRef.current.add(wingMesh);
    wingMeshRef.current = wingMesh;

    // Wireframe overlay
    const wireGeo = new THREE.WireframeGeometry(geo3D);
    const wireMesh = new THREE.LineSegments(
      wireGeo,
      new THREE.LineBasicMaterial({
        color: 0x94a3b8,
        transparent: true,
        opacity: 0.22,
      })
    );
    wireMesh.visible = state.show3DWireframe;
    wingGroupRef.current.add(wireMesh);
    wireMeshRef.current = wireMesh;

    // Wing angle of attack pitch rotation
    wingGroupRef.current.rotation.z = -(state.alpha * Math.PI) / 180;

    // Notify export listener if active
    if (onMeshExport) {
      onMeshExport(posArray, idxArray);
    }
  }, [geom, state, aero]);

  // Update Force Vectors & Shock Wave Cone
  useEffect(() => {
    if (!vectorsGroupRef.current || !liftArrowRef.current || !dragArrowRef.current || !resultArrowRef.current) return;

    const vRootChord = 1.7;
    vectorsGroupRef.current.visible = state.show3DForceVectors;
    vectorsGroupRef.current.position.set((aero.xcp - 0.25) * vRootChord, 0, 0);

    // Lift Arrow (cyan)
    const liftDir = new THREE.Vector3(0, aero.CL >= 0 ? 1 : -1, 0);
    const liftLen = Math.min(2.2, Math.max(0.12, Math.abs(aero.CL) * 0.85));
    liftArrowRef.current.setDirection(liftDir);
    liftArrowRef.current.setLength(liftLen, 0.22, 0.12);

    // Drag Arrow (rose)
    const dragDir = new THREE.Vector3(1, 0, 0);
    const dragLen = Math.min(1.8, Math.max(0.1, aero.CD * 4.5));
    dragArrowRef.current.setDirection(dragDir);
    dragArrowRef.current.setLength(dragLen, 0.18, 0.1);

    // Resultant force arrow (amber)
    const resVec = new THREE.Vector3(dragLen, aero.CL >= 0 ? liftLen : -liftLen, 0);
    const resDir = resVec.clone().normalize();
    resultArrowRef.current.setDirection(resDir);
    resultArrowRef.current.setLength(resVec.length(), 0.2, 0.12);

    // Supersonic Schlieren shock cone
    if (shockConeRef.current) {
      if (aero.Mach >= 1.0 && state.showShockWaves) {
        shockConeRef.current.visible = true;
        // Mach cone angle: sin(mu) = 1 / M
        const machAngle = Math.asin(1 / aero.Mach);
        const radius = Math.tan(machAngle) * 3.2;
        shockConeRef.current.scale.set(radius, 1, radius);
      } else {
        shockConeRef.current.visible = false;
      }
    }
  }, [aero, state.show3DForceVectors, state.showShockWaves]);

  // Update Pitot Probe position & sample local flow
  useEffect(() => {
    if (probeMeshRef.current) {
      probeMeshRef.current.visible = state.show3DPitotProbe;
      probeMeshRef.current.position.set(state.probePosition.x, state.probePosition.y, state.probePosition.z);
    }

    if (state.show3DPitotProbe) {
      const sampled = samplePitotProbe(state.probePosition, state, atmo, aero.CL, aero.stallFactor);
      setProbeData(sampled);
    } else {
      setProbeData(null);
    }
  }, [state.probePosition, state.show3DPitotProbe, state, atmo, aero]);

  // Flow Animation (Streamlines, Particles & Vortices)
  const updateFlowAnimation = (tSec: number) => {
    if (!streamGroupRef.current || !vortexGroupRef.current) return;
    streamGroupRef.current.visible = state.show3DStreamlines;
    vortexGroupRef.current.visible = state.show3DVortices;

    const gamma = aero.CL * 0.28;
    const stall = aero.stallFactor;
    const ptsPerStreamer = 70;

    // Update Streamlines
    if (state.show3DStreamlines) {
      const pArr = particlesGeoRef.current?.attributes.position.array as Float32Array | undefined;
      let pIdx = 0;

      streamLinesDataRef.current.forEach((item) => {
        const arr = item.line.geometry.attributes.position.array as Float32Array;
        for (let i = 0; i < ptsPerStreamer; i++) {
          const frac = i / (ptsPerStreamer - 1);
          const x = -3.2 + frac * 6.5;

          // Flow deflection around wing (bound vortex downwash)
          let dy = (gamma * Math.exp(-Math.abs(x) * 1.1)) / Math.sqrt(x * x + item.y0 * item.y0 + 0.14);
          if (x > 0.2) dy -= gamma * 0.38 * (x - 0.2);

          // Boundary layer detachment and turbulent eddies during stall
          if (stall > 0.15 && x > -0.1 && item.y0 > -0.05 && item.y0 < 0.6) {
            const eddy = Math.sin(x * 9.5 - tSec * 14 + item.z0 * 3) * stall * 0.22;
            dy += eddy;
          }

          arr[i * 3] = x;
          arr[i * 3 + 1] = item.y0 + dy;
          arr[i * 3 + 2] = item.z0;
        }
        item.line.geometry.attributes.position.needsUpdate = true;

        // Flow particles
        if (pArr) {
          for (let k = 0; k < 4; k++) {
            const speed = 0.4 + state.velocity / 120;
            const pFrac = (tSec * speed + k / 4 + Math.abs(item.z0) * 0.1) % 1;
            const xp = -3.2 + pFrac * 6.5;
            let dy = (gamma * Math.exp(-Math.abs(xp) * 1.1)) / Math.sqrt(xp * xp + item.y0 * item.y0 + 0.14);
            if (xp > 0.2) dy -= gamma * 0.38 * (xp - 0.2);
            if (stall > 0.15 && xp > -0.1 && item.y0 > -0.05 && item.y0 < 0.6) {
              dy += Math.sin(xp * 9.5 - tSec * 14 + item.z0 * 3) * stall * 0.22;
            }

            pArr[pIdx++] = xp;
            pArr[pIdx++] = item.y0 + dy;
            pArr[pIdx++] = item.z0;
          }
        }
      });

      if (particlesGeoRef.current) {
        particlesGeoRef.current.attributes.position.needsUpdate = true;
      }
    }

    // Update Prandtl Wingtip Vortices
    if (state.show3DVortices) {
      const vSpan = Math.min(5.2, Math.max(1.8, (state.span / state.chord) * 0.5));
      const sweepTan = Math.tan((state.sweep * Math.PI) / 180);
      const tipX = (vSpan * 0.5) * sweepTan + 0.4;
      const vortAmp = Math.min(0.32, Math.abs(aero.CL) * 0.13 + 0.03);

      vortexMeshesRef.current.forEach((vm) => {
        const arr = vm.line.geometry.attributes.position.array as Float32Array;
        const zTip = vm.side * (vSpan * 0.5);
        for (let i = 0; i < 100; i++) {
          const s = i / 99;
          const x = tipX + s * 3.4;
          const radius = vortAmp * Math.sqrt(s);
          const angle = s * 18.0 - tSec * 11.0 * Math.sign(aero.CL || 1) * vm.side;

          arr[i * 3] = x;
          arr[i * 3 + 1] = -s * gamma * 0.55 + Math.sin(angle) * radius;
          arr[i * 3 + 2] = zTip + Math.cos(angle) * radius;
        }
        vm.line.geometry.attributes.position.needsUpdate = true;
      });
    }
  };

  // Camera presets
  const setCameraView = (view: 'iso' | 'top' | 'side' | 'front' | 'wingtip') => {
    if (!cameraRef.current || !controlsRef.current) return;
    const c = cameraRef.current;
    const ctrl = controlsRef.current;

    switch (view) {
      case 'top':
        c.position.set(0.2, 6.5, 0.01);
        ctrl.target.set(0.2, 0, 0);
        break;
      case 'side':
        c.position.set(0, 0, 5.5);
        ctrl.target.set(0, 0, 0);
        break;
      case 'front':
        c.position.set(-6.0, 0, 0);
        ctrl.target.set(0, 0, 0);
        break;
      case 'wingtip':
        c.position.set(1.5, 0.5, 3.2);
        ctrl.target.set(0.5, 0, 1.8);
        break;
      case 'iso':
      default:
        c.position.set(3.2, 2.0, 4.6);
        ctrl.target.set(0, 0, 0);
        break;
    }
    ctrl.update();
  };

  return (
    <div className="relative w-full h-full min-h-[460px] bg-[#050813] rounded-xl overflow-hidden border border-cyan-500/20 shadow-2xl flex flex-col">
      {/* 3D Canvas Mount */}
      <div ref={containerRef} className="w-full flex-1 cursor-grab active:cursor-grabbing" />

      {/* Floating Camera Toolbar */}
      <div className="absolute top-3 left-3 flex flex-wrap items-center gap-1.5 p-1.5 rounded-lg glass-panel z-10">
        <button
          onClick={() => setCameraView('iso')}
          className="px-2.5 py-1 rounded text-[11px] font-medium bg-slate-800/80 hover:bg-cyan-900/40 text-cyan-300 hover:text-cyan-200 border border-cyan-500/30 flex items-center gap-1 transition"
          title="Изометрический вид"
        >
          <Compass className="w-3.5 h-3.5" /> 3D Изометрия
        </button>
        <button
          onClick={() => setCameraView('side')}
          className="px-2.5 py-1 rounded text-[11px] font-medium bg-slate-800/80 hover:bg-cyan-900/40 text-slate-300 hover:text-cyan-200 border border-slate-700/50 flex items-center gap-1 transition"
          title="Вид сбоку (Угол атаки)"
        >
          <Eye className="w-3.5 h-3.5" /> Профиль (сбоку)
        </button>
        <button
          onClick={() => setCameraView('top')}
          className="px-2.5 py-1 rounded text-[11px] font-medium bg-slate-800/80 hover:bg-cyan-900/40 text-slate-300 hover:text-cyan-200 border border-slate-700/50 flex items-center gap-1 transition"
          title="Вид сверху (Стреловидность и Сужение)"
        >
          План (сверху)
        </button>
        <button
          onClick={() => setCameraView('front')}
          className="px-2.5 py-1 rounded text-[11px] font-medium bg-slate-800/80 hover:bg-cyan-900/40 text-slate-300 hover:text-cyan-200 border border-slate-700/50 flex items-center gap-1 transition"
          title="Вид спереди (Лобовое сопротивление)"
        >
          Спереди
        </button>
        <button
          onClick={() => setCameraView('wingtip')}
          className="px-2.5 py-1 rounded text-[11px] font-medium bg-slate-800/80 hover:bg-purple-900/40 text-purple-300 hover:text-purple-200 border border-purple-500/30 flex items-center gap-1 transition"
          title="Законцовка крыла (Вихри Прандтля)"
        >
          <Zap className="w-3.5 h-3.5" /> Законцовка
        </button>
      </div>

      {/* Interactive Pitot Probe Telemetry Card */}
      {state.show3DPitotProbe && probeData && (
        <div className="absolute bottom-3 left-3 p-3 rounded-lg glass-panel z-10 w-72 border border-cyan-500/30 font-mono text-[11px] shadow-xl">
          <div className="flex items-center justify-between pb-1.5 mb-2 border-b border-cyan-500/20 text-cyan-400 font-semibold">
            <span className="flex items-center gap-1.5">
              <Move className="w-3.5 h-3.5 text-cyan-400 animate-pulse" />
              Трубка Пито-Прандтля 3D
            </span>
            <span className="text-[10px] text-slate-400">
              ({state.probePosition.x.toFixed(1)}, {state.probePosition.y.toFixed(1)}, {state.probePosition.z.toFixed(1)})
            </span>
          </div>

          <div className="space-y-1 text-slate-300">
            <div className="flex justify-between">
              <span className="text-slate-400">Локальная скор. V_loc:</span>
              <span className="font-bold text-cyan-300">{probeData.localVelocity.toFixed(1)} м/с</span>
            </div>
            <div className="flex justify-between">
              <span className="text-slate-400">Локальный Мах M_loc:</span>
              <span className="font-bold text-amber-300">{probeData.localMach.toFixed(2)}</span>
            </div>
            <div className="flex justify-between">
              <span className="text-slate-400">Статическое давл. P_s:</span>
              <span className="text-slate-200">{(probeData.staticPressure / 1000).toFixed(2)} кПа</span>
            </div>
            <div className="flex justify-between">
              <span className="text-slate-400">Полное давл. P_tot:</span>
              <span className="font-bold text-emerald-400">{(probeData.totalPressure / 1000).toFixed(2)} кПа</span>
            </div>
            <div className="flex justify-between">
              <span className="text-slate-400">Скоростной напор q:</span>
              <span className="text-cyan-300">{probeData.dynamicPressure.toFixed(0)} Па</span>
            </div>
          </div>

          {/* Quick coordinate nudgers */}
          {onProbeMove && (
            <div className="mt-2.5 pt-2 border-t border-slate-700/50 flex items-center justify-between text-[10px]">
              <span className="text-slate-400">Смещение X/Y:</span>
              <div className="flex gap-1">
                <button
                  onClick={() => onProbeMove({ ...state.probePosition, x: state.probePosition.x - 0.2 })}
                  className="px-1.5 py-0.5 rounded bg-slate-800 hover:bg-slate-700 text-slate-300"
                >
                  ←
                </button>
                <button
                  onClick={() => onProbeMove({ ...state.probePosition, x: state.probePosition.x + 0.2 })}
                  className="px-1.5 py-0.5 rounded bg-slate-800 hover:bg-slate-700 text-slate-300"
                >
                  →
                </button>
                <button
                  onClick={() => onProbeMove({ ...state.probePosition, y: state.probePosition.y + 0.1 })}
                  className="px-1.5 py-0.5 rounded bg-slate-800 hover:bg-slate-700 text-slate-300"
                >
                  ↑
                </button>
                <button
                  onClick={() => onProbeMove({ ...state.probePosition, y: state.probePosition.y - 0.1 })}
                  className="px-1.5 py-0.5 rounded bg-slate-800 hover:bg-slate-700 text-slate-300"
                >
                  ↓
                </button>
              </div>
            </div>
          )}
        </div>
      )}

      {/* Vector Legend Badge */}
      {state.show3DForceVectors && (
        <div className="absolute top-3 right-3 p-2 rounded-lg glass-panel z-10 text-[10px] space-y-1 font-mono">
          <div className="flex items-center gap-1.5">
            <span className="w-2.5 h-2.5 rounded-full bg-sky-400 shadow-[0_0_8px_#38bdf8]" />
            <span className="text-slate-300">Подъемная сила (L): {aero.Lift.toFixed(0)} Н</span>
          </div>
          <div className="flex items-center gap-1.5">
            <span className="w-2.5 h-2.5 rounded-full bg-rose-500 shadow-[0_0_8px_#f43f5e]" />
            <span className="text-slate-300">Сопротивление (D): {aero.Drag.toFixed(1)} Н</span>
          </div>
          <div className="flex items-center gap-1.5">
            <span className="w-2.5 h-2.5 rounded-full bg-amber-400 shadow-[0_0_8px_#fbbf24]" />
            <span className="text-slate-300">Полная аэродинамич. сила (R)</span>
          </div>
        </div>
      )}
    </div>
  );
};
