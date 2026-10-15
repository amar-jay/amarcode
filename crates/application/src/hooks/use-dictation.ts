import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type RefObject,
} from "react";
import { useAtomValue } from "jotai";
import { daemonApi } from "@/api";
import { agentsAtom } from "@/state/agents";
import { mergeDictatedText } from "@/lib/dictation";

export type DictationStatus =
  "idle" | "requesting" | "recording" | "transcribing" | "error";
export const MAX_DICTATION_SECONDS = 120;
const MAX_AUDIO_BYTES = 25 * 1024 * 1024;
const MIME_CANDIDATES = [
  "audio/webm;codecs=opus",
  "audio/webm",
  "audio/mp4",
  "audio/ogg;codecs=opus",
  "audio/ogg",
];

async function blobToBase64(blob: Blob): Promise<string> {
  const bytes = new Uint8Array(await blob.arrayBuffer());
  let binary = "";
  for (let offset = 0; offset < bytes.length; offset += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(offset, offset + 0x8000));
  }
  return btoa(binary);
}

function errorMessage(error: unknown, fallback: string): string {
  return error instanceof Error
    ? error.message
    : typeof error === "string"
      ? error
      : fallback;
}

interface UseDictationOptions {
  textareaContainerRef: RefObject<HTMLDivElement | null>;
  onError: (message: string) => void;
}

export function useDictation({
  textareaContainerRef,
  onError,
}: UseDictationOptions) {
  const [status, setStatus] = useState<DictationStatus>("idle");
  const [seconds, setSeconds] = useState(0);
  const [configured, setConfigured] = useState<boolean | null>(null);
  const catalog = useAtomValue(agentsAtom);
  const statusRef = useRef<DictationStatus>("idle");
  const generationRef = useRef(0);
  const recorderRef = useRef<MediaRecorder | null>(null);
  const streamRef = useRef<MediaStream | null>(null);
  const audioRef = useRef<Blob | null>(null);
  const timerRef = useRef<number | null>(null);
  const callbacksRef = useRef({ onError });
  useEffect(() => {
    callbacksRef.current = { onError };
  }, [onError]);

  const insertTranscript = useCallback(
    (text: string) => {
      const textarea =
        textareaContainerRef.current?.querySelector<HTMLTextAreaElement>(
          'textarea[name="message"]',
        );
      if (!textarea) return;
      const merged = mergeDictatedText(
        textarea.value,
        textarea.selectionStart ?? textarea.value.length,
        textarea.selectionEnd ?? textarea.value.length,
        text,
      );
      const setter = Object.getOwnPropertyDescriptor(
        HTMLTextAreaElement.prototype,
        "value",
      )?.set;
      if (setter) setter.call(textarea, merged.value);
      else textarea.value = merged.value;
      textarea.dispatchEvent(new Event("input", { bubbles: true }));
      textarea.focus();
      textarea.setSelectionRange(merged.cursor, merged.cursor);
    },
    [textareaContainerRef],
  );

  const supported =
    typeof MediaRecorder !== "undefined" &&
    typeof navigator !== "undefined" &&
    Boolean(navigator.mediaDevices?.getUserMedia);

  // Settings refreshes the agent catalog after saving provider credentials.
  useEffect(() => {
    let cancelled = false;
    void daemonApi
      .getAmarcodeAcpConfig()
      .then((config) => {
        if (!cancelled) setConfigured(config.configured);
      })
      .catch(() => {
        if (!cancelled) setConfigured(null);
      });
    return () => {
      cancelled = true;
    };
  }, [catalog]);

  const changeStatus = useCallback((next: DictationStatus) => {
    statusRef.current = next;
    setStatus(next);
  }, []);

  const stopTimer = useCallback(() => {
    if (timerRef.current !== null) window.clearInterval(timerRef.current);
    timerRef.current = null;
  }, []);

  const cleanup = useCallback(() => {
    // Invalidate permission requests and transcription replies from older recordings.
    generationRef.current += 1;
    stopTimer();
    const recorder = recorderRef.current;
    recorderRef.current = null;
    if (recorder && recorder.state !== "inactive") recorder.stop();
    for (const track of streamRef.current?.getTracks() ?? []) track.stop();
    streamRef.current = null;
    audioRef.current = null;
    statusRef.current = "idle";
  }, [stopTimer]);

  const cancel = useCallback(() => {
    cleanup();
    setSeconds(0);
    setStatus("idle");
  }, [cleanup]);

  useEffect(() => cleanup, [cleanup]);

  const transcribe = useCallback(
    async (blob: Blob, generation: number) => {
      changeStatus("transcribing");
      try {
        const audioBase64 = await blobToBase64(blob);
        if (generation !== generationRef.current) return;
        const { text } = await daemonApi.transcribeAudio(
          audioBase64,
          blob.type,
        );
        if (generation !== generationRef.current) return;
        if (!text.trim())
          throw new Error("No speech was detected. Try recording again.");
        insertTranscript(text);
        audioRef.current = null;
        changeStatus("idle");
      } catch (error) {
        if (generation !== generationRef.current) return;
        changeStatus("error");
        callbacksRef.current.onError(
          errorMessage(
            error,
            "Transcription failed. Retry or discard the recording.",
          ),
        );
      }
    },
    [changeStatus, insertTranscript],
  );

  const stopAndTranscribe = useCallback(() => {
    stopTimer();
    const recorder = recorderRef.current;
    if (recorder && recorder.state !== "inactive") {
      changeStatus("transcribing");
      recorder.stop();
    }
  }, [changeStatus, stopTimer]);

  const start = useCallback(async () => {
    if (!supported || statusRef.current !== "idle") return;
    const generation = ++generationRef.current;
    changeStatus("requesting");
    let stream: MediaStream | null = null;
    try {
      // Recheck immediately before capture so cleared credentials are respected.
      const config = await daemonApi.getAmarcodeAcpConfig();
      if (generation !== generationRef.current) return;
      setConfigured(config.configured);
      if (!config.configured)
        throw new Error(
          "Set up Amarcode ACP in Settings before using dictation.",
        );
      stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      if (generation !== generationRef.current) {
        for (const track of stream.getTracks()) track.stop();
        return;
      }
      streamRef.current = stream;
      const mimeType = MIME_CANDIDATES.find((type) =>
        MediaRecorder.isTypeSupported(type),
      );
      const recorder = new MediaRecorder(
        stream,
        mimeType ? { mimeType } : undefined,
      );
      const chunks: Blob[] = [];
      let size = 0;
      recorderRef.current = recorder;
      recorder.ondataavailable = (event) => {
        if (generation !== generationRef.current) return;
        if (event.data.size) {
          chunks.push(event.data);
          size += event.data.size;
        }
        if (size >= MAX_AUDIO_BYTES) stopAndTranscribe();
      };
      recorder.onerror = () => {
        if (generation !== generationRef.current) return;
        cancel();
        callbacksRef.current.onError(
          "Audio recording failed. Check your microphone and try again.",
        );
      };
      recorder.onstop = () => {
        for (const track of stream?.getTracks() ?? []) track.stop();
        if (generation !== generationRef.current) return;
        stopTimer();
        streamRef.current = null;
        recorderRef.current = null;
        setSeconds(0);
        const blob = new Blob(chunks, {
          type: recorder.mimeType || mimeType || "audio/webm",
        });
        if (!blob.size || blob.size > MAX_AUDIO_BYTES) {
          changeStatus("idle");
          callbacksRef.current.onError(
            blob.size
              ? "Recording exceeds the 25 MB limit. Try a shorter dictation."
              : "No audio was recorded. Try again.",
          );
          return;
        }
        audioRef.current = blob;
        void transcribe(blob, generation);
      };
      recorder.start(1000);
      setSeconds(0);
      changeStatus("recording");
      const startedAt = Date.now();
      timerRef.current = window.setInterval(() => {
        const elapsed = Math.floor((Date.now() - startedAt) / 1000);
        setSeconds(elapsed);
        if (elapsed >= MAX_DICTATION_SECONDS) stopAndTranscribe();
      }, 250);
    } catch (error) {
      for (const track of stream?.getTracks() ?? []) track.stop();
      if (generation !== generationRef.current) return;
      cleanup();
      changeStatus("idle");
      const denied =
        error instanceof DOMException &&
        ["NotAllowedError", "SecurityError"].includes(error.name);
      callbacksRef.current.onError(
        denied
          ? "Microphone access was denied. Allow Amarcode microphone access in your system settings, then try again."
          : errorMessage(
              error,
              "Could not access the microphone. Check that one is connected, then try again.",
            ),
      );
    }
  }, [
    cancel,
    changeStatus,
    cleanup,
    stopAndTranscribe,
    stopTimer,
    supported,
    transcribe,
  ]);

  const toggle = useCallback(() => {
    if (statusRef.current === "recording") stopAndTranscribe();
    else if (statusRef.current === "error" && audioRef.current)
      void transcribe(audioRef.current, generationRef.current);
    else if (statusRef.current === "idle") void start();
  }, [start, stopAndTranscribe, transcribe]);

  return { status, seconds, configured, supported, toggle, cancel };
}
