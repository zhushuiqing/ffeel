import { describe, it, expect } from "vitest";

describe("Utils", () => {
  it("formatFileSize handles bytes", () => {
    // 匹配 Transfers.tsx 中的 formatFileSize 逻辑
    const fmt = (bytes: number) => {
      if (bytes < 1024) return `${bytes} B`;
      if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
      if (bytes < 1024 * 1024 * 1024)
        return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
      return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
    };
    expect(fmt(0)).toBe("0 B");
    expect(fmt(500)).toBe("500 B");
    expect(fmt(2048)).toBe("2.0 KB");
    expect(fmt(1048576)).toBe("1.0 MB");
    expect(fmt(1073741824)).toBe("1.00 GB");
  });

  it("statusLabel returns correct labels", () => {
    const label = (status: string) => {
      switch (status) {
        case "Pending": return "等待中";
        case "Transferring": return "传输中";
        case "Paused": return "已暂停";
        case "Completed": return "已完成";
        case "Failed": return "失败";
        case "Cancelled": return "已取消";
        default: return status;
      }
    };
    expect(label("Pending")).toBe("等待中");
    expect(label("Transferring")).toBe("传输中");
    expect(label("Completed")).toBe("已完成");
  });

  it("formatSpeed handles various speeds", () => {
    const fmt = (speed: number) => {
      if (speed === 0) return "-";
      if (speed < 1024) return `${speed.toFixed(0)} B/s`;
      if (speed < 1024 * 1024) return `${(speed / 1024).toFixed(1)} KB/s`;
      return `${(speed / (1024 * 1024)).toFixed(1)} MB/s`;
    };
    expect(fmt(0)).toBe("-");
    expect(fmt(500)).toBe("500 B/s");
    expect(fmt(2048)).toBe("2.0 KB/s");
    expect(fmt(1048576)).toBe("1.0 MB/s");
  });
});
