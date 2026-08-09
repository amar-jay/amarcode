import MainPromptInput from "@/components/main-prompt-input";
import { TopBar } from "@/components/top-bar";
import { Toaster } from "@/components/ui/sonner";
import { useTheme } from "@/hooks/use-theme";

/**
 * Deliberately minimal application shell.
 *
 * The previous session/sidebar/controller composition was tied to RPC methods
 * that no longer exist. New chat state will be introduced here only after the
 * daemon-backed controller layer is designed.
 */
export default function App() {
  const { theme } = useTheme();

  return (
    <>
      <TopBar />
      <main className="m-auto w-full max-w-2xl px-8 py-12">
        <MainPromptInput />
      </main>
      <Toaster
        position="bottom-right"
        closeButton
        theme={theme}
        className="pointer-events-auto !z-[100]"
      />
    </>
  );
}
