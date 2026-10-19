const UNITS = ["B", "KB", "MB", "GB", "TB", "PB"];

export function formatSize(bytes: number): string {
  let v = bytes;
  let i = 0;
  while (v >= 1024 && i < UNITS.length - 1) {
    v /= 1024;
    i++;
  }
  return `${i === 0 ? v : v.toFixed(v >= 100 ? 0 : v >= 10 ? 1 : 2)} ${UNITS[i]}`;
}

export function formatBytes(bytes: number): string {
  return `${bytes.toLocaleString()} bytes`;
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

export function formatCount(n: number): string {
  return n.toLocaleString();
}

export function percent(part: number, whole: number): string {
  return whole > 0 ? `${((part / whole) * 100).toFixed(1)}%` : "0%";
}
