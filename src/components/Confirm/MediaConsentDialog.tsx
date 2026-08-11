import { useAppStore } from "../../lib/store";
import ConfirmDialog from "./ConfirmDialog";

/** `CST-1`: the first paid generation per cloud backend asks first, in plain
 * terms — what it costs and that local stays on-device. Every generation
 * after this one shows cost only in the media block's caption. */
export default function MediaConsentDialog() {
  const pending = useAppStore((s) => s.pendingMediaConsent);
  if (!pending) return null;

  const price = pending.priceLabel ? ` and costs about ${pending.priceLabel} per image` : "";

  return (
    <ConfirmDialog
      title={`Generate with ${pending.backendLabel}?`}
      body={`This sends your prompt to ${pending.backendLabel}${price}. Local generation stays on your device — pick a local model in the chooser instead if you'd rather not.`}
      confirmLabel="Generate"
      cancelLabel="Cancel"
      onConfirm={() => pending.resolve(true)}
      onCancel={() => pending.resolve(false)}
    />
  );
}
