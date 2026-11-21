export function bytes(
  value: number,
  decimals = 1,
): { value: string; unit: string } {
  if (!Number.isFinite(value) || value <= 0) return { value: "0", unit: "B" };
  const units = ["B", "KB", "MB", "GB", "TB"];
  const exponent = Math.min(
    Math.floor(Math.log(value) / Math.log(1024)),
    units.length - 1,
  );
  return {
    value: (value / 1024 ** exponent).toFixed(exponent === 0 ? 0 : decimals),
    unit: units[exponent] ?? "B",
  };
}
export function formatBytes(value: number) {
  const result = bytes(value);
  return `${result.value} ${result.unit}`;
}
export function duration(start: string | number) {
  const seconds =
    typeof start === "string"
      ? Math.max(0, (Date.now() - Date.parse(start)) / 1000)
      : Math.max(0, start);
  if (!Number.isFinite(seconds)) return "—";
  if (seconds < 60) return `${Math.floor(seconds)} 秒`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)} 分钟`;
  if (seconds < 86400)
    return `${Math.floor(seconds / 3600)} 小时 ${Math.floor((seconds % 3600) / 60)} 分钟`;
  return `${Math.floor(seconds / 86400)} 天 ${Math.floor((seconds % 86400) / 3600)} 小时`;
}
export function nodeRegion(name: string): { label: string; code: string } {
  if (/香港|hong.?kong|\bhk\b/i.test(name))
    return { label: "香港", code: "HK" };
  if (/日本|东京|tokyo|japan|\bjp\b/i.test(name))
    return { label: "日本", code: "JP" };
  if (/新加坡|singapore|\bsg\b/i.test(name))
    return { label: "新加坡", code: "SG" };
  if (/美国|united.?states|\bus\b|洛杉矶/i.test(name))
    return { label: "美国", code: "US" };
  if (/台湾|taiwan|\btw\b/i.test(name)) return { label: "台湾", code: "TW" };
  if (name === "DIRECT") return { label: "直连", code: "↗" };
  if (name === "REJECT") return { label: "拒绝", code: "—" };
  return { label: "节点", code: "GL" };
}
export function lastDelay(history: { delay: number }[] = []) {
  return history.at(-1)?.delay ?? null;
}
export function delayClass(delay: number | null) {
  return delay === null || delay === 0
    ? "latency-unknown"
    : delay < 150
      ? "latency-good"
      : delay < 300
        ? "latency-medium"
        : "latency-poor";
}
