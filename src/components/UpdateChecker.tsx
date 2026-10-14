import { openUrl } from "@tauri-apps/plugin-opener";
import { useCallback, useState } from "react";
import { commands } from "@/bindings";
import { Button } from "@/components/ui/button";
import { useT } from "@/i18n";

type CheckStatus =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "up-to-date" }
  | {
      kind: "available";
      currentVersion: string;
      latestVersion: string;
      releaseNotes: string;
      releaseUrl: string;
      canAutoInstall: boolean;
    }
  | { kind: "installing" }
  | { kind: "error"; message: string };

/**
 * Settings-page widget that checks GitHub Releases for a newer version and
 * offers to install it. Path taken depends on how the app was installed:
 * AppImage / MSI / EXE / DMG → full auto via tauri-plugin-updater.
 * .deb → download + pkexec apt install (user sees a PolicyKit password prompt).
 * Anything else → "Open release page" fallback in the browser.
 */
export function UpdateChecker() {
  const t = useT();
  const [status, setStatus] = useState<CheckStatus>({ kind: "idle" });

  const checkUpdates = useCallback(async () => {
    setStatus({ kind: "checking" });
    try {
      const result = await commands.checkForUpdateCmd();
      if (result.status !== "ok") {
        setStatus({ kind: "error", message: result.error });
        return;
      }
      const info = result.data;
      if (!info) {
        setStatus({ kind: "up-to-date" });
        return;
      }
      setStatus({
        kind: "available",
        currentVersion: info.current_version,
        latestVersion: info.latest_version,
        releaseNotes: info.release_notes,
        releaseUrl: info.release_url,
        canAutoInstall: info.can_auto_install,
      });
    } catch (e) {
      setStatus({
        kind: "error",
        message: e instanceof Error ? e.message : String(e),
      });
    }
  }, []);

  const installUpdate = useCallback(async () => {
    setStatus({ kind: "installing" });
    try {
      const result = await commands.installUpdateCmd();
      if (result.status !== "ok") {
        setStatus({ kind: "error", message: result.error });
      }
      // On success the Rust side calls `app.restart()`, so this callback
      // will never return (the process is replaced).
    } catch (e) {
      setStatus({
        kind: "error",
        message: e instanceof Error ? e.message : String(e),
      });
    }
  }, []);

  const openReleasePage = useCallback(async (url: string) => {
    try {
      await openUrl(url);
    } catch (e) {
      console.error("Failed to open release URL:", e);
    }
  }, []);

  return (
    <div className="flex flex-col gap-3 rounded-lg border p-4">
      <div className="flex items-center justify-between">
        <div>
          <h3 className="text-sm font-semibold">{t.updates.title}</h3>
          <p className="text-xs text-muted-foreground">{t.updates.subtitle}</p>
        </div>
        <Button
          size="sm"
          variant="outline"
          onClick={checkUpdates}
          disabled={status.kind === "checking" || status.kind === "installing"}
        >
          {status.kind === "checking" ? t.updates.checking : t.updates.check}
        </Button>
      </div>

      {status.kind === "up-to-date" && (
        <p className="text-sm text-emerald-500">{t.updates.upToDate}</p>
      )}

      {status.kind === "error" && (
        <p className="text-sm text-red-500">{t.updates.error(status.message)}</p>
      )}

      {status.kind === "installing" && (
        <p className="text-sm text-muted-foreground">{t.updates.installing}</p>
      )}

      {status.kind === "available" && (
        <div className="flex flex-col gap-2 border-t pt-3">
          <p className="text-sm">
            <span className="font-semibold">v{status.latestVersion}</span>
            {t.updates.availableSuffix(status.currentVersion)}
          </p>
          {status.releaseNotes && (
            <pre className="max-h-40 overflow-auto whitespace-pre-wrap rounded bg-muted p-2 text-xs">
              {status.releaseNotes}
            </pre>
          )}
          <div className="flex gap-2">
            {status.canAutoInstall && (
              <Button size="sm" onClick={installUpdate}>
                {t.updates.installAndRestart}
              </Button>
            )}
            <Button size="sm" variant="outline" onClick={() => openReleasePage(status.releaseUrl)}>
              {t.updates.openGithub}
            </Button>
          </div>
        </div>
      )}
    </div>
  );
}
