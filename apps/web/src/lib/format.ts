/*
 * Display formatting shared across panels (pure, locale from the UI clock —
 * timestamps are shown in the user's local time, sorted nowhere).
 */

/** Local "YYYY-MM-DD HH:mm" from an ISO timestamp; "-" when absent. */
export function formatDateTime(iso: string | undefined | null): string {
  if (!iso) return "-";
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return iso;
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}`;
}
