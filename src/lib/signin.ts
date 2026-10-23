import * as api from "./api";
import { useAppStore } from "./store";

const POLL_MS = 3000;
const GIVE_UP_MS = 10 * 60 * 1000;

const watching = new Set<string>();

/**
 * Keep an eye on a sign-in the person started from the `/` menu, which has no
 * card in the chat to do the watching. When it finishes (or fails) it says so in
 * the toast and refreshes what the rest of the app knows about connected apps.
 */
export function watchSignIn(account: Pick<api.AppAccount, "id" | "app" | "app_name">): void {
  if (watching.has(account.id)) return;
  watching.add(account.id);
  const started = Date.now();
  const stop = () => {
    clearInterval(timer);
    watching.delete(account.id);
  };
  const say = (text: string) => useAppStore.setState({ savedToast: `◆ ${text}` });
  const tick = async () => {
    if (Date.now() - started > GIVE_UP_MS) return stop();
    try {
      const list = await api.composioAccounts(true, [account.app]);
      // Folded into one row per account once the app has two (`CAP-C6`).
      const mine =
        list.find((a) => a.id === account.id) ?? list.find((a) => a.app === account.app && a.status === "ACTIVE");
      if (!mine) return;
      if (mine.status === "ACTIVE") {
        stop();
        await useAppStore.getState().refreshApps();
        say(mine.label ? `${mine.app_name} is connected as ${mine.label}` : `${mine.app_name} is connected`);
      } else if (api.accountBroken(mine)) {
        stop();
        await useAppStore.getState().refreshApps();
        say(`The ${mine.app_name} sign-in didn't finish`);
      }
    } catch {
      // A hiccup is not a failure; the next look tries again.
    }
  };
  const timer = setInterval(() => void tick(), POLL_MS);
}
