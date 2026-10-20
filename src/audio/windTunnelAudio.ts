export class WindTunnelAudioEngine {
  private ctx: AudioContext | null = null;
  private masterGain: GainNode | null = null;
  private noiseNode: AudioBufferSourceNode | null = null;
  private filterNode: BiquadFilterNode | null = null;
  private rumbleOsc: OscillatorNode | null = null;
  private rumbleGain: GainNode | null = null;
  private isRunning: boolean = false;

  public init(): void {
    if (this.ctx) return;
    try {
      const AudioCtx = window.AudioContext || (window as unknown as { webkitAudioContext: typeof AudioContext }).webkitAudioContext;
      this.ctx = new AudioCtx();

      // Master output gain
      this.masterGain = this.ctx.createGain();
      this.masterGain.gain.setValueAtTime(0.2, this.ctx.currentTime);
      this.masterGain.connect(this.ctx.destination);

      // Noise generator for wind rush
      const bufferSize = this.ctx.sampleRate * 2;
      const noiseBuffer = this.ctx.createBuffer(1, bufferSize, this.ctx.sampleRate);
      const output = noiseBuffer.getChannelData(0);
      let b0 = 0, b1 = 0, b2 = 0, b3 = 0, b4 = 0, b5 = 0, b6 = 0;

      // Pink noise filter algorithm
      for (let i = 0; i < bufferSize; i++) {
        const white = Math.random() * 2 - 1;
        b0 = 0.99886 * b0 + white * 0.0555179;
        b1 = 0.99332 * b1 + white * 0.0750759;
        b2 = 0.96900 * b2 + white * 0.1538520;
        b3 = 0.86650 * b3 + white * 0.3104856;
        b4 = 0.55000 * b4 + white * 0.5329522;
        b5 = -0.7616 * b5 - white * 0.0168980;
        output[i] = (b0 + b1 + b2 + b3 + b4 + b5 + b6 + white * 0.5362) * 0.11;
        b6 = white * 0.115926;
      }

      this.noiseNode = this.ctx.createBufferSource();
      this.noiseNode.buffer = noiseBuffer;
      this.noiseNode.loop = true;

      // Bandpass / Lowpass filter for airflow frequency
      this.filterNode = this.ctx.createBiquadFilter();
      this.filterNode.type = 'lowpass';
      this.filterNode.frequency.setValueAtTime(450, this.ctx.currentTime);
      this.filterNode.Q.setValueAtTime(1.5, this.ctx.currentTime);

      this.noiseNode.connect(this.filterNode);
      this.filterNode.connect(this.masterGain);
      this.noiseNode.start();

      // Sub-bass rumble oscillator for stall buffeting
      this.rumbleOsc = this.ctx.createOscillator();
      this.rumbleOsc.type = 'sawtooth';
      this.rumbleOsc.frequency.setValueAtTime(32, this.ctx.currentTime);

      this.rumbleGain = this.ctx.createGain();
      this.rumbleGain.gain.setValueAtTime(0, this.ctx.currentTime);

      const rumbleFilter = this.ctx.createBiquadFilter();
      rumbleFilter.type = 'lowpass';
      rumbleFilter.frequency.setValueAtTime(80, this.ctx.currentTime);

      this.rumbleOsc.connect(rumbleFilter);
      rumbleFilter.connect(this.rumbleGain);
      this.rumbleGain.connect(this.masterGain);
      this.rumbleOsc.start();

      this.isRunning = true;
    } catch {
      // Audio not supported or blocked by browser policy until user gesture
    }
  }

  public update(velocity: number, dynamicPressure: number, stallFactor: number, mach: number, volume: number): void {
    if (!this.ctx || !this.isRunning) return;

    if (this.ctx.state === 'suspended') {
      this.ctx.resume().catch(() => {});
    }

    const t = this.ctx.currentTime;

    // Master volume
    if (this.masterGain) {
      this.masterGain.gain.setTargetAtTime(Math.max(0, Math.min(1, volume * 0.35)), t, 0.05);
    }

    // Dynamic wind frequency based on velocity and Mach
    if (this.filterNode) {
      const targetFreq = Math.min(6500, Math.max(120, velocity * 4.5 + (mach > 0.8 ? (mach - 0.8) * 4000 : 0)));
      this.filterNode.frequency.setTargetAtTime(targetFreq, t, 0.08);
    }

    // Stall buffeting rumble
    if (this.rumbleGain && this.rumbleOsc) {
      const stallBuffetGain = Math.max(0, stallFactor - 0.1) * 0.8;
      this.rumbleGain.gain.setTargetAtTime(stallBuffetGain, t, 0.05);
      // Wobble frequency slightly with stall severity
      const freq = 28 + stallFactor * 16 + Math.sin(t * 12) * 5;
      this.rumbleOsc.frequency.setTargetAtTime(freq, t, 0.05);
    }
  }

  public setMute(muted: boolean): void {
    if (this.masterGain && this.ctx) {
      this.masterGain.gain.setTargetAtTime(muted ? 0 : 0.2, this.ctx.currentTime, 0.05);
    }
  }

  public destroy(): void {
    try {
      if (this.noiseNode) this.noiseNode.stop();
      if (this.rumbleOsc) this.rumbleOsc.stop();
      if (this.ctx) this.ctx.close();
    } catch {}
    this.ctx = null;
    this.isRunning = false;
  }
}

export const windTunnelAudio = new WindTunnelAudioEngine();
