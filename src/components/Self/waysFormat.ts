/** "1st", "2nd", "3rd", "11th": a version, read aloud. */
export function ordinal(n: number): string {
  const rest = n % 100;
  if (rest >= 11 && rest <= 13) return `${n}th`;
  return `${n}${["th", "st", "nd", "rd"][n % 10] ?? "th"}`;
}

const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/** "Nov 7" from a time in ms, or from `YYYY-MM-DD`. */
export function shortDate(when: number | string): string {
  const d = typeof when === "number" ? new Date(when) : new Date(`${when}T12:00:00`);
  if (Number.isNaN(d.getTime())) return "";
  return `${MONTHS[d.getMonth()]} ${d.getDate()}`;
}
