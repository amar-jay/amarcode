import { LoaderCircle, Mic, RotateCcw, Square, X } from "lucide-react";
import { PromptInputButton } from "@/components/ai-elements/prompt-input";
import type { useDictation } from "@/hooks/use-dictation";
import { formatDictationTime } from "@/lib/dictation";

type Dictation = ReturnType<typeof useDictation>;

interface DictationControlsProps {
  dictation: Dictation;
}

export function DictationControls({ dictation }: DictationControlsProps) {
  return (
    <>
      <PromptInputButton
        aria-label={
          dictation.status === "recording"
            ? "Stop dictation"
            : dictation.status === "error"
              ? "Retry transcription"
              : dictation.status === "transcribing"
                ? "Transcribing audio"
                : dictation.status === "requesting"
                  ? "Requesting microphone access"
                  : "Start dictation"
        }
        className={
          dictation.status === "recording" ? "text-destructive" : undefined
        }
        disabled={
          !dictation.supported ||
          dictation.configured === false ||
          dictation.status === "transcribing" ||
          dictation.status === "requesting"
        }
        onClick={dictation.toggle}
        tooltip={
          dictation.configured === false
            ? "Set up Amarcode ACP first to use dictation"
            : !dictation.supported
              ? "Dictation is not available on this system"
              : dictation.status === "recording"
                ? "Stop dictation"
                : dictation.status === "transcribing"
                  ? "Transcribing…"
                  : dictation.status === "error"
                    ? "Retry transcription"
                    : dictation.status === "requesting"
                      ? "Requesting microphone access…"
                      : "Dictate with Whisper"
        }
      >
        {dictation.status === "transcribing" ||
        dictation.status === "requesting" ? (
          <LoaderCircle className="animate-spin" size={16} />
        ) : dictation.status === "recording" ? (
          <>
            <Square className="animate-pulse" size={14} />
            <span className="tabular-nums">
              {formatDictationTime(dictation.seconds)}
            </span>
          </>
        ) : dictation.status === "error" ? (
          <RotateCcw size={16} />
        ) : (
          <Mic size={16} />
        )}
      </PromptInputButton>
      {dictation.status !== "idle" && (
        <PromptInputButton
          aria-label="Discard dictation"
          tooltip="Discard dictation"
          onClick={dictation.cancel}
        >
          <X size={16} />
        </PromptInputButton>
      )}
    </>
  );
}
