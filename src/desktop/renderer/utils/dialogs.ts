import { isTauri } from "@tauri-apps/api/core";
import { confirm, message } from "@tauri-apps/plugin-dialog";

export async function confirmAction(text: string): Promise<boolean> {
  return isTauri() ? confirm(text, { title: "KanVibe" }) : window.confirm(text);
}

export async function showAlert(text: string): Promise<void> {
  if (isTauri()) {
    await message(text, { title: "KanVibe" });
  } else {
    window.alert(text);
  }
}
