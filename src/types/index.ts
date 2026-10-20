export type PlanetType = 'earth' | 'mars' | 'venus' | 'titan' | 'water' | 'custom';

export type AirfoilType =
  | 'naca4'
  | 'naca5'
  | 'supercritical'
  | 'biconvex'
  | 'double_wedge'
  | 'cylinder'
  | 'flat_plate'
  | 'custom';

export type FlapType = 'plain' | 'split' | 'slotted' | 'fowler';

export type EngineType = 'turbofan' | 'turbojet' | 'piston_prop' | 'electric_prop' | 'rocket';

export interface Point2D {
  x: number;
  y: number;
}

export interface CpPoint {
  x: number;
  cp: number;
}

export interface AtmosphereData {
  name: string;
  altitude: number; // m
  temperature: number; // K
  pressure: number; // Pa
  density: number; // kg/m^3
  speedOfSound: number; // m/s
  dynamicViscosity: number; // Pa*s
  dynamicPressure: number; // Pa
  gravity: number; // m/s^2
}

export interface AirfoilGeometry {
  upper: Point2D[];
  lower: Point2D[];
  camberLine: Point2D[];
  thicknessDist: Point2D[];
  chord: number;
  maxThickness: number;
  maxThicknessX: number;
  maxCamber: number;
  maxCamberX: number;
  leadingEdgeRadius: number;
  trailingEdgeAngle: number;
}

export interface AeroResults {
  // Dimensionless coefficients
  CL: number;
  CD: number;
  CD0: number; // Parasitic profile drag
  CDi: number; // Induced drag
  CDwave: number; // Wave drag
  CDstall: number; // Stall drag increment
  LD: number; // Lift-to-drag ratio
  Cm: number; // Pitching moment at c/4
  xcp: number; // Center of pressure (x/c)

  // Dimensional forces & moments
  Lift: number; // N
  Drag: number; // N
  PitchingMoment: number; // N*m

  // Flow conditions
  velocity: number; // m/s
  Mach: number;
  MachCrit: number;
  Reynolds: number;
  dynamicPressure: number; // Pa
  wingArea: number; // m^2
  aspectRatio: number;
  oswaldEfficiency: number;

  // Stall & Boundary Layer
  stallFactor: number; // 0 to 1
  isStalled: boolean;
  stallState: 'attached' | 'incipient' | 'stalled';
  boundaryLayerTransX: number; // x/c transition location
  stagnationPointX: number; // x/c

  // Pressure distributions
  cpUpper: CpPoint[];
  cpLower: CpPoint[];
  cpMin: number;

  // Spanwise 3D distribution
  spanwiseY: number[];
  spanwiseLift: number[];
  spanwiseCirculation: number[];
  spanwiseDownwash: number[];
}

export interface FlightPerformance {
  stallSpeed: number; // m/s
  minDragSpeed: number; // m/s (Best glide)
  minPowerSpeed: number; // m/s (Max endurance)
  maxSpeed: number; // m/s
  maxGlideRatio: number;
  minSinkRate: number; // m/s
  maxClimbRate: number; // m/s
  climbAngle: number; // degrees
  serviceCeiling: number; // m
  thrustAvailable: number; // N
  dragAtCurrentSpeed: number; // N
  excessThrust: number; // N
  rangeKm: number; // km
  enduranceHours: number; // h
}

export interface PitotProbeData {
  x: number;
  y: number;
  z: number;
  localVelocity: number;
  localMach: number;
  staticPressure: number;
  totalPressure: number;
  dynamicPressure: number;
}

export interface SimState {
  // Atmosphere & Flight
  planet: PlanetType;
  customDensity: number;
  customSpeedOfSound: number;
  altitude: number; // m
  velocity: number; // m/s
  alpha: number; // degrees (-20 to 30)

  // Wing Profile
  profileType: AirfoilType;
  profileName: string;
  camber: number; // % (0 to 10)
  camberPos: number; // % (10 to 90)
  thickness: number; // % (2 to 30)
  cylinderRpm: number; // RPM for cylinder Magnus effect

  // High-lift devices
  flapAngle: number; // degrees (-20 to 45)
  flapType: FlapType;
  slatAngle: number; // degrees (0 to 25)

  // 3D Wing Planform
  chord: number; // m
  span: number; // m
  sweep: number; // degrees
  taper: number; // tip / root ratio
  twist: number; // washout degrees
  dihedral: number; // degrees
  hasWinglets: boolean;

  // Propulsion & Aircraft
  engineType: EngineType;
  maxThrust: number; // N
  aircraftMass: number; // kg
  fuelMass: number; // kg
  tsfc: number; // Thrust specific fuel consumption

  // Visual & Tunnel Config
  show3DStreamlines: boolean;
  show3DVortices: boolean;
  show3DForceVectors: boolean;
  show3DPressureMap: boolean;
  show3DWireframe: boolean;
  show3DPitotProbe: boolean;
  showShockWaves: boolean;
  colormap: 'rainbow' | 'jet' | 'plasma' | 'thermal';
  smokeDensity: number; // 1 to 5

  // Interactive Pitot probe position
  probePosition: { x: number; y: number; z: number };

  // Audio
  audioEnabled: boolean;
  audioVolume: number; // 0 to 1

  // Custom Airfoil raw coordinates
  customUpper: Point2D[] | null;
  customLower: Point2D[] | null;
}
