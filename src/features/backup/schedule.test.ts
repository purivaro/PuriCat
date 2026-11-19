import { describe, expect, it } from "vitest";

import { fileName, fromTimeInput, scheduleOf, sizeText, toTimeInput } from "./schedule";

describe("backup schedule form", () => {
  it("turns the form into the schedule that is saved", () => {
    expect(scheduleOf("off", "02:00", [1])).toEqual({ kind: "off" });
    expect(scheduleOf("daily", "02:30", [])).toEqual({ kind: "daily", hour: 2, minute: 30 });
    // Days in order, each once.
    expect(scheduleOf("weekly", "23:05", [5, 1, 5])).toEqual({ kind: "weekly", days: [1, 5], hour: 23, minute: 5 });
    // Every day of the week is a daily schedule, as the backend stores it.
    expect(scheduleOf("weekly", "06:00", [7, 6, 5, 4, 3, 2, 1])).toEqual({ kind: "daily", hour: 6, minute: 0 });
    // No day chosen: the form shows it as incomplete; nothing invents a day.
    expect(scheduleOf("weekly", "06:00", [])).toEqual({ kind: "weekly", days: [], hour: 6, minute: 0 });
  });

  it("reads and writes the time input", () => {
    expect(toTimeInput(2, 5)).toBe("02:05");
    expect(fromTimeInput("23:59")).toEqual({ hour: 23, minute: 59 });
    expect(fromTimeInput("")).toEqual({ hour: 0, minute: 0 });
    expect(fromTimeInput("25:70")).toEqual({ hour: 0, minute: 0 });
  });

  it("previews the file name the backend will give", () => {
    const at = new Date(2026, 10, 19, 14, 5, 9);
    // The same cases as `backup::tests::file_names_are_safe_and_say_what_and_when`.
    expect(fileName("Main Shop", "shopdb", at, "sql", true)).toBe("Main-Shop_shopdb_2026-11-19_140509.sql.gz");
    expect(fileName("ฐานข้อมูล วัด", "../etc/passwd", at, "custom", true)).toBe("ฐานข้อมูล-วัด_etc-passwd_2026-11-19_140509.dump");
    expect(fileName("  ", "...", at, "sql", false)).toBe("connection_database_2026-11-19_140509.sql");
  });

  it("shows a size with the unit that reads best", () => {
    expect(sizeText(0)).toBe("0 B");
    expect(sizeText(812)).toBe("812 B");
    expect(sizeText(43_000)).toBe("42 KB");
    expect(sizeText(1_572_864)).toBe("1.5 MB");
    expect(sizeText(250 * 1_048_576)).toBe("250 MB");
    expect(sizeText(2.31 * 1024 * 1_048_576)).toBe("2.31 GB");
  });
});
