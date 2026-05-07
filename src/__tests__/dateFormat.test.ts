import { describe, it, expect } from "vitest";
import { formatDate, formatRelative } from "../utils/dateFormat";

describe("formatDate", () => {
  it("test_格式化标准日期", () => {
    const result = formatDate("2026-05-06");
    expect(result).toBe("2026-05-06");
  });

  it("test_格式化Date对象", () => {
    const result = formatDate(new Date(2026, 0, 1));
    expect(result).toBe("2026-01-01");
  });

  it("test_格式化数字时间戳", () => {
    const result = formatDate(1700000000000);
    expect(result).toMatch(/^2023-11-1[45]$/);
  });

  it("test_无效日期抛出错误", () => {
    expect(() => formatDate("not-a-date")).toThrow("Invalid date input");
  });
});

describe("formatRelative", () => {
  it("test_刚刚返回刚刚", () => {
    const result = formatRelative(new Date());
    expect(result).toBe("刚刚");
  });

  it("test_几分钟前", () => {
    const date = new Date(Date.now() - 5 * 60000);
    const result = formatRelative(date);
    expect(result).toBe("5分钟前");
  });

  it("test_几小时前", () => {
    const date = new Date(Date.now() - 3 * 3600000);
    const result = formatRelative(date);
    expect(result).toBe("3小时前");
  });

  it("test_几天前", () => {
    const date = new Date(Date.now() - 2 * 86400000);
    const result = formatRelative(date);
    expect(result).toBe("2天前");
  });

  it("test_超过7天显示完整日期", () => {
    const date = new Date("2026-01-01");
    const result = formatRelative(date);
    expect(result).toBe("2026-01-01");
  });
});
