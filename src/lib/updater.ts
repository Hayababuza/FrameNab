/**
 * Updates from the GitHub releases (tauri-plugin-updater): the app asks
 * `releases/latest/download/latest.json`, verifies the signature against the public key in
 * `tauri.conf.json`, installs and restarts.
 */
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";

export type { Update };

/** The available update, or `null` if this is the newest version. Throws if the check fails. */
export async function findUpdate(): Promise<Update | null> {
  return await check({ timeout: 15000 });
}

/** Downloads and installs `update` (progress 0–1, if the size is known), then restarts. */
export async function installUpdate(update: Update, onprogress: (fraction: number | null) => void) {
  let total = 0;
  let done = 0;
  await update.downloadAndInstall((event) => {
    if (event.event === "Started") total = event.data.contentLength ?? 0;
    if (event.event === "Progress") {
      done += event.data.chunkLength;
      onprogress(total ? done / total : null);
    }
  });
  await relaunch();
}
