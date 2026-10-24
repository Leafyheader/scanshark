const UNITS = ["B", "KB", "MB", "GB", "TB", "PB"];

/** "binary" counts 1 KB as 1024 bytes, as Windows does; "decimal" as 1000, as drive makers, macOS and GNOME do. */
export type SizeUnits = "binary" | "decimal";
let step = 1024;

/** Set from the app's settings before each render. */
export function setSizeUnits(units: SizeUnits) {
  step = units === "decimal" ? 1000 : 1024;
}

export function formatSize(bytes: number): string {
  let v = bytes;
  let i = 0;
  while (v >= step && i < UNITS.length - 1) {
    v /= step;
    i++;
  }
  return `${i === 0 ? v : v.toFixed(v >= 100 ? 0 : v >= 10 ? 1 : 2)} ${UNITS[i]}`;
}

const dateFmt = new Intl.DateTimeFormat(undefined, {
  day: "numeric",
  month: "short",
  year: "numeric",
  hour: "2-digit",
  minute: "2-digit",
  second: "2-digit",
});

export function formatDate(unixSeconds: number): string {
  return unixSeconds > 0 ? dateFmt.format(new Date(unixSeconds * 1000)) : "";
}

const shortDateFmt = new Intl.DateTimeFormat(undefined, {
  day: "numeric",
  month: "short",
  year: "numeric",
  hour: "2-digit",
  minute: "2-digit",
});

export function formatShortDate(unixSeconds: number): string {
  return unixSeconds > 0 ? shortDateFmt.format(new Date(unixSeconds * 1000)) : "";
}

/** "5 min ago", "3 h ago", "2 days ago". */
export function formatAge(unixSeconds: number): string {
  const s = Math.max(0, Date.now() / 1000 - unixSeconds);
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.floor(s / 60)} min ago`;
  if (s < 86400) return `${Math.floor(s / 3600)} h ago`;
  const d = Math.floor(s / 86400);
  return d === 1 ? "1 day ago" : `${d} days ago`;
}

export function formatCount(n: number): string {
  return n.toLocaleString();
}

export function percent(part: number, whole: number): string {
  return whole > 0 ? `${((part / whole) * 100).toFixed(1)}%` : "0%";
}
