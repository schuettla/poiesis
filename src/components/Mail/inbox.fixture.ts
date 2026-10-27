import type { InboxRow } from "../../lib/api";

/** An Inbox row for tests; override what the case is about. */
export const row = (over: Partial<InboxRow> = {}): InboxRow => ({
  account_id: "a1",
  account_label: "Work",
  uid: 1,
  from: "Ana Ruiz",
  subject: "Contract draft for Thursday",
  date: "Thu, 23 Oct 2026 08:52:00 +0000",
  sent_at: new Date(2026, 9, 23, 8, 52).getTime(),
  unread: false,
  checked: true,
  category: null,
  category_p: null,
  urgency: null,
  urgency_p: null,
  reply: null,
  retagged: false,
  ...over,
});
