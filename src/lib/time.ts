/** Midnight `days` calendar days before `now`. Built with `setDate`, not by
 * subtracting 24h multiples, so a DST change never shifts a boundary. */
function dayStart(now: number, days = 0): number {
  const d = new Date(now);
  d.setDate(d.getDate() - days);
  d.setHours(0, 0, 0, 0);
  return d.getTime();
}

/** In the UI's own language: an OS locale would put `Do` and `6. Sept.`
 * under an English "Previous 7 days", and its longer forms cost title width. */
const STAMP_LOCALE = "en-US";

/** A list-column timestamp: as short as it can be and still read at a glance.
 * `now`, `12m`, `3h`, `Tue`, `Mar 3`, `Mar 24`. */
export function shortTime(ts: number, now = Date.now()): string {
  const mins = Math.floor((now - ts) / 60_000);
  if (mins < 1) return "now";
  if (mins < 60) return `${mins}m`;
  if (ts >= dayStart(now)) return `${Math.floor(mins / 60)}h`;
  const d = new Date(ts);
  if (ts >= dayStart(now, 6)) return d.toLocaleDateString(STAMP_LOCALE, { weekday: "short" });
  if (d.getFullYear() === new Date(now).getFullYear()) {
    return d.toLocaleDateString(STAMP_LOCALE, { month: "short", day: "numeric" });
  }
  return d.toLocaleDateString(STAMP_LOCALE, { month: "short", year: "2-digit" });
}

/** The clock time, as the person's system writes it. */
export function clockTime(ts: number): string {
  return new Date(ts).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

/** A message's stamp: the time for one of today, the day for an older one.
 * `15:44`, `Nov 20`, `Nov 20, 25`. */
export function turnTime(ts: number, now = Date.now()): string {
  if (ts >= dayStart(now) && ts < dayStart(now, -1)) return clockTime(ts);
  const d = new Date(ts);
  const year = d.getFullYear() === new Date(now).getFullYear() ? undefined : "2-digit";
  return d.toLocaleDateString(STAMP_LOCALE, { month: "short", day: "numeric", year });
}

/** The whole date and time, for a stamp's tooltip. */
export function fullTime(ts: number): string {
  const day = new Date(ts).toLocaleDateString(STAMP_LOCALE, {
    weekday: "long",
    month: "long",
    day: "numeric",
    year: "numeric",
  });
  return `${day}, ${clockTime(ts)}`;
}

export const BUCKETS =["Today", "Yesterday", "Previous 7 days", "Previous 30 days", "Older"] as const;
export type Bucket = (typeof BUCKETS)[number];

/** Calendar days, not a rolling window: a chat from 11pm last night is
 * Yesterday at 1am, which a rolling 24h would still call Today. */
export function bucketFor(ts: number, now = Date.now()): Bucket {
  if (ts >= dayStart(now)) return "Today";
  if (ts >= dayStart(now, 1)) return "Yesterday";
  if (ts >= dayStart(now, 7)) return "Previous 7 days";
  if (ts >= dayStart(now, 30)) return "Previous 30 days";
  return "Older";
}

/** Items in their buckets, buckets in order, empty ones dropped. Order inside
 * a bucket is the order given. */
export function groupByBucket<T extends { updatedAt: number }>(
  items: T[],
  now = Date.now()
): { label: Bucket; items: T[] }[] {
  const byBucket = new Map<Bucket, T[]>();
  for (const item of items) {
    const b = bucketFor(item.updatedAt, now);
    const list = byBucket.get(b);
    if (list) list.push(item);
    else byBucket.set(b, [item]);
  }
  return BUCKETS.filter((b) => byBucket.has(b)).map((label) => ({
    label,
    items: byBucket.get(label)!,
  }));
}
