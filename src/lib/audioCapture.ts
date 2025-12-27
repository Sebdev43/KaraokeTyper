/**
 * Web Audio API based audio capture
 * This uses navigator.mediaDevices.getUserMedia which works reliably with PipeWire
 */

import { invoke } from "@tauri-apps/api/core";

export class WebAudioCapture {
  private audioContext: AudioContext | null = null;
  private mediaStream: MediaStream | null = null;
  private scriptProcessor: ScriptProcessorNode | null = null;
  private sourceNode: MediaStreamAudioSourceNode | null = null;
  private isRecording = false;
  private audioChunks: Float32Array[] = [];
  private targetSampleRate = 16000; // Whisper expects 16kHz

  async startRecording(deviceId?: string): Promise<void> {
    if (this.isRecording) {
      console.warn("[WebAudio] Already recording");
      return;
    }

    try {
      console.log("[WebAudio] Starting audio capture...");

      // Create audio context with target sample rate
      this.audioContext = new AudioContext({ sampleRate: 48000 });
      console.log(`[WebAudio] AudioContext created at ${this.audioContext.sampleRate}Hz`);

      // Request microphone access
      const constraints: MediaStreamConstraints = {
        audio: deviceId
          ? {
              deviceId: { exact: deviceId },
              echoCancellation: false,
              noiseSuppression: false,
              autoGainControl: false,
            }
          : {
              echoCancellation: false,
              noiseSuppression: false,
              autoGainControl: false,
            },
      };

      console.log("[WebAudio] Requesting microphone access...");
      this.mediaStream = await navigator.mediaDevices.getUserMedia(constraints);
      console.log("[WebAudio] Microphone access granted");

      // Create source from stream
      this.sourceNode = this.audioContext.createMediaStreamSource(this.mediaStream);

      // Use ScriptProcessorNode to capture audio data
      // Buffer size of 4096 gives good balance between latency and efficiency
      const bufferSize = 4096;
      this.scriptProcessor = this.audioContext.createScriptProcessor(bufferSize, 1, 1);

      // Reset audio chunks
      this.audioChunks = [];
      this.isRecording = true;

      // Process audio data
      this.scriptProcessor.onaudioprocess = (event) => {
        if (!this.isRecording) return;

        const inputData = event.inputBuffer.getChannelData(0);
        // Clone the data since the buffer is reused
        const chunk = new Float32Array(inputData.length);
        chunk.set(inputData);
        this.audioChunks.push(chunk);

        // Log progress periodically
        const totalSamples = this.audioChunks.reduce((sum, c) => sum + c.length, 0);
        if (totalSamples % (this.audioContext!.sampleRate * 1) < bufferSize) {
          const duration = totalSamples / this.audioContext!.sampleRate;
          console.log(`[WebAudio] Captured ${totalSamples} samples (${duration.toFixed(1)}s)`);
        }
      };

      // Connect the nodes
      this.sourceNode.connect(this.scriptProcessor);
      // Connect to destination (required for scriptProcessor to work, but at zero volume)
      const silentGain = this.audioContext.createGain();
      silentGain.gain.value = 0;
      this.scriptProcessor.connect(silentGain);
      silentGain.connect(this.audioContext.destination);

      console.log("[WebAudio] ✅ Recording started");
    } catch (error) {
      console.error("[WebAudio] Failed to start recording:", error);
      this.cleanup();
      throw error;
    }
  }

  stopRecording(): Float32Array {
    console.log("[WebAudio] Stopping recording...");
    this.isRecording = false;

    if (this.audioChunks.length === 0) {
      console.warn("[WebAudio] No audio captured");
      this.cleanup();
      return new Float32Array(0);
    }

    // Calculate total length
    const totalLength = this.audioChunks.reduce((sum, chunk) => sum + chunk.length, 0);
    console.log(`[WebAudio] Total samples: ${totalLength}`);

    // Combine all chunks
    const combined = new Float32Array(totalLength);
    let offset = 0;
    for (const chunk of this.audioChunks) {
      combined.set(chunk, offset);
      offset += chunk.length;
    }

    // Resample to 16kHz if needed
    const sourceSampleRate = this.audioContext?.sampleRate || 48000;
    let finalAudio: Float32Array;

    if (sourceSampleRate !== this.targetSampleRate) {
      console.log(`[WebAudio] Resampling from ${sourceSampleRate}Hz to ${this.targetSampleRate}Hz`);
      finalAudio = this.resample(combined, sourceSampleRate, this.targetSampleRate);
    } else {
      finalAudio = combined;
    }

    const duration = finalAudio.length / this.targetSampleRate;
    console.log(
      `[WebAudio] Final audio: ${finalAudio.length} samples (${duration.toFixed(2)}s at ${this.targetSampleRate}Hz)`,
    );

    this.cleanup();
    return finalAudio;
  }

  private resample(input: Float32Array, fromRate: number, toRate: number): Float32Array {
    if (input.length === 0) return new Float32Array(0);

    const ratio = toRate / fromRate;
    const outputLength = Math.round(input.length * ratio);
    const output = new Float32Array(outputLength);

    for (let i = 0; i < outputLength; i++) {
      const srcIndex = i / ratio;
      const srcIndexFloor = Math.floor(srcIndex);
      const srcIndexCeil = Math.min(srcIndexFloor + 1, input.length - 1);
      const fraction = srcIndex - srcIndexFloor;

      // Linear interpolation
      output[i] = input[srcIndexFloor] * (1 - fraction) + input[srcIndexCeil] * fraction;
    }

    return output;
  }

  private cleanup(): void {
    console.log("[WebAudio] Cleaning up...");

    if (this.scriptProcessor) {
      this.scriptProcessor.disconnect();
      this.scriptProcessor = null;
    }

    if (this.sourceNode) {
      this.sourceNode.disconnect();
      this.sourceNode = null;
    }

    if (this.mediaStream) {
      this.mediaStream.getTracks().forEach((track) => {
        track.stop();
      });
      this.mediaStream = null;
    }

    if (this.audioContext) {
      this.audioContext.close().catch(console.error);
      this.audioContext = null;
    }

    this.audioChunks = [];
    this.isRecording = false;
  }
}

// Send audio to backend for transcription
export async function transcribeAudio(audioData: Float32Array): Promise<string> {
  // Convert to array for JSON serialization
  const audioArray = Array.from(audioData);
  console.log(`[WebAudio] Sending ${audioArray.length} samples to backend for transcription`);

  try {
    const result = await invoke<string>("transcribe_audio", {
      audioData: audioArray,
    });
    return result;
  } catch (error) {
    console.error("[WebAudio] Transcription failed:", error);
    throw error;
  }
}

// Singleton instance
let captureInstance: WebAudioCapture | null = null;

export function getAudioCapture(): WebAudioCapture {
  if (!captureInstance) {
    captureInstance = new WebAudioCapture();
  }
  return captureInstance;
}
