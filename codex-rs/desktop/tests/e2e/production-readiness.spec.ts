import { expect, test } from "@playwright/test";
import axe from "axe-core";
import path from "node:path";
import { fileURLToPath } from "node:url";

const desktopRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "../..",
);
const screenshotRoot = path.resolve(
  desktopRoot,
  "../../output/playwright/screenshots",
);

const viewports = [
  { width: 760, height: 600, compact: true },
  { width: 1024, height: 768, compact: false },
  { width: 1600, height: 1000, compact: false },
] as const;

for (const viewport of viewports) {
  test(`keeps the conversation primary at ${viewport.width}px`, async ({
    page,
  }) => {
    await page.setViewportSize(viewport);
    await page.goto("/?preview=1");
    await expect(page.getByRole("main", { name: "Current task" })).toBeVisible();
    await expect(page.locator(".application-error")).toHaveCount(0);

    const shell = page.locator(".bridge-shell");
    if (viewport.compact) {
      await expect(shell).toHaveAttribute("data-compact-layout", "true");
      await expect(page.getByLabel("Expand navigation")).toHaveCount(0);
    } else {
      await expect(shell).not.toHaveAttribute("data-compact-layout", "true");
    }

    const geometry = await page.evaluate(() => {
      const main = document.querySelector<HTMLElement>("#agent-activity");
      return {
        viewportWidth: document.documentElement.clientWidth,
        scrollWidth: document.documentElement.scrollWidth,
        mainWidth: main?.getBoundingClientRect().width ?? 0,
      };
    });
    expect(geometry.scrollWidth).toBeLessThanOrEqual(geometry.viewportWidth);
    expect(geometry.mainWidth).toBeGreaterThan(viewport.width * 0.56);

    await page.screenshot({
      path: path.join(screenshotRoot, `bridge-${viewport.width}.png`),
      animations: "disabled",
      fullPage: true,
    });
  });
}

test("follows system theme and preserves drawer keyboard focus", async ({
  page,
}) => {
  await page.setViewportSize({ width: 760, height: 600 });
  await page.emulateMedia({ colorScheme: "dark" });
  await page.goto("/?preview=1");
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");

  const theme = page.getByLabel("Color theme");
  await theme.selectOption("light");
  await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
  await theme.selectOption("system");
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");

  const browserButton = page.getByRole("button", { name: "Agent browser" });
  await browserButton.click();
  const title = page.getByRole("heading", { name: "Agent browser" });
  await expect(title).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(page.locator("#tool-drawer")).toHaveCount(0);
  await expect(browserButton).toBeFocused();
});

test("exposes explicit model safety and secure A2A token controls", async ({
  page,
}) => {
  await page.goto("/?preview=1");
  await page.getByRole("button", { name: "Models & routing" }).click();
  await page.getByRole("button", { name: "Choose active model" }).click();
  const modelMenu = page.getByRole("menu");
  await expect(modelMenu.getByText("Known · conformance tested")).toBeVisible();
  await expect(
    modelMenu.getByText(
      "Experimental · unavailable · not conformance tested",
    ),
  ).toBeVisible();
  await page.keyboard.press("Escape");
  await page.keyboard.press("Escape");

  await page.getByRole("button", { name: "Delegated tasks" }).click();
  await page.getByRole("button", { name: "Generate token" }).click();
  await expect(
    page.getByText("Copy this token now. It will not be shown again."),
  ).toBeVisible();
  await page.getByRole("button", { name: "Dismiss" }).click();
  await expect(page.getByText("bridge-preview-token-shown-once")).toHaveCount(0);
});

test("has no serious automated accessibility violations", async ({ page }) => {
  await page.goto("/?preview=1");
  await page.addScriptTag({ content: axe.source });
  const violations = await page.evaluate(async () => {
    const axeRuntime = (window as typeof window & {
      axe: { run: () => Promise<{ violations: Array<{ impact: string | null }> }> };
    }).axe;
    const result = await axeRuntime.run();
    return result.violations.filter(
      (violation) =>
        violation.impact === "serious" || violation.impact === "critical",
    );
  });
  expect(violations).toEqual([]);
});
